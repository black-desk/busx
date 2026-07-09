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

fn obj(path: &str, interfaces: usize, children: Vec<ObjectNode>) -> ObjectNode {
    ObjectNode { path: path.to_string(), interfaces, children }
}

#[test]
fn objects_screen_renders_flat_paths() {
    // `/` and `/org` are pure containers (no interfaces); only the leaves that
    // actually expose an object survive the flat view.
    let tree = obj(
        "/",
        0,
        vec![obj("/org", 0, vec![obj("/org/foo", 2, vec![])]), obj("/bar", 1, vec![])],
    );
    let paths = busx::tui::flatten_paths(&tree);
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(busx::tui::state::ObjectsScreen {
            service: "org.busx.Test".into(),
            paths,
            selected: 0,
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 48, 9));
}

#[test]
fn flatten_paths_skips_empty_objects() {
    // Pure container paths (0 interfaces) are filtered; only paths that expose
    // at least one interface survive, depth-first.
    let tree = obj(
        "/",
        0,
        vec![
            obj("/org", 0, vec![obj("/org/foo", 2, vec![])]),
            obj("/bar", 1, vec![]),
            obj("/empty", 0, vec![obj("/empty/x", 1, vec![])]),
        ],
    );
    assert_eq!(busx::tui::flatten_paths(&tree), vec!["/org/foo", "/bar", "/empty/x"]);
}

// --- Objects screen behavior: Enter / load / auto-skip / error (pure `update`) ---

fn objects_screen(service: &str) -> busx::tui::state::ObjectsScreen {
    busx::tui::state::ObjectsScreen {
        service: service.into(),
        paths: vec![],
        selected: 0,
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
            assert!(o.paths.is_empty());
        }
        _ => panic!("top screen should be Objects"),
    }
}

#[test]
fn objects_loaded_populates_paths_without_skip() {
    let mut state = State { screens: vec![Screen::Objects(objects_screen("org.busx.A"))], quit: false };
    let tree = obj("/", 1, vec![obj("/a", 1, vec![]), obj("/b", 1, vec![])]);
    let effect = update(&mut state, Msg::ObjectsLoaded(Ok(tree)));
    assert!(effect.is_none(), "multiple paths ⇒ no auto-skip, no fetch");
    match state.top() {
        Screen::Objects(o) => {
            assert!(!o.loading);
            // all three expose an object ⇒ all three in the flat list
            assert_eq!(o.paths, vec!["/", "/a", "/b"]);
        }
        _ => panic!("still on Objects"),
    }
}

#[test]
fn objects_loaded_single_path_auto_skips_to_interfaces() {
    let mut state = State { screens: vec![Screen::Objects(objects_screen("org.busx.A"))], quit: false };
    // Only the root "/" exposes an object ⇒ one path ⇒ auto-skip into its interfaces.
    let tree = obj("/", 1, vec![]);
    let effect = update(&mut state, Msg::ObjectsLoaded(Ok(tree)));
    match effect {
        Some(Effect::FetchInterfaces(s, p)) => {
            assert_eq!(s, "org.busx.A");
            assert_eq!(p, "/");
        }
        _ => panic!("single path ⇒ FetchInterfaces"),
    }
    assert_eq!(state.screens.len(), 2, "auto-skip pushed Interfaces");
    match state.top() {
        Screen::Interfaces(i) => {
            assert_eq!(i.service, "org.busx.A");
            assert_eq!(i.object, "/");
            assert!(i.loading, "Interfaces pushed in loading state");
        }
        _ => panic!("top should be Interfaces after auto-skip"),
    }
}

