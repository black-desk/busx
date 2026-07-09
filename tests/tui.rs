// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TUI snapshot tests (spec §13). Drive the pure State/render core, render to a
//! ratatui TestBackend, compare to an insta golden snapshot. No real bus.

use busx::dbus::types::ServiceInfo;
use busx::tui::{render, update, Effect, Msg, Screen, State};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Render `state` to a `w`×`h` buffer and return its text view for insta.
/// TestBackend's Display is ratatui's readable buffer_view (text only).
fn render_to_string(state: &State, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, state)).unwrap();
    format!("{}", term.backend())
}

fn svc(name: &str, pid: Option<u64>, process: Option<&str>) -> ServiceInfo {
    ServiceInfo { name: name.into(), pid, process: process.map(Into::into) }
}

#[test]
fn service_screen_renders_populated_list() {
    let state = State::service(vec![
        svc("org.busx.Test", Some(1234), Some("dbus-daemon")),
        svc("org.busx.Other", None, None),
    ]);
    insta::assert_snapshot!(render_to_string(&state, 60, 8));
}

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn selected_of(state: &State) -> usize {
    match state.top() {
        Screen::Service(s) => s.selected,
        _ => 0,
    }
}

#[test]
fn service_screen_down_arrow_moves_selection() {
    let mut state =
        State::service(vec![svc("a", None, None), svc("b", None, None), svc("c", None, None)]);
    assert_eq!(selected_of(&state), 0, "starts on row 0");
    update(&mut state, key(KeyCode::Down));
    assert_eq!(selected_of(&state), 1, "Down → row 1");
    insta::assert_snapshot!(render_to_string(&state, 40, 7));
}

#[test]
fn service_screen_up_clamps_at_top() {
    let mut state = State::service(vec![svc("a", None, None), svc("b", None, None)]);
    update(&mut state, key(KeyCode::Up));
    assert_eq!(selected_of(&state), 0, "Up at top stays at 0");
}

#[test]
fn quit_on_q_and_escape() {
    let mut state = State::service(vec![]);
    update(&mut state, key(KeyCode::Char('q')));
    assert!(state.quit, "q quits");
    let mut state = State::service(vec![]);
    update(&mut state, key(KeyCode::Esc));
    assert!(state.quit, "Esc quits");
}

#[test]
fn service_screen_loading_state() {
    let state = State::loading_service();
    insta::assert_snapshot!(render_to_string(&state, 40, 6));
}

#[test]
fn service_screen_error_state() {
    let state = State {
        screens: vec![busx::tui::Screen::Service(busx::tui::ServiceScreen {
            services: vec![],
            selected: 0,
            loading: false,
            error: Some("org.freedesktop.DBus.Error.ServiceUnknown: no owner".into()),
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 6));
}

use busx::tui::app::App;

#[test]
fn loop_loads_services_then_navigates() {
    let services = vec![
        svc("org.busx.A", Some(111), None),
        svc("org.busx.B", None, None),
    ];
    let events = vec![
        Msg::ServicesLoaded(Ok(services)),
        Msg::Key(crossterm::event::KeyCode::Down.into()),
    ];
    let mut app = App { state: busx::tui::State::loading_service() };
    let backend = TestBackend::new(44, 8);
    let mut term = Terminal::new(backend).unwrap();
    app.run_loop(&mut term, events.into_iter(), |_| {}).unwrap();
    // The scripted Down moved selection to row 1 (REVERSED highlight is the only
    // selection cue in the real UI; the text snapshot can't show styling, so
    // assert the selection explicitly).
    assert_eq!(selected_of(&app.state), 1, "Down moved selection to row 1");
    insta::assert_snapshot!(format!("{}", term.backend()));
}

use busx::dbus::types::ObjectNode;

fn obj(path: &str, children: Vec<ObjectNode>) -> ObjectNode {
    ObjectNode { path: path.to_string(), children }
}

#[test]
fn objects_screen_renders_tree() {
    let tree = obj(
        "/",
        vec![obj("/org", vec![obj("/org/busx", vec![])]), obj("/foo", vec![])],
    );
    let items = busx::tui::tree_items(&tree);
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(busx::tui::state::ObjectsScreen {
            service: "org.busx.Test".into(),
            tree,
            items,
            state: Default::default(),
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 48, 9));
}

// --- Objects screen behavior: Enter / load / auto-skip / error (pure `update`) ---

fn objects_screen(service: &str) -> busx::tui::state::ObjectsScreen {
    busx::tui::state::ObjectsScreen {
        service: service.into(),
        tree: ObjectNode { path: "/".into(), children: vec![] },
        items: vec![],
        state: Default::default(),
        loading: true,
        error: None,
    }
}

#[test]
fn service_enter_pushes_objects_and_requests_fetch() {
    let mut state = State::service(vec![
        svc("org.busx.A", None, None),
        svc("org.busx.B", None, None),
    ]);
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::FetchObjects(s)) => assert_eq!(s, "org.busx.A"),
        _ => panic!("Enter should request FetchObjects"),
    }
    assert_eq!(state.screens.len(), 2, "Enter pushed an Objects screen");
    match state.top() {
        Screen::Objects(o) => {
            assert_eq!(o.service, "org.busx.A");
            assert!(o.loading, "new Objects screen starts loading");
            assert!(o.items.is_empty());
        }
        _ => panic!("top screen should be Objects"),
    }
}

#[test]
fn objects_loaded_populates_items_without_skip() {
    let mut state = State { screens: vec![Screen::Objects(objects_screen("org.busx.A"))], quit: false };
    let tree = obj("/", vec![obj("/a", vec![]), obj("/b", vec![])]);
    let effect = update(&mut state, Msg::ObjectsLoaded(Ok(tree)));
    assert!(effect.is_none(), "two children ⇒ no auto-skip, no fetch");
    match state.top() {
        Screen::Objects(o) => {
            assert!(!o.loading);
            assert_eq!(o.items.len(), 2, "two top-level items");
        }
        _ => panic!("still on Objects"),
    }
}

#[test]
fn objects_loaded_single_child_auto_skips_to_interfaces() {
    let mut state = State { screens: vec![Screen::Objects(objects_screen("org.busx.A"))], quit: false };
    let tree = obj("/", vec![obj("/org", vec![])]);
    let effect = update(&mut state, Msg::ObjectsLoaded(Ok(tree)));
    match effect {
        Some(Effect::FetchInterfaces(s, p)) => {
            assert_eq!(s, "org.busx.A");
            assert_eq!(p, "/org");
        }
        _ => panic!("single child ⇒ FetchInterfaces"),
    }
    assert_eq!(state.screens.len(), 2, "auto-skip pushed Interfaces");
    match state.top() {
        Screen::Interfaces(i) => {
            assert_eq!(i.service, "org.busx.A");
            assert_eq!(i.object, "/org");
            assert!(i.loading, "Interfaces pushed in loading state");
        }
        _ => panic!("top should be Interfaces after auto-skip"),
    }
}

#[test]
fn objects_loaded_error_sets_error_without_skip() {
    let mut state = State { screens: vec![Screen::Objects(objects_screen("org.busx.A"))], quit: false };
    let effect = update(&mut state, Msg::ObjectsLoaded(Err("boom".into())));
    assert!(effect.is_none(), "error path requests no fetch");
    match state.top() {
        Screen::Objects(o) => {
            assert!(!o.loading);
            assert_eq!(o.error.as_deref(), Some("boom"));
        }
        _ => panic!("still Objects on error"),
    }
}
