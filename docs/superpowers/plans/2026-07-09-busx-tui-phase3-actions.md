<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — Phase 3: Actions (call / get / set)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the Interface screen, act on the selected method/property: a method's `[调用]` button and a property's `[读取]`/`[设置]` buttons open a **Detail** form (per-arg `tui-input` for calls, single input for set, no input for get); `[触发]` performs the one-shot operation and pushes a **Result** screen showing the outcome (return values / property value / success), with `Esc` back. Monitor (`[监听]`), copy-as, clipboard, and JSON-view toggle are NOT in this phase.

**Architecture:** Two new `Screen` variants — `Detail` (a form: `tui-input` fields + a `[触发]` button) and `Result` (the outcome, scrollable) — pushed onto the existing nav stack. The Interface screen gains a right-column action-button list; `Tab` toggles focus between the three member-lists (left) and the buttons (right). Three new `Effect` variants (`CallMethod` / `GetProperty` / `SetProperty`) keep `update` pure; the loop's `run_effect` spawns the existing async core (`dbus::call::call_method`, `dbus::property::{get_one, set}`) and delivers a unified `Msg::ActionResult`. `MethodMember` is enriched to carry per-method in-args so the call Detail form can label each input with its arg name + signature.

**Tech Stack:** ratatui 0.30 · **tui-input 0.15** (new — ratatui has no native text-input widget; confirmed against the 0.30 widget set) · the existing async `dbus::` core · insta snapshots.

**Spec:** `docs/superpowers/specs/2026-07-08-busx-tui-design.md` (§8 Interface action buttons + Detail/Result pages, §9 Detail forms, §14.3 phase scope). Built on Phase 2's browse flow + `Effect`/`run_loop` design.

---

## Conventions

- REUSE SPDX header on touched/created files (copyright `2026 Chen Linxuan <me@black-desk.cn>`, GPL-3.0-or-later). Commit ends with blank line + `Assisted-by: claude:glm-5.2`.
- **Testing:** TestBackend + insta snapshots, driving keys through `update`/`run_loop` with the no-op effect handler `|_| {}` (no real bus). First generation via `INSTA_UPDATE=always cargo test --test tui`; then pinned. `.snap` files are covered by `.reuse/dep5`.
- **Confirmed core APIs** (already built in Phase 0):
  - `dbus::call::call_method(&Connection, svc, obj, iface, method, signature: &str, args: &[String]) -> Result<Vec<OwnedValue>>` (async).
  - `dbus::property::get_one(&Connection, svc, obj, iface, prop) -> Result<OwnedValue>` (async).
  - `dbus::property::set(&Connection, svc, obj, iface, prop, signature: &str, value_tokens: &[String]) -> Result<()>` (async).
  - `crate::value::pretty::pretty(&Value) -> String`; an `&OwnedValue` coerces to `&Value` at the call site.
  - `zbus_xml::Node::interfaces()` → `Interface`; `Interface::methods()` → `[Method]`; `Method::args()` → `[Arg]`; `Arg::name() -> Option<&str>`, `Arg::direction() -> Option<ArgDirection>` (In/Out), `Arg::ty() -> &Signature`; `sig.inner().to_string()` stringifies a `Signature`.
- **tui-input 0.15** (confirm against the crate source under `~/.cargo/registry/.../tui-input-0.*/`): `tui_input::Input` (state: value + cursor), `Input::new(default)`, `input.handle_event(&crossterm::event::Event)`, `input.value() -> &str`. We feed `Msg::Key(KeyEvent)` by wrapping `Event::Key(k)`. Rendering is our own `Paragraph` of `input.value()` (tui-input is state-only; it does not render).
- The Interface render's member display is unchanged ("name  signature"); only the `methods` field type changes (carrying args), so the existing Interface snapshot stays valid modulo the new right-column button area (regenerated in Task 1).

## File structure (after Phase 3)

