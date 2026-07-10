# busx TUI — Focus redesign + mouse support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the Interface screen's focus model (Tab cycles columns, Enter drills into the action buttons, Esc backs out — no more Tab-toggle / active_column), and add app-wide mouse support (click to select / click buttons to activate / scroll), with TestBackend tests for both.

**Architecture:** Interface focus: `InterfaceFocus` loses its `Buttons` variant; `InterfaceScreen` swaps `active_column` + the `Buttons` focus state for a single `in_buttons: bool` (the focused column is always `focus`; the button bar is a drill-in reached via Enter). Mouse: `EnableMouseCapture` + a new `Msg::Mouse`; render records each interactive widget's `Rect` + a `ClickTarget` into an out-param buffer (render stays a pure read of `&State`), the loop stores it in `State.click_targets`, and `update`'s mouse handler hit-tests a click against those targets.

**Tech Stack:** ratatui 0.30 · crossterm 0.29 (mouse events) · the existing Elm `state`/`update`/`render` + injectable `run_loop` · TestBackend + insta.

**Spec:** `docs/superpowers/specs/2026-07-10-busx-tui-focus-mouse-design.md`.

---

## Conventions

- REUSE SPDX header; commit ends with blank line + `Assisted-by: claude:glm-5.2`. No `highlight_symbol` (REVERSED only). `cargo clippy --all-targets -- -D warnings` clean.
- Testing: TestBackend + insta. Focus tests drive keys through `update`; mouse tests do `draw` (to populate `click_targets`) → feed `Msg::Mouse` at a coord read from a target's `Rect` → assert.
- The real crossterm mouse-capture path (`EnableMouseCapture`, real event reading) is acceptance-tested, not auto-tested (TestBackend scripts `Msg::Mouse` directly).

## File structure

- **Modify** `src/tui/state.rs` — drop `InterfaceFocus::Buttons` + `InterfaceScreen.active_column`; add `InterfaceScreen.in_buttons`; add `ClickTarget` enum + `State.click_targets`.
- **Modify** `src/tui/msg.rs` — add `Msg::Mouse(crossterm::event::MouseEvent)`.
- **Modify** `src/tui/update.rs` — rewrite `update_interface_key` (Tab/Enter/Esc/↑↓ per the new model); the global `Esc` special-cases Interface+`in_buttons`; add the `Msg::Mouse` hit-test handler.
- **Modify** `src/tui/render.rs` — `render` gains a `&mut Vec<(Rect, ClickTarget)>` out-param; each `render_*` records its interactive targets; `render_interface` highlight uses `in_buttons`.
- **Modify** `src/tui/app.rs` — `setup_terminal`/`restore_terminal` enable/disable mouse capture; `CrosstermSource` forwards `Event::Mouse`; `run_loop` stores the rendered targets into `State.click_targets`.
- **Modify** `tests/tui.rs` — update Interface literals (drop `active_column`, add `in_buttons`); add focus + mouse tests.

---

## Task 1: Interface focus redesign

**Files:** `src/tui/state.rs`, `src/tui/update.rs`, `src/tui/render.rs`, `tests/tui.rs`.

Drop the `Buttons` focus variant + `active_column`; add `in_buttons`; rewrite the Interface key handling so Tab cycles the 3 columns, Enter drills into the buttons, Esc backs out, ↑↓ selects within the focused region.

- [ ] **Step 1: state model**

In `src/tui/state.rs`:
- Remove the `Buttons` variant from `InterfaceFocus` (it becomes `Methods`/`Properties`/`Signals` only).
- In `InterfaceScreen`: remove `active_column`; add `pub in_buttons: bool`. Keep `focus`, `button_selected`, `selected: [usize; 3]`.
- Update every `InterfaceScreen { .. }` literal (the `interface_screen()` helper, `push_interface`, and all test literals) — drop `active_column`, add `in_buttons: false`.

- [ ] **Step 2: rewrite `update_interface_key`**

