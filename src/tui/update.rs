// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6, §7). Returns an `Option<Effect>` so it stays
//! IO-free: pushing/loading a screen requests a fetch the loop performs.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use tui_tree_widget::TreeState;
use zbus_xml::{ArgDirection, Node, PropertyAccess, Signature};
use zvariant::OwnedValue;

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{
    tree_items, InterfaceFocus, InterfaceScreen, InterfacesScreen, ObjectsScreen, Screen,
    ServiceScreen, State,
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
        Msg::PropertiesLoaded(res) => load_properties(state, res),
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
        Screen::Interface(i) => return update_interface_key(i, k.code),
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

fn update_interface_key(i: &mut InterfaceScreen, code: KeyCode) -> Option<Effect> {
    match code {
        KeyCode::Tab => {
            i.focus = match i.focus {
                InterfaceFocus::Methods => InterfaceFocus::Properties,
                InterfaceFocus::Properties => InterfaceFocus::Signals,
                InterfaceFocus::Signals => InterfaceFocus::Methods,
            };
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let idx = focus_index(i.focus);
            let len = column_len(i, idx);
            if len > 0 {
                i.selected[idx] = (i.selected[idx] + 1).min(len - 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let idx = focus_index(i.focus);
            i.selected[idx] = i.selected[idx].saturating_sub(1);
        }
        // Refresh the property-value snapshot.
        KeyCode::Char('r') => {
            i.loading = true;
            return Some(Effect::FetchProperties(
                i.service.clone(),
                i.object.clone(),
                i.interface.clone(),
            ));
        }
        _ => {}
    }
    None
}

fn focus_index(focus: InterfaceFocus) -> usize {
    match focus {
        InterfaceFocus::Methods => 0,
        InterfaceFocus::Properties => 1,
        InterfaceFocus::Signals => 2,
    }
}

fn column_len(i: &InterfaceScreen, idx: usize) -> usize {
    match idx {
        0 => i.methods.len(),
        1 => i.properties.len(),
        _ => i.signals.len(),
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

/// Populate the top Interface screen's property-value snapshot from a GetAll result.
fn load_properties(state: &mut State, res: Result<Vec<(String, OwnedValue)>, String>) -> Option<Effect> {
    if let Screen::Interface(i) = state.top_mut() {
        i.loading = false;
        match res {
            Ok(vals) => {
                i.prop_values = vals
                    .into_iter()
                    .map(|(k, v)| (k, crate::value::pretty::pretty(&v)))
                    .collect();
            }
            Err(e) => i.error = Some(e),
        }
    }
    None
}

/// `org.freedesktop.DBus*` interfaces (Introspectable / Properties / Peer / the
/// bus itself) are housekeeping — exclude them from the browseable list.
fn is_standard_interface(name: &str) -> bool {
    name.starts_with("org.freedesktop.DBus")
}

/// (name, signature) per method/signal.
type Members = Vec<(String, String)>;
/// (name, signature, access) per property.
type Properties = Vec<(String, String, String)>;

/// Extract (methods, properties, signals) for `iface_name` from an introspection
/// node, mirroring `ops::introspect`'s formatting. Method signature = the
/// concatenated IN-arg signatures; signal signature = all args; property = (name, type, access).
fn members_of(node: &Node, iface_name: &str) -> (Members, Properties, Members) {
    let Some(iface) = node.interfaces().iter().find(|i| i.name().as_ref() == iface_name) else {
        return (vec![], vec![], vec![]);
    };
    let methods = iface
        .methods()
        .iter()
        .map(|m| {
            let in_sig: String = m
                .args()
                .iter()
                .filter(|a| a.direction() == Some(ArgDirection::In))
                .map(|a| sig_str(a.ty()))
                .collect();
            (m.name().to_string(), in_sig)
        })
        .collect();
    let properties = iface
        .properties()
        .iter()
        .map(|p| (p.name().to_string(), sig_str(p.ty()), access_str(p.access()).to_string()))
        .collect();
    let signals = iface
        .signals()
        .iter()
        .map(|s| {
            let args: String = s.args().iter().map(|a| sig_str(a.ty())).collect();
            (s.name().to_string(), args)
        })
        .collect();
    (methods, properties, signals)
}

/// `zbus_xml::Signature` is not `Display`; go through the inner `zvariant::Signature`.
fn sig_str(sig: &Signature) -> String {
    sig.inner().to_string()
}

fn access_str(a: PropertyAccess) -> &'static str {
    match a {
        PropertyAccess::Read => "read",
        PropertyAccess::Write => "write",
        PropertyAccess::ReadWrite => "readwrite",
    }
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

/// Push an Interface screen for (service, object, interface). Members are parsed
/// from the parent Interfaces screen's cached introspection node (no extra fetch).
fn push_interface(state: &mut State, service: String, object: String, interface: String) {
    let members = match state.top() {
        Screen::Interfaces(i) => i.node.as_ref().map(|n| members_of(n, &interface)),
        _ => None,
    };
    let (methods, properties, signals) = members.unwrap_or_default();
    state.screens.push(Screen::Interface(InterfaceScreen {
        service,
        object,
        interface,
        methods,
        properties,
        signals,
        prop_values: vec![],
        focus: Default::default(),
        selected: [0, 0, 0],
        loading: true,
        error: None,
    }));
}