- **Modify** `Cargo.toml` — add `tui-input = "0.15"`.
- **Modify** `src/tui/state.rs` — `MethodMember` struct (replaces the methods tuple); `Detail`/`Result` Screen variants + `DetailScreen`/`ResultScreen` structs.
- **Modify** `src/tui/msg.rs` — `Effect::{CallMethod, GetProperty, SetProperty}` + `Msg::ActionResult` + `ActionResult` enum.
- **Modify** `src/tui/update.rs` — Interface `Tab` focus (lists↔buttons) + action-button `Enter`; `handle_enter`/Detail form key handling; `load_action_result`.
- **Modify** `src/tui/render.rs` — Interface right-column buttons; `render_detail`; `render_result`.
- **Modify** `src/tui/app.rs` — `run_effect` arms for the three new effects.
- **Modify** `tests/tui.rs` — snapshots + behavior tests for each action + a call capstone.

---

## Task 1: tui-input dep + Detail/Result scaffolds + Interface action buttons + enriched MethodMember

**Files:** `Cargo.toml`, `src/tui/state.rs`, `src/tui/msg.rs`, `src/tui/update.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

Introduce the new types/variants, the right-column action buttons with `Tab` focus, and wire `run_effect` for the three actions. Detail/Result render as placeholders this task; the real Detail form (Task 2/3) and Result body (Task 2/4) come next. `Enter` on an action button pushes a stub `Detail` screen so the nav stack is exercisable.

- [ ] **Step 1: add tui-input**

In `Cargo.toml` `[dependencies]` add `tui-input = "0.15"`. Run `cargo build` to confirm it resolves against ratatui 0.30.

- [ ] **Step 2: enrich method members to carry in-args**

In `src/tui/state.rs`, replace the `methods: Vec<(String, String)>` field of `InterfaceScreen` with `methods: Vec<MethodMember>`, and add the struct:

```rust
/// One method of an interface, for the Interface screen's methods column and the
/// call Detail form. `signature` is the concatenated IN-arg signature (display);
/// `args` is one (name, signature) per IN-arg (the Detail form's input fields).
#[derive(Clone, Debug)]
pub struct MethodMember {
    pub name: String,
    pub signature: String,
    pub args: Vec<(String, String)>,
}
```

(Adjust `members_of` in `update.rs` — Task 2 Step 1 — to populate `args`; for Task 1 leave `members_of` producing `args: vec![]` so it compiles, then Task 2 fills it. Update the Interface render + the Interface snapshot test's literal to use `MethodMember { name, signature, args }` instead of `(name, signature)`.)

- [ ] **Step 3: add Detail/Result Screen variants + structs**

In `src/tui/state.rs`:

```rust
pub enum Screen {
    Service(ServiceScreen),
    Objects(ObjectsScreen),
    Interfaces(InterfacesScreen),
    Interface(InterfaceScreen),
    Detail(DetailScreen),
    Result(ResultScreen),
}

/// An action form. Method call: one `tui-input` per IN-arg. Property set: one
/// input. Property get: no inputs (just confirm).
pub struct DetailScreen {
    pub service: String,
    pub object: String,
    pub interface: String,
    pub kind: ActionKind,
    /// One input per form field (call args / set value). Empty for get.
    pub inputs: Vec<tui_input::Input>,
    pub field_labels: Vec<String>, // "name  sig" per input, for display
    pub focus: DetailFocus,
    pub loading: bool,
    pub error: Option<String>,
}

