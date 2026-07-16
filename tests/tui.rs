// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TUI snapshot tests (spec §13). Drive the pure State/render core, render to a
//! ratatui TestBackend, compare to an insta golden snapshot. No real bus.

use busx::dbus::conn::Bus;
use busx::dbus::types::ServiceInfo;
use busx::tui::{Effect, Msg, Screen, State, render, update};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Render `state` to a `w`×`h` buffer and return its text view for insta.
/// TestBackend's Display is ratatui's readable buffer_view (text only).
fn render_to_string(state: &State, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    let mut targets = Vec::new();
    let mut scroll = [0usize; 3];
    term.draw(|f| render(f, state, &mut targets, &mut scroll))
        .unwrap();
    format!("{}", term.backend())
}

/// Render with a *persisted* scroll offset threaded across calls, mirroring how
/// `run_loop` drives `render` (where the scroll out-param survives between
/// frames). `render_to_string` resets scroll each call, so it can't exercise
/// cross-frame scroll behavior — this helper can.
fn render_with_scroll(state: &State, scroll: &mut [usize; 3], w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    let mut targets = Vec::new();
    term.draw(|f| render(f, state, &mut targets, scroll))
        .unwrap();
    format!("{}", term.backend())
}

fn svc(name: &str, pid: Option<u64>, process: Option<&str>) -> ServiceInfo {
    ServiceInfo {
        name: name.into(),
        pid,
        process: process.map(Into::into),
    }
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
    let mut state = State::service(vec![
        svc("a", None, None),
        svc("b", None, None),
        svc("c", None, None),
    ]);
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

/// Regression: after scrolling the list *down* past the viewport and then moving
/// the cursor back *up*, the highlight must leave the bottom row and climb
/// within the viewport (vim/less-style) — the viewport only scrolls again once
/// the cursor reaches the top. Previously each frame rebuilt a fresh ListState
/// (offset 0), so ratatui re-pinned the cursor to the bottom every render; the
/// highlight stayed glued to the last row until it climbed back into the first
/// page.
#[test]
fn service_screen_scroll_up_after_down_does_not_pin_to_bottom() {
    // 20 services, viewport shows 4 list rows (h=8 → breadcrumb/footer 2 + block
    // borders 2 = 4 rows of list). Names are zero-padded so they're easy to grep
    // for in the rendered buffer.
    let services: Vec<_> = (0..20)
        .map(|n| svc(&format!("svc{n:02}"), None, None))
        .collect();
    let mut state = State::service(services);
    let mut scroll = [0usize; 3];

    // Move the cursor down 10 rows (to svc10). With a 4-row viewport the offset
    // lands at 7, so the visible window is svc07..svc10 with the cursor on svc10
    // (the bottom row).
    for _ in 0..10 {
        update(&mut state, key(KeyCode::Down));
    }
    let after_down = render_with_scroll(&state, &mut scroll, 40, 8);
    assert_eq!(selected_of(&state), 10);
    assert!(
        after_down.contains("svc07") && after_down.contains("svc10"),
        "scrolled-down window shows svc07..svc10:\n{after_down}"
    );

    // Now move UP one row (cursor → svc09). The viewport must NOT jump to keep
    // the cursor at the bottom: svc07 should remain the top visible row and
    // svc10 should still be visible (cursor is now one up from the bottom).
    update(&mut state, key(KeyCode::Up));
    let after_up = render_with_scroll(&state, &mut scroll, 40, 8);

    // With the fix the offset stays 7 (window svc07..svc10). The pre-fix bug
    // reset offset to 0 each frame and recomputed offset = 6 (window svc06..
    // svc09), so svc10 scrolled off and svc06 scrolled in. Assert both halves.
    assert!(
        after_up.contains("svc10"),
        "after one Up the bottom row (svc10) must still be visible, cursor \
         climbed within the viewport rather than the viewport jumping:\n{after_up}"
    );
    assert!(
        !after_up.contains("svc06"),
        "after one Up svc06 must NOT be visible (the viewport didn't jump down \
         to re-pin the cursor to the bottom):\n{after_up}"
    );

    // Keep climbing to the top of the window: the viewport holds until the cursor
    // reaches svc07, then scrolls. Verify the window only starts moving once the
    // cursor hits the top row.
    for _ in 0..2 {
        update(&mut state, key(KeyCode::Up));
    }
    let climbing = render_with_scroll(&state, &mut scroll, 40, 8);
    assert_eq!(selected_of(&state), 7);
    assert!(
        climbing.contains("svc07"),
        "cursor at the top of the viewport (svc07); window hasn't scrolled yet"
    );
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
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
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
    let mut app = App {
        state: busx::tui::State::loading_service(),
    };
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
    ObjectNode {
        path: path.to_string(),
        interfaces,
        children,
    }
}

#[test]
fn objects_screen_renders_flat_paths() {
    // `/` and `/org` are pure containers (no interfaces); only the leaves that
    // actually expose an object survive the flat view.
    let tree = obj(
        "/",
        0,
        vec![
            obj("/org", 0, vec![obj("/org/foo", 2, vec![])]),
            obj("/bar", 1, vec![]),
        ],
    );
    let paths = busx::tui::flatten_paths(&tree);
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(
            busx::tui::state::ObjectsScreen {
                service: "org.busx.Test".into(),
                paths,
                selected: 0,
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
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
    assert_eq!(
        busx::tui::flatten_paths(&tree),
        vec!["/org/foo", "/bar", "/empty/x"]
    );
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
    let mut state = State {
        screens: vec![Screen::Objects(objects_screen("org.busx.A"))],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
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
    let mut state = State {
        screens: vec![Screen::Objects(objects_screen("org.busx.A"))],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
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
        screens: vec![busx::tui::Screen::Objects(
            busx::tui::state::ObjectsScreen {
                service: "org.busx.A".into(),
                paths: vec!["/".into(), "/org".into(), "/org/x".into()],
                selected: 2, // "/org/x"
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
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
    let mut state = State {
        screens: vec![Screen::Objects(objects_screen("org.busx.A"))],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
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
        screens: vec![busx::tui::Screen::Interfaces(
            busx::tui::state::InterfacesScreen {
                service: "org.busx.Test".into(),
                object: "/org/busx/Test".into(),
                names: vec!["org.busx.Test".into()],
                node: None,
                selected: 0,
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 44, 7));
}

fn introspect_node(xml: &str) -> zbus_xml::Node<'static> {
    zbus_xml::Node::from_reader(xml.as_bytes()).expect("valid introspection XML")
}

#[test]
fn interfaces_loaded_hides_standard_by_default() {
    // The standard D-Bus interfaces (Properties/Introspectable/Peer) are
    // filtered out by default; only the "real" interfaces are listed.
    let node = introspect_node(
        "<node>\
         <interface name=\"org.freedesktop.DBus.Peer\"/>\
         <interface name=\"org.freedesktop.DBus.Properties\"/>\
         <interface name=\"org.freedesktop.DBus.Introspectable\"/>\
         <interface name=\"org.busx.A\"/>\
         <interface name=\"org.busx.B\"/>\
         </node>",
    );
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(
            busx::tui::state::InterfacesScreen {
                service: "org.busx.Test".into(),
                object: "/o".into(),
                names: vec![],
                node: None,
                selected: 0,
                loading: true,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(
        &mut state,
        Msg::InterfacesLoaded("org.busx.Test".into(), "/o".into(), Ok(node)),
    );
    assert!(effect.is_none(), "two interfaces ⇒ no auto-skip");
    match state.top() {
        Screen::Interfaces(i) => {
            assert!(!i.loading);
            assert_eq!(
                i.names,
                vec!["org.busx.A".to_string(), "org.busx.B".to_string()]
            );
            // The full node (incl. standard interfaces) is still cached, so
            // drilling into a shown interface finds its members.
            assert!(i.node.is_some(), "node cached for drilling in");
        }
        _ => panic!("still on Interfaces"),
    }
}

#[test]
fn interfaces_loaded_shows_standard_with_flag() {
    // `--show-standard-interfaces` disables the filter: every interface is
    // listed, including the three standard ones.
    let node = introspect_node(
        "<node>\
         <interface name=\"org.freedesktop.DBus.Peer\"/>\
         <interface name=\"org.freedesktop.DBus.Properties\"/>\
         <interface name=\"org.freedesktop.DBus.Introspectable\"/>\
         <interface name=\"org.busx.A\"/>\
         </node>",
    );
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(
            busx::tui::state::InterfacesScreen {
                service: "org.busx.Test".into(),
                object: "/o".into(),
                names: vec![],
                node: None,
                selected: 0,
                loading: true,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: true,
    };
    update(
        &mut state,
        Msg::InterfacesLoaded("org.busx.Test".into(), "/o".into(), Ok(node)),
    );
    match state.top() {
        Screen::Interfaces(i) => assert_eq!(
            i.names,
            vec![
                "org.freedesktop.DBus.Peer".to_string(),
                "org.freedesktop.DBus.Properties".to_string(),
                "org.freedesktop.DBus.Introspectable".to_string(),
                "org.busx.A".to_string(),
            ]
        ),
        _ => panic!("still on Interfaces"),
    }
}

#[test]
fn interfaces_loaded_single_interface_auto_skips() {
    let node = introspect_node(
        "<node><interface name=\"org.busx.Test\">\
         <property name=\"X\" type=\"s\" access=\"read\"/>\
         </interface></node>",
    );
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(
            busx::tui::state::InterfacesScreen {
                service: "org.busx.Test".into(),
                object: "/o".into(),
                names: vec![],
                node: None,
                selected: 0,
                loading: true,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
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

#[test]
fn interfaces_loaded_propertyless_interface_skips_getall() {
    // A single interface with NO properties still auto-skips to the Interface
    // screen, but does NOT request GetAll — pointless for a property-less
    // interface, and some objects' GetAll rejects such interfaces. So no Effect.
    let node = introspect_node("<node><interface name=\"org.busx.Test\"/></node>");
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(
            busx::tui::state::InterfacesScreen {
                service: "org.busx.Test".into(),
                object: "/o".into(),
                names: vec![],
                node: None,
                selected: 0,
                loading: true,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(
        &mut state,
        Msg::InterfacesLoaded("org.busx.Test".into(), "/o".into(), Ok(node)),
    );
    assert!(
        effect.is_none(),
        "property-less interface ⇒ no GetAll (no FetchProperties): {effect:?}"
    );
    match state.top() {
        Screen::Interface(i) => {
            assert_eq!(i.interface, "org.busx.Test");
            assert!(!i.loading, "no fetch in flight ⇒ not loading");
            assert!(i.properties.is_empty());
        }
        _ => panic!("auto-skip still lands on Interface"),
    }
}

use busx::tui::state::InterfaceFocus;

#[test]
fn interface_screen_renders_three_columns() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(
            busx::tui::state::InterfaceScreen {
                service: "org.busx.Test".into(),
                object: "/org/busx/Test".into(),
                interface: "org.busx.Test".into(),
                methods: vec![method("BumpVolume", ""), method("Join", "as")],
                properties: vec![
                    ("volume".into(), "d".into(), "readwrite".into()),
                    ("name".into(), "s".into(), "read".into()),
                ],
                signals: vec![],
                prop_values: vec![
                    ("volume".into(), "0.5".into()),
                    ("name".into(), r#""busx-test""#.into()),
                ],
                focus: InterfaceFocus::Properties,
                in_buttons: false,
                button_selected: 0,
                selected: [0, 1, 0],
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 60, 16));
}

#[test]
fn properties_loaded_fills_pretty_values() {
    use zvariant::Value;
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(
            busx::tui::state::InterfaceScreen {
                service: "s".into(),
                object: "/o".into(),
                interface: "i".into(),
                methods: vec![],
                properties: vec![("volume".into(), "d".into(), "readwrite".into())],
                signals: vec![],
                prop_values: vec![],
                focus: Default::default(),
                in_buttons: false,
                button_selected: 0,
                selected: [0, 0, 0],
                loading: true,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let vals = vec![("volume".into(), Value::F64(0.5).try_to_owned().unwrap())];
    let effect = update(&mut state, Msg::PropertiesLoaded(Ok(vals)));
    assert!(effect.is_none(), "PropertiesLoaded requests no fetch");
    match state.top() {
        Screen::Interface(i) => {
            assert!(!i.loading);
            assert_eq!(
                i.prop_values,
                vec![("volume".to_string(), "0.5".to_string())]
            );
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
        in_buttons: false,
        button_selected: 0,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    }
}

#[test]
fn interface_screen_shows_getall_error_scoped_to_properties() {
    // GetAll failed for this interface (some objects' GetAll rejects interfaces
    // they don't track — e.g. the standard org.freedesktop.DBus.* ones). The
    // error must NOT blank the screen: methods + signals stay visible and the
    // error shows only in the properties column.
    let mut screen = interface_screen();
    screen.error = Some("org.freedesktop.DBus.Error.InvalidArgs: 无此接口\"i\"".into());
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let rendered = render_to_string(&state, 64, 16);
    assert!(rendered.contains("m1"), "methods still visible: {rendered}");
    assert!(
        rendered.contains("sig1"),
        "signals still visible: {rendered}"
    );
    assert!(
        rendered.contains("properties (unavailable)"),
        "error scoped to the properties column: {rendered}"
    );
    insta::assert_snapshot!(rendered);
}

/// A `MethodMember` with no per-arg detail (Task 2 fills `args`).
fn method(name: &str, signature: &str) -> busx::tui::state::MethodMember {
    busx::tui::state::MethodMember {
        name: name.into(),
        signature: signature.into(),
        args: vec![],
    }
}

/// A `MethodMember` whose `args` carry per-IN-arg (name, signature) pairs — the
/// source of the call Detail form's input fields. The concatenated `signature`
/// is derived from the args.
fn method_with_args(name: &str, args: &[(&str, &str)]) -> busx::tui::state::MethodMember {
    let signature = args.iter().map(|(_, s)| *s).collect::<String>();
    busx::tui::state::MethodMember {
        name: name.into(),
        signature,
        args: args
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect(),
    }
}

/// Tab cycles the three member columns (Methods→Properties→Signals→Methods)
/// and leaves `in_buttons == false` throughout — the button bar is NOT part of
/// Tab's ring (you drill into it with `Enter`, not `Tab`). Shift+Tab (BackTab)
/// cycles the same ring in reverse.
#[test]
fn interface_tab_cycles_columns() {
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    // Start on the Methods column, not in the button bar.
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
    assert!(!top_in_buttons(&state));
    // Tab cycles forward: Methods → Properties → Signals → Methods.
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Properties);
    assert!(!top_in_buttons(&state), "Tab never enters the button bar");
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    assert!(!top_in_buttons(&state));
    update(&mut state, key(KeyCode::Tab));
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
    assert!(
        !top_in_buttons(&state),
        "Tab wraps Methods→Properties→Signals→Methods"
    );
    // Shift+Tab (BackTab) cycles backward: Methods → Signals → Properties → Methods.
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    assert!(!top_in_buttons(&state));
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Properties);
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
}

/// Tab from inside the button bar leaves the button bar first (`in_buttons =
/// false`) and THEN cycles the column — a single Tab never stays in the buttons.
#[test]
fn interface_tab_leaves_buttons_before_cycling() {
    let mut screen = interface_screen();
    screen.in_buttons = true;
    screen.focus = InterfaceFocus::Methods;
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    assert!(top_in_buttons(&state));
    update(&mut state, key(KeyCode::Tab));
    assert!(!top_in_buttons(&state), "Tab leaves the button bar");
    assert_eq!(
        state.top_focus(),
        InterfaceFocus::Properties,
        "and cycles the column forward"
    );
}

#[test]
fn interface_backtab_cycles_columns() {
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    // Shift+Tab (BackTab) cycles the column Methods→Signals→Properties→Methods
    // (reverse of Tab). Three presses return to Methods.
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Properties);
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
}

#[test]
fn interface_arrows_move_within_focused_column() {
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    // Methods focus, two methods, starts at 0.
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.top_selected(), [1, 0, 0]);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.top_selected(), [1, 0, 0], "clamped at last method");
    // BackTab once to signals (1 signal), Down clamps.
    update(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.top_focus(), InterfaceFocus::Signals);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.top_selected(), [1, 0, 0]);
    update(&mut state, key(KeyCode::Up)); // no-op above 0
    assert_eq!(state.top_selected(), [1, 0, 0]);
}

/// `in_buttons` of the top Interface screen (test convenience, mirrors
/// `top_focus` / `top_selected`).
fn top_in_buttons(state: &busx::tui::State) -> bool {
    match state.top() {
        Screen::Interface(i) => i.in_buttons,
        _ => false,
    }
}

/// Enter from a member column drills INTO the button bar (`in_buttons = true`)
/// without firing anything; a second Enter then fires the selected button.
/// Esc inside the button bar backs out (`in_buttons = false`, screen NOT
/// popped); a second Esc from a column pops the screen.
#[test]
fn interface_enter_drills_then_fires_and_esc_backs_out() {
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    // Start on the Methods column, not in the button bar.
    assert_eq!(state.top_focus(), InterfaceFocus::Methods);
    assert!(!top_in_buttons(&state));
    assert_eq!(state.screens.len(), 1);

    // Enter from the column → drill into the button bar (no Detail pushed).
    let effect = update(&mut state, key(KeyCode::Enter));
    assert!(effect.is_none(), "drill-Enter fires nothing");
    assert!(top_in_buttons(&state), "Enter set in_buttons = true");
    assert_eq!(state.screens.len(), 1, "drill-Enter does not push a screen");

    // Esc inside the button bar → back out, screen NOT popped.
    update(&mut state, key(KeyCode::Esc));
    assert!(!top_in_buttons(&state), "Esc backed out of the button bar");
    assert_eq!(
        state.screens.len(),
        1,
        "Esc from the button bar does not pop"
    );

    // Re-enter the button bar, then a second Enter fires `Call`. m1 has no
    // IN-args, so the fire auto-skips the Detail form and pushes a Result.
    update(&mut state, key(KeyCode::Enter)); // drill in
    assert!(top_in_buttons(&state));
    let effect = update(&mut state, key(KeyCode::Enter)); // fire Call (m1, 0 inputs)
    assert!(effect.is_some(), "fire-Enter returns the Call effect");
    assert_eq!(state.screens.len(), 2, "fire-Enter pushed a Result screen");
    assert!(matches!(state.top(), Screen::Result(_)));
}

/// Esc from a member column (not in the button bar) pops the Interface screen.
#[test]
fn interface_esc_from_column_pops_screen() {
    let mut state = busx::tui::State {
        screens: vec![
            Screen::Objects(objects_screen("s")),
            Screen::Interface(interface_screen()),
        ],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    assert!(!top_in_buttons(&state));
    assert_eq!(state.screens.len(), 2);
    update(&mut state, key(KeyCode::Esc));
    assert_eq!(
        state.screens.len(),
        1,
        "Esc from a column pops the Interface screen"
    );
    assert!(matches!(state.top(), Screen::Objects(_)));
}

/// When `in_buttons`, ↑↓ move `button_selected` (clamped to the focused
/// column's button list); when not `in_buttons`, ↑↓ move the column's `selected`
/// (and leave `button_selected` untouched). This pins the split behavior of the
/// two focus regions for the same arrow key.
#[test]
fn interface_arrows_in_buttons_move_button_selected() {
    let mut screen = interface_screen();
    // Properties column has three buttons: Get/Set/Listen.
    screen.focus = InterfaceFocus::Properties;
    screen.in_buttons = true;
    screen.button_selected = 0;
    screen.selected = [0, 0, 0];
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };

    // In the button bar, Down moves button_selected (0→1→2, clamped at 2).
    update(&mut state, key(KeyCode::Down));
    assert_eq!(button_selected(&state), 1);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(button_selected(&state), 2);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(button_selected(&state), 2, "clamped at last button");
    // The column's property selection is untouched while in the buttons.
    assert_eq!(state.top_selected(), [0, 0, 0]);
    // Up moves button_selected back down (2→1→0, clamped at 0).
    update(&mut state, key(KeyCode::Up));
    assert_eq!(button_selected(&state), 1);
    update(&mut state, key(KeyCode::Up));
    assert_eq!(button_selected(&state), 0);
    update(&mut state, key(KeyCode::Up));
    assert_eq!(button_selected(&state), 0, "clamped above 0");
}

/// `button_selected` of the top Interface screen (test convenience).
fn button_selected(state: &busx::tui::State) -> usize {
    match state.top() {
        Screen::Interface(i) => i.button_selected,
        _ => 0,
    }
}

#[test]
fn interface_r_requests_property_refresh() {
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
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
        Screen::Interface(i) => {
            assert!(i.loading, "r marks the screen loading until values arrive")
        }
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
    let mut app = App {
        state: busx::tui::State::loading_service(),
    };
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
fn interface_button_enter_auto_fires_call() {
    // Methods column, already in the button bar (Enter drilled in earlier),
    // button_selected on `Call`. m1 has signature "u" but no IN-args → a 0-input
    // Call, so Enter skips the Detail form and fires straight to the Result.
    let mut screen = interface_screen();
    screen.focus = InterfaceFocus::Methods;
    screen.in_buttons = true;
    screen.button_selected = 0;
    screen.selected = [0, 0, 0]; // m1 (signature "u", no IN-args)
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CallMethod {
            method,
            signature,
            args,
            ..
        }) => {
            assert_eq!(method, "m1");
            assert_eq!(signature, "u");
            assert!(args.is_empty(), "m1 has no IN-args → empty args");
        }
        other => panic!("button Enter should fire CallMethod, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "i.m1");
        }
        _ => panic!("Enter should push a Result screen"),
    }
}

#[test]
fn interface_button_enter_auto_fires_get() {
    // Properties column, already in the button bar, `Get` button (index 0) on p1.
    // Get is a 0-input action → Enter skips the Detail form and fires straight to
    // the Result, returning `Effect::GetProperty` immediately.
    let mut screen = interface_screen();
    screen.focus = InterfaceFocus::Properties;
    screen.in_buttons = true;
    screen.button_selected = 0;
    screen.selected = [0, 0, 0]; // p1
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::GetProperty { property, .. }) => assert_eq!(property, "p1"),
        other => panic!("button Enter should fire GetProperty, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "i.p1");
        }
        _ => panic!("Enter should push a Result screen"),
    }
}

#[test]
fn interface_button_enter_pushes_set_detail() {
    // Properties column, already in the button bar, `Set` button (index 1) on p1.
    let mut screen = interface_screen();
    screen.focus = InterfaceFocus::Properties;
    screen.in_buttons = true;
    screen.button_selected = 1; // Set
    screen.selected = [0, 0, 0];
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => match &d.kind {
            busx::tui::state::ActionKind::Set {
                property,
                signature,
            } => {
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
    // Methods column with a method selected, focus in the button bar
    // (`in_buttons = true`) → the right panel shows the buttons with `Call`
    // highlighted.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(
            busx::tui::state::InterfaceScreen {
                service: "org.busx.Test".into(),
                object: "/org/busx/Test".into(),
                interface: "org.busx.Test".into(),
                methods: vec![method("Ping", ""), method("Echo", "ss")],
                properties: vec![("Name".into(), "s".into(), "read".into())],
                signals: vec![],
                prop_values: vec![],
                focus: InterfaceFocus::Methods,
                in_buttons: true,
                button_selected: 0,
                selected: [0, 0, 0],
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 64, 16));
}

// --- Phase 3 Task 2: method-call Detail form + Result ---

use busx::tui::state::{
    ActionKind, ActionResult, DetailFocus, DetailScreen, ListenTarget, ResultScreen,
};

/// An Interface screen focused on the button bar, with `button_selected` on the
/// given button index; `selected[0]` points at `methods[idx]`.
fn interface_on_button(
    methods: Vec<busx::tui::state::MethodMember>,
    button: usize,
) -> busx::tui::State {
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "i".into(),
        methods,
        properties: vec![],
        signals: vec![],
        prop_values: vec![],
        focus: InterfaceFocus::Methods,
        in_buttons: true,
        button_selected: button,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    busx::tui::State {
        screens: vec![screen],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    }
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
fn zero_arg_method_auto_fires_to_result() {
    // A method with no IN-args → a 0-input Call: Enter skips the Detail form and
    // fires straight to the Result, returning `Effect::CallMethod` (empty args).
    let state = interface_on_button(vec![method_with_args("Ping", &[])], 0);
    let mut state = state;
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CallMethod {
            method,
            signature,
            args,
            ..
        }) => {
            assert_eq!(method, "Ping");
            assert_eq!(signature, "", "zero-arg method has empty signature");
            assert!(args.is_empty(), "zero-arg call sends no args");
        }
        other => panic!("expected CallMethod, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "i.Ping");
        }
        _ => panic!("Enter should push a Result screen"),
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
            assert_eq!(
                d.focus,
                DetailFocus::Field,
                "still field-focused while typing"
            );
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
    let mut state =
        interface_on_button(vec![method_with_args("Add", &[("a", "u"), ("b", "u")])], 0);
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
fn detail_backtab_cycles_fields_in_reverse() {
    let mut state =
        interface_on_button(vec![method_with_args("Add", &[("a", "u"), ("b", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the Detail (2 inputs), focus Field0
    // Shift+Tab (BackTab) reverse-cycles: Field0 → Trigger → Field1 → Field0.
    let foci = [
        (DetailFocus::Trigger, 0),
        (DetailFocus::Field, 1),
        (DetailFocus::Field, 0),
    ];
    for (want_focus, want_field) in foci {
        update(&mut state, key(KeyCode::BackTab));
        match state.top() {
            Screen::Detail(d) => {
                assert_eq!(d.focus, want_focus, "backtab reverse cycle");
                assert_eq!(d.field_selected, want_field);
            }
            _ => panic!(),
        }
    }
}

#[test]
fn detail_arrows_move_field_selection() {
    let mut state =
        interface_on_button(vec![method_with_args("Add", &[("a", "u"), ("b", "u")])], 0);
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
        Some(Effect::CallMethod {
            service,
            object,
            iface,
            method,
            signature,
            args,
        }) => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "i");
            assert_eq!(method, "Add");
            assert_eq!(signature, "u");
            assert_eq!(
                args,
                vec!["42".to_string()],
                "field values flow as call args"
            );
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
fn zero_arg_call_button_fires_call_with_empty_args() {
    // A zero-arg method's `Call` button is a 0-input action → Enter fires
    // straight to the Result and requests `Effect::CallMethod` (empty args).
    let mut state = interface_on_button(vec![method_with_args("Ping", &[])], 0);
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CallMethod {
            method,
            signature,
            args,
            ..
        }) => {
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
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(
        &mut state,
        Msg::ActionResult(Ok(ActionResult::Call(vec!["7".into()]))),
    );
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
    // The 1-arg call Detail, with the field focused: the field row + `[Trigger]`.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Detail(DetailScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            kind: ActionKind::Call {
                method: "Add".into(),
                signature: "u".into(),
            },
            inputs: vec!["42".into()],
            field_labels: vec!["n  u".into()],
            field_selected: 0,
            focus: DetailFocus::Field,
            loading: false,
            error: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

#[test]
fn call_detail_aligns_arg_labels() {
    // Multi-arg call: the label column is padded to the widest arg so the
    // value (input) column lines up across rows regardless of name/signature
    // width.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Detail(DetailScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            kind: ActionKind::Call {
                method: "Move".into(),
                signature: "us".into(),
            },
            inputs: vec!["3".into(), "x".into()],
            field_labels: vec!["count  u".into(), "name  s".into()],
            field_selected: 0,
            focus: DetailFocus::Field,
            loading: false,
            error: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
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
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

// --- Phase 3 Task 3: property get/set Detail + Result ---

/// An Interface screen whose Properties column has one property (name, sig,
/// access) and is focused on the button bar with `button_selected` on the given
/// action (`Get`=0 / `Set`=1).
fn interface_on_prop_button(button: usize, sig: &str) -> busx::tui::State {
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "i".into(),
        methods: vec![],
        properties: vec![("p1".into(), sig.into(), "readwrite".into())],
        signals: vec![],
        prop_values: vec![],
        focus: InterfaceFocus::Properties,
        in_buttons: true,
        button_selected: button,
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    busx::tui::State {
        screens: vec![screen],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    }
}

#[test]
fn get_button_auto_fires_to_result() {
    // `Get` on p1 is a 0-input action → Enter skips the Detail form and fires
    // straight to the Result, returning `Effect::GetProperty` immediately.
    let mut state = interface_on_prop_button(0, "d");
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::GetProperty { property, .. }) => assert_eq!(property, "p1"),
        other => panic!("button Enter should fire GetProperty, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "i.p1");
        }
        _ => panic!("Enter should push a Result screen"),
    }
}

#[test]
fn get_button_fires_result_and_requests_get() {
    let mut state = interface_on_prop_button(0, "d");
    // 0 inputs → Enter fires straight to the Result (no Detail form).
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::GetProperty {
            service,
            object,
            iface,
            property,
        }) => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "i");
            assert_eq!(property, "p1");
        }
        other => panic!("button Enter should request GetProperty, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading);
            assert_eq!(r.title, "i.p1");
        }
        _ => panic!("button Enter pushed a Result screen"),
    }
    // The result payload populates the Result screen.
    update(
        &mut state,
        Msg::ActionResult(Ok(ActionResult::Get("0.5".into()))),
    );
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
    // `Set` on p1 (signature "s") → a Set Detail with one input, label "s".
    let mut state = interface_on_prop_button(1, "s");
    update(&mut state, key(KeyCode::Enter));
    match state.top() {
        Screen::Detail(d) => {
            match &d.kind {
                ActionKind::Set {
                    property,
                    signature,
                } => {
                    assert_eq!(property, "p1");
                    assert_eq!(signature, "s");
                }
                other => panic!("expected Set, got {other:?}"),
            }
            assert_eq!(d.inputs.len(), 1, "Set → one input field");
            assert_eq!(
                d.field_labels,
                vec!["s".to_string()],
                "label is the signature"
            );
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
        Some(Effect::SetProperty {
            service,
            object,
            iface,
            property,
            signature,
            value,
        }) => {
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
            assert_eq!(r.title, "i.p1");
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
            kind: ActionKind::Set {
                property: "p1".into(),
                signature: "s".into(),
            },
            inputs: vec!["hi".into()],
            field_labels: vec!["s".into()],
            field_selected: 0,
            focus: DetailFocus::Field,
            loading: false,
            error: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

#[test]
fn get_result_renders_value() {
    // A completed Get Result shows the property value.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "i.p1".into(),
            result: Some(ActionResult::Get("0.5".into())),
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 8));
}

// --- Phase 3 Task 4: capstone loop test (full call through run_loop) ---

/// Drive a full method call through `run_loop`: Interface (Methods column) →
/// Enter drills into the button bar → Enter (`Call`) pushes the Detail →
/// type "42" → Tab to the trigger → Enter pushes the Result (loading) +
/// a `CallMethod` Effect (no-op'd by the bus-free handler) → a scripted
/// `ActionResult::Call` reply lands in the Result screen. Snapshots the final
/// Result frame.
#[test]
fn call_action_flows_interface_to_result() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(
            busx::tui::state::InterfaceScreen {
                service: "s".into(),
                object: "/o".into(),
                interface: "i".into(),
                // One method "Add(n: u)" → signature "u", one IN-arg input field.
                methods: vec![method_with_args("Add", &[("n", "u")])],
                properties: vec![],
                signals: vec![],
                prop_values: vec![],
                focus: InterfaceFocus::Methods,
                in_buttons: false,
                button_selected: 0,  // Call
                selected: [0, 0, 0], // Add
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let events = vec![
        key(KeyCode::Enter),     // Methods column → drill into the button bar
        key(KeyCode::Enter),     // Call → push Call Detail (1 input)
        key(KeyCode::Char('4')), // type into the field
        key(KeyCode::Char('2')),
        key(KeyCode::Tab),   // Field → Trigger
        key(KeyCode::Enter), // push Result (loading) + CallMethod (no-op'd)
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

// --- Phase 4 Task 2: signal/property listen — Listen Detail + streaming Result ---

/// An Interface screen whose Signals column has one signal and is focused on the
/// button bar with `button_selected` on `Listen` (the only signal button). Uses a
/// valid D-Bus interface name so the match-rule preview parses cleanly.
fn interface_on_signal_button() -> busx::tui::State {
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "org.busx.Test".into(),
        methods: vec![],
        properties: vec![],
        signals: vec![("Changed".into(), "u".into())],
        prop_values: vec![],
        focus: InterfaceFocus::Signals,
        in_buttons: true,
        button_selected: 0, // Listen
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    busx::tui::State {
        screens: vec![screen],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    }
}

#[test]
fn signal_listen_button_auto_fires_listen() {
    // Signals column, `Listen` button → a 0-input Listen action, so Enter skips
    // the Detail form and fires straight to the Result, returning
    // `Effect::Listen { target: Signal { member: "Changed" } }` immediately. (The
    // match-rule preview is a Detail-form artifact and is no longer shown.)
    let mut state = interface_on_signal_button();
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::Listen { target, .. }) => match target {
            ListenTarget::Signal { member } => assert_eq!(member, "Changed"),
            other => panic!("expected Signal listen, got {other:?}"),
        },
        other => panic!("button Enter should fire Listen, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "listen org.busx.Test.Changed");
        }
        _ => panic!("Enter should push a Result screen"),
    }
}

#[test]
fn property_listen_button_auto_fires_property_listen() {
    // Properties column, `Listen` button (index 2) → a 0-input Listen action, so
    // Enter skips the Detail form and fires straight to the Result, targeting the
    // property (`ListenTarget::Property { property: "volume" }`). The shared
    // PropertiesChanged signal subscription that the old match-rule preview
    // described is now built internally from the target, not shown as a label.
    let screen = busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "org.busx.Test".into(),
        methods: vec![],
        properties: vec![("volume".into(), "d".into(), "readwrite".into())],
        signals: vec![],
        prop_values: vec![],
        focus: InterfaceFocus::Properties,
        in_buttons: true,
        button_selected: 2, // Listen
        selected: [0, 0, 0],
        loading: false,
        error: None,
    };
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(screen)],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::Listen { target, .. }) => match target {
            ListenTarget::Property { property } => assert_eq!(property, "volume"),
            other => panic!("expected Property listen, got {other:?}"),
        },
        other => panic!("button Enter should fire Listen, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "listen org.busx.Test.volume");
        }
        _ => panic!("Enter should push a Result screen"),
    }
}

#[test]
fn signal_listen_button_fires_result_and_requests_listen() {
    // From the Signals column's `Listen` button (0 inputs), Enter fires straight
    // to the Result (loading) + `Effect::Listen { target: Signal }`.
    let mut state = interface_on_signal_button();
    // 0 inputs → Enter fires straight to the Result (no Detail form).
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::Listen {
            service,
            object,
            iface,
            target,
        }) => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "org.busx.Test");
            match target {
                ListenTarget::Signal { member } => assert_eq!(member, "Changed"),
                other => panic!("expected Signal listen, got {other:?}"),
            }
        }
        other => panic!("button Enter should request Listen, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(
                r.loading,
                "Result starts loading until ListenStarted arrives"
            );
            assert_eq!(r.title, "listen org.busx.Test.Changed");
            assert!(r.messages.is_empty());
            assert!(r.cancel.is_none(), "cancel arrives with ListenStarted");
        }
        _ => panic!("button Enter pushed a Result screen"),
    }
}

#[test]
fn listen_started_stores_cancel_and_clears_loading() {
    // ListenStarted carries the cancel sender onto the Result and clears loading.
    let mut state = interface_on_signal_button();
    update(&mut state, key(KeyCode::Enter)); // push Listen Detail
    update(&mut state, key(KeyCode::Tab)); // → trigger
    update(&mut state, key(KeyCode::Enter)); // push Result + Effect.Listen (no-op'd)
    let (cancel_tx, _cancel_rx) = futures::channel::oneshot::channel::<()>();
    update(&mut state, Msg::ListenStarted(cancel_tx));
    match state.top() {
        Screen::Result(r) => {
            assert!(r.cancel.is_some(), "cancel sender stored on the Result");
            assert!(!r.loading, "ListenStarted cleared loading");
        }
        _ => panic!("still on Result"),
    }
}

#[test]
fn listen_messages_append_and_esc_stops() {
    // Two ListenMessages append to the Result; Esc pops it and drops the cancel
    // sender, so the matching receiver sees Canceled (the listen task exits).
    let mut state = interface_on_signal_button();
    update(&mut state, key(KeyCode::Enter)); // push Listen Detail
    update(&mut state, key(KeyCode::Tab)); // → trigger
    update(&mut state, key(KeyCode::Enter)); // push Result + Effect.Listen (no-op'd)
    // Arm the listen with a real cancel pair we hold the receiver of.
    let (cancel_tx, cancel_rx) = futures::channel::oneshot::channel::<()>();
    update(&mut state, Msg::ListenStarted(cancel_tx));
    update(
        &mut state,
        Msg::ListenMessage("signal  sender=:1.1\n  …block1\n".into()),
    );
    update(
        &mut state,
        Msg::ListenMessage("signal  sender=:1.2\n  …block2\n".into()),
    );
    match state.top() {
        Screen::Result(r) => assert_eq!(r.messages.len(), 2, "two message blocks appended"),
        _ => panic!("still on Result"),
    }
    // Esc pops the Result → cancel sender drops → receiver errors Canceled.
    update(&mut state, key(KeyCode::Esc));
    assert!(
        !matches!(state.top(), Screen::Result(_)),
        "Esc popped the Result"
    );
    use futures::FutureExt;
    assert!(
        matches!(
            cancel_rx.now_or_never(),
            Some(Err(futures::channel::oneshot::Canceled))
        ),
        "dropping the Result dropped the cancel sender → Canceled",
    );
}

#[test]
fn listen_result_renders_streaming_messages() {
    // A streaming Result with two message blocks renders them joined.
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "listen i.Changed".into(),
            result: None,
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![
                "signal  sender=:1.1\n  interface=i  member=Changed  serial=7\n  3".into(),
                "signal  sender=:1.1\n  interface=i  member=Changed  serial=9\n  4".into(),
            ],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 52, 10));
}

#[test]
fn method_listen_button_auto_fires_method_listen() {
    // Methods column, `Listen` button → a 0-input Listen action targeting a
    // Method, so Enter skips the Detail form and fires straight to the Result,
    // requesting `Effect::Listen { target: Method { member: "Ping" } }` (no real
    // spawn — the no-op `|_| {}` handler is used, so nothing touches the bus
    // here). The method_call match-rule preview is a Detail-form artifact and is
    // no longer shown.
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "org.busx.Test".into(),
        methods: vec![method("Ping", "")],
        properties: vec![],
        signals: vec![],
        prop_values: vec![],
        focus: InterfaceFocus::Methods,
        in_buttons: true,
        button_selected: 1, // Listen
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    let mut state = busx::tui::State {
        screens: vec![screen],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    // 0 inputs → Enter fires straight to the Result (no Detail form).
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::Listen {
            target: ListenTarget::Method { member },
            ..
        }) => {
            assert_eq!(member, "Ping");
        }
        other => panic!("expected Method Listen, got {other:?}"),
    }
    match state.top() {
        Screen::Result(r) => {
            assert!(r.loading, "Result starts loading");
            assert_eq!(r.title, "listen org.busx.Test.Ping");
        }
        _ => panic!("Enter should push a Result screen"),
    }
}

// --- Phase 4 Task 4: listen capstone loop test (full signal listen through run_loop) ---

/// Drive a full signal listen through `run_loop`: Interface (Signals column) →
/// Enter drills into the button bar → Enter (`Listen`) is a 0-input action, so
/// it skips the Detail form and fires straight to the streaming Result
/// (loading) + `Effect::Listen` (no-op'd by the bus-free handler) → a scripted
/// `ListenStarted` arms the cancel + clears loading → two `ListenMessage`s
/// append message blocks → Esc pops the Result, dropping the cancel sender, so
/// the matching receiver sees `Canceled` (the listen task would exit).
/// Snapshots the streaming Result frame (two message blocks) *before* the Esc.
///
/// Focus sequence to reach the signal's `Listen` button: start on the Signals
/// column (`focus == Signals`, one signal `Changed`), then `Enter` drills into
/// the button bar (Signals offers only `Listen`, so `button_selected` 0 is
/// already on it), then a second `Enter` fires it.
#[test]
fn listen_action_flows_interface_to_streaming_result() {
    // Start on the Signals column (not yet on the button bar) so the first Enter
    // exercises the column→button-bar drill, just as a real user would.
    let screen = busx::tui::Screen::Interface(busx::tui::state::InterfaceScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "org.busx.Test".into(),
        methods: vec![],
        properties: vec![],
        signals: vec![("Changed".into(), "u".into())],
        prop_values: vec![],
        focus: InterfaceFocus::Signals,
        in_buttons: false,
        button_selected: 0, // Listen (Signals offers only one button)
        selected: [0, 0, 0],
        loading: false,
        error: None,
    });
    let state = busx::tui::State {
        screens: vec![screen],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };

    // Arm a real cancel pair we keep the receiver of, so the Esc-drop assertion
    // can observe the sender going away. The ListenStarted message carries the
    // sender onto the Result; the receiver stays here in the test.
    let (cancel_tx, cancel_rx) = futures::channel::oneshot::channel::<()>();

    // Events up to (but excluding) the Esc: the streaming Result is fully armed
    // with two message blocks when this list is exhausted.
    let events = vec![
        key(KeyCode::Enter),           // Signals column → drill into the button bar
        key(KeyCode::Enter), // Listen (0 inputs) → auto-fire: push Result (loading) + Effect::Listen (no-op'd)
        Msg::ListenStarted(cancel_tx), // store cancel, clear loading
        Msg::ListenMessage("signal  sender=:1.1\n  …block1\n".into()),
        Msg::ListenMessage("signal  sender=:1.2\n  …block2\n".into()),
    ];
    let mut app = App { state };
    let backend = TestBackend::new(40, 10);
    let mut term = Terminal::new(backend).unwrap();
    app.run_loop(&mut term, events.into_iter(), |_| {}).unwrap();

    // The streaming Result is armed: two messages, not loading, cancel stored.
    match app.state.top() {
        Screen::Result(r) => {
            assert_eq!(r.messages.len(), 2, "two message blocks streamed in");
            assert!(!r.loading, "ListenStarted cleared loading");
            assert!(r.cancel.is_some(), "cancel sender stored on the Result");
        }
        _ => panic!("should land on the streaming Result before Esc"),
    }
    // Snapshot the streaming Result frame (two message blocks), BEFORE Esc.
    insta::assert_snapshot!(format!("{}", term.backend()));

    // Esc through a second run_loop pass: pops the Result → drops the cancel
    // sender → the receiver we kept yields Canceled (proves Esc-stop).
    app.run_loop(&mut term, std::iter::once(key(KeyCode::Esc)), |_| {})
        .unwrap();
    assert!(
        !matches!(app.state.top(), Screen::Result(_)),
        "Esc popped the streaming Result",
    );
    use futures::FutureExt;
    assert!(
        matches!(
            cancel_rx.now_or_never(),
            Some(Err(futures::channel::oneshot::Canceled))
        ),
        "popping the Result dropped the cancel sender → Canceled (listen task exits)",
    );
}

/// A streaming-listen Result whose BecomeMonitor (or match-rule setup) was
/// refused renders the error rather than a blank/loading body — and the keyhint
/// reflects the live listen (Esc back/stop) until the error clears it.
#[test]
fn listen_refused_renders_error_on_result() {
    // An armed streaming Result that then receives a refused error (the
    // `Msg::ActionResult(Err(..))` path BecomeMonitor refuses emit). The cancel
    // sender is still present, so the keyhint still reads "back/stop".
    let (cancel_tx, _cancel_rx) = futures::channel::oneshot::channel::<()>();
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "listen org.busx.Test.Ping".into(),
            result: None,
            error: None,
            loading: true,
            scroll: 0,
            messages: vec![],
            cancel: Some(cancel_tx),
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    update(
        &mut state,
        Msg::ActionResult(Err("BecomeMonitor refused: ...".into())),
    );
    match state.top() {
        Screen::Result(r) => {
            assert!(!r.loading, "the error cleared loading");
            assert_eq!(r.error.as_deref(), Some("BecomeMonitor refused: ..."));
            assert!(r.result.is_none());
        }
        _ => panic!("still on Result"),
    }
    insta::assert_snapshot!(render_to_string(&state, 44, 6));
}

// --- Phase 5 Task 2: copy-as popup + clipboard ---

use busx::tui::copy::{CopyOp, Tool, generate};

/// A call Detail for `Add(n: u)` with "42" typed, so `c` reflects the typed value.
fn call_detail_with_input() -> busx::tui::State {
    busx::tui::State {
        screens: vec![busx::tui::Screen::Detail(DetailScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            kind: ActionKind::Call {
                method: "Add".into(),
                signature: "u".into(),
            },
            inputs: vec!["42".into()],
            field_labels: vec!["n  u".into()],
            field_selected: 0,
            focus: DetailFocus::Field,
            loading: false,
            error: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    }
}

#[test]
fn c_opens_copy_as_popup_with_four_tool_commands() {
    // On a call Detail, `c` opens the popup carrying the Call CopyOp (with the
    // typed "42" arg) and a precomputed command per tool. busctl is Some (1:1);
    // every tool can express a basic-type call here.
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    let popup = state.popup.as_ref().expect("c opened the popup");
    match &popup.op {
        CopyOp::Call {
            service,
            object,
            iface,
            method,
            signature,
            args,
        } => {
            assert_eq!(service, "s");
            assert_eq!(object, "/o");
            assert_eq!(iface, "i");
            assert_eq!(method, "Add");
            assert_eq!(signature, "u");
            assert_eq!(
                args,
                &vec!["42".to_string()],
                "popup op carries the typed value"
            );
        }
        other => panic!("expected Call op, got {other:?}"),
    }
    assert_eq!(
        popup.commands.len(),
        4,
        "one entry per tool (Tool::ALL order)"
    );
    assert_eq!(popup.commands[0].0, Tool::DbusSend);
    assert_eq!(popup.commands[1].0, Tool::Busctl);
    assert_eq!(popup.commands[2].0, Tool::Qdbus);
    assert_eq!(popup.commands[3].0, Tool::Gdbus);
    assert_eq!(popup.selected, 0, "popup opens focused on row 0");
    // busctl is 1:1 and must contain the typed arg "42".
    let busctl_cmd = popup.commands[1]
        .1
        .as_ref()
        .expect("busctl supports a basic call");
    assert!(busctl_cmd.starts_with("busctl --user call"));
    assert!(
        busctl_cmd.contains(" 42"),
        "busctl command reflects the typed arg: {busctl_cmd}"
    );
}

#[test]
fn popup_down_then_enter_copies_selected_command() {
    // From the popup, Down moves to row 1 (busctl); Enter copies that tool's
    // command (Effect::CopyToClipboard) but KEEPS THE POPUP OPEN showing a
    // "copying…" status (the copy result arrives later via ClipboardResult).
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    update(&mut state, key(KeyCode::Down));
    assert_eq!(
        state.popup.as_ref().unwrap().selected,
        1,
        "Down moved to busctl (row 1)"
    );
    let expected = generate(
        &state.popup.as_ref().unwrap().op,
        &Bus::Session,
        Tool::Busctl,
    )
    .unwrap();
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CopyToClipboard(cmd)) => {
            assert_eq!(cmd, expected, "copied the busctl command")
        }
        other => panic!("Enter should copy via CopyToClipboard, got {other:?}"),
    }
    let popup = state
        .popup
        .as_ref()
        .expect("Enter keeps the popup open to show the status");
    assert_eq!(
        popup.status.as_deref(),
        Some("copying…"),
        "Enter set the transient status"
    );

    // The copy result arrives → status flips to "copied".
    update(&mut state, Msg::ClipboardResult(Ok(())));
    assert_eq!(
        state.popup.as_ref().unwrap().status.as_deref(),
        Some("copied"),
        "ClipboardResult(Ok) sets the copied status",
    );

    // A second Enter now dismisses the popup.
    update(&mut state, key(KeyCode::Enter));
    assert!(
        state.popup.is_none(),
        "a second Enter (status shown) dismisses the popup"
    );
}

#[test]
fn popup_up_clamps_at_top() {
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    update(&mut state, key(KeyCode::Up));
    assert_eq!(
        state.popup.as_ref().unwrap().selected,
        0,
        "Up at row 0 stays at 0"
    );
}

#[test]
fn popup_down_clamps_at_bottom() {
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    for _ in 0..10 {
        update(&mut state, key(KeyCode::Down));
    }
    assert_eq!(
        state.popup.as_ref().unwrap().selected,
        3,
        "Down clamps at the last tool (row 3)"
    );
}

#[test]
fn popup_enter_then_clipboard_result_error_shows_error_status() {
    // Enter copies (popup stays, "copying…"); a failed ClipboardResult flips the
    // status to "error: …" (surfaced in the popup, never the TTY).
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    let effect = update(&mut state, key(KeyCode::Enter));
    assert!(matches!(effect, Some(Effect::CopyToClipboard(_))));
    assert_eq!(
        state.popup.as_ref().unwrap().status.as_deref(),
        Some("copying…")
    );

    update(&mut state, Msg::ClipboardResult(Err("no tool".into())));
    assert_eq!(
        state.popup.as_ref().unwrap().status.as_deref(),
        Some("error: no tool"),
        "ClipboardResult(Err) sets an error status",
    );
    // The popup is still open; a second Enter dismisses it.
    assert!(state.popup.is_some());
    update(&mut state, key(KeyCode::Enter));
    assert!(
        state.popup.is_none(),
        "Enter dismisses the popup after an error status"
    );
}

#[test]
fn popup_navigation_locked_after_copy() {
    // Once a copy has happened (status is set), ↑↓ no longer move the selection
    // — the user is reading the result; navigation is locked until dismiss.
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    update(&mut state, key(KeyCode::Enter)); // copy row 0 → status "copying…"
    assert_eq!(state.popup.as_ref().unwrap().selected, 0);
    update(&mut state, key(KeyCode::Down)); // locked: should NOT move
    assert_eq!(
        state.popup.as_ref().unwrap().selected,
        0,
        "Down is locked after a copy"
    );
    update(&mut state, key(KeyCode::Up)); // locked: should NOT move
    assert_eq!(
        state.popup.as_ref().unwrap().selected,
        0,
        "Up is locked after a copy"
    );
}

#[test]
fn clipboard_result_without_popup_is_ignored() {
    // A ClipboardResult arriving with no popup open (shouldn't normally happen —
    // e.g. the popup was Esc'd before the result arrived) is a harmless no-op.
    let mut state = call_detail_with_input();
    assert!(state.popup.is_none());
    update(&mut state, Msg::ClipboardResult(Ok(())));
    assert!(
        state.popup.is_none(),
        "no popup → ClipboardResult is ignored"
    );
}

#[test]
fn popup_esc_closes_without_popping_the_screen() {
    // Esc on the popup closes it but must NOT pop the underlying screen — the
    // popup routing runs before the global Esc handler. The Detail stays on top.
    let mut state = call_detail_with_input();
    let depth_before = state.screens.len();
    update(&mut state, key(KeyCode::Char('c')));
    assert!(state.popup.is_some());
    update(&mut state, key(KeyCode::Esc));
    assert!(state.popup.is_none(), "Esc closed the popup");
    assert_eq!(
        state.screens.len(),
        depth_before,
        "Esc did not pop the screen"
    );
    assert!(
        matches!(state.top(), Screen::Detail(_)),
        "still on the Detail screen"
    );
}

#[test]
fn q_quits_even_with_popup_open() {
    // `q` is checked before popup routing, so it quits regardless of the popup.
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    update(&mut state, key(KeyCode::Char('q')));
    assert!(state.quit, "q quits even with the popup open");
}

#[test]
fn popup_enter_on_unsupported_tool_is_noop() {
    // A Listen op: qdbus can't express it (returns None). Selecting qdbus and
    // pressing Enter is a no-op — the popup stays open and no Effect is emitted.
    let screen = busx::tui::Screen::Detail(DetailScreen {
        service: "s".into(),
        object: "/o".into(),
        interface: "org.busx.Test".into(),
        kind: ActionKind::Listen {
            target: ListenTarget::Signal {
                member: "Changed".into(),
            },
        },
        inputs: vec![],
        field_labels: vec!["type='signal',...".into()],
        field_selected: 0,
        focus: DetailFocus::Field,
        loading: false,
        error: None,
    });
    let mut state = busx::tui::State {
        screens: vec![screen],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    update(&mut state, key(KeyCode::Char('c')));
    // Move to qdbus (row 2).
    update(&mut state, key(KeyCode::Down));
    update(&mut state, key(KeyCode::Down));
    assert_eq!(state.popup.as_ref().unwrap().selected, 2);
    assert!(
        state.popup.as_ref().unwrap().commands[2].1.is_none(),
        "qdbus can't monitor"
    );
    let effect = update(&mut state, key(KeyCode::Enter));
    assert!(
        effect.is_none(),
        "Enter on unsupported tool emits no Effect"
    );
    assert!(
        state.popup.is_some(),
        "popup stays open on an unsupported Enter"
    );
}

#[test]
fn c_on_result_opens_popup_from_stored_op() {
    // A Result whose trigger attached a CopyOp: `c` opens the popup from that op.
    let mut state = interface_on_button(vec![method_with_args("Add", &[("n", "u")])], 0);
    update(&mut state, key(KeyCode::Enter)); // push the call Detail
    update(&mut state, key(KeyCode::Char('4')));
    update(&mut state, key(KeyCode::Tab)); // → trigger
    update(&mut state, key(KeyCode::Enter)); // push Result + attach CopyOp
    match state.top() {
        Screen::Result(r) => assert!(r.op.is_some(), "the trigger attached a CopyOp"),
        _ => panic!("on the Result"),
    }
    update(&mut state, key(KeyCode::Char('c')));
    let popup = state
        .popup
        .as_ref()
        .expect("c opened the popup from the Result's op");
    assert!(matches!(popup.op, CopyOp::Call { .. }));
    // The CopyOp reflects the value typed before the trigger ("4", not "42" — only
    // one digit was typed in this fixture). The busctl command carries it.
    let busctl = popup.commands[1].1.as_ref().unwrap();
    assert!(
        busctl.contains(" 4"),
        "popup op mirrors the value at trigger time: {busctl}"
    );
}

#[test]
fn c_on_result_without_op_is_noop() {
    // A Result created with op: None (a bare literal) → `c` does nothing.
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "bare".into(),
            result: None,
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    update(&mut state, key(KeyCode::Char('c')));
    assert!(state.popup.is_none(), "no op → no popup");
}

#[test]
fn c_on_other_screen_is_noop() {
    // `c` only opens the popup on Detail/Result; on other screens it's inert.
    let mut state = State::service(vec![svc("a", None, None)]);
    update(&mut state, key(KeyCode::Char('c')));
    assert!(
        state.popup.is_none(),
        "c on the Service screen does nothing"
    );
}

#[test]
fn copy_as_popup_renders_over_detail() {
    // The popup open over a call Detail: the four tools, row 0 selected, and a
    // preview area. Snapshot the overlay.
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c')));
    insta::assert_snapshot!(render_to_string(&state, 56, 14));
}

// --- Phase 5 Task 3: copy-as capstone loop + copy-result-text (`y`) ---

/// Drive a method call through `run_loop` to a completed Result, then open the
/// copy-as popup, move to busctl, and Enter to copy. The `run_loop`'s `|_| {}`
/// swallows the `Effect::CopyToClipboard` (so no `ClipboardResult` ever arrives),
/// which means the popup stays open with the transient "copying…" status — a
/// second Enter then dismisses it. This test asserts that dismiss flow and that
/// the top stays Result; the copied-command content is asserted in the sibling
/// `copy_as_capstone_copies_busctl_command` direct-update test.
#[test]
fn copy_as_capstone_loop_closes_popup_over_result() {
    let state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(
            busx::tui::state::InterfaceScreen {
                service: "s".into(),
                object: "/o".into(),
                interface: "i".into(),
                methods: vec![method_with_args("Add", &[("n", "u")])],
                properties: vec![],
                signals: vec![],
                prop_values: vec![],
                focus: InterfaceFocus::Methods,
                in_buttons: false,
                button_selected: 0, // Call
                selected: [0, 0, 0],
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let events = vec![
        key(KeyCode::Enter),     // Methods column → drill into the button bar
        key(KeyCode::Enter),     // Call → push Call Detail (1 input)
        key(KeyCode::Char('4')), // type "4" then "2"
        key(KeyCode::Char('2')),
        key(KeyCode::Tab),   // Field → Trigger
        key(KeyCode::Enter), // push Result (loading) + CallMethod (no-op'd)
        Msg::ActionResult(Ok(ActionResult::Call(vec!["42".into()]))), // scripted reply
        key(KeyCode::Char('c')), // open the copy-as popup over the Result
        key(KeyCode::Down),  // dbus-send (row 0) → busctl (row 1)
        key(KeyCode::Enter), // copy busctl cmd (no-op'd) → popup stays, "copying…"
        key(KeyCode::Enter), // status is set → second Enter dismisses the popup
    ];
    let mut app = App { state };
    let backend = TestBackend::new(56, 14);
    let mut term = Terminal::new(backend).unwrap();
    app.run_loop(&mut term, events.into_iter(), |_| {}).unwrap();
    // The popup closed after the second Enter; the top is still the Result.
    assert!(
        app.state.popup.is_none(),
        "second Enter dismissed the popup"
    );
    assert!(
        matches!(app.state.top(), Screen::Result(_)),
        "still on the Result screen"
    );
    // Snapshot the Result frame after the popup closed (the completed call).
    insta::assert_snapshot!(format!("{}", term.backend()));
}

/// The copy-as capstone's content assertion: build the popup over a Result, then
/// a direct `update` of Enter returns `Effect::CopyToClipboard(s)` where `s` is
/// exactly the busctl command `generate` produces for the stored Call CopyOp.
/// (The `run_loop` test above can't observe the swallowed effect, so this
/// sibling test pins the copied string.)
#[test]
fn copy_as_capstone_copies_busctl_command() {
    // Drive a call to a completed Result carrying a Call CopyOp (Add(n:u) = 42).
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interface(
            busx::tui::state::InterfaceScreen {
                service: "s".into(),
                object: "/o".into(),
                interface: "i".into(),
                methods: vec![method_with_args("Add", &[("n", "u")])],
                properties: vec![],
                signals: vec![],
                prop_values: vec![],
                focus: InterfaceFocus::Methods,
                in_buttons: false,
                button_selected: 0,
                selected: [0, 0, 0],
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    update(&mut state, key(KeyCode::Enter)); // Methods column → drill into the button bar
    update(&mut state, key(KeyCode::Enter)); // Call → push the Call Detail
    update(&mut state, key(KeyCode::Char('4')));
    update(&mut state, key(KeyCode::Char('2')));
    update(&mut state, key(KeyCode::Tab)); // → trigger
    update(&mut state, key(KeyCode::Enter)); // push Result + attach CopyOp
    // Open the popup and move to busctl (row 1).
    update(&mut state, key(KeyCode::Char('c')));
    update(&mut state, key(KeyCode::Down));
    assert_eq!(
        state.popup.as_ref().unwrap().selected,
        1,
        "on busctl (row 1)"
    );
    // The expected busctl command, computed from the same CopyOp the popup holds.
    let expected = generate(
        &state.popup.as_ref().unwrap().op,
        &Bus::Session,
        Tool::Busctl,
    )
    .unwrap();
    assert_eq!(expected, "busctl --user call s /o i Add u 42");
    // Enter copies it via CopyToClipboard and KEEPS the popup open (so the
    // eventual ClipboardResult can show its status).
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CopyToClipboard(cmd)) => {
            assert_eq!(cmd, expected, "copied the busctl command string");
            assert_eq!(cmd, "busctl --user call s /o i Add u 42");
        }
        other => panic!("Enter should copy via CopyToClipboard, got {other:?}"),
    }
    assert!(
        state.popup.is_some(),
        "popup stays open after copying (to show the status)"
    );
    assert_eq!(
        state.popup.as_ref().unwrap().status.as_deref(),
        Some("copying…")
    );
}

/// `y` on a one-shot call Result copies the reply values joined by `\n`.
#[test]
fn y_copies_call_result_text_joined() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "i.Add".into(),
            result: Some(ActionResult::Call(vec!["7".into(), "8".into()])),
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Char('y')));
    match effect {
        Some(Effect::CopyToClipboard(text)) => assert_eq!(text, "7\n8", "values joined by newline"),
        other => panic!("y should copy the result text, got {other:?}"),
    }
}

/// `y` on a Get Result copies the single value; `y` on a Set Result copies "ok".
#[test]
fn y_copies_get_and_set_result_text() {
    let mut get_state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "p1".into(),
            result: Some(ActionResult::Get("0.5".into())),
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    match update(&mut get_state, key(KeyCode::Char('y'))) {
        Some(Effect::CopyToClipboard(text)) => assert_eq!(text, "0.5"),
        other => panic!("y on Get should copy the value, got {other:?}"),
    }

    let mut set_state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "p1".into(),
            result: Some(ActionResult::Set),
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    match update(&mut set_state, key(KeyCode::Char('y'))) {
        Some(Effect::CopyToClipboard(text)) => assert_eq!(text, "ok"),
        other => panic!("y on Set should copy \"ok\", got {other:?}"),
    }
}

/// `y` on a streaming Result copies the message blocks joined by `\n`.
#[test]
fn y_copies_streaming_result_text_joined() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "listen i.Changed".into(),
            result: None,
            error: None,
            loading: false,
            scroll: 0,
            messages: vec![
                "signal  sender=:1.1\n  interface=i  member=Changed  serial=7\n  3".into(),
                "signal  sender=:1.1\n  interface=i  member=Changed  serial=9\n  4".into(),
            ],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Char('y')));
    match effect {
        Some(Effect::CopyToClipboard(text)) => {
            let joined = "signal  sender=:1.1\n  interface=i  member=Changed  serial=7\n  3\n\
                          signal  sender=:1.1\n  interface=i  member=Changed  serial=9\n  4";
            assert_eq!(text, joined, "message blocks joined by newline");
        }
        other => panic!("y should copy the streaming text, got {other:?}"),
    }
}