#[test]
fn objects_enter_drills_selected_path() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(busx::tui::state::ObjectsScreen {
            service: "org.busx.A".into(),
            paths: vec!["/".into(), "/org".into(), "/org/x".into()],
            selected: 2, // "/org/x"
            loading: false,
            error: None,
        })],
        quit: false,
    };
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::FetchInterfaces(s, p)) => {
            assert_eq!(s, "org.busx.A");
            assert_eq!(p, "/org/x");
        }
        _ => panic!("Enter drills the selected path"),
    }
    match state.top() {
        Screen::Interfaces(i) => assert_eq!(i.object, "/org/x"),
        _ => panic!("pushed an Interfaces screen"),
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

#[test]
fn interfaces_screen_lists_interfaces() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(busx::tui::state::InterfacesScreen {
            service: "org.busx.Test".into(),
            object: "/org/busx/Test".into(),
            names: vec!["org.busx.Test".into()],
            node: None,
            selected: 0,
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 44, 7));
}

fn introspect_node(xml: &str) -> zbus_xml::Node<'static> {
    zbus_xml::Node::from_reader(xml.as_bytes()).expect("valid introspection XML")
}

#[test]
fn interfaces_loaded_lists_all() {
    // No filtering: every interface (incl. standard org.freedesktop.DBus.*) is shown.
    let node = introspect_node(
        "<node>\
         <interface name=\"org.freedesktop.DBus.Peer\"/>\
         <interface name=\"org.freedesktop.DBus.Properties\"/>\
         <interface name=\"org.busx.A\"/>\
         <interface name=\"org.busx.B\"/>\
         </node>",
    );
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(busx::tui::state::InterfacesScreen {
            service: "org.busx.Test".into(),
            object: "/o".into(),
            names: vec![],
            node: None,
            selected: 0,
            loading: true,
            error: None,
        })],
        quit: false,
    };
    let effect = update(
        &mut state,
        Msg::InterfacesLoaded("org.busx.Test".into(), "/o".into(), Ok(node)),
    );
    assert!(effect.is_none(), "four interfaces ⇒ no auto-skip");
    match state.top() {
        Screen::Interfaces(i) => {
            assert!(!i.loading);
            assert_eq!(
                i.names,
                vec![
                    "org.freedesktop.DBus.Peer".to_string(),
                    "org.freedesktop.DBus.Properties".to_string(),
                    "org.busx.A".to_string(),
                    "org.busx.B".to_string(),
                ]
            );
            assert!(i.node.is_some(), "node cached for drilling in");
        }
        _ => panic!("still on Interfaces"),
    }
}

#[test]
fn interfaces_loaded_single_interface_auto_skips() {
    let node = introspect_node("<node><interface name=\"org.busx.Test\"/></node>");
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(busx::tui::state::InterfacesScreen {
            service: "org.busx.Test".into(),
            object: "/o".into(),
            names: vec![],
            node: None,
            selected: 0,
            loading: true,
            error: None,
        })],
        quit: false,
    };
    let effect = update(
        &mut state,
        Msg::InterfacesLoaded("org.busx.Test".into(), "/o".into(), Ok(node)),
    );
    match effect {
        Some(Effect::FetchProperties(_, _, iface)) => assert_eq!(iface, "org.busx.Test"),
        _ => panic!("single interface ⇒ FetchProperties"),
    }
    match state.top() {
        Screen::Interface(i) => assert_eq!(i.interface, "org.busx.Test"),
        _ => panic!("auto-skip pushed an Interface screen"),
    }
}

use busx::tui::state::InterfaceFocus;

#[test]
fn interface_screen_renders_three_columns() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
            service: "org.busx.Test".into(),
            object: "/org/busx/Test".into(),
            interface: "org.busx.Test".into(),
            methods: vec![
                method("BumpVolume", ""),
                method("Join", "as"),
            ],
            properties: vec![
                ("volume".into(), "d".into(), "readwrite".into()),
                ("name".into(), "s".into(), "read".into()),
            ],
            signals: vec![],
            prop_values: vec![("volume".into(), "0.5".into()), ("name".into(), r#""busx-test""#.into())],
            focus: InterfaceFocus::Properties,
            active_column: InterfaceFocus::Properties,
            button_selected: 0,
            selected: [0, 1, 0],
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 60, 16));
}

