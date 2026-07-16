<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TODO

Follow-up items from the initial code review. Each has a **decided direction**
but is **not yet implemented**. Grouped by dependency; see "Suggested order" at
the bottom.

## A. Connection layer — go all-async

- Delete `src/conn.rs` (the blocking `connect`); make `src/dbus/conn.rs` (async)
  the single source of truth for the session→system fallback.
- `complete.rs`: connect via `block_on(async)` like the other ops.
- `ops/monitor.rs`: rewrite the streaming loop from blocking `MessageIterator`
  to async `MessageStream` + `select!` with a timer future.
- Delete the blocking `become_monitor` in `ops/monitor.rs`; reuse the async
  `dbus::monitor::become_monitor`. (This also resolves the blocking/async
  `become_monitor` duplication.)
- **Bug fixed for free**: `ops/monitor.rs`'s `--timeout` currently hangs forever
  when no messages arrive — the blocking iterator's `next()` dead-waits and the
  deadline check is inside the loop body, so it only runs after a message
  arrives. The async `select!` + timer path makes `--timeout` actually fire.

Rationale (verified against zbus 5.17): `blocking::MessageIterator` is just
`block_on(MessageStream)` in a coat — its `next()` is literally
`block_on(self.azync.next())`, zero functional gain over async. Git history
shows blocking was the original; the async core was added later for the TUI and
the two blocking consumers (`complete.rs`, `ops/monitor.rs`) were never
migrated.

## B. TUI state — split domain context from view state

- `State` holds a single `NavContext { service, object, interface }` — the
  single source of truth for the navigation context.
- Each `Screen` holds only view state (selection / scroll / focus / loading /
  error) plus its own level's list data (methods / paths / names) — **no copies
  of service/object/interface**.
- Bake the "screen stack never empty" invariant into the type: make `screens`
  private with `push_screen` / `pop_screen` / `top` / `top_mut` methods; delete
  the `expect("screen stack never empty")`.
- `render` receives `&NavContext` as a parameter (read freely); `Screen` structs
  must **not** store a `&NavContext` reference (avoids the self-referential
  lifetime trap).