/// `y` on a Result with no result yet (still loading, no messages) is a no-op.
#[test]
fn y_on_result_without_result_is_noop() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "i.Add".into(),
            result: None,
            error: None,
            loading: true,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Char('y')));
    assert!(effect.is_none(), "no result yet → nothing to copy");
}

/// `y` on a Result showing an error is a no-op (don't copy the error text).
#[test]
fn y_on_result_with_error_is_noop() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "i.Add".into(),
            result: None,
            error: Some("org.freedesktop.DBus.Error.NoReply".into()),
            loading: false,
            scroll: 0,
            messages: vec![],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Char('y')));
    assert!(effect.is_none(), "error showing → don't copy error text");
}

/// `y` does not leak into the Detail input: on a Detail, `y` types into the
/// focused field rather than triggering a copy (Detail has no Result text).
#[test]
fn y_on_detail_edits_input_not_copy() {
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('y')));
    match state.top() {
        Screen::Detail(d) => {
            // tui-input appended 'y' to the existing "42".
            assert_eq!(
                d.inputs[0].value(),
                "42y",
                "y typed into the field, not a copy"
            );
        }
        _ => panic!("still on Detail"),
    }
}

// ---------------------------------------------------------------------------
// `?` help overlay (spec §6). A global keybindings popup: `?` opens it, Esc/`?`
// close it, and while it's open it captures keys (arrow keys don't move the
// selection). Renders on top of the current screen, like the copy-as popup.
// ---------------------------------------------------------------------------