#[test]
fn properties_loaded_fills_pretty_values() {
    use zvariant::Value;
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            methods: vec![],
            properties: vec![("volume".into(), "d".into(), "readwrite".into())],
            signals: vec![],
            prop_values: vec![],
            focus: Default::default(),
            active_column: Default::default(),
            button_selected: 0,
            selected: [0, 0, 0],
            loading: true,
            error: None,
        })],
        quit: false,
    };
    let vals = vec![("volume".into(), Value::F64(0.5).try_to_owned().unwrap())];
    let effect = update(&mut state, Msg::PropertiesLoaded(Ok(vals)));
    assert!(effect.is_none(), "PropertiesLoaded requests no fetch");
    match state.top() {
        Screen::Interface(i) => {
            assert!(!i.loading);
            assert_eq!(i.prop_values, vec![("volume".to_string(), "0.5".to_string())]);
        }
        _ => panic!("still on Interface"),
    }
}

fn interface_screen() -> busx::tui::state::InterfaceScreen {
    busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "i".into(),
        methods: vec![method("m1", "u"), method("m2", "")],
        properties: vec![("p1".into(), "s".into(), "read".into())],
        signals: vec![("sig1".into(), "u".into())],
        prop_values: vec![],
        focus: InterfaceFocus::Methods,
        active_column: InterfaceFocus::Methods,
        button_selected: 0,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    }
}

/// A `MethodMember` with no per-arg detail (Task 2 fills `args`).
fn method(name: &str, signature: &str) -> busx::tui::state::MethodMember {
    busx::tui::state::MethodMember { name: name.into(), signature: signature.into(), args: vec![] }
}

/// A `MethodMember` whose `args` carry per-IN-arg (name, signature) pairs — the
/// source of the call Detail form's input fields. The concatenated `signature`
/// is derived from the args.
fn method_with_args(name: &str, args: &[(&str, &str)]) -> busx::tui::state::MethodMember {
    let signature = args.iter().map(|(_, s)| *s).collect::<String>();
    busx::tui::state::MethodMember {
        name: name.into(),
        signature,
        args: args.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect(),
    }
}

#[test]
fn interface_tab_toggles_column_and_buttons() {
    let mut state = busx::tui::State { screens: vec![Screen::Interface(interface_screen())], quit: false };
    // Start on the Methods column (focus == active_column == Methods).
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
    // Tab jumps to the button bar.
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Buttons);
    // Tab again returns to the active column.
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
}

#[test]
fn interface_backtab_cycles_active_column() {
    let mut state = busx::tui::State { screens: vec![Screen::Interface(interface_screen())], quit: false };
    // Shift+Tab (BackTab) cycles the active column Methods→Properties→Signals→Methods.
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Properties);
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
}

#[test]
fn interface_arrows_move_within_focused_column() {
    let mut state = busx::tui::State { screens: vec![Screen::Interface(interface_screen())], quit: false };
    // Methods focus, two methods, starts at 0.
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.top_selected(), [1, 0, 0]);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.top_selected(), [1, 0, 0], "clamped at last method");
    // BackTab to signals (1 signal), Down clamps.
    update(&mut state, key(KeyCode::BackTab));
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.top_selected(), [1, 0, 0]);
    update(&mut state, key(KeyCode::Up)); // no-op above 0
    assert_eq!(state.top_selected(), [1, 0, 0]);
}

#[test]
fn interface_r_requests_property_refresh() {
    let mut state = busx::tui::State { screens: vec![Screen::Interface(interface_screen())], quit: false };
    let effect = update(&mut state, key(KeyCode::Char('r')));
    match effect {
        Some(Effect::FetchProperties(s, o, i)) => {
            assert_eq!(s, "s");
            assert_eq!(o, "/o");
            assert_eq!(i, "i");
        }
        _ => panic!("r should request FetchProperties"),
    }
    match state.top() {
        Screen::Interface(i) => assert!(i.loading, "r marks the screen loading until values arrive"),
        _ => panic!(),
    }
}

