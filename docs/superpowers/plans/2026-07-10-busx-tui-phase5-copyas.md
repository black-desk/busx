<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — Phase 5: copy-as + clipboard

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From a Detail or Result screen, `[copy as ▾]` opens a popup listing the four tools (dbus-send / busctl / qdbus / gdbus) with a live preview of the generated command for the current operation; `↑↓` picks a tool, `Enter` copies it to the system clipboard (`arboard`), `Esc` cancels. A tool that can't express the operation is shown greyed with a reason. Result also gets a `[Copy]` that copies the result text directly.

**Architecture:** A new pure module `src/tui/copy.rs` turns an operation descriptor (`CopyOp`) into each tool's command string (`generate(op, tool) -> Option<String>`, `None` = unsupported). The popup is a modal overlay: `State.popup: Option<CopyAsPopup>` holds the op + the precomputed per-tool commands + a selected index; when open, `update_key` routes keys to the popup (`↑↓`/`Enter`/`Esc`) before the underlying screen. Copying is a side effect, so `Enter` returns `Effect::CopyToClipboard(String)`; the production `on_effect` closure writes it via `arboard` (tests pass `|_| {}`, bus/display-free — the popup snapshot is the coverage, the actual clipboard write is acceptance-tested).

**Tech Stack:** ratatui 0.30 (`Clear` + centered `Block` for the popup) · **arboard 3.6** (new — system clipboard) · the pure `tui/copy.rs` · insta snapshots.

**Spec:** `docs/superpowers/specs/2026-07-08-busx-tui-design.md` (§10 copy-as generation, §8 `[copy as ▾]`/`[Copy]`). Built on Phases 3–4's Detail/Result screens + `Effect` machinery.

---

## Conventions

- REUSE SPDX header; commit ends with blank line + `Assisted-by: claude:glm-5.2`. No `highlight_symbol`. `cargo clippy --all-targets -- -D warnings` clean.
- Testing: TestBackend + insta for the popup (preview the generated command); direct assertions on `tui/copy::generate` for the command strings. `arboard` is NOT exercised in tests (no display) — `Effect::CopyToClipboard` is the injectable seam; production writes via arboard, acceptance-tested by the user.
- **busx args are busctl-style** (`src/value/encode.rs`: signature separate; tokens positional — basic→one token, variant→inner-sig+value, array→count+N, dict→count+N pairs, struct→flat). So **busctl copy-as is 1:1** (signature + tokens map directly); the other tools convert (basic types exact; complex types best-effort — dbus-send/qdbus/gdbus have limited/different nested syntax).

---

## Task 1: `tui/copy.rs` — command generation (pure)

**Files:** Create `src/tui/copy.rs`; modify `src/tui/mod.rs` (re-export). Tests in `tests/tui.rs` (or a `tests/copy.rs`).

- [ ] **Step 1: the `CopyOp` descriptor + `Tool` enum**

```rust
/// An operation we can render as another tool's command.
#[derive(Clone, Debug)]
pub enum CopyOp {
    Call { service: String, object: String, iface: String, method: String, signature: String, args: Vec<String> },
    Get { service: String, object: String, iface: String, property: String },
    Set { service: String, object: String, iface: String, property: String, signature: String, value: Vec<String> },
    Listen { rule: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool { DbusSend, Busctl, Qdbus, Gdbus }

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::DbusSend, Tool::Busctl, Tool::Qdbus, Tool::Gdbus];
    pub fn name(self) -> &'static str { match self { Self::DbusSend => "dbus-send", Self::Busctl => "busctl", Self::Qdbus => "qdbus", Self::Gdbus => "gdbus" } }
}
```

- [ ] **Step 2: `generate(op, tool) -> Option<String>`** — `None` where the tool can't express the op.