#[test]
fn question_mark_opens_and_closes_help() {
    let mut state = State::service(vec![svc("a", None, None)]);
    assert!(!state.help_open, "help starts closed");
    update(&mut state, key(KeyCode::Char('?')));
    assert!(state.help_open, "? opens the help overlay");
    // Esc closes it.
    update(&mut state, key(KeyCode::Esc));
    assert!(!state.help_open, "Esc closes the help overlay");
    // `?` toggles it back open, then `?` again closes it (toggle semantics).
    update(&mut state, key(KeyCode::Char('?')));
    assert!(state.help_open, "? reopens help");
    update(&mut state, key(KeyCode::Char('?')));
    assert!(!state.help_open, "? closes help (toggle)");
}

#[test]
fn help_overlay_swallows_arrow_keys() {
    // Two services so a Down would normally move selection 0 → 1.
    let mut state = State::service(vec![svc("a", None, None), svc("b", None, None)]);
    // Open help, then try Down — selection must NOT change (help captured it).
    update(&mut state, key(KeyCode::Char('?')));
    assert!(state.help_open);
    update(&mut state, key(KeyCode::Down));
    assert_eq!(selected_of(&state), 0, "Down swallowed while help open");
    // After closing, Down moves normally again.
    update(&mut state, key(KeyCode::Esc));
    update(&mut state, key(KeyCode::Down));
    assert_eq!(
        selected_of(&state),
        1,
        "Down moves selection once help is closed"
    );
}

