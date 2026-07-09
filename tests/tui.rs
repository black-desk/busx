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
            methods: vec![("BumpVolume".into(), "".into()), ("Join".into(), "as".into())],
            properties: vec![
                ("volume".into(), "d".into(), "readwrite".into()),
                ("name".into(), "s".into(), "read".into()),
            ],
            signals: vec![],
            prop_values: vec![("volume".into(), "0.5".into()), ("name".into(), r#""busx-test""#.into())],
            focus: InterfaceFocus::Properties,
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
        methods: vec![("m1".into(), "u".into()), ("m2".into(), "".into())],
        properties: vec![("p1".into(), "s".into(), "read".into())],
        signals: vec![("sig1".into(), "u".into())],
        prop_values: vec![],
        focus: InterfaceFocus::Methods,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    }
}

#[test]
fn interface_tab_cycles_focus() {
    let mut state = busx::tui::State { screens: vec![Screen::Interface(interface_screen())], quit: false };
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Properties);
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    update(&mut state, key(KeyCode::Tab));
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
    // Tab to signals (1 signal), Down clamps.
    update(&mut state, key(KeyCode::Tab));
    update(&mut state, key(KeyCode::Tab));
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
