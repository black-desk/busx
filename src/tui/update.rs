// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6, §7). Returns an `Option<Effect>` so it stays
//! IO-free: pushing/loading a screen requests a fetch the loop performs.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use tui_tree_widget::TreeState;
use zbus_xml::Node;

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{
    tree_items, InterfaceScreen, InterfacesScreen, ObjectsScreen, Screen, ServiceScreen, State,
};

pub fn update(state: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Key(k) => update_key(state, k),
        Msg::Resize(_, _) => None,
        Msg::ServicesLoaded(res) => {
            if let Screen::Service(s) = state.top_mut() {
                load_services(s, res);
            }
            None
        }
        Msg::ObjectsLoaded(res) => load_objects(state, res),
        Msg::InterfacesLoaded(svc, obj, res) => load_interfaces(state, svc, obj, res),
        Msg::PropertiesLoaded(_) => None, // Task 4
    }
}

fn update_key(state: &mut State, k: KeyEvent) -> Option<Effect> {
    if k.kind != KeyEventKind::Press && !matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
        return None;
    }
    if matches!(k.code, KeyCode::Char('q')) {
        state.quit = true;
        return None;
    }
    if matches!(k.code, KeyCode::Esc) {
        // Esc pops the stack (back); at the root Service screen, Esc quits.
        if state.screens.len() > 1 {
            state.screens.pop();
        } else {
            state.quit = true;
        }
        return None;
    }
    if k.code == KeyCode::Enter {
        return handle_enter(state);
    }
    match state.top_mut() {
        Screen::Service(s) => update_service_key(s, k.code),
        Screen::Objects(o) => update_objects_key(o, k.code),
        Screen::Interfaces(i) => update_interfaces_key(i, k.code),
        Screen::Interface(_) => {} // Task 4
    }
    None
}

/// `Enter` drills one level deeper, pushing the next screen + requesting its fetch.
fn handle_enter(state: &mut State) -> Option<Effect> {
    match state.top() {
        Screen::Service(s) => {
            let svc = s.services.get(s.selected).map(|sv| sv.name.clone())?;
            state.screens.push(Screen::Objects(ObjectsScreen {
                service: svc.clone(),
                tree: ObjectNode { path: "/".into(), children: vec![] },
                items: vec![],
                state: RefCell::new(TreeState::default()),
                loading: true,
                error: None,
            }));
            Some(Effect::FetchObjects(svc))
        }
        Screen::Objects(o) => {
            // The selected tree node's identifier is its full object path; the path
            // to the selected node is a chain, the last element being the node itself.
            let path = o.state.borrow().selected().last().cloned()?;
            let svc = o.service.clone();
            push_interfaces(state, svc.clone(), path.clone());
            Some(Effect::FetchInterfaces(svc, path))
        }
        Screen::Interfaces(i) => {
            let iface = i.names.get(i.selected).cloned()?;
            let (svc, obj) = (i.service.clone(), i.object.clone());
            push_interface(state, svc.clone(), obj.clone(), iface.clone());
            Some(Effect::FetchProperties(svc, obj, iface))
        }
        Screen::Interface(_) => None, // Phase 3 wires method/property actions
    }
}

fn update_service_key(s: &mut ServiceScreen, code: KeyCode) {
    match code {
        KeyCode::Down | KeyCode::Char('j') if !s.services.is_empty() => {
            s.selected = (s.selected + 1).min(s.services.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') if !s.services.is_empty() => {
            s.selected = s.selected.saturating_sub(1);
        }
        _ => {}
    }
}

fn update_objects_key(o: &mut ObjectsScreen, code: KeyCode) {
    // `Enter` is handled in `handle_enter` (drill into the selected object).
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            o.state.get_mut().key_down();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            o.state.get_mut().key_up();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            o.state.get_mut().key_right();
        }
        KeyCode::Left | KeyCode::Char('h') => {
            o.state.get_mut().key_left();
        }
        _ => {}
    }
}