- Impact: `state.rs` reshaped, `render.rs` sub-functions gain a `ctx` param,
  copy-op building + breadcrumb read from `ctx`. This structurally removes the
  ~65-clone borrow dance in `update.rs` (pushing a child screen no longer copies
  the parent's context strings).

## C. Effects & logging (do together — the logging sink is the prerequisite)

### Logging system

- Crate: `tracing` + `tracing-appender` (non-blocking writer thread; also
  captures zbus's internal tracing events — valuable for a D-Bus tool).
- Verbosity: **delete `--verbose`**, replace with `-v` / `-vv` / `-vvv`
  (`ArgAction::Count`): default WARN / `-v` INFO / `-vv` DEBUG / `-vvv` TRACE.
- Sinks: CLI → stderr only (no `--log`; use `2> file` for file capture). TUI →
  file, default `$XDG_CACHE_HOME/busx/busx.log` (fallback `~/.cache`), append;
  `--log=<path>` overrides.
- Level policy — principle: **don't log expected/normal events**.
  - WARN (default): genuinely unexpected problems (empty on a normal run, as
    desired).
  - DEBUG (`-vv`): expected degradation — session→system fallback, BecomeMonitor
    refused→signal subscription, dropped malformed messages, `get_all_by_one`
    skipping write-only props.
  - TRACE (`-vvv`): zbus internals.
- Details: respect `XDG_CACHE_HOME`; removing `--verbose` requires updating
  `cli.rs` and both README `--help` blocks.

### Clipboard routing + blocking

- Hard constraint: the clipboard write must run on a background thread (`spawn`
  / `spawn_blocking`), never synchronously on the loop thread — it's inherently
  blocking (subprocess + compositor round-trip), not `.await`.
- Move `write_to_clipboard` out of the `on_effect` interception closure into
  `run_effect` as a normal arm that spawns off-thread and sends
  `Msg::ClipboardResult` back — identical to the dbus effects.
- Result: the loop thread never blocks (the arboard fallback's no-timeout
  compositor hang degrades to "popup stuck on copying…" instead of freezing the
  TUI), and routing becomes a single exhaustive match — delete the interception
  closure and the dead `Effect::CopyToClipboard(_) => {}` arm. Optional: add a
  timeout.

### Error chain

- The 3 places that stringify a typed error into `Error::Msg(String)` should
  preserve the type instead: add a dedicated variant + `#[from]` for
  `zbus_xml::Error`; keep `#[source]` for context-wrapping errors (the
  connection errors in `conn`).
- Pure-validation `Msg(String)` (parse errors etc.) stays as-is.
- Goal: verbose mode can `walk .source()` to print the full cause chain. (CLI
  can go to stderr directly; TUI needs the logging sink above first.)

### Monitor silently dropped messages

- The `Some(Err(_)) => {}` arms in `app.rs` (listen) → `tracing::debug!` to the
  file. **DEBUG, not WARN** — occasional malformed messages are expected bus
  noise; only real anomalies hit WARN. (Rides on the logging sink.)

## D. CLI surface cleanup (independent)

### Delete the `tree` command

- Its capability will be replaced by a future TUI search.
- Delete: `cli.rs` `Tree` variant, `main.rs` `Command::Tree` arm, `ops/mod.rs`
  `pub mod tree`, `ops/tree.rs`, the 4 `tree` references in `complete.rs`,
  `tests/tree.rs`, the `tree` line in both README `--help` blocks.
- Keep: `dbus/tree.rs`'s `object_tree` + `ObjectNode` — the TUI Objects screen
  (`app.rs:207`) depends on it. Don't touch `ObjectNode` (the
  interfaces-count-vs-names question is moot once `tree` is deleted; the TUI
  doesn't need names either).
- When implementing, also add a one-line note in both READMEs that TUI search
  will replace `tree` (the search itself is implemented separately, see Future).

### `list` table — TTY-aware layout

- TTY: use `crossterm::terminal::size()` (already a dep; fall back to 80 if
  unavailable), lay out right-to-left — reserve PROCESS (the last column) a
  fixed slot of 15 (`/proc/<pid>/comm` is ≤15 bytes → ≤15 display cols is the
  safe upper bound; CJK is actually _narrower_ at 3 bytes/char, so 15 is the
  true max), PID natural width, NAME gets the remainder and is truncated to it
  (NAME is ASCII, so char-truncation == display-truncation, safe). Pad NAME/PID,
  don't pad PROCESS (trailing column). Total width is then always ≤ terminal
  width → Chinese process names never cause wrapping. This essentially replaces
  the hardcoded `NAME_CAP = 54` (= 80 − 15 − 7 − 4) with a dynamic value.
- Non-TTY: tab-separated output, no width calc, no alignment.
- Zero new dependencies (`IsTerminal` from std 1.70+, `terminal::size` from the
  existing crossterm).
- Leave `introspect`'s `.{:<16}` alone — validating malformed introspection is
  zbus's job, not ours.

## E. Completion — kill the hand-written Command mirror

- Verified against the current deps (clap 4.6.1 + clap_complete 4.6.7 +
  clap_derive 4.6.1): the `#[arg(add = ArgValueCompleter::new(...))]` derive
  attribute works. `Arg::add` exists (clap_builder `arg.rs:947`),
  `ArgValueCompleter` implements `ArgExt` (`engine/custom.rs:85`), and non-magic
  attribute names are forwarded to builder method calls by clap_derive
  (`attr.rs:78-104` + `item.rs:449`). The "derive has no `add` attribute"
  comment in `complete.rs` is stale.
- Fix: keep the completer closure logic in `complete.rs` exported as `pub fn`;
  add `#[arg(add = ...)]` to the positionals in `cli.rs`; delete the whole
  `command()` mirror + `SERVICE_SUBS` + per-subcommand mirror (~200 lines).
  `cli.rs` will `use clap_complete::ArgValueCompleter` (mild coupling, far
  better than a hand-maintained mirror). (Deleting `tree` naturally drops its
  mirror entry too.)

## Future (details undecided)

- **TUI search**: replace the deleted `tree` command — locate objects /
  interfaces under a service. Details TBD (what to search, how to trigger, which
  screen). Not implemented together with the `tree` deletion.

## Suggested order

A → C (logging first) → B → D / E. A and C untangle the monitor / clipboard /
error / logging knot; B's state refactor is cleaner once that's done; D and E
are independent cleanups.