In `src/tui/update.rs`, `update_interface_key` now takes `(i: &mut InterfaceScreen, k: KeyEvent) -> Option<Effect>` and implements:
- `Tab` (no Shift): `i.in_buttons = false; i.focus = cycle_next(i.focus)` (Methods→Properties→Signals→Methods).
- `Shift+Tab` (`BackTab`, or `Tab` with `KeyModifiers::SHIFT`): `i.in_buttons = false; i.focus = cycle_prev(i.focus)`.
- `↑`/`k`: if `!i.in_buttons` → `i.selected[focus_idx] = saturating_sub(1)` (within the focused column, clamped to its length); if `i.in_buttons` → `i.button_selected = saturating_sub(1)` (clamped to `action_buttons(i.focus).len()`).
- `↓`/`j`: symmetric (`+1` clamped to last).
- `Enter`: if `!i.in_buttons` → `i.in_buttons = true; i.button_selected = 0; None`. If `i.in_buttons` → fire the selected button (same logic as the current `handle_enter` Interface-button arm: build the `ActionKind` from `i.focus` + `i.selected` + `action_buttons(i.focus)[i.button_selected]`, `push_detail`, return `None`).
- `r` (refresh): unchanged — `i.loading = true; return Some(Effect::FetchProperties { service, object, iface, property: <focused property, if focus==Properties> })`. (Keep the existing `r` behavior; it works regardless of `in_buttons`.)
- `c`/`y`: handled in `update_key` (global, before the screen dispatch) as today — no change here.

(`cycle_next`/`cycle_prev`/`focus_idx` are small helpers, or inline matches. `action_buttons(column)` already exists — it returns the per-column button list; reuse it.)

- [ ] **Step 3: Esc backs out of buttons before popping**

In `update_key`'s global `Esc` arm, BEFORE the `screens.pop()`, special-case Interface+buttons:
```rust
    if matches!(k.code, KeyCode::Esc) {
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
```
(So Esc on the Interface button bar → back to the column; Esc on a column → pop the screen, as before.)

- [ ] **Step 4: render highlight**

In `src/tui/render.rs` `render_interface`: the methods/properties/signals columns are highlighted (focused) when `!i.in_buttons && i.focus == <that column>`; the action-button bar is highlighted when `i.in_buttons`. The button bar always shows `action_buttons(i.focus)`'s actions for `i.selected[focus_idx]`'s member. (Replace the old `i.focus == InterfaceFocus::Buttons` / `i.active_column` reads with `i.in_buttons` / `i.focus`.) Update the keyhint to `"Tab column · ↑↓ select · Enter open · r refresh · Esc back"` (drop the "Shift+Tab column" / "Tab buttons" split — Tab now just cycles columns).

- [ ] **Step 5: tests + snapshots + commit**

Update the Phase 3–5 Interface tests: any that set `active_column` or `InterfaceFocus::Buttons` or asserted the old Tab-toggle behavior → rewrite for the new model (e.g. `interface_tab_cycles_focus` asserts Tab cycles Methods→Properties→Signals→Methods; a new `interface_enter_drills_into_buttons_then_esc_backs_out` test). Regenerate affected snapshots. Run `INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings`. Commit:
```bash
git commit -m "feat(busx): tui Interface focus redesign (Tab cycles columns, Enter drills into buttons)

Assisted-by: claude:glm-5.2"
```

---

## Task 2: Mouse infrastructure (capture + Msg::Mouse + click_targets + render out-param)

**Files:** `Cargo.toml` (if needed), `src/tui/state.rs`, `src/tui/msg.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

Wire mouse events end-to-end and have render record interactive targets. No hit-test logic yet (Task 3).

- [ ] **Step 1: `ClickTarget` + `State.click_targets`**

In `src/tui/state.rs`:
```rust
use ratatui::layout::Rect;

/// A clickable region recorded by render, mapping a screen rect to the action a
/// left-click there should perform (so `update`'s mouse handler can hit-test).
#[derive(Clone, Debug)]
pub enum ClickTarget {
    ServiceRow(usize),
    ObjectsRow(usize),
    InterfacesRow(usize),
    MethodRow(usize),
    PropertyRow(usize),
    SignalRow(usize),
    ActionButton(usize),
    DetailField(usize),
    DetailTrigger,
    PopupTool(usize),
}