#[test]
fn drill_down_auto_skips_service_to_interface() {
    // One service → one object ("/") → one interface ⇒ the whole chain auto-skips,
    // landing on the Interface screen with members parsed from the node.
    let node = introspect_node(
        "<node>\
         <interface name=\"org.busx.Test\">\
         <method name=\"Ping\"/>\
         <property name=\"Name\" type=\"s\" access=\"read\"/>\
         </interface>\
         </node>",
    );
    let tree = obj("/", 1, vec![]);
    let events = vec![
        Msg::ServicesLoaded(Ok(vec![svc("org.busx.Test", None, None)])),
        key(KeyCode::Enter),
        Msg::ObjectsLoaded(Ok(tree)),
        Msg::InterfacesLoaded("org.busx.Test".into(), "/".into(), Ok(node)),
    ];
    let mut app = App { state: busx::tui::State::loading_service() };
    let backend = TestBackend::new(60, 16);
    let mut term = Terminal::new(backend).unwrap();
    app.run_loop(&mut term, events.into_iter(), |_| {}).unwrap();
    match app.state.top() {
        Screen::Interface(i) => {
            assert_eq!(i.service, "org.busx.Test");
            assert_eq!(i.object, "/");
            assert_eq!(i.interface, "org.busx.Test");
            assert_eq!(i.methods.len(), 1, "Ping parsed from the node");
            assert_eq!(i.properties.len(), 1, "Name parsed from the node");
        }
        _ => panic!("auto-skip chain should land on Interface"),
    }
    insta::assert_snapshot!(format!("{}", term.backend()));
}

// --- Phase 3: action buttons push a stub Detail screen ---

#[test]
fn interface_button_enter_pushes_call_detail() {
    // Methods column, focus on the button bar, button_selected on `调用`.
    let mut screen = interface_screen();
    screen.active_column = InterfaceFocus::Methods;
    screen.focus = InterfaceFocus::Buttons;
    screen.button_selected = 0;
    screen.selected = [0, 0, 0]; // m1 (signature "u")
    let mut state = busx::tui::State { screens: vec![Screen::Interface(screen)], quit: false };
    let effect = update(&mut state, key(KeyCode::Enter));
    assert!(effect.is_none(), "button Enter pushes a stub (no Effect this task)");
    match state.top() {
        Screen::Detail(d) => {
            assert_eq!(d.service, "s");
            assert_eq!(d.object, "/o");
            assert_eq!(d.interface, "i");
            match &d.kind {
                busx::tui::state::ActionKind::Call { method, signature } => {
                    assert_eq!(method, "m1");
                    assert_eq!(signature, "u");
                }
                other => panic!("expected Call, got {other:?}"),
            }
            assert!(d.inputs.is_empty(), "stub Detail has no inputs yet");
            assert!(!d.loading);
        }
        _ => panic!("Enter should push a Detail screen"),
    }
}

#[test]
fn interface_button_enter_pushes_get_detail() {
    // Properties column, `读取` button (index 0) on p1.
    let mut screen = interface_screen();
    screen.active_column = InterfaceFocus::Properties;
    screen.focus = InterfaceFocus::Buttons;
    screen.button_selected = 0;
    screen.selected = [0, 0, 0]; // p1
    let mut state = busx::tui::State { screens: vec![Screen::Interface(screen)], quit: false };
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => match &d.kind {
            busx::tui::state::ActionKind::Get { property } => assert_eq!(property, "p1"),
            other => panic!("expected Get, got {other:?}"),
        },
        _ => panic!("Detail screen expected"),
    }
}

#[test]
fn interface_button_enter_pushes_set_detail() {
    // Properties column, `设置` button (index 1) on p1 (signature "s").
    let mut screen = interface_screen();
    screen.active_column = InterfaceFocus::Properties;
    screen.focus = InterfaceFocus::Buttons;
    screen.button_selected = 1; // 设置
    screen.selected = [0, 0, 0];
    let mut state = busx::tui::State { screens: vec![Screen::Interface(screen)], quit: false };
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => match &d.kind {
            busx::tui::state::ActionKind::Set { property, signature } => {
                assert_eq!(property, "p1");
                assert_eq!(signature, "s");
            }
            other => panic!("expected Set, got {other:?}"),
        },
        _ => panic!("Detail screen expected"),
    }
}

