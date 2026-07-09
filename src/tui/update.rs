// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6, §7). Returns an `Option<Effect>` so it stays
//! IO-free: pushing/loading a screen requests a fetch the loop performs.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zbus_xml::{ArgDirection, Node, PropertyAccess, Signature};
use zvariant::OwnedValue;

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{
    flatten_paths, ActionKind, DetailFocus, DetailScreen, InterfaceFocus, InterfaceScreen,
    InterfacesScreen, MethodMember, ObjectsScreen, Screen, ServiceScreen, State,
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
        Msg::ActionResult(res) => {
            if let Screen::Result(r) = state.top_mut() {
                r.loading = false;
                match res {
                    Ok(ar) => r.result = Some(ar),
                    Err(e) => r.error = Some(e),
                }
            }
            None
        }
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
        Screen::Interface(i) => return update_interface_key(i, k),
        Screen::Detail(d) => update_detail_key(d, k.code),
        Screen::Result(_) => {} // Task 4 wires scroll; Esc handled above.
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
                paths: vec![],
                selected: 0,
                loading: true,
                error: None,
            }));
            Some(Effect::FetchObjects(svc))
        }
        Screen::Objects(o) => {
            let path = o.paths.get(o.selected).cloned()?;
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
        Screen::Interface(i) => {
            // Gather owned identity data while holding the immutable borrow, then
            // release it before the mutable `push_detail`.
            if i.focus != InterfaceFocus::Buttons {
                return None;
            }
            let buttons = buttons_for(i.active_column);
            let action = *buttons.get(i.button_selected)?;
            let (svc, obj, iface) = (i.service.clone(), i.object.clone(), i.interface.clone());
            let kind = match i.active_column {
                InterfaceFocus::Methods => {
                    let m = i.methods.get(i.selected[0])?;
                    match action {
                        "调用" => ActionKind::Call {
                            method: m.name.clone(),
                            signature: m.signature.clone(),
                        },
                        _ => return None,
                    }
                }
                InterfaceFocus::Properties => {
                    let (name, sig, _access) = i.properties.get(i.selected[1])?;
                    match action {
                        "读取" => ActionKind::Get { property: name.clone() },
                        "设置" => ActionKind::Set { property: name.clone(), signature: sig.clone() },
                        _ => return None,
                    }
                }
                _ => return None,
            };
            push_detail(state, svc, obj, iface, kind);
            None
        }
        Screen::Detail(_) => None, // Task 2/3: `[触发]` returns the action Effect.
        Screen::Result(_) => None,
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
    // `Enter` is handled in `handle_enter` (drill into the selected path).
    match code {
        KeyCode::Down | KeyCode::Char('j') if !o.paths.is_empty() => {
            o.selected = (o.selected + 1).min(o.paths.len() - 1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            o.selected = o.selected.saturating_sub(1);
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

/// Interface focus scheme:
/// - `Tab` toggles between the active member column and the button bar.
///   Landing on a column sets `active_column`.
/// - `Shift+Tab` (BackTab) cycles the active column
///   Methods → Properties → Signals → Methods (skipping Buttons).
/// - Column focus: `↑↓`/`jk` move that column's member selection.
/// - `Buttons` focus: `↑↓`/`jk` move `button_selected` within the active column's
///   action list; `Enter` pushes a stub `Detail` screen.
fn update_interface_key(i: &mut InterfaceScreen, k: KeyEvent) -> Option<Effect> {
    match (k.code, k.modifiers.contains(KeyModifiers::SHIFT)) {
        (KeyCode::Tab, false) => {
            i.focus = match i.focus {
                InterfaceFocus::Buttons => i.active_column,
                // From any column, jump to the button bar.
                InterfaceFocus::Methods | InterfaceFocus::Properties | InterfaceFocus::Signals => {
                    InterfaceFocus::Buttons
                }
            };
            // Clamp button selection into range when entering the bar.
            if i.focus == InterfaceFocus::Buttons {
                i.button_selected = i.button_selected.min(buttons_for(i.active_column).len().saturating_sub(1));
            }
        }
        (KeyCode::BackTab, _) | (KeyCode::Tab, true) => {
            // Cycle the active column among member columns, and focus it.
            i.active_column = match i.active_column {
                InterfaceFocus::Methods => InterfaceFocus::Properties,
                InterfaceFocus::Properties => InterfaceFocus::Signals,
                InterfaceFocus::Signals => InterfaceFocus::Methods,
                InterfaceFocus::Buttons => InterfaceFocus::Methods, // unreachable, kept safe
            };
            i.focus = i.active_column;
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) if i.focus == InterfaceFocus::Buttons => {
            let len = buttons_for(i.active_column).len();
            if len > 0 {
                i.button_selected = (i.button_selected + 1).min(len - 1);
            }
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) if i.focus == InterfaceFocus::Buttons => {
            i.button_selected = i.button_selected.saturating_sub(1);
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            let idx = focus_index(i.focus);
            let len = column_len(i, idx);
            if len > 0 {
                i.selected[idx] = (i.selected[idx] + 1).min(len - 1);
            }
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            let idx = focus_index(i.focus);
            i.selected[idx] = i.selected[idx].saturating_sub(1);
        }
        // Refresh the property-value snapshot.
        (KeyCode::Char('r'), _) => {
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
        InterfaceFocus::Buttons => 0, // unreachable in the column branch
    }
}

fn column_len(i: &InterfaceScreen, idx: usize) -> usize {
    match idx {
        0 => i.methods.len(),
        1 => i.properties.len(),
        _ => i.signals.len(),
    }
}

/// The action buttons offered for a given active column (Signals → none this phase).
fn buttons_for(column: InterfaceFocus) -> &'static [&'static str] {
    match column {
        InterfaceFocus::Methods => &["调用"],
        InterfaceFocus::Properties => &["读取", "设置"],
        InterfaceFocus::Signals => &[],
        InterfaceFocus::Buttons => &[],
    }
}

fn update_detail_key(_d: &mut DetailScreen, _code: KeyCode) {
    // Task 2/3: field editing, Field↔Trigger focus, `[触发]` returning the Effect.
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

/// Populate the top Objects screen with the flattened path list; auto-skip if
/// the service's only object is the root "/" (drill straight into its interfaces).
fn load_objects(state: &mut State, res: Result<ObjectNode, String>) -> Option<Effect> {
    let mut drill = None;
    if let Screen::Objects(o) = state.top_mut() {
        o.loading = false;
        match res {
            Ok(root) => {
                let paths = flatten_paths(&root);
                if paths.len() == 1 {
                    drill = Some((o.service.clone(), paths[0].clone()));
                }
                o.selected = 0;
                o.paths = paths;
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

/// Populate the top Interfaces screen from the introspection node; auto-skip if
/// the object exposes exactly one interface.
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

/// Methods extracted from an interface.
type Methods = Vec<MethodMember>;
/// (name, signature, access) per property.
type Properties = Vec<(String, String, String)>;
/// (name, signature) per signal.
type Signals = Vec<(String, String)>;

/// Extract (methods, properties, signals) for `iface_name` from an introspection
/// node, mirroring `ops::introspect`'s formatting. Method signature = the
/// concatenated IN-arg signatures; signal signature = all args; property = (name, type, access).
fn members_of(node: &Node, iface_name: &str) -> (Methods, Properties, Signals) {
    let Some(iface) = node.interfaces().iter().find(|i| i.name().as_ref() == iface_name) else {
        return (vec![], vec![], vec![]);
    };
    let methods = iface
        .methods()
        .iter()
        .map(|m| {
            let in_args: Vec<(String, String)> = m
                .args()
                .iter()
                .filter(|a| a.direction() == Some(ArgDirection::In))
                .map(|a| (a.name().unwrap_or("").to_string(), sig_str(a.ty())))
                .collect();
            let in_sig: String = in_args.iter().map(|(_, s)| s.clone()).collect();
            MethodMember { name: m.name().to_string(), signature: in_sig, args: in_args }
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
        active_column: Default::default(),
        button_selected: 0,
        selected: [0, 0, 0],
        loading: true,
        error: None,
    }));
}

/// Push a stub Detail screen for an action (Task 2/3 fills `inputs`/`field_labels`).
fn push_detail(
    state: &mut State,
    service: String,
    object: String,
    interface: String,
    kind: ActionKind,
) {
    state.screens.push(Screen::Detail(DetailScreen {
        service,
        object,
        interface,
        kind,
        inputs: vec![],
        field_labels: vec![],
        field_selected: 0,
        focus: DetailFocus::default(),
        loading: false,
        error: None,
    }));
}