pub struct State {
    pub screens: Vec<Screen>,
    pub quit: bool,
    pub popup: Option<CopyAsPopup>,
    /// Interactive widget rects from the last draw, for mouse hit-testing.
    /// Populated by the loop after `render` (render writes them to an out-param).
    pub click_targets: Vec<(Rect, ClickTarget)>,
}
```
Add `click_targets: Vec::new()` to every `State { .. }` literal (constructors + tests). Re-export `ClickTarget`.

- [ ] **Step 2: `Msg::Mouse`**

In `src/tui/msg.rs`:
```rust
pub enum Msg {
    // ... existing ...
    Mouse(crossterm::event::MouseEvent),
}
```

- [ ] **Step 3: render out-param — record targets**

Change `render`'s signature to take a targets buffer and thread it through:
```rust
pub fn render(frame: &mut Frame, state: &State, targets: &mut Vec<(Rect, ClickTarget)>) {
    // ... layout as before ...
    match state.top() {
        Screen::Service(s) => render_service(frame, main, s, targets),
        // ... each render_* gains `targets: &mut Vec<(Rect, ClickTarget)>` ...
    }
    // popup also records PopupTool targets when open.
}
```
Each `render_*` pushes the rect of each interactive widget it places:
- `render_service`: for service row `i`, after computing its `row_area`, `targets.push((row_area, ClickTarget::ServiceRow(i)))`.
- `render_objects` / `render_interfaces`: `ObjectsRow(i)` / `InterfacesRow(i)` per row.
- `render_interface`: per method/property/signal row → `MethodRow(i)`/`PropertyRow(i)`/`SignalRow(i)`; per action button → `ActionButton(i)`.
- `render_detail`: per field → `DetailField(i)`; the trigger row → `DetailTrigger`.
- `render_popup`: per tool row → `PopupTool(i)`.
(Use the SAME row rects the widgets render into. For stateful lists, the per-row rect is the one you'd compute for each item; if you currently render the whole list in one `render_stateful_widget` call without per-row rects, compute the row rects from the list area + item index + item height (1 for single-line rows).)

- [ ] **Step 4: update render callers**

- `run_loop`'s draw closure: `let mut targets: Vec<(Rect, ClickTarget)> = Vec::new(); terminal.draw(|f| render(f, &self.state, &mut targets))?; self.state.click_targets = targets;` (the closure borrows `&self.state` immutably; storing into `self.state.click_targets` happens after `draw` returns, so no borrow conflict).
- `render_to_string` test helper: `let mut targets = Vec::new(); term.draw(|f| render(f, state, &mut targets)); format!("{}", term.backend())` (throwaway `targets` — snapshots don't need them).
- Any other `render(...)` call site (grep `render(f` / `render(frame`).

- [ ] **Step 5: enable mouse capture + forward events**

In `src/tui/app.rs`:
- `setup_terminal`: after `enable_raw_mode` + `EnterAlternateScreen`, also `execute!(stdout, EnableMouseCapture)?`. (Import `crossterm::event::EnableMouseCapture`.)
- `restore_terminal`: `disable_raw_mode` + `LeaveAlternateScreen` + `DisableMouseCapture` (import `DisableMouseCapture`).
- `CrosstermSource::next` / `non_mouse`: forward `Event::Mouse(m) => Some(Msg::Mouse(m))` (rename `non_mouse` to `to_msg` if it now handles mouse too).
- `update`'s `Msg::Mouse(_)` arm: `None` for now (Task 3 fills the hit-test). (So the app compiles + mouse events flow but do nothing yet.)

- [ ] **Step 6: build + tests + clippy + commit**

`cargo test -q && cargo clippy --all-targets -- -D warnings` (snapshots unchanged — `targets` is metadata, not rendered). Commit:
```bash
git commit -m "feat(busx): tui mouse infrastructure — capture, Msg::Mouse, click_targets

Assisted-by: claude:glm-5.2"
```

---

## Task 3: Mouse hit-testing + interactions + tests

**Files:** `src/tui/update.rs`, `tests/tui.rs`.

Implement the `Msg::Mouse` handler: hit-test the click against `state.click_targets` and perform the equivalent of the keyboard action.

- [ ] **Step 1: the hit-test handler**

In `src/tui/update.rs`, replace the `Msg::Mouse(_) => None` stub:
```rust
        Msg::Mouse(ev) => handle_mouse(state, ev),
```
```rust
use crossterm::event::{MouseEvent, MouseEventKind, MouseButton};

fn handle_mouse(state: &mut State, ev: MouseEvent) -> Option<Effect> {
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let (col, row) = (ev.column, ev.row);
            // Find the topmost target containing the click (popup targets first if open).
            let target = hit_test(state, col, row);
            if let Some(t) = target { apply_click(state, t) } else { None }
        }
        MouseEventKind::ScrollDown => scroll(state, 1),
        MouseEventKind::ScrollUp => scroll(state, -1),
        _ => None,
    }
}

