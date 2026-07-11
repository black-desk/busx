// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6, §7). Returns an `Option<Effect>` so it stays
//! IO-free: pushing/loading a screen requests a fetch the loop performs.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zbus_xml::{ArgDirection, Node, PropertyAccess, Signature};
use zvariant::OwnedValue;

use crate::dbus::types::{ObjectNode, ServiceInfo};
use crate::tui::copy::{generate, CopyOp, Tool};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{
    flatten_paths, ActionKind, ActionResult, CopyAsPopup, DetailFocus, DetailScreen, InterfaceFocus,
    InterfaceScreen, InterfacesScreen, ListenTarget, MethodMember, ObjectsScreen, ResultScreen,
    Screen, ServiceScreen, State,
};
use tui_input::backend::crossterm::EventHandler;

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
        Msg::ListenStarted(cancel) => {
            // Fast-Esc edge: the Result may already be gone (popped before this
            // arrived) → `cancel` drops here, which ends the listen task's
            // `cancel_rx` → `select!` breaks. No task leak.
            if let Screen::Result(r) = state.top_mut() {
                r.cancel = Some(cancel);
                r.loading = false;
            }
            None
        }
        Msg::ListenMessage(body) => {
            if let Screen::Result(r) = state.top_mut() {
                r.messages.push(body);
            }
            None
        }
        Msg::ClipboardResult(res) => {
            // The copy-as popup's status reflects the outcome: "copied" on
            // success, "error: …" on failure. No popup (e.g. it was dismissed
            // before the result arrived) → ignore. Never prints to the TTY.
            if let Some(p) = state.popup.as_mut() {
                p.status = Some(match res {
                    Ok(()) => "copied".to_string(),
                    Err(e) => format!("error: {e}"),
                });
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
    // The copy-as popup, when open, captures all keys except `q` (handled above):
    // Esc closes it without popping the screen, ↑↓/jk move the tool selection,
    // Enter copies the focused tool's command. Route there before the ordinary
    // Esc/Enter/screen-dispatch so those keys don't leak through to the screen.
    if state.popup.is_some() {
        return update_popup_key(state, k.code);
    }
    // `c` opens the copy-as popup on a Detail or Result (the screens that carry
    // a copyable operation). No popup is open here, so `c` is unambiguous.
    if matches!(k.code, KeyCode::Char('c')) {
        if let Some(op) = copy_op_for_screen(state.top()) {
            open_copy_as_popup(state, op);
        }
        return None;
    }
    // `y` copies the Result's output as plain text (distinct from `c`, which
    // copies a CLI *command*). Scoped to the Result screen only — on other
    // screens `y` falls through to the normal dispatch (a Detail's printable
    // chars go to its input fields, so `y` must reach that handler, not this one).
    // No-op (returns None) when the Result has nothing to copy.
    if matches!(k.code, KeyCode::Char('y')) {
        if let Screen::Result(r) = state.top() {
            if let Some(text) = copy_result_text(r) {
                return Some(Effect::CopyToClipboard(text));
            }
            return None;
        }
    }
    if matches!(k.code, KeyCode::Esc) {
        // On the Interface screen, Esc first backs out of the action-button bar
        // (back to the member column); only a second Esc pops the screen.
        if let Screen::Interface(i) = state.top_mut() {
            if i.in_buttons {
                i.in_buttons = false;
                return None;
            }
        }
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
        Screen::Detail(d) => return update_detail_key(d, k),
        Screen::Result(r) => update_result_key(r, k.code),
    }
    None
}

/// Build the `CopyOp` for the top screen: a Detail from its current inputs, or a
/// Result from its stored `op`. `None` for any other screen (nothing to copy).
fn copy_op_for_screen(screen: &Screen) -> Option<CopyOp> {
    match screen {
        Screen::Detail(d) => Some(copy_op_from_detail(d)),
        Screen::Result(r) => r.op.clone(),
        _ => None,
    }
}

/// The plain-text rendering of a Result's output for `y` (copy-result-text).
/// Streaming-listen Results (with messages) take precedence: the message blocks
/// joined by `\n`. One-shot results: `Call(lines)` → lines joined by `\n`,
/// `Get(v)` → `v`, `Set` → `"ok"`. `None` when there's no result yet (nothing
/// to copy) or an error is showing (don't copy error text).
fn copy_result_text(r: &ResultScreen) -> Option<String> {
    if !r.messages.is_empty() {
        return Some(r.messages.join("\n"));
    }
    if r.error.is_some() {
        return None;
    }
    match &r.result {
        Some(ActionResult::Call(lines)) => Some(lines.join("\n")),
        Some(ActionResult::Get(v)) => Some(v.clone()),
        Some(ActionResult::Set) => Some("ok".to_string()),
        None => None,
    }
}

/// Build the `CopyOp` a Detail's current input values would produce — mirrors the
/// `Effect` the trigger builds, so copy-as reflects what's typed (not just what
/// was last invoked).
fn copy_op_from_detail(d: &DetailScreen) -> CopyOp {
    match &d.kind {
        ActionKind::Call { method, signature } => CopyOp::Call {
            service: d.service.clone(),
            object: d.object.clone(),
            iface: d.interface.clone(),
            method: method.clone(),
            signature: signature.clone(),
            args: d.inputs.iter().map(|i| i.value().to_string()).collect(),
        },
        ActionKind::Get { property } => CopyOp::Get {
            service: d.service.clone(),
            object: d.object.clone(),
            iface: d.interface.clone(),
            property: property.clone(),
        },
        ActionKind::Set { property, signature } => CopyOp::Set {
            service: d.service.clone(),
            object: d.object.clone(),
            iface: d.interface.clone(),
            property: property.clone(),
            signature: signature.clone(),
            value: vec![d.inputs.first().map(|i| i.value().to_string()).unwrap_or_default()],
        },
        ActionKind::Listen { target } => {
            // Reuse the same match-rule helper the live listen uses; on a rule
            // that can't be built (rare: malformed name) fall back to an empty
            // rule — the popup just shows a degenerate command for each tool.
            let rule = listen_rule(&d.interface, &d.object, target)
                .map(|r| r.to_string())
                .unwrap_or_default();
            CopyOp::Listen { rule }
        }
    }
}

/// Precompute each tool's command for `op` and open the popup focused on row 0.
fn open_copy_as_popup(state: &mut State, op: CopyOp) {
    let commands = Tool::ALL.map(|t| (t, generate(&op, t)));
    state.popup = Some(CopyAsPopup { op, commands, selected: 0, status: None });
}

/// Key handling for the open copy-as popup. The flow keeps the popup open so the
/// copy result can be shown:
/// - No copy yet (`status.is_none()`): ↑↓/jk move the tool selection (clamped
///   0..=3); Enter copies the focused tool's command — sets a transient
///   "copying…" status and emits `Effect::CopyToClipboard` WITHOUT closing. A
///   no-op if the tool can't express the op.
/// - After a copy (`status.is_some()`): navigation is locked; Enter dismisses
///   the popup.
/// - Esc always closes (whether or not a copy happened).
fn update_popup_key(state: &mut State, code: KeyCode) -> Option<Effect> {
    let popup = state.popup.as_mut()?;
    let copy_done = popup.status.is_some();
    match code {
        KeyCode::Esc => {
            state.popup = None;
            None
        }
        KeyCode::Up | KeyCode::Char('k') if !copy_done => {
            popup.selected = popup.selected.saturating_sub(1);
            None
        }
        KeyCode::Down | KeyCode::Char('j') if !copy_done => {
            popup.selected = (popup.selected + 1).min(3);
            None
        }
        KeyCode::Enter if copy_done => {
            // A copy already happened (its result is showing) → Enter dismisses.
            state.popup = None;
            None
        }
        KeyCode::Enter => {
            // First Enter: trigger the copy. Set the transient "copying…" status
            // and emit the effect; the popup STAYS OPEN so the eventual
            // `Msg::ClipboardResult` can update the status. A no-op (popup stays
            // open, no status) if the selected tool can't express the op.
            let cmd = popup.commands.get(popup.selected).and_then(|(_, c)| c.clone());
            match cmd {
                Some(cmd) => {
                    popup.status = Some("copying…".to_string());
                    Some(Effect::CopyToClipboard(cmd))
                }
                None => None,
            }
        }
        _ => None,
    }
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
            push_interface(state, svc, obj, iface)
        }
        Screen::Interface(_) => {
            // If focus is still on a member column, Enter drills INTO the action
            // button bar (does not fire anything yet). Once `in_buttons`, Enter
            // fires the selected button (builds `ActionKind`, pushes a Detail).
            if let Screen::Interface(i) = state.top_mut() {
                if !i.in_buttons {
                    i.in_buttons = true;
                    i.button_selected = 0;
                    return None;
                }
            }
            // Gather owned identity data while holding the immutable borrow, then
            // release it before the mutable `push_detail`.
            let i = match state.top() {
                Screen::Interface(i) => i,
                _ => return None,
            };
            let buttons = buttons_for(i.focus);
            let action = *buttons.get(i.button_selected)?;
            let (svc, obj, iface) = (i.service.clone(), i.object.clone(), i.interface.clone());
            let kind = match i.focus {
                InterfaceFocus::Methods => {
                    let m = i.methods.get(i.selected[0])?;
                    match action {
                        "调用" => ActionKind::Call {
                            method: m.name.clone(),
                            signature: m.signature.clone(),
                        },
                        "监听" => ActionKind::Listen {
                            target: ListenTarget::Method { member: m.name.clone() },
                        },
                        _ => return None,
                    }
                }
                InterfaceFocus::Properties => {
                    let (name, sig, _access) = i.properties.get(i.selected[1])?;
                    match action {
                        "读取" => ActionKind::Get { property: name.clone() },
                        "设置" => ActionKind::Set { property: name.clone(), signature: sig.clone() },
                        "监听" => ActionKind::Listen {
                            target: ListenTarget::Property { property: name.clone() },
                        },
                        _ => return None,
                    }
                }
                InterfaceFocus::Signals => {
                    let (name, _sig) = i.signals.get(i.selected[2])?;
                    match action {
                        "监听" => ActionKind::Listen {
                            target: ListenTarget::Signal { member: name.clone() },
                        },
                        _ => return None,
                    }
                }
            };
            // Build the form fields: Call → one input per IN-arg; Set → one input
            // (the new value), labeled with the property's signature; Get/Listen →
            // no inputs. A Listen carries its match-rule preview as a single label.
            let (inputs, field_labels) = match &kind {
                ActionKind::Call { .. } => {
                    let m = i.methods.get(i.selected[0]);
                    match m {
                        Some(m) => call_fields(&m.args),
                        None => (vec![], vec![]),
                    }
                }
                ActionKind::Set { signature, .. } => {
                    (vec![tui_input::Input::default()], vec![signature.clone()])
                }
                ActionKind::Get { .. } => (vec![], vec![]),
                ActionKind::Listen { target } => {
                    let rule = listen_rule(&iface, &obj, target).map(|r| r.to_string());
                    (vec![], vec![rule.unwrap_or_else(|e| format!("invalid match rule: {e}"))])
                }
            };
            push_detail(state, svc, obj, iface, kind, inputs, field_labels);
            None
        }
        Screen::Detail(d) => {
            // `[触发]` Enter: only fires when the trigger button is focused.
            // Extract owned (title, Effect, CopyOp) data for Call/Get/Set while
            // holding the immutable borrow, then push one Result screen carrying
            // the CopyOp (so `c` on the Result can copy-as it) and return it.
            if d.focus != DetailFocus::Trigger {
                return None;
            }
            let copy_op = copy_op_from_detail(d);
            let (title, effect) = match &d.kind {
                ActionKind::Call { method, signature } => {
                    let args: Vec<String> =
                        d.inputs.iter().map(|i| i.value().to_string()).collect();
                    (
                        format!("{}.{}", d.interface, method),
                        Effect::CallMethod {
                            service: d.service.clone(),
                            object: d.object.clone(),
                            iface: d.interface.clone(),
                            method: method.clone(),
                            signature: signature.clone(),
                            args,
                        },
                    )
                }
                ActionKind::Get { property } => (
                    property.clone(),
                    Effect::GetProperty {
                        service: d.service.clone(),
                        object: d.object.clone(),
                        iface: d.interface.clone(),
                        property: property.clone(),
                    },
                ),
                ActionKind::Set { property, signature } => {
                    let value =
                        d.inputs.first().map(|i| i.value().to_string()).unwrap_or_default();
                    (
                        property.clone(),
                        Effect::SetProperty {
                            service: d.service.clone(),
                            object: d.object.clone(),
                            iface: d.interface.clone(),
                            property: property.clone(),
                            signature: signature.clone(),
                            value,
                        },
                    )
                }
                ActionKind::Listen { target } => {
                    // Listen targets a member/property; title surfaces which.
                    let member_or_prop = match target {
                        ListenTarget::Signal { member }
                        | ListenTarget::Method { member } => member.clone(),
                        ListenTarget::Property { property } => property.clone(),
                    };
                    (
                        format!("listen {}.{}", d.interface, member_or_prop),
                        Effect::Listen {
                            service: d.service.clone(),
                            object: d.object.clone(),
                            iface: d.interface.clone(),
                            target: target.clone(),
                        },
                    )
                }
            };
            state.screens.push(Screen::Result(ResultScreen {
                title,
                result: None,
                error: None,
                loading: true,
                scroll: 0,
                messages: vec![],
                cancel: None,
                op: Some(copy_op),
            }));
            Some(effect)
        }
        Screen::Result(_) => None,
    }
}