#[test]
fn interface_renders_action_button_bar() {
    // Methods column with a method selected → the right panel shows `actions` /
    // `调用`, focused when focus == Buttons.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
            service: "org.busx.Test".into(),
            object: "/org/busx/Test".into(),
            interface: "org.busx.Test".into(),
            methods: vec![method("Ping", ""), method("Echo", "ss")],
            properties: vec![("Name".into(), "s".into(), "read".into())],
            signals: vec![],
            prop_values: vec![],
            focus: InterfaceFocus::Buttons,
            active_column: InterfaceFocus::Methods,
            button_selected: 0,
            selected: [0, 0, 0],
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 64, 16));
}

// --- Phase 3 Task 2: method-call Detail form + Result ---

use busx::tui::state::{ActionKind, ActionResult, DetailFocus, DetailScreen, ResultScreen};

/// An Interface screen focused on the button bar, with `button_selected` on the
/// given button index; `selected[0]` points at `methods[idx]`.
fn interface_on_button(methods: Vec<busx::tui::state::MethodMember>, button: usize) -> busx::tui::State {
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "i".into(),
        methods,
        properties: vec![],
        signals: vec![],
        prop_values: vec![],
        focus: InterfaceFocus::Buttons,
        active_column: InterfaceFocus::Methods,
        button_selected: button,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    busx::tui::State { screens: vec![screen], quit: false }
}

#[test]
fn call_button_pushes_detail_with_one_input_per_arg() {
    // One IN-arg `n: u` → the call Detail has one input, labeled "n  u".
    let state = interface_on_button(vec![method_with_args("Add", &[("n", "u")])], 0);
    let mut state = state;
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => {
            assert_eq!(d.inputs.len(), 1, "one input per IN-arg");
            assert_eq!(d.field_labels, vec!["n  u".to_string()]);
            match &d.kind {
                ActionKind::Call { method, signature } => {
                    assert_eq!(method, "Add");
                    assert_eq!(signature, "u", "concatenated IN-signature");
                }
                other => panic!("expected Call, got {other:?}"),
            }
            assert_eq!(d.focus, DetailFocus::Field, "starts focused on the field");
        }
        _ => panic!("Enter should push a Detail screen"),
    }
}

#[test]
fn call_detail_anonymous_arg_labeled_with_signature_only() {
    // An anonymous IN-arg (empty name) → the label is just the signature.
    let state = interface_on_button(vec![method_with_args("Echo", &[("", "s")])], 0);
    let mut state = state;
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => assert_eq!(d.field_labels, vec!["s".to_string()]),
        _ => panic!("Detail screen expected"),
    }
}

#[test]
fn zero_arg_method_pushes_detail_with_no_inputs() {
    // A method with no IN-args → the Detail is just the trigger button.
    let state = interface_on_button(vec![method_with_args("Ping", &[])], 0);
    let mut state = state;
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => {
            assert!(d.inputs.is_empty(), "zero-arg call → no inputs");
            assert!(d.field_labels.is_empty());
        }
        _ => panic!("Detail screen expected"),
    }
}