fn hit_test(state: &State, col: u16, row: u16) -> Option<ClickTarget> {
    // Popup targets take precedence (drawn on top).
    if state.popup.is_some() {
        if let Some((_, t)) = state.click_targets.iter().rev()
            .find(|(r, _)| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
                && matches!(t, ClickTarget::PopupTool(_))) {
            return Some(t.clone());
        }
    }
    state.click_targets.iter().rev()
        .find(|(r, _)| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height)
        .map(|(_, t)| t.clone())
}

fn apply_click(state: &mut State, t: ClickTarget) -> Option<Effect> {
    match t {
        ClickTarget::ServiceRow(i) => { if let Screen::Service(s) = state.top_mut() { s.selected = i; } None }
        ClickTarget::ObjectsRow(i) => { if let Screen::Objects(o) = state.top_mut() { o.selected = i; } None }
        ClickTarget::InterfacesRow(i) => { if let Screen::Interfaces(it) = state.top_mut() { it.selected = i; } None }
        ClickTarget::MethodRow(i) => { if let Screen::Interface(it) = state.top_mut() { it.focus = InterfaceFocus::Methods; it.in_buttons = false; it.selected[0] = i; } None }
        ClickTarget::PropertyRow(i) => { if let Screen::Interface(it) = state.top_mut() { it.focus = InterfaceFocus::Properties; it.in_buttons = false; it.selected[1] = i; } None }
        ClickTarget::SignalRow(i) => { if let Screen::Interface(it) = state.top_mut() { it.focus = InterfaceFocus::Signals; it.in_buttons = false; it.selected[2] = i; } None }
        ClickTarget::ActionButton(i) => {
            // Select the button + fire it (== Enter on the button bar).
            if let Screen::Interface(it) = state.top_mut() {
                it.in_buttons = true;
                it.button_selected = i;
            }
            // Reuse the Enter-fires-button path: synthesize an Enter key.
            // (handle_enter reads the top Interface screen's in_buttons/button_selected.)
            handle_enter(state)
        }
        ClickTarget::DetailField(i) => { if let Screen::Detail(d) = state.top_mut() { d.field_selected = i; d.focus = DetailFocus::Field; } None }
        ClickTarget::DetailTrigger => {
            if let Screen::Detail(d) = state.top_mut() { d.focus = DetailFocus::Trigger; }
            handle_enter(state) // Enter on trigger fires the action.
        }
        ClickTarget::PopupTool(i) => {
            if let Some(p) = state.popup.as_mut() { p.selected = i; }
            None // select-for-preview; copy via Enter.
        }
    }
}

