<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — Phase 4: Listen (signal / property / method monitor)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the Interface screen, `[监听]` on a method/property/signal opens a Detail (match-rule preview + `[监听]` trigger); triggering starts a **streaming Result** that appends each matching bus message (dbus-send-style block) as it arrives, and **`Esc` leaving the Result stops the listen**. Signal & property listen use a match-rule subscription on the main connection; method listen uses a **dedicated connection + `BecomeMonitor`** (it sees method_call/return/error; privileged — refused ⇒ error).

**Architecture:** A new `ActionKind::Listen { target, service, object, iface, member }` flows through the existing Detail → trigger → Result path. Triggering returns `Effect::Listen { … }`; `run_effect` spawns a **long-lived listen task** that (a) creates a `futures::channel::oneshot` cancel channel and sends `Msg::ListenStarted(cancel_tx)` back so the loop stores it on the Result, then (b) `futures::select!`-loops between the message stream and the cancel receiver, sending `Msg::ListenMessage(String)` per message. The `ResultScreen` gains a `cancel: Option<oneshot::Sender<()>>` + a `messages: Vec<String>` (streaming mode); `Esc`-popping the Result drops `cancel` → the task's select sees the disconnect → it exits. This keeps `update` pure and `on_effect: FnMut(Effect)` unchanged (tests still pass `|_| {}` and script `ListenStarted`/`ListenMessage` directly).

**Tech Stack:** ratatui 0.30 · **futures 0.3** (new — `select!`/`StreamExt`/`oneshot`) · the existing async `dbus::` core (extended with async monitor streaming) · insta snapshots.

**Spec:** `docs/superpowers/specs/2026-07-08-busx-tui-design.md` (§5 concurrency / cancel-via-drop / BecomeMonitor dedicated connection, §9 listen semantics, §14.4 phase scope). Built on Phase 3's Detail/Result/`Effect` machinery.

---

## Conventions

- REUSE SPDX header; commit ends with blank line + `Assisted-by: claude:glm-5.2`. No `highlight_symbol`. `cargo clippy --all-targets -- -D warnings` clean.
- Testing: TestBackend + insta, driving `update`/`run_loop` with `|_| {}` (no real bus); script `Msg::ListenStarted` + `Msg::ListenMessage` for streaming.
- **Confirmed reference**: `src/ops/monitor.rs` already has `msg_to_human(&zbus::Message) -> String` (the dbus-send-style block) and `become_monitor` (blocking). Phase 4 mirrors these into the async core. zbus async APIs available: `zbus::MessageStream` (a `Stream` of messages; `MessageStream::from(&conn)` for a monitor, `MessageStream::for_match_rule(rule, &conn, None)` — confirm exact async constructor — for a match-rule subscription), `zbus::fdo::MonitoringProxy::become_monitor(&rules, flags)` (async), `Proxy::receive_signal` (async, returns a `SignalStream`).

---

## Task 1: async monitor core (format_message + async become_monitor + futures dep)

**Files:** `Cargo.toml`, `src/dbus/monitor.rs`, `src/ops/monitor.rs`, `tests/` (CLI e2e unchanged — parity guard).

Move the pure message formatter to the core so the TUI reuses it, add the async `become_monitor`, and add `futures`. No TUI yet; the CLI keeps working (its e2e tests are the parity guard).

- [ ] **Step 1: add `futures`**

In `Cargo.toml` `[dependencies]` add `futures = "0.3"`.

- [ ] **Step 2: move `msg_to_human` to the core as `format_message`**