/// `↑↓`/`jk` scroll the result. Clamp coarsely: never below 0, never past the
/// last content line. `update` has no frame area, so visible-row-precise
/// clamping can't be done here — render applies the offset (and single-line
/// results simply don't scroll). Real precise scrolling matters most for
/// streaming monitor results (Phase 4).
fn update_result_key(r: &mut ResultScreen, code: KeyCode) {
    // Streaming-listen mode counts received message blocks; one-shot mode counts
    // reply lines (1 for Get/Set/empty). Clamp coarsely: render applies the offset.
    let lines = if !r.messages.is_empty() {
        r.messages.len()
    } else {
        match &r.result {
            Some(ActionResult::Call(vs)) => vs.len(),
            Some(ActionResult::Get(_)) | None | Some(ActionResult::Set) => 1,
        }
    };
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            r.scroll = (r.scroll + 1).min(lines.saturating_sub(1));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            r.scroll = r.scroll.saturating_sub(1);
        }
        _ => {}
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

/// Interface focus scheme (spec §2):
/// - `Tab` / `Shift+Tab` cycle the three columns (Methods→Properties→Signals→
///   Methods, reverse for Shift+Tab). The button bar is NOT in this ring; Tab
///   always leaves the button bar first (`in_buttons = false`) then cycles.
/// - Column focus (`!in_buttons`): `↑↓`/`jk` move the focused column's member
///   selection.
/// - Button-bar focus (`in_buttons`): `↑↓`/`jk` move `button_selected` within the
///   focused column's action list.
/// - `Enter` (drill into buttons / fire a button) and `Esc` (back out of buttons
///   before popping) are handled in `handle_enter` / the global Esc arm — NOT
///   here, since they need `&mut State`.
/// - `r` refreshes the property-value snapshot (GetAll) for this interface.
fn update_interface_key(i: &mut InterfaceScreen, k: KeyEvent) -> Option<Effect> {
    match (k.code, k.modifiers.contains(KeyModifiers::SHIFT)) {
        // Tab (no Shift): leave the button bar if in it, then cycle forward.
        (KeyCode::Tab, false) => {
            i.in_buttons = false;
            i.focus = cycle_focus(i.focus, 1);
        }
        // Shift+Tab (BackTab or Tab+Shift): leave the button bar, cycle backward.
        (KeyCode::BackTab, _) | (KeyCode::Tab, true) => {
            i.in_buttons = false;
            i.focus = cycle_focus(i.focus, -1);
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            if i.in_buttons {
                let len = buttons_for(i.focus).len();
                if len > 0 {
                    i.button_selected = (i.button_selected + 1).min(len - 1);
                }
            } else {
                let idx = focus_index(i.focus);
                let len = column_len(i, idx);
                if len > 0 {
                    i.selected[idx] = (i.selected[idx] + 1).min(len - 1);
                }
            }
        }
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            if i.in_buttons {
                i.button_selected = i.button_selected.saturating_sub(1);
            } else {
                let idx = focus_index(i.focus);
                i.selected[idx] = i.selected[idx].saturating_sub(1);
            }
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

/// Cycle `focus` among Methods/Properties/Signals by `dir` (+1 forward, -1 back).
fn cycle_focus(focus: InterfaceFocus, dir: i32) -> InterfaceFocus {
    let idx = focus_index(focus) as i32;
    // Three columns: wrap with Euclidean remainder so -1 from Methods → Signals.
    let next = ((idx + dir) % 3 + 3) % 3;
    match next {
        0 => InterfaceFocus::Methods,
        1 => InterfaceFocus::Properties,
        _ => InterfaceFocus::Signals,
    }
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

/// The action buttons offered for a given column. Each column carries a
/// `监听` (listen) button; methods also `调用`, properties `读取`/`设置`.
fn buttons_for(column: InterfaceFocus) -> &'static [&'static str] {
    match column {
        InterfaceFocus::Methods => &["调用", "监听"],
        InterfaceFocus::Properties => &["读取", "设置", "监听"],
        InterfaceFocus::Signals => &["监听"],
    }
}

/// Detail key handling: `Tab` cycles Field0→Field1→…→Trigger→Field0, `↑↓`/`jk`
/// move the focused field, and any other key edits the focused input via tui-input.
/// `Esc` (pop) and `Enter` (trigger) are handled globally / in `handle_enter`.
fn update_detail_key(d: &mut DetailScreen, k: KeyEvent) -> Option<Effect> {
    let n = d.inputs.len();
    match k.code {
        KeyCode::Tab => {
            // Cycle the focus. With 0 inputs the field row is empty, so a single
            // Tab lands on the trigger; otherwise Field0→Field1→…→last→Trigger→Field0.
            match d.focus {
                DetailFocus::Field => {
                    if n == 0 {
                        d.focus = DetailFocus::Trigger;
                    } else if d.field_selected + 1 < n {
                        d.field_selected += 1;
                    } else {
                        d.focus = DetailFocus::Trigger;
                    }
                }
                DetailFocus::Trigger => {
                    d.focus = DetailFocus::Field;
                    d.field_selected = 0;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') if d.focus == DetailFocus::Field && n > 0 => {
            d.field_selected = (d.field_selected + 1).min(n - 1);
        }
        KeyCode::Up | KeyCode::Char('k') if d.focus == DetailFocus::Field && n > 0 => {
            d.field_selected = d.field_selected.saturating_sub(1);
        }
        _ if d.focus == DetailFocus::Field && n > 0 => {
            // Any other key edits the focused input (tui-input mutates in place;
            // ignore its changed-state return value).
            d.inputs[d.field_selected].handle_event(&crossterm::event::Event::Key(k));
        }
        _ => {}
    }
    None
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
        return push_interface(state, service, object, iface);
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
fn push_interface(
    state: &mut State,
    service: String,
    object: String,
    interface: String,
) -> Option<Effect> {
    let members = match state.top() {
        Screen::Interfaces(i) => i.node.as_ref().map(|n| members_of(n, &interface)),
        _ => None,
    };
    let (methods, properties, signals) = members.unwrap_or_default();
    // Only fetch property VALUES (GetAll) when the interface actually has
    // properties — calling GetAll on a property-less interface is pointless, and
    // some objects error on it (their GetAll rejects interfaces they don't
    // track, e.g. the standard org.freedesktop.DBus.* ones). `loading` is true
    // only while such a fetch is in flight.
    let has_props = !properties.is_empty();
    state.screens.push(Screen::Interface(InterfaceScreen {
        service: service.clone(),
        object: object.clone(),
        interface: interface.clone(),
        methods,
        properties,
        signals,
        prop_values: vec![],
        focus: Default::default(),
        in_buttons: false,
        button_selected: 0,
        selected: [0, 0, 0],
        loading: has_props,
        error: None,
    }));
    if has_props {
        Some(Effect::FetchProperties(service, object, interface))
    } else {
        None
    }
}

/// Build the form fields for a method call: one `tui-input` per IN-arg, labeled
/// "name  sig" (or just `sig` when the arg name is empty). Zero-arg methods yield
/// zero fields — the Detail is just the `[触发]` button.
fn call_fields(args: &[(String, String)]) -> (Vec<tui_input::Input>, Vec<String>) {
    let labels = args
        .iter()
        .map(|(name, sig)| if name.is_empty() { sig.clone() } else { format!("{name}  {sig}") })
        .collect();
    let inputs = args.iter().map(|_| tui_input::Input::default()).collect();
    (inputs, labels)
}

/// Push a Detail form for an action. `inputs`/`field_labels` are non-empty only
/// for calls (one input per IN-arg) and Set (one input, labeled with the
/// property's signature); Get keeps both empty. A Listen carries its match-rule
/// preview as a single label (no inputs).
fn push_detail(
    state: &mut State,
    service: String,
    object: String,
    interface: String,
    kind: ActionKind,
    inputs: Vec<tui_input::Input>,
    field_labels: Vec<String>,
) {
    state.screens.push(Screen::Detail(DetailScreen {
        service,
        object,
        interface,
        kind,
        inputs,
        field_labels,
        field_selected: 0,
        focus: DetailFocus::default(),
        loading: false,
        error: None,
    }));
}

/// The match rule that subscribes to a listen target — shared by the Detail
/// preview here and the live `MessageStream` in `app.rs`.
///
/// - Signal → the signal's own rule on `(iface, member, object)`.
/// - Property → subscribe `org.freedesktop.DBus.Properties.PropertiesChanged` on
///   `object`; the named property is filtered client-side in `app.rs`.
/// - Method → a `type='method_call'` rule on `(iface, member, object)`; the TUI
///   listens via BecomeMonitor on a dedicated connection, which delivers only
///   matching calls (Task 3).
pub(crate) fn listen_rule(
    iface: &str,
    object: &str,
    target: &ListenTarget,
) -> crate::error::Result<zbus::MatchRule<'static>> {
    match target {
        ListenTarget::Signal { member } => crate::dbus::monitor::build_match_rule(
            Some(iface),
            Some(member),
            Some(object),
            None,
            None,
            Some(zbus::message::Type::Signal),
        ),
        ListenTarget::Property { .. } => crate::dbus::monitor::build_match_rule(
            Some("org.freedesktop.DBus.Properties"),
            Some("PropertiesChanged"),
            Some(object),
            None,
            None,
            Some(zbus::message::Type::Signal),
        ),
        ListenTarget::Method { member } => crate::dbus::monitor::build_match_rule(
            Some(iface),
            Some(member),
            Some(object),
            None,
            None,
            Some(zbus::message::Type::MethodCall),
        ),
    }
}