Per-tool command shapes (spec §10; omit `--user`/`--system` bus flags — the user adds them):
- **method call** (signature = the method's IN-signature, args = busctl-style tokens):
  - busctl: `busctl call {service} {object} {iface} {method} {signature} {args...}` (args space-joined, each shell-quoted if it contains spaces — use a simple `quote` helper).
  - dbus-send: `dbus-send --print-reply --dest={service} {object} {iface}.{method} {args-as type:value}`. Convert each top-level signature type to a dbus-send `type:value` arg; for **basic** types exact (`string:`, `uint32:`, `boolean:`, `double:`, …); for complex types (arrays/structs/variants/dicts) emit a best-effort `type:value` and accept imperfection (dbus-send can't nest like busx) — **or** return the command with the basic args filled and a trailing `# dbus-send cannot express <sig>` note. Pick best-effort + note.
  - qdbus: `qdbus {service} {object} {iface}.{method} {args-as literals}` (qdbus infers types from introspection; emit the literal token values, dropping busctl count prefixes for arrays → best-effort).
  - gdbus: `gdbus call --session --dest {service} --object-path {object} --method {iface}.{method} {args-as GVariant text}` (best-effort for complex).
- **property get**:
  - busctl: `busctl get-property {service} {object} {iface} {property}`.
  - dbus-send: `dbus-send --print-reply --dest={service} {object} org.freedesktop.DBus.Properties Get string:{iface} string:{property}`.
  - qdbus: `qdbus {service} {object} {iface} {property}` (qdbus reads a property as `qdbus SVC /OBJ IFACE PROP`? — confirm qdbus syntax; if `getProperty` is needed, use `qdbus {service} {object} org.freedesktop.DBus.Properties.Get string:{iface} string:{property}`-style is not qdbus — qdbus uses `qdbus SVC /OBJ IFACE.PROP`? **Verify qdbus property syntax**; best-effort).
  - gdbus: `gdbus call --session --dest {service} --object-path {object} --method org.freedesktop.DBus.Properties.Get '<{iface}>' '<{property}>'` (GVariant `<s>`-style args).
- **property set**: analogous (`busctl set-property … SIG VALUE`; dbus-send/gdbus wrap `Properties.Set`; qdbus `setProperty` or limited).
- **listen**:
  - dbus-send (dbus-monitor): `dbus-monitor "{rule}"`.
  - busctl: `busctl monitor "{rule}"`.
  - qdbus: **`None`** (qdbus has no monitor).
  - gdbus: `gdbus monitor --session` (gdbus monitor is unfiltered — emit the bare command + a `# gdbus monitor is unfiltered (ignores the rule)` note, OR `None`; pick one and note it).

(For each tool/op, confirm the exact syntax against the tool's man page conventions; the test assertions pin the chosen form. Where a tool genuinely can't express the op, return `None` and the popup greys it.)

- [ ] **Step 3: a `quote` helper + a signature-splitting helper**

`quote(s) -> String`: wrap in `"` if it contains whitespace/`"`/special, escaping `"`. `split_signature(sig) -> Vec<String>`: split a D-Bus signature string into its top-level complete types (e.g. `"su"` → `["s","u"]`, `"as"` → `["as"]`, `"(ii)"` → `["(ii)"]`) — walk one complete type at a time (use `zvariant::Signature` parsing or a manual walker matching the encoder's approach).

- [ ] **Step 4: tests + commit**

Direct `generate` tests: a basic call (`Call{method:"Add", signature:"u", args:["42"]}`) → assert the exact busctl/dbus-send/qdbus/gdbus strings; a get; a set; a listen (qdbus → None). Commit:
```bash
git commit -m "feat(busx): tui copy-as command generation (dbus-send/busctl/qdbus/gdbus)

Assisted-by: claude:glm-5.2"
```

---

## Task 2: copy-as popup + `Effect::CopyToClipboard` + arboard

**Files:** `Cargo.toml`, `src/tui/state.rs`, `src/tui/msg.rs`, `src/tui/update.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

- [ ] **Step 1: add arboard**

In `Cargo.toml` `[dependencies]` add `arboard = "3.6"`. `cargo build` to confirm (it pulls a clipboard backend; on Linux it's X11/Wayland — fine for build, not for headless tests, which is why copy is behind `Effect::CopyToClipboard`).

- [ ] **Step 2: popup state**

In `src/tui/state.rs`:
```rust
pub struct State {
    pub screens: Vec<Screen>,
    pub quit: bool,
    pub popup: Option<CopyAsPopup>, // None = no popup open
}

pub struct CopyAsPopup {
    pub op: CopyOp,
    /// Per-tool generated command (None = unsupported → greyed).
    pub commands: [(Tool, Option<String>); 4],
    pub selected: usize,
}
```
(Add `popup: None` to every `State { .. }` literal — `loading_service`, `service`, and the tests. Re-export `CopyOp`/`Tool`/`CopyAsPopup`.)

- [ ] **Step 3: `[copy as ▾]` opens the popup; `Effect::CopyToClipboard`**

In `src/tui/msg.rs` add `Effect::CopyToClipboard(String)`. In `update.rs`, handle a `c` key (or the `[copy as ▾]` button on Detail/Result) when a Detail/Result is top: build the `CopyOp` from that screen's data (Call's service/object/iface/method/signature/args; Get/Set analogous; Listen's rule from the Result title / the listen op — store enough on the Result to rebuild the rule), precompute `commands` via `tui/copy::generate`, and set `state.popup = Some(CopyAsPopup { .. })`.

Popup keys (handled FIRST in `update_key`, before the screen dispatch, when `state.popup.is_some()`): `↑↓`/`jk` move `selected` (skipping `None` entries or allowing them but Enter no-ops); `Enter` on a `Some(cmd)` entry → `Effect::CopyToClipboard(cmd.clone())` + close popup (`state.popup = None`); `Esc` → close (no copy).

- [ ] **Step 4: production on_effect writes the clipboard**

In `src/tui/app.rs`, the `run()` `on_effect` closure now matches: `Effect::CopyToClipboard(s) => { let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(&s)); }`, else `run_effect(other, …)`. (`run_effect` itself does NOT handle `CopyToClipboard` — it's not a dbus op; only the closure intercepts it. Tests' `|_| {}` skips both.)

- [ ] **Step 5: render the popup (Clear + centered Block)**

In `render.rs`, after rendering the top screen, if `state.popup.is_some()`, render an overlay: `Clear` a centered `Rect` (e.g. 80% × 60%), then a `Block` titled "copy as" listing the 4 tools (each line: the tool name + status — the command if `Some`, "— (unsupported: <reason>)" if `None`); the selected line REVERSED; below the list, a preview Paragraph of the selected tool's command (or the unsupported reason). (Detail/Result get a `[copy as ▾]` button — render it in the action row; the keyhint adds "c copy-as".)

- [ ] **Step 6: tests + snapshot + commit**

Open the popup from a call Detail (`c` key) → assert `state.popup.is_some()` with the right `CopyOp` + 4 commands; `↓`/`Enter` → assert `Effect::CopyToClipboard(<busctl cmd>)` + popup closed. Snapshot the popup (4 tools, one selected, the preview). Commit:
```bash
git commit -m "feat(busx): tui copy-as popup + clipboard (arboard)

Assisted-by: claude:glm-5.2"
```

---

## Task 3: `[Copy]` result text + capstone + polish

**Files:** `src/tui/update.rs`, `src/tui/render.rs`, `tests/tui.rs`.

- [ ] **Step 1: `[Copy]` on Result** — a `c` key (or a `[Copy]` button) on a one-shot Result copies the result text (the pretty value / "ok"); on a streaming Result copies the joined message blocks. Returns `Effect::CopyToClipboard(text)`. (Distinguish `c` = copy-as-popup vs `C`/`Shift+c` = copy-text, OR a `[Copy]` button in the Result's action row + `c` = popup — decide and make the keyhint clear.)
- [ ] **Step 2: capstone** — a `run_loop` test: trigger a call → Result → open copy-as popup → select busctl → `Enter` → assert `Effect::CopyToClipboard` carries the busctl command. Snapshot the popup over the Result.
- [ ] **Step 3: suite + clippy + commit**
```bash
INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings
git commit -m "test(busx): tui copy-as capstone + Result [Copy] (Phase 5)

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist

- **Spec coverage:** `[copy as ▾]` popup on Detail/Result (§8) ✓ T2; four-tool generation (§10) ✓ T1; unsupported-greyed-with-reason (§10) ✓ T1/T2; `[Copy]` result text (§8) ✓ T3; clipboard via arboard (§12) ✓ T2. Deferred (Phase 6): JSON view, `?` help, the Phase-3 focus redesign.
- **Placeholders:** per-tool command shapes are concretely specified; the exact qdbus property syntax + gdbus GVariant arg format are flagged for the implementer to confirm (pin via tests). Complex-type conversion is scoped best-effort (basic exact) — documented.
- **Type consistency:** `CopyOp`/`Tool` (T1) used by the popup (T2); `Effect::CopyToClipboard` (T2) intercepted by the production closure, skipped by tests.
- **Risk:** (1) arboard in headless tests — MUST stay behind `Effect::CopyToClipboard` (never call arboard in `update`/render or in tests). (2) Popup key routing: `update_key` must check `state.popup` BEFORE the screen dispatch so `↑↓`/`Enter`/`Esc` go to the popup while open. (3) The `State.popup: None` field addition touches every `State { .. }` literal — update them all. (4) qdbus monitor / gdbus unfiltered monitor — return `None`/note per Step 2.

---

## Roadmap — remaining plan

6. **Polish** — `?` help overlay, error toasts, JSON view toggle, column sizing, the Phase-3 Interface focus redesign the user flagged, copy-as for complex types (full conversion), snapshot coverage.