In `src/dbus/monitor.rs`, add a `pub fn format_message(m: &zbus::Message) -> String` that is exactly `ops/monitor.rs::msg_to_human` (the dbus-send-style block: line 1 = type + sender/destination/path; line 2 = interface/member/serial (+reply_serial/error); then each body arg pretty-printed). In `src/ops/monitor.rs`, replace the local `msg_to_human` with a call to `crate::dbus::monitor::format_message`. (Keep `msg_to_json` in `ops/` — it's CLI-only NDJSON; or move it too if convenient, but not required.)

- [ ] **Step 3: async `become_monitor`**

In `src/dbus/monitor.rs`, add:
```rust
use zbus::fdo::MonitoringProxy;
/// Async BecomeMonitor. After this the connection only receives messages.
pub async fn become_monitor(conn: &zbus::Connection, rule: Option<&MatchRule<'_>>) -> Result<()> {
    let proxy = MonitoringProxy::new(conn).await?;
    let rules: Vec<MatchRule<'_>> = match rule { Some(r) => vec![r.clone()], None => vec![] };
    proxy.become_monitor(&rules, 0).await?;
    Ok(())
}
```
(Confirm the async `MonitoringProxy::new` + `become_monitor` signatures against zbus 5.16/5.17 source — the blocking version in `ops/monitor.rs:208-219` is the reference. The async proxy's `become_monitor` takes `&self` or `self`; adjust.)

- [ ] **Step 4: build + CLI parity + commit**

`cargo build && cargo test -q && cargo clippy --all-targets -- -D warnings`. The CLI `busx monitor` e2e tests must still pass (format_message is byte-identical). Commit:
```bash
git commit -m "feat(busx): async monitor core — format_message + become_monitor

Assisted-by: claude:glm-5.2"
```

---

## Task 2: listen buttons + Listen Detail + Effect + cancel mechanism + streaming Result (signal/property)

**Files:** `src/tui/state.rs`, `src/tui/msg.rs`, `src/tui/update.rs`, `src/tui/render.rs`, `src/tui/app.rs`, `tests/tui.rs`.

Wire signal & property listen end-to-end (main-connection match-rule subscription). Method listen (BecomeMonitor) is Task 3.

- [ ] **Step 1: types — `ActionKind::Listen`, streaming `ResultScreen`, `ListenTarget`**

In `src/tui/state.rs`:
```rust
#[derive(Clone, Debug)]
pub enum ActionKind {
    Call { method: String, signature: String },
    Get { property: String },
    Set { property: String, signature: String },
    Listen { target: ListenTarget },
}

#[derive(Clone, Debug)]
pub enum ListenTarget {
    Signal { member: String },          // iface implied by the Interface screen
    Property { property: String },
    Method { member: String },
}
```
Extend `ResultScreen` for streaming:
```rust
pub struct ResultScreen {
    pub title: String,
    pub result: Option<ActionResult>,    // None while streaming (listen) / loading
    pub error: Option<String>,
    pub loading: bool,
    pub scroll: usize,
    /// streaming-listen mode: appended message blocks.
    pub messages: Vec<String>,
    /// The cancel sender for an active listen; dropped when this screen is
    /// popped (Esc) → the listen task's select exits.
    pub cancel: Option<futures::channel::oneshot::Sender<()>>,
}
```
(Update all `ResultScreen { .. }` literals — Phase 3's call/get/set triggers — to add `messages: vec![], cancel: None`.)

- [ ] **Step 2: `[监听]` buttons + Listen Detail**

`action_buttons(column)` in `render.rs` / `update.rs` now returns: Methods → `["调用", "监听"]`; Properties → `["读取", "设置", "监听"]`; Signals → `["监听"]`. `handle_enter`'s Interface-button arm builds `ActionKind::Listen { target: ListenTarget::{Method|Property|Signal} }` for the `监听` button (member/property name from the selected member). `push_detail` for a Listen builds a Detail with `inputs: vec![]` (no inputs) and a `field_labels` carrying the **match-rule preview** (build it via `dbus::monitor::build_match_rule(iface, member, path/object, …)` → `rule.to_string()`), shown by `render_detail` as a read-only line above `[触发]`.

- [ ] **Step 3: Listen trigger → streaming Result + `Effect::Listen`**

In `src/tui/msg.rs`:
```rust
pub enum Effect {
    // ... existing ...
    Listen { service: String, object: String, iface: String, target: ListenTarget },
}
```
In `handle_enter`'s Detail arm, add the `ActionKind::Listen { target }` case: push a `ResultScreen { title: <"listen iface.member"/etc>, result: None, error: None, loading: true, scroll: 0, messages: vec![], cancel: None }` and return `Effect::Listen { service, object, iface, target }`.

- [ ] **Step 4: `Msg::ListenStarted` + `Msg::ListenMessage` + handlers**

In `src/tui/msg.rs`:
```rust
pub enum Msg {
    // ... existing ...
    ListenStarted(futures::channel::oneshot::Sender<()>),
    ListenMessage(String),
}
```
In `update.rs`:
```rust
        Msg::ListenStarted(cancel) => {
            if let Screen::Result(r) = state.top_mut() { r.cancel = Some(cancel); r.loading = false; }
            None
        }
        Msg::ListenMessage(body) => {
            if let Screen::Result(r) = state.top_mut() { r.messages.push(body); }
            None
        }
```
(Esc-pop of the Result already happens globally; dropping `ResultScreen` drops `cancel` → task exits. No new Esc code.)

- [ ] **Step 5: `run_effect` Listen arm (signal/property, main connection)**

In `src/tui/app.rs` `run_effect`, add the `Effect::Listen` arm. It spawns a task that: builds the cancel oneshot, sends `Msg::ListenStarted(tx)`, builds the match rule (`dbus::monitor::build_match_rule(Some(iface), Some(member), Some(object), None, None, /*signals=*/ target is Signal)` — for property listen, subscribe `org.freedesktop.DBus.Properties.PropertiesChanged` and client-filter to the property), gets a `MessageStream` for the rule on the main `conn`, and `select!`-loops. Per message, `Msg::ListenMessage(crate::dbus::monitor::format_message(&m))`.

```rust
        Effect::Listen { service: _, object, iface, target } => {
            let (member_or_prop, is_signal) = match &target {
                ListenTarget::Signal { member } | ListenTarget::Method { member } => (member.clone(), true),
                ListenTarget::Property { property } => (property.clone(), false),
            };
            async_global_executor::spawn(async move {
                use futures::{FutureExt, StreamExt, select};
                let (cancel_tx, mut cancel_rx) = futures::channel::oneshot::channel::<()>();
                let _ = tx.send(Msg::ListenStarted(cancel_tx));
                let mut cancel_rx = cancel_rx.fuse();
                // Property listen → subscribe PropertiesChanged and filter; signal → match-rule.
                // (Build the stream via MessageStream::for_match_rule / receive_signal — confirm
                //  the exact async constructor against zbus; for property, match on the
                //  PropertiesChanged args to the named property.)
                let rule = build_listen_rule(&iface, &object, &member_or_prop, is_signal, &target);
                let stream = match zbus::MessageStream::for_match_rule(rule, &conn, None).await {
                    Ok(s) => s.fuse(),
                    Err(e) => { let _ = tx.send(Msg::ActionResult(Err(e.to_string()))); return; }
                };
                futures::pin_mut!(stream);
                loop {
                    select! {
                        msg = stream.next() => match msg {
                            Some(Ok(m)) => {
                                if listen_message_matches(&m, &target) {
                                    let _ = tx.send(Msg::ListenMessage(
                                        crate::dbus::monitor::format_message(&m)));
                                }
                            }
                            Some(Err(_)) => {} // drop malformed
                            None => break,
                        },
                        _ = cancel_rx => break, // Esc left the Result
                    }
                }
            }).detach();
        }
```
(`build_listen_rule` / `listen_message_matches` are small helpers: the rule selects the signal/method by iface+member+path; for property listen, subscribe `PropertiesChanged` and `listen_message_matches` checks the changed-property name in the message body. Confirm zbus `MessageStream::for_match_rule` async signature; if it differs, use `conn.add_match` + `MessageStream::from(&conn)` with client filtering, mirroring the CLI.)

- [ ] **Step 6: render the streaming Result**

In `render.rs` `render_result`: if `r.messages` is non-empty (streaming/listening), render the joined message blocks (offset by `scroll`); else fall back to the Phase-3 one-shot rendering (`result`/loading/error). `update_result_key`'s line-count for scroll clamping must count `messages.len()` (or total message lines) when streaming.

- [ ] **Step 7: tests + snapshot + commit**

Behavior (no real bus, `|_| {}`): push a Listen Detail, `[监听]` Enter → Result pushed + `Effect::Listen`; feed `Msg::ListenStarted(cancel_tx)` (store it on the Result), then `Msg::ListenMessage("signal …".into())` twice → assert `r.messages.len() == 2` and `r.cancel.is_some()`. Then send `Msg::Key(Esc)` → the Result is popped; assert the cancel sender was dropped (e.g. the `oneshot::Receiver` you kept in the test now resolves / a flag you set in a wrapper). Snapshot the streaming Result with two message blocks. Commit:
```bash
git commit -m "feat(busx): tui signal/property listen + streaming Result + Esc-stop

Assisted-by: claude:glm-5.2"
```

---

## Task 3: method listen (dedicated connection + BecomeMonitor)

**Files:** `src/tui/app.rs` (run_effect Listen arm — method branch), `tests/tui.rs`.

Method listen sees method_call/return/error, which needs `BecomeMonitor` on a **dedicated connection** (BecomeMonitor makes the connection recv-only; can't reuse the main one).

- [ ] **Step 1: dedicated connection + BecomeMonitor in the Listen arm**

When `target` is `ListenTarget::Method { .. }`, the spawned task builds a **fresh** connection (`dbus::conn::connect(...)` — needs the user/system/address; thread those through `Effect::Listen` or store on App), calls `dbus::monitor::become_monitor(&dedicated, Some(&rule))`, then streams from `MessageStream::from(&dedicated)` and client-filters to messages matching `(iface, member, object)`. If `become_monitor` errors (privileged, refused by the bus) → `Msg::ActionResult(Err("..BecomeMonitor refused.."))` (the Result shows the error) and the task exits.

(To get connect params into the task: extend `Effect::Listen` with `user/system/address` clones, OR — cleaner — have `run()`'s `on_effect` closure capture them alongside `conn` and pass a fresh connect into `run_effect`. Decide what's cleanest; the constraint is the method-listen task must build its OWN connection.)

- [ ] **Step 2: tests + commit**

The method-listen path can't easily run without a bus, so cover it with: a behavior test that the `监听` button on a method builds `ActionKind::Listen { target: Method }` + the right `Effect::Listen` (no real spawn). Optionally a CLI-parity note that `become_monitor` is exercised by the existing `busx monitor` e2e (Task 1's `become_monitor` is shared). Commit:
```bash
git commit -m "feat(busx): tui method listen (dedicated connection + BecomeMonitor)

Assisted-by: claude:glm-5.2"
```

---

## Task 4: capstone + polish

**Files:** `src/tui/render.rs`, `src/tui/update.rs`, `tests/tui.rs`.

- [ ] **Step 1: listen capstone loop test** — drive a signal listen through `run_loop` with `|_| {}`: Interface → `Tab`→buttons → `Enter`(监听) → Detail → `Tab`→`Enter`(触发) → Result; script `Msg::ListenStarted` + two `Msg::ListenMessage` → assert the Result streams both; `Msg::Key(Esc)` pops and the cancel is dropped. Snapshot the streaming frame.
- [ ] **Step 2: polish** — BecomeMonitor-refused error renders clearly on the Result; the streaming Result scroll clamps to total message lines; the keyhint for a streaming Result notes "Esc back/stop".
- [ ] **Step 3: suite + clippy + commit**
```bash
INSTA_UPDATE=always cargo test --test tui && cargo test -q && cargo clippy --all-targets -- -D warnings
git commit -m "test(busx): tui listen capstone + Result polish (Phase 4)

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist

- **Spec coverage:** `[监听]` buttons on method/property/signal (§8) ✓ T2; signal listen (main conn match-rule) ✓ T2; property listen (PropertiesChanged filter) ✓ T2; method listen (dedicated conn BecomeMonitor) ✓ T3; streaming Result (§8) ✓ T2; Esc-leaves-stops via cancel-via-drop (§5) ✓ T2; BecomeMonitor-refused error (§9) ✓ T3/T4. Deferred (Phase 5/6): copy-as of a listen command / copied message, JSON view.
- **Placeholders:** the listen task's `select!` + cancel + MessageStream construction is concrete (mirrors the CLI's blocking stream). The exact async `MessageStream::for_match_rule` / `from` constructor is flagged for the implementer to confirm against zbus (as with prior phases' API checks).
- **Type consistency:** `ActionKind::Listen`/`ListenTarget` (T2) used by the trigger (T2) + run_effect (T2/T3). `Msg::ListenStarted`/`ListenMessage` (T2). `ResultScreen.messages`/`cancel` (T2). `Effect::Listen` (T2).
- **Risk:** (1) `futures::select!` requires `Fuse`d stream/future — use `.fuse()` on both. (2) The cancel-via-`Msg::ListenStarted` round-trip: the task sends the sender back; the loop stores it on the Result; Esc drops the Result → drops the sender → task exits. Verify the fast-Esc edge (Result popped before `ListenStarted` processed ⇒ sender dropped in the handler ⇒ task exits, no leak). (3) Property listen needs body inspection to filter to the named property (PropertiesChanged carries an array of changed property names). (4) Method listen's dedicated connection needs the connect params — thread them cleanly.

---

## Roadmap — remaining plans

5. **copy-as + clipboard** — dbus-send/busctl/qdbus/gdbus; `arboard`; `[copy as ▾]` on Detail/Result.
6. **Polish** — `?` help, error toasts, JSON view toggle, column sizing, the Phase-3 Interface focus redesign the user flagged, snapshot coverage.