fn scroll(state: &mut State, delta: i32) -> Option<Effect> {
    if let Screen::Result(r) = state.top_mut() {
        let lines = r.messages.len().max(r.result.is_some() as usize);
        r.scroll = (r.scroll as i32 + delta).clamp(0, lines.saturating_sub(1) as i32) as usize;
    }
    None
}
```
(Confirm `handle_enter` reads `in_buttons`/`button_selected` for the Interface-button-fire path — Task 1's rewrite put the button-fire in `update_interface_key`'s Enter. To reuse it from `apply_click`, either call `update_interface_key` with a synthesized Enter, or factor the button-fire into a helper `fire_interface_button(state) -> Option<Effect>` that both `update_interface_key`'s Enter and `apply_click`'s ActionButton call. Pick the helper — cleaner. Note this in the implementation.)

- [ ] **Step 2: mouse tests**

Add tests using the `draw → read click_targets → feed Msg::Mouse → assert` pattern:
```rust
#[test]
fn mouse_click_selects_service_row() {
    let mut state = State::service(vec![svc("a", None, None), svc("b", None, None), svc("c", None, None)]);
    let mut targets = Vec::new();
    let mut term = Terminal::new(TestBackend::new(60, 8)).unwrap();
    term.draw(|f| render(f, &state, &mut targets)).unwrap();
    state.click_targets = targets;
    // find ServiceRow(2)'s rect, click its center
    let rect = state.click_targets.iter().find(|(_, t)| matches!(t, ClickTarget::ServiceRow(2))).unwrap().0;
    update(&mut state, Msg::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + 1, row: rect.y, modifiers: KeyModifiers::NONE }));
    assert_eq!(selected_of(&state), 2);
}
```
Add analogous tests: click an Interface method row (selects + switches focus to Methods); click an `ActionButton` (fires — assert a Detail is pushed / an Effect returned); click a `DetailField` (focuses it); click a `PopupTool` (selects); scroll on a Result (changes `scroll`). For each, draw first to populate `click_targets`, then feed the `Msg::Mouse` at a coord read from the target's rect.

- [ ] **Step 3: suite + clippy + commit**

`INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings`. Commit:
```bash
git commit -m "feat(busx): tui mouse hit-testing + interactions (click select / click button / scroll)

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist

- **Spec coverage:** Interface focus redesign (Tab cycles columns, Enter drills, Esc backs out, drop active_column, §2) ✓ T1; mouse capture + Msg::Mouse + click_targets + render out-param (§3) ✓ T2; hit-test + interactions (click-select / click-button / scroll, §3) ✓ T3; focus tests ✓ T1; mouse tests (draw→click→assert, §4) ✓ T3. Non-goals (double-click, right-click, drag, hover, other-screen focus changes) — not built. ✓
- **Placeholders:** the hit-test handler is concrete (per-ClickTarget arms). The one flagged decision — reuse the button-fire path via a `fire_interface_button` helper (so `update_interface_key`'s Enter and `apply_click`'s ActionButton share it) — is named, not left vague.
- **Type consistency:** `ClickTarget` (T2) used by render (T2) + hit-test (T3). `State.click_targets` (T2) read by hit-test (T3). `Msg::Mouse` (T2) handled in T3. `InterfaceFocus` (no Buttons) + `in_buttons` (T1) used by render (T1) + hit-test (T3). render's `&mut Vec<(Rect, ClickTarget)>` out-param (T2) threaded through all `render_*`.
- **Risk:** (1) The render out-param ripples through every `render_*` + caller — mechanical but widespread; grep all `render(` call sites. (2) Per-row rects for stateful lists (List rendered via `render_stateful_widget` without per-row rects) — compute row rects from the list area + index. (3) `run_loop` storing `click_targets` after `draw` — the draw closure borrows `&self.state`; the store is after, no conflict. (4) `apply_click`'s `ActionButton`/`DetailTrigger` reuse the Enter-fire path — factor a shared helper, don't duplicate.

---

## Roadmap — remaining

This is a slice of Phase 6 (polish). Remaining Phase 6 items (separate plan): `?` help overlay, error toasts, JSON-view toggle, column sizing, complex-type copy-as conversion, the `eprintln!` audit.