#[test]
fn help_overlay_renders_over_screen() {
    let mut state = State::service(vec![svc("org.busx.A", None, None)]);
    update(&mut state, key(KeyCode::Char('?')));
    insta::assert_snapshot!(render_to_string(&state, 60, 16));
}

// ---------------------------------------------------------------------------
// Mouse hit-testing + interactions (spec §3). Each test draws to populate
// `click_targets` (render's out-param), then feeds a `Msg::Mouse` whose column
// falls inside the target's recorded Rect. Real app flow: `run_loop` copies the
// out-param into `state.click_targets` after every frame.
// ---------------------------------------------------------------------------

use busx::tui::ClickTarget;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

/// Click `target` by drawing to populate `click_targets`, finding its Rect, and
/// feeding a left-click at the Rect's top-center. Panics if `target` isn't among
/// the recorded click_targets (so a missing widget fails loudly, not silently).
fn click(state: &mut State, target: &ClickTarget, w: u16, h: u16) {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    let mut targets = Vec::new();
    let mut scroll = [0usize; 3];
    term.draw(|f| render(f, state, &mut targets, &mut scroll))
        .unwrap();
    state.click_targets = targets;
    let rect = state
        .click_targets
        .iter()
        .find(|(_, t)| t == target)
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("target {target:?} not in click_targets"));
    update(
        state,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + rect.width.saturating_sub(1) / 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
}