#[test]
fn detail_typing_edits_focused_input() {
    let mut state = interface_on_button(vec![method_with_args("Add", &[("n", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the Detail
    update(&mut state, key(KeyCode::Char('4')));
    update(&mut state, key(KeyCode::Char('2')));
    match state.top() {
        Screen::Detail(d) => {
            assert_eq!(d.inputs[0].value(), "42", "keystrokes flow into the field");
            assert_eq!(d.focus, DetailFocus::Field, "still field-focused while typing");
        }
        _ => panic!("still on Detail"),
    }
}

#[test]
fn detail_tab_cycles_field_to_trigger_then_back() {
    let mut state = interface_on_button(vec![method_with_args("Add", &[("n", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the Detail (1 input)
    // Field0 → (only one field) → Trigger.
    update(&mut state, key(KeyCode::Tab));
    match state.top() {
        Screen::Detail(d) => assert_eq!(d.focus, DetailFocus::Trigger),
        _ => panic!(),
    }
    // Trigger → Field0.
    update(&mut state, key(KeyCode::Tab));
    match state.top() {
        Screen::Detail(d) => {
            assert_eq!(d.focus, DetailFocus::Field);
            assert_eq!(d.field_selected, 0);
        }
        _ => panic!(),
    }
}

#[test]
fn detail_tab_cycles_across_multiple_fields() {
    let mut state = interface_on_button(vec![method_with_args("Add", &[("a", "u"), ("b", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the Detail (2 inputs)
    // Field0 → Field1 → Trigger → Field0.
    let foci = [
        (DetailFocus::Field, 1),
        (DetailFocus::Trigger, 1),
        (DetailFocus::Field, 0),
    ];
    for (want_focus, want_field) in foci {
        update(&mut state, key(KeyCode::Tab));
        match state.top() {
            Screen::Detail(d) => {
                assert_eq!(d.focus, want_focus, "tab cycle");
                assert_eq!(d.field_selected, want_field);
            }
            _ => panic!(),
        }
    }
}

#[test]
fn detail_arrows_move_field_selection() {
    let mut state = interface_on_button(vec![method_with_args("Add", &[("a", "u"), ("b", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // 2 inputs, field_selected 0
    update(&mut state, key(KeyCode::Down));
    match state.top() {
        Screen::Detail(d) => assert_eq!(d.field_selected, 1, "Down moves to field 1"),
        _ => panic!(),
    }
    update(&mut state, key(KeyCode::Up));
    match state.top() {
        Screen::Detail(d) => assert_eq!(d.field_selected, 0, "Up moves back to field 0"),
        _ => panic!(),
    }
}

#[test]
fn detail_trigger_enter_pushes_result_and_requests_call() {
    let mut state = interface_on_button(vec![method_with_args("Add", &[("n", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the Detail
    update(&mut state, key(KeyCode::Char('4')));
    update(&mut state, key(KeyCode::Char('2')));
    // Tab to the trigger, then Enter → Result (loading) + CallMethod effect.
    update(&mut state, key(KeyCode::Tab));
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CallMethod { service, object, iface, method, signature, args }) => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "i");
            assert_eq!(method, "Add");
            assert_eq!(signature, "u");
            assert_eq!(args, vec!["42".to_string()], "field values flow as call args");
        }
        other => panic!("trigger Enter should request CallMethod, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "i.Add");
            assert!(r.result.is_none());
        }
        _ => panic!("trigger pushed a Result screen"),
    }
}

#[test]
fn zero_arg_call_trigger_requests_call_with_empty_args() {
    let mut state = interface_on_button(vec![method_with_args("Ping", &[])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the 0-input Detail
    // 0 inputs → Field collapses; one Tab lands on Trigger.
    update(&mut state, key(KeyCode::Tab));
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CallMethod { method, signature, args, .. }) => {
            assert_eq!(method, "Ping");
            assert_eq!(signature, "", "zero-arg method has empty signature");
            assert!(args.is_empty(), "zero-arg call sends no args");
        }
        other => panic!("expected CallMethod, got {other:?}"),
    }
    assert!(matches!(state.top(), Screen::Result(_)));
}

#[test]
fn action_result_populates_result_screen() {
    // A Result screen mid-flight (loading) receiving a Call result shows the value.
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "i.Add".into(),
            result: None,
            error: None,
            loading: true,
            scroll: 0,
        })],
        quit: false,
    };
    let effect = update(&mut state, Msg::ActionResult(Ok(ActionResult::Call(vec!["7".into()]))));
    assert!(effect.is_none(), "ActionResult requests no fetch");
    match state.top() {
        Screen::Result(r) => {
            assert!(!r.loading);
            match &r.result {
                Some(ActionResult::Call(lines)) => assert_eq!(lines, &vec!["7".to_string()]),
                other => panic!("expected Call, got {other:?}"),
            }
        }
        _ => panic!("still on Result"),
    }
}

#[test]
fn call_detail_form_renders_field_and_trigger() {
    // The 1-arg call Detail, with the field focused: the field row + `[触发]`.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Detail(DetailScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            kind: ActionKind::Call { method: "Add".into(), signature: "u".into() },
            inputs: vec!["42".into()],
            field_labels: vec!["n  u".into()],
            field_selected: 0,
            focus: DetailFocus::Field,
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

#[test]
fn call_result_renders_reply_value() {
    // A completed call Result shows one line per reply value.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "i.Add".into(),
            result: Some(ActionResult::Call(vec!["49".into()])),
            error: None,
            loading: false,
            scroll: 0,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

// --- Phase 3 Task 3: property get/set Detail + Result ---

/// An Interface screen whose Properties column has one property (name, sig,
/// access) and is focused on the button bar with `button_selected` on the given
/// action (`读取`=0 / `设置`=1).
fn interface_on_prop_button(button: usize, sig: &str) -> busx::tui::State {
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "i".into(),
        methods: vec![],
        properties: vec![("p1".into(), sig.into(), "readwrite".into())],
        signals: vec![],
        prop_values: vec![],
        focus: InterfaceFocus::Buttons,
        active_column: InterfaceFocus::Properties,
        button_selected: button,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    busx::tui::State { screens: vec![screen], quit: false }
}

#[test]
fn get_button_pushes_detail_with_no_inputs() {
    // `读取` on p1 → a Get Detail with zero inputs and zero labels.
    let mut state = interface_on_prop_button(0, "d");
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => {
            match &d.kind {
                ActionKind::Get { property } => assert_eq!(property, "p1"),
                other => panic!("expected Get, got {other:?}"),
            }
            assert!(d.inputs.is_empty(), "Get → no input fields");
            assert!(d.field_labels.is_empty());
        }
        _ => panic!("Enter should push a Detail screen"),
    }
}

#[test]
fn get_trigger_pushes_result_and_requests_get() {
    let mut state = interface_on_prop_button(0, "d");
    update(&mut state, key(KeyCode::Enter)); // push the Get Detail (0 inputs)
    // 0 inputs → a single Tab lands on the trigger.
    update(&mut state, key(KeyCode::Tab));
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::GetProperty { service, object, iface, property }) => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "i");
            assert_eq!(property, "p1");
        }
        other => panic!("trigger Enter should request GetProperty, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading);
            assert_eq!(r.title, "p1");
        }
        _ => panic!("trigger pushed a Result screen"),
    }
    // The result payload populates the Result screen.
    update(&mut state, Msg::ActionResult(Ok(ActionResult::Get("0.5".into()))));
    match state.top() {
        Screen::Result(r) => match &r.result {
            Some(ActionResult::Get(v)) => assert_eq!(v, "0.5"),
            other => panic!("expected Get result, got {other:?}"),
        },
        _ => panic!("still on Result"),
    }
}

#[test]
fn set_button_pushes_detail_with_one_input_labeled_by_signature() {
    // `设置` on p1 (signature "s") → a Set Detail with one input, label "s".
    let mut state = interface_on_prop_button(1, "s");
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => {
            match &d.kind {
                ActionKind::Set { property, signature } => {
                    assert_eq!(property, "p1");
                    assert_eq!(signature, "s");
                }
                other => panic!("expected Set, got {other:?}"),
            }
            assert_eq!(d.inputs.len(), 1, "Set → one input field");
            assert_eq!(d.field_labels, vec!["s".to_string()], "label is the signature");
        }
        _ => panic!("Enter should push a Detail screen"),
    }
}

#[test]
fn set_trigger_pushes_result_with_typed_value() {
    let mut state = interface_on_prop_button(1, "s");
    update(&mut state, key(KeyCode::Enter)); // push the Set Detail (1 input)
    // Type "hi" into the field.
    update(&mut state, key(KeyCode::Char('h')));
    update(&mut state, key(KeyCode::Char('i')));
    match state.top() {
        Screen::Detail(d) => assert_eq!(d.inputs[0].value(), "hi"),
        _ => panic!("still on Detail while typing"),
    }
    // Tab to the trigger, Enter → Result (loading) + SetProperty effect.
    update(&mut state, key(KeyCode::Tab));
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::SetProperty { service, object, iface, property, signature, value }) => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "i");
            assert_eq!(property, "p1");
            assert_eq!(signature, "s");
            assert_eq!(value, "hi", "typed field value flows as the set value");
        }
        other => panic!("trigger Enter should request SetProperty, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading);
            assert_eq!(r.title, "p1");
        }
        _ => panic!("trigger pushed a Result screen"),
    }
    // Set success populates the Result screen.
    update(&mut state, Msg::ActionResult(Ok(ActionResult::Set)));
    match state.top() {
        Screen::Result(r) => match &r.result {
            Some(ActionResult::Set) => {}
            other => panic!("expected Set result, got {other:?}"),
        },
        _ => panic!("still on Result"),
    }
}

#[test]
fn set_detail_form_renders_one_field() {
    // A Set Detail with one field (label "s"), field focused.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Detail(DetailScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            kind: ActionKind::Set { property: "p1".into(), signature: "s".into() },
            inputs: vec!["hi".into()],
            field_labels: vec!["s".into()],
            field_selected: 0,
            focus: DetailFocus::Field,
            loading: false,
            error: None,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

#[test]
fn get_result_renders_value() {
    // A completed Get Result shows the property value.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "p1".into(),
            result: Some(ActionResult::Get("0.5".into())),
            error: None,
            loading: false,
            scroll: 0,
        })],
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

// --- Phase 3 Task 4: capstone loop test (full call through run_loop) ---

/// Drive a full method call through `run_loop`: Interface → Tab to the button
/// bar → Enter (`调用`) pushes the Detail → type "42" → Tab to the trigger →
/// Enter pushes the Result (loading) + a `CallMethod` Effect (no-op'd by the
/// bus-free handler) → a scripted `ActionResult::Call` reply lands in the
/// Result screen. Snapshots the final Result frame.
#[test]
fn call_action_flows_interface_to_result() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            // One method "Add(n: u)" → signature "u", one IN-arg input field.
            methods: vec![method_with_args("Add", &[("n", "u")])],
            properties: vec![],
            signals: vec![],
            prop_values: vec![],
            focus: InterfaceFocus::Methods,
            active_column: InterfaceFocus::Methods,
            button_selected: 0, // 调用
            selected: [0, 0, 0], // Add
            loading: false,
            error: None,
        })],
        quit: false,
    };
    let _ = &mut state; // (kept for parity with the literal style; state moves below)
    let events = vec![
        key(KeyCode::Tab),           // Methods column → Buttons
        key(KeyCode::Enter),         // 调用 → push Call Detail (1 input)
        key(KeyCode::Char('4')),     // type into the field
        key(KeyCode::Char('2')),
        key(KeyCode::Tab),           // Field → Trigger
        key(KeyCode::Enter),         // push Result (loading) + CallMethod (no-op'd)
        Msg::ActionResult(Ok(ActionResult::Call(vec!["42".into()]))), // scripted reply
    ];
    let mut app = App { state };
    let backend = TestBackend::new(40, 8);
    let mut term = Terminal::new(backend).unwrap();
    app.run_loop(&mut term, events.into_iter(), |_| {}).unwrap();
    match app.state.top() {
        Screen::Result(r) => {
            assert!(!r.loading, "the scripted reply cleared loading");
            assert_eq!(r.title, "i.Add");
            match &r.result {
                Some(ActionResult::Call(v)) => assert_eq!(v, &vec!["42".to_string()]),
                other => panic!("expected Call result, got {other:?}"),
            }
        }
        _ => panic!("should land on Result"),
    }
    insta::assert_snapshot!(format!("{}", term.backend()));
}