/// What the Detail form triggers.
#[derive(Clone, Debug)]
pub enum ActionKind {
    /// method call: name + concatenated IN-signature (for call_method).
    Call { method: String, signature: String },
    /// property get: name.
    Get { property: String },
    /// property set: name + the property's signature.
    Set { property: String, signature: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DetailFocus {
    #[default]
    Field,
    Trigger, // the `[触发]` button
}

/// The outcome of a one-shot action. Pushed (loading) when the action is
/// triggered; populated by `Msg::ActionResult`.
pub struct ResultScreen {
    pub title: String, // e.g. "org.busx.Test.Ping" or "volume"
    pub result: Option<ActionResult>,
    pub error: Option<String>,
    pub loading: bool,
    pub scroll: usize,
}

/// The typed payload of an action result (shared by call/get/set).
#[derive(Clone, Debug)]
pub enum ActionResult {
    Call(Vec<String>), // each reply value, pretty-printed
    Get(String),       // the property value, pretty-printed
    Set,               // success (no payload)
}
```

- [ ] **Step 4: add Effects + ActionResult Msg**

In `src/tui/msg.rs`:

```rust
pub enum Effect {
    FetchServices,
    FetchObjects(String),
    FetchInterfaces(String, String),
    FetchProperties(String, String, String),
    CallMethod { service: String, object: String, iface: String, method: String, signature: String, args: Vec<String> },
    GetProperty { service: String, object: String, iface: String, property: String },
    SetProperty { service: String, object: String, iface: String, property: String, signature: String, value: String },
}
```

Move `ActionResult` to `msg.rs` (or keep in `state.rs` and re-export — pick one; `state.rs` is fine since `ResultScreen` uses it) and add the Msg:

```rust
    /// A one-shot action (call/get/set) completed.
    ActionResult(Result<ActionResult, String>),
```

- [ ] **Step 5: Interface action buttons + Tab focus (lists ↔ buttons)**

In `src/tui/state.rs`, add a focus field to `InterfaceScreen`:

```rust
pub struct InterfaceScreen {
    // ... existing fields ...
    pub focus: InterfaceFocus2, // lists vs buttons
}
```
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InterfaceFocus2 {
    #[default]
    Lists,   // the methods/properties/signals columns
    Buttons, // the right-column action buttons
}
```
(If clippy complains about the `2` suffix, name it `InterfaceArea` / `Pane` — your call, just be consistent.)

In `update.rs`, `update_interface_key`: `Tab` toggles `InterfaceFocus2` (Lists↔Buttons). When focus is Buttons, `↑↓` moves a `button_selected` index within the current member's action list; `Enter` on a button pushes a stub `DetailScreen` (Task 2 fills the form). When focus is Lists, the existing column nav applies. Compute the available buttons from the focused column + selected member: method → `[调用]`; property → `[读取]`, `[设置]`; signal → (none in Phase 3).

```rust
fn interface_buttons(i: &InterfaceScreen) -> Vec<&'static str> {
    match i.focus_of_column() {
        ColumnKind::Methods => vec!["调用"],
        ColumnKind::Properties => vec!["读取", "设置"],
        ColumnKind::Signals => vec![],
    }
}
```
(`focus_of_column` = which of methods/properties/signals `i.focus` (the `InterfaceFocus` from Phase 2) points at. Reuse the Phase-2 `InterfaceFocus` for the within-columns focus and add `InterfaceFocus2` only for the Lists↔Buttons split. `Enter` on a button in Task 1 pushes a stub `DetailScreen { kind: <inferred>, inputs: vec![], .., loading: false }` and returns `None` — Task 2 builds the real form + returns the Effect.)

- [ ] **Step 6: render the button column + placeholder Detail/Result**

In `render.rs`, the Interface screen splits `main` horizontally: left = the existing three stacked lists (e.g. `Constraint::Percentage(72)`), right = a `List` of action buttons (`Percentage(28)`), with the buttons column titled `▶ actions` and BOLD border when `InterfaceFocus2::Buttons`. `render_detail` / `render_result` render a placeholder ("Detail (loading…)" / "Result") for Task 1.

- [ ] **Step 7: wire run_effect for the three actions**

In `src/tui/app.rs` `run_effect`, add arms (the bodies are simple; they call the core and send `Msg::ActionResult`):

```rust
        Effect::CallMethod { service, object, iface, method, signature, args } => {
            async_global_executor::spawn(async move {
                let res = dbus::call::call_method(&conn, &service, &object, &iface, &method, &signature, &args).await;
                let _ = tx.send(Msg::ActionResult(res.map(|vs| ActionResult::Call(
                    vs.iter().map(|v| crate::value::pretty::pretty(v)).collect())).map_err(|e| e.to_string())));
            }).detach();
        }
        Effect::GetProperty { service, object, iface, property } => {
            async_global_executor::spawn(async move {
                let res = dbus::property::get_one(&conn, &service, &object, &iface, &property).await;
                let _ = tx.send(Msg::ActionResult(res.map(|v| ActionResult::Get(pretty(&v))).map_err(|e| e.to_string())));
            }).detach();
        }
        Effect::SetProperty { service, object, iface, property, signature, value } => {
            async_global_executor::spawn(async move {
                let res = dbus::property::set(&conn, &service, &object, &iface, &property, &signature, &[value]).await;
                let _ = tx.send(Msg::ActionResult(res.map(|_| ActionResult::Set).map_err(|e| e.to_string())));
            }).detach();
        }
```
(`pretty` = a thin local `fn pretty(v: &OwnedValue) -> String { crate::value::pretty::pretty(v) }`, or inline it.)

- [ ] **Step 8: tests + snapshots + commit**

Add an Interface snapshot showing the button column (a method selected → `▶ actions` / `调用`), and a behavior test that `Tab` moves focus Lists→Buttons and `Enter` on `调用` pushes a Detail screen. Regenerate the existing Interface snapshot (new right column). Run `INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings`, then commit:

```bash
git commit -m "feat(busx): tui action buttons + Detail/Result scaffolds (Phase 3)

Assisted-by: claude:glm-5.2"
```

---

## Task 2: Method call — Detail form (per-arg tui-input) + Result

**Files:** `src/tui/update.rs`, `src/tui/render.rs`, `tests/tui.rs`.

`members_of` populates `MethodMember.args` (per IN-arg name+sig). `Enter` on a method's `[调用]` button builds a `DetailScreen` with one `tui-input` per IN-arg (labeled "name sig", or just "sig" if the arg is anonymous), `Tab` moves field↔`[触发]`, typing edits the focused input, and `Enter` on `[触发]` pushes a `ResultScreen` (loading) + returns `Effect::CallMethod`. `ActionResult(Ok(Call(lines)))` populates the Result; it renders one pretty value per line, scrollable with `↑↓`.

- [ ] **Step 1: populate method args in `members_of`**

In `update.rs` `members_of`, for each method collect its IN-args:

```rust
    let methods = iface.methods().iter().map(|m| {
        let mut args = Vec::new();
        let mut signature = String::new();
        for a in m.args() {
            if a.direction() == Some(ArgDirection::In) {
                let sig = sig_str(a.ty());
                let name = a.name().unwrap_or("").to_string();
                signature.push_str(&sig);
                args.push((name, sig));
            }
        }
        MethodMember { name: m.name().to_string(), signature, args }
    }).collect();
```
(Return type of `members_of` becomes `(Vec<MethodMember>, Vec<(String,String,String)>, Vec<(String,String)>)` — update the `Members` type alias / `InterfaceScreen.methods` accordingly.)

- [ ] **Step 2: build the call Detail on `[调用]` Enter**

In `update.rs`, when `Enter` fires on the `调用` button (top is Interface, `InterfaceFocus2::Buttons`), find the selected method, read its `args`, and push:

```rust
fn push_call_detail(state: &mut State) -> Option<Effect> {
    let (svc, obj, iface, method, signature, fields) = match state.top() {
        Screen::Interface(i) if i.focus2 == InterfaceFocus2::Buttons && i.focus == InterfaceFocus::Methods => {
            let m = i.methods.get(i.selected[0])?;
            (i.service.clone(), i.object.clone(), i.interface.clone(),
             m.name.clone(), m.signature.clone(),
             m.args.iter().map(|(n, s)| format!("{n}  {s}")).collect::<Vec<_>>())
        }
        _ => return None,
    };
    let inputs = match state.top() {
        Screen::Interface(i) => i.methods.get(i.selected[0]).map(|m| m.args.iter()
            .map(|(_, _)| tui_input::Input::default()).collect::<Vec<_>>()).unwrap_or_default(),
        _ => vec![],
    };
    state.screens.push(Screen::Detail(DetailScreen {
        service: svc, object: obj, interface: iface,
        kind: ActionKind::Call { method, signature },
        field_labels: fields, inputs, focus: DetailFocus::Field,
        loading: false, error: None,
    }));
    None // the user fills the form; `[触发]` (Task 2 Step 4) returns the Effect
}
```
(Extract-then-mutate: read `state.top()` immutably to gather owned data, release, then `push`. The double `match state.top()` is to avoid holding the borrow across the push; collapse into one if NLL allows.)

- [ ] **Step 3: Detail key handling (tui-input editing + Tab + Esc)**

`update_detail_key`:
- `Tab` toggles `DetailFocus` (Field↔Trigger); within Field, if multiple inputs, `Tab` could also cycle fields — keep it simple: Field focuses "the inputs" and `↑↓`/`Tab`-within-fields cycles the active input index (add `field_selected: usize` to `DetailScreen` if you want multi-field cycling; otherwise edit only input[0] — **decide**: multi-field is the spec's intent, so add `field_selected`).
- When `DetailFocus::Field`: feed the key to `inputs[field_selected].handle_event(&Event::Key(k))` (ignore the `bool` it returns).
- `Enter` when `DetailFocus::Trigger` → trigger (Step 4).
- `Esc` pops.

- [ ] **Step 4: trigger → Result + Effect::CallMethod**

On `[触发]` Enter for a `Call` kind:

```rust
fn trigger_call(state: &mut State) -> Option<Effect> {
    let (svc, obj, iface, method, signature, args) = match state.top() {
        Screen::Detail(d) => match &d.kind {
            ActionKind::Call { method, signature } => {
                let args = d.inputs.iter().map(|i| i.value().to_string()).collect();
                (d.service.clone(), d.object.clone(), d.interface.clone(), method.clone(), signature.clone(), args)
            }
            _ => return None,
        },
        _ => return None,
    };
    state.screens.push(Screen::Result(ResultScreen {
        title: format!("{iface}.{method}"), result: None, error: None, loading: true, scroll: 0,
    }));
    Some(Effect::CallMethod { service: svc, object: obj, iface, method, signature, args })
}
```

- [ ] **Step 5: `load_action_result` populates the Result**

```rust
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
```

- [ ] **Step 6: render the Detail form + Result body**

`render_detail`: list the `field_labels` with their `inputs[i].value()`; highlight the focused field (REVERSED); draw a `[触发]` button row (BOLD when `DetailFocus::Trigger`). `render_result`: if loading → "…"; if error → "error: {e}"; else match `ActionResult` (Call → one pretty line per element; Get → the value; Set → "ok"). `↑↓` adjust `r.scroll` (clamp to line count).

- [ ] **Step 7: tests + commit**

Behavior: type into a call Detail's arg field (assert `inputs[0].value()`), `Tab` to Trigger, `Enter` → assert a Result screen pushed + `Effect::CallMethod` with the typed arg; feed `Msg::ActionResult(Ok(Call(vec!["42"])))` and assert the Result holds it. Snapshot the Detail form + Result. Commit:

```bash
git commit -m "feat(busx): tui method-call Detail form + Result

Assisted-by: claude:glm-5.2"
```

---

## Task 3: Property get / set — Detail + Result

**Files:** `src/tui/update.rs`, `src/tui/render.rs`, `tests/tui.rs`.

Both go through Detail (uniform with method call — a zero-input action just shows `[触发]` with no fields; this is the same shape as a zero-arg method call). `[读取]` → `DetailScreen { kind: Get { property }, inputs: vec![] }`; `[触发]` → `Effect::GetProperty` → Result. `[设置]` → `DetailScreen { kind: Set { property, signature }, inputs: vec![Input::default()] }` (label = the property signature); `[触发]` → `Effect::SetProperty` → Result. Reuse the Result screen (`ActionResult::Get` / `Set`).

- [ ] **Step 1: get → Detail (0 inputs), trigger → GetProperty**

`Enter` on `读取` pushes a Detail with `ActionKind::Get { property }`, `inputs: vec![]`, `field_labels: vec![]`. `trigger` for `Get` (in `handle_enter`/the trigger path) returns `Effect::GetProperty { service, object, iface, property }` and pushes the Result screen (loading). No form interaction is needed — the user `Tab`s straight to `[触发]` and `Enter`s. (Render: a Detail with 0 inputs just shows the `[触发]` button.)

- [ ] **Step 2: set → Detail (one input), trigger → SetProperty**

`Enter` on `设置` → push `DetailScreen { kind: ActionKind::Set { property, signature }, inputs: vec![Input::default()], field_labels: vec![signature], .. }`. `trigger` for `Set` returns `Effect::SetProperty { .. value: inputs[0].value().to_string() }`. Add a `trigger_set` alongside `trigger_call`.

- [ ] **Step 3: tests + commit**

Behavior: `读取` → Detail pushed (0 inputs); `Tab`→`Enter` → Result pushed + `Effect::GetProperty`; feed `ActionResult(Ok(Get("0.5")))` → Result holds it. `设置` → Detail with one input; type, `Tab`→`Enter` → `Effect::SetProperty` with the typed value. Commit:

```bash
git commit -m "feat(busx): tui property get/set Detail + Result

Assisted-by: claude:glm-5.2"
```

---

## Task 4: Result polish (scroll / errors / loading) + capstone

**Files:** `src/tui/render.rs`, `src/tui/update.rs`, `tests/tui.rs`.

Finalize the Result screen: clamp scroll to content, render the loading + error states clearly, and add a Phase-3 capstone loop test that drives a full method call (Interface → `[调用]` → type an arg → `[触发]` → scripted `ActionResult` → Result) through `run_loop` with the no-op handler, snapshotting the Result frame.

- [ ] **Step 1: scroll clamp + states**

`update_result_key`: `↑↓` adjust `r.scroll`, clamped to `max(0, line_count.saturating_sub(visible_rows))`. `render_result`: loading → "calling…"/"getting…"; error → the D-Bus error string; success → the pretty lines, offset by `scroll`.

- [ ] **Step 2: capstone loop test**

Append to `tests/tui.rs` a test that builds an Interface screen for a one-arg method, scripts `Tab`→(buttons)→`Enter`(调用)→type the arg→`Tab`→`Enter`(触发)→`ActionResult(Ok(Call(vec!]...])))`, runs `run_loop(.., |_| {})`, and snapshots the Result frame. Assert the stack top is `Screen::Result` with the call's return value.

- [ ] **Step 3: suite + clippy + commit**

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings
git commit -m "test(busx): tui action capstone + Result polish

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist

- **Spec coverage:** Interface action buttons (§8 — `[调用]`/`[读取]`/`[设置]`; `[监听]` deferred to Phase 4) ✓ T1; method-call Detail (per-arg tui-input) + Result ✓ T2; property get/set Detail + Result ✓ T3; Result scroll/error/loading (§8 Result, §11 errors) ✓ T4. Deferred (explicitly out of Phase 3 per §14): monitor/listen (Phase 4), copy-as + clipboard (Phase 5), JSON-view toggle + `?` help (Phase 6).
- **Placeholders:** the Detail multi-field focus (`field_selected`) and the Result scroll clamp are named concretely. `members_of`'s arg extraction is concrete (not a stub). tui-input integration (`handle_event(&Event::Key(k))`, render via `Paragraph` of `value()`) is spelled out.
- **Type consistency:** `MethodMember`, `ActionKind`, `DetailFocus`, `InterfaceFocus2`, `ActionResult`, `DetailScreen`, `ResultScreen`, and the three `Effect` variants are defined in T1 and used throughout. `run_effect`'s three new arms match the Effect variants. `Msg::ActionResult(Result<ActionResult, String>)` is the single result channel.
- **Risk:** (1) `tui-input`'s exact 0.15 API — confirm `Input::default()`/`handle_event`/`value()` against the crate source (implementer verifies, as with tui-tree-widget). (2) The Interface horizontal split (lists | buttons) regenerates the Interface snapshot — expected. (3) `MethodMember` changes the `methods` field type — update the Phase-2 Interface render + its snapshot-test literal. (4) The "Lists↔Buttons" focus adds a second focus dimension to the Interface screen — keep the Phase-2 `InterfaceFocus` (within-columns) and add `InterfaceFocus2` (Lists↔Buttons) without conflating them.

---

## Roadmap — remaining plans

4. **Listen + cancel** — signal / property / method listen; Result streaming; `Esc`-leaves-stops. *(Migrates `ops/monitor.rs` to the async core; the `[监听]` buttons appear here.)*
5. **copy-as + clipboard** — dbus-send/busctl/qdbus/gdbus; `arboard`; the `[copy as ▾]` button on Detail/Result.
6. **Polish** — `?` help, error toasts, JSON view toggle, column sizing, empty/edge states, snapshot coverage.