fn update_interfaces_key(i: &mut InterfacesScreen, code: KeyCode) {
    match code {
        KeyCode::Down | KeyCode::Char('j') if !i.names.is_empty() => {
            i.selected = (i.selected + 1).min(i.names.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') if !i.names.is_empty() => {
            i.selected = i.selected.saturating_sub(1);
        }
        _ => {}
    }
}

fn load_services(s: &mut ServiceScreen, res: Result<Vec<ServiceInfo>, String>) {
    s.loading = false;
    match res {
        Ok(services) => {
            s.selected = s.selected.min(services.len().saturating_sub(1));
            s.services = services;
        }
        Err(e) => s.error = Some(e),
    }
}

/// Populate the top Objects screen; select the first object so `Enter` works;
/// auto-skip if there is exactly one top-level object.
fn load_objects(state: &mut State, res: Result<ObjectNode, String>) -> Option<Effect> {
    let mut drill = None;
    if let Screen::Objects(o) = state.top_mut() {
        o.loading = false;
        match res {
            Ok(root) => {
                o.items = tree_items(&root);
                // Select the first object so `Enter` drills into something.
                if let Some(first) = root.children.first() {
                    o.state.get_mut().select(vec![first.path.clone()]);
                }
                if root.children.len() == 1 {
                    drill = Some((o.service.clone(), root.children[0].path.clone()));
                }
                o.tree = root;
            }
            Err(e) => o.error = Some(e),
        }
    }
    if let Some((svc, path)) = drill {
        push_interfaces(state, svc.clone(), path.clone());
        return Some(Effect::FetchInterfaces(svc, path));
    }
    None
}

/// Populate the top Interfaces screen from the introspection node, filtering out
/// the standard D-Bus interfaces; auto-skip if exactly one remains.
fn load_interfaces(
    state: &mut State,
    service: String,
    object: String,
    res: Result<Node<'static>, String>,
) -> Option<Effect> {
    let mut drill = None;
    if let Screen::Interfaces(i) = state.top_mut() {
        // Stale-result guard: only apply a result for the object currently shown.
        if i.service != service || i.object != object {
            return None;
        }
        i.loading = false;
        match res {
            Ok(node) => {
                let names: Vec<String> = node
                    .interfaces()
                    .iter()
                    .map(|iface| iface.name().to_string())
                    .filter(|n| !is_standard_interface(n))
                    .collect();
                if names.len() == 1 {
                    drill = Some(names[0].clone());
                }
                i.selected = i.selected.min(names.len().saturating_sub(1));
                i.names = names;
                i.node = Some(node);
            }
            Err(e) => i.error = Some(e),
        }
    }
    if let Some(iface) = drill {
        push_interface(state, service.clone(), object.clone(), iface.clone());
        return Some(Effect::FetchProperties(service, object, iface));
    }
    None
}

/// `org.freedesktop.DBus*` interfaces (Introspectable / Properties / Peer / the
/// bus itself) are housekeeping — exclude them from the browseable list.
fn is_standard_interface(name: &str) -> bool {
    name.starts_with("org.freedesktop.DBus")
}

/// Push an Interfaces screen for (service, object) in loading state.
fn push_interfaces(state: &mut State, service: String, object: String) {
    state.screens.push(Screen::Interfaces(InterfacesScreen {
        service,
        object,
        names: vec![],
        node: None,
        selected: 0,
        loading: true,
        error: None,
    }));
}

/// Push an Interface screen for (service, object, interface) in loading state.
/// (Task 4 parses members from the cached introspection node.)
fn push_interface(state: &mut State, service: String, object: String, interface: String) {
    state.screens.push(Screen::Interface(InterfaceScreen {
        service,
        object,
        interface,
        methods: vec![],
        properties: vec![],
        signals: vec![],
        prop_values: vec![],
        focus: Default::default(),
        selected: [0, 0, 0],
        loading: true,
        error: None,
    }));
}