/// Feed a bare scroll Msg (no click_targets lookup needed — scroll doesn't
/// hit-test). Used by the scroll test.
fn scroll_msg(kind: MouseEventKind) -> Msg {
    Msg::Mouse(MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn mouse_click_selects_service_row() {
    let mut state = State::service(vec![
        svc("org.busx.A", None, None),
        svc("org.busx.B", None, None),
        svc("org.busx.C", None, None),
    ]);
    click(&mut state, &ClickTarget::ServiceRow(2), 40, 7);
    assert_eq!(
        selected_of(&state),
        2,
        "clicking ServiceRow(2) selects row 2"
    );
}

#[test]
fn mouse_click_selects_objects_row() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(
            busx::tui::state::ObjectsScreen {
                service: "s".into(),
                paths: vec!["/a".into(), "/b".into(), "/c".into()],
                selected: 0,
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    click(&mut state, &ClickTarget::ObjectsRow(1), 40, 7);
    match state.top() {
        Screen::Objects(o) => assert_eq!(o.selected, 1, "clicking ObjectsRow(1) selects row 1"),
        _ => panic!("still on Objects"),
    }
}

#[test]
fn mouse_click_selects_interfaces_row() {
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Interfaces(
            busx::tui::state::InterfacesScreen {
                service: "s".into(),
                object: "/o".into(),
                names: vec!["i0".into(), "i1".into(), "i2".into()],
                node: None,
                selected: 0,
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    click(&mut state, &ClickTarget::InterfacesRow(2), 44, 7);
    match state.top() {
        Screen::Interfaces(i) => {
            assert_eq!(i.selected, 2, "clicking InterfacesRow(2) selects row 2")
        }
        _ => panic!("still on Interfaces"),
    }
}

#[test]
fn mouse_click_on_interface_method_row_switches_focus() {
    // An Interface screen on the methods column (default focus). Clicking
    // MethodRow(1) moves the selection to m2 AND keeps focus on Methods with the
    // focus out of the button bar.
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    click(&mut state, &ClickTarget::MethodRow(1), 64, 16);
    match state.top() {
        Screen::Interface(i) => {
            assert_eq!(i.focus, InterfaceFocus::Methods, "focus stays Methods");
            assert!(!i.in_buttons, "focus leaves the button bar");
            assert_eq!(i.selected[0], 1, "methods selection → m2");
        }
        _ => panic!("still on Interface"),
    }
}

#[test]
fn mouse_click_on_action_button_fires() {
    // An Interface with a method. Clicking ActionButton(0) (the Call button)
    // selects + fires it — reusing the Enter "fire button" path. m1 has no
    // IN-args, so the fire auto-skips the Detail form and pushes a Result.
    // Assert the stack grew and landed on a Call Result for "m1".
    let mut state = busx::tui::State {
        screens: vec![Screen::Interface(interface_screen())],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let before = state.screens.len();
    click(&mut state, &ClickTarget::ActionButton(0), 64, 16);
    assert_eq!(
        state.screens.len(),
        before + 1,
        "ActionButton click pushed a Result"
    );
    match state.top() {
        Screen::Result(r) => {
            // Call on the selected method m1 → a Call Result titled "i.m1".
            assert_eq!(r.title, "i.m1", "fired the call button");
            assert!(r.loading);
        }
        _ => panic!("expected Result"),
    }
}

#[test]
fn mouse_click_on_detail_field_focuses_it() {
    // A call Detail with one input field. Clicking DetailField(0) sets
    // field_selected=0 and focus=Field (so subsequent typing edits that field).
    let mut state = call_detail_with_input();
    // Move focus off the field first so the click's effect is observable.
    match state.top_mut() {
        Screen::Detail(d) => d.focus = DetailFocus::Trigger,
        _ => panic!("expected Detail"),
    }
    click(&mut state, &ClickTarget::DetailField(0), 48, 10);
    match state.top() {
        Screen::Detail(d) => {
            assert_eq!(d.field_selected, 0, "DetailField(0) selected");
            assert_eq!(d.focus, DetailFocus::Field, "focus on the field");
        }
        _ => panic!("still on Detail"),
    }
}

#[test]
fn mouse_click_on_popup_tool_selects_it() {
    // A copy-as popup open over a Detail. Clicking PopupTool(1) moves the popup
    // selection without copying (copy happens via Enter).
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c'))); // open the popup
    let popup = state.popup.as_ref().expect("popup opened");
    assert_eq!(popup.selected, 0, "popup starts on tool 0");
    click(&mut state, &ClickTarget::PopupTool(1), 56, 14);
    let popup = state.popup.as_ref().expect("popup still open");
    assert_eq!(popup.selected, 1, "PopupTool(1) selected");
}

#[test]
fn mouse_click_on_already_selected_service_row_drills_in() {
    // Clicking the already-selected row == Enter (drill in), so a mouse user can
    // click to select, then click again to open.
    let mut state = State::service(vec![
        svc("org.busx.A", None, None),
        svc("org.busx.B", None, None),
    ]);
    // selected starts at 0; clicking ServiceRow(0) drills into Objects.
    click(&mut state, &ClickTarget::ServiceRow(0), 60, 8);
    assert!(
        matches!(state.top(), Screen::Objects(_)),
        "clicking the already-selected row drills in (== Enter)"
    );
}

#[test]
fn mouse_click_on_already_selected_popup_tool_copies() {
    // Clicking the already-selected popup tool == Enter on the popup → copy.
    let mut state = call_detail_with_input();
    update(&mut state, key(KeyCode::Char('c'))); // open popup; selected == 0
    click(&mut state, &ClickTarget::PopupTool(0), 56, 14); // already-selected tool
    let popup = state.popup.as_ref().expect("popup still open");
    assert_eq!(
        popup.status.as_deref(),
        Some("copying…"),
        "clicking the already-selected tool triggers the copy"
    );
}

#[test]
fn mouse_scroll_on_result_changes_scroll() {
    // A Result with several streaming messages (the scrollable content). ScrollDown
    // increases `scroll`; ScrollUp decreases it (clamped at 0).
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Result(ResultScreen {
            title: "listen".into(),
            result: None,
            error: None,
            loading: false,
            scroll: 0,
            messages: vec!["m0".into(), "m1".into(), "m2".into(), "m3".into()],
            cancel: None,
            op: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    // ScrollDown twice: 0 → 1 → 2.
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    match state.top() {
        Screen::Result(r) => assert_eq!(r.scroll, 1, "ScrollDown 0 → 1"),
        _ => panic!("still on Result"),
    }
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    match state.top() {
        Screen::Result(r) => assert_eq!(r.scroll, 2, "ScrollDown 1 → 2"),
        _ => panic!("still on Result"),
    }
    // ScrollUp three times: 2 → 1 → 0 → 0 (clamped).
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    match state.top() {
        Screen::Result(r) => assert_eq!(r.scroll, 0, "ScrollUp clamps at 0"),
        _ => panic!("still on Result"),
    }
}

#[test]
fn mouse_scroll_moves_service_selection() {
    // The wheel moves the Service list's cursor one row (like ↓), clamped to the
    // list bounds; the viewport already follows the selection (render persists
    // the offset), so the cursor stays visible.
    let mut state = State::service(vec![
        svc("org.busx.A", None, None),
        svc("org.busx.B", None, None),
        svc("org.busx.C", None, None),
    ]);
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    assert_eq!(selected_of(&state), 1, "ScrollDown 0 → 1");
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    assert_eq!(selected_of(&state), 2, "ScrollDown 1 → 2");
    // Clamp at the bottom — a third ScrollDown stays on the last row.
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    assert_eq!(selected_of(&state), 2, "ScrollDown clamps at last row");
    // ScrollUp walks back up and clamps at 0.
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    assert_eq!(selected_of(&state), 1, "ScrollUp 2 → 1");
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    assert_eq!(selected_of(&state), 0, "ScrollUp clamps at 0");
}

#[test]
fn mouse_scroll_moves_objects_selection() {
    // Same as Service: the wheel moves the Objects list cursor, clamped.
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Objects(
            busx::tui::state::ObjectsScreen {
                service: "s".into(),
                paths: vec!["/a".into(), "/b".into(), "/c".into(), "/d".into()],
                selected: 0,
                loading: false,
                error: None,
            },
        )],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    match state.top() {
        Screen::Objects(o) => assert_eq!(o.selected, 2, "two ScrollDowns → row 2"),
        _ => panic!("still on Objects"),
    }
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    match state.top() {
        Screen::Objects(o) => assert_eq!(o.selected, 1, "ScrollUp 2 → 1"),
        _ => panic!("still on Objects"),
    }
}

#[test]
fn mouse_scroll_on_empty_service_list_is_noop() {
    // An empty list: the wheel is a no-op (no panic, selection stays 0).
    let mut state = State::service(vec![]);
    update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    assert_eq!(selected_of(&state), 0, "empty list: ScrollDown is a no-op");
    update(&mut state, scroll_msg(MouseEventKind::ScrollUp));
    assert_eq!(selected_of(&state), 0, "empty list: ScrollUp is a no-op");
}

#[test]
fn mouse_click_after_scroll_hits_the_visible_row() {
    // Regression: click targets used to ignore the scroll offset, so after
    // scrolling, clicking a visible row selected the wrong index (the top
    // visible row mapped to row 0). Now targets are offset by the scroll, so a
    // click lands on the row actually rendered under the cursor.
    let mut state = State::service(
        (0..20)
            .map(|i| svc(&format!("svc{i:02}"), None, None))
            .collect(),
    );
    // Scroll well past the top so the viewport offset is > 0.
    for _ in 0..10 {
        update(&mut state, scroll_msg(MouseEventKind::ScrollDown));
    }
    assert!(selected_of(&state) > 0, "scrolling moved the cursor");

    // Render to populate click_targets for the scrolled viewport.
    let mut targets = Vec::new();
    let mut scroll = [0usize; 3];
    let mut term = Terminal::new(TestBackend::new(40, 8)).unwrap();
    term.draw(|f| render(f, &state, &mut targets, &mut scroll))
        .unwrap();

    // The topmost service-row target is the row rendered at the top of the
    // viewport; its index must reflect the scroll (not 0). Clicking it selects
    // that index.
    let (rect, top_idx) = targets
        .iter()
        .filter_map(|(r, t)| match t {
            ClickTarget::ServiceRow(i) => Some((*r, *i)),
            _ => None,
        })
        .min_by_key(|(r, _)| r.y)
        .expect("a service row target");
    assert!(
        top_idx > 0,
        "top visible row reflects the scroll, got index {top_idx}"
    );

    state.click_targets = targets;
    update(
        &mut state,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + rect.width / 2,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(
        selected_of(&state),
        top_idx,
        "click after scroll selects the row under the cursor"
    );
}

#[test]
fn mouse_click_on_unrendered_rect_is_noop() {
    // A left-click that hits no recorded Rect does nothing (no panic, no state
    // change). Guards the hit-test's `None` path.
    let mut state = State::service(vec![svc("a", None, None)]);
    let before = state.screens.len();
    update(
        &mut state,
        Msg::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            // With no click_targets populated, any coord misses → no-op.
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(state.screens.len(), before, "missed click is a no-op");
    assert_eq!(selected_of(&state), 0, "selection unchanged");
}

#[test]
fn call_as_arg_shell_splits_field_value() {
    // An `as` field with "1 a" → shell-split into ["1", "a"] (count=1, elem=a),
    // matching how `busx call … as 1 a` works.
    use busx::tui::state::{ActionKind, DetailFocus};
    let mut state = busx::tui::State {
        screens: vec![busx::tui::Screen::Detail(busx::tui::state::DetailScreen {
            service: "s".into(),
            object: "/o".into(),
            interface: "i".into(),
            kind: ActionKind::Call {
                method: "M".into(),
                signature: "as".into(),
            },
            inputs: vec![tui_input::Input::new("1 a".into())],
            field_labels: vec!["as".into()],
            field_selected: 0,
            focus: DetailFocus::Trigger,
            loading: false,
            error: None,
        })],
        quit: false,
        popup: None,
        click_targets: Vec::new(),
        help_open: false,
        bus: Bus::Session,
        show_standard_interfaces: false,
    };
    let effect = update(&mut state, key(KeyCode::Enter));
    match effect {
        Some(Effect::CallMethod { args, .. }) => {
            assert_eq!(args, vec!["1".to_string(), "a".to_string()]);
        }
        _ => panic!("Enter should fire CallMethod with shell-split args"),
    }
}
