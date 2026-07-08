<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — Phase 1: Skeleton + Service page

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bare `busx` (no subcommand) opens a fullscreen TUI showing the live service list; the user can move the selection with ↑↓/jk and quit with `q`/`Esc`. Existing CLI subcommands keep working.

**Architecture:** Elm-style split — pure `State` + `update(state, Msg)` + `render(frame, &State)` (verified by `ratatui` `TestBackend` snapshots), and a thin terminal glue `App::run` (raw-mode/alt-screen setup, crossterm poll, flume channel, `async_global_executor` task to fetch `dbus::list::list_names`) that is manual-smoke-tested. The async `dbus::` core from Phase 0 backs the data.

**Tech Stack:** ratatui 0.30 · crossterm 0.29 · flume 0.12 · async-global-executor 2 · the existing async `dbus::` core · insta (dev) for snapshots.

**Spec:** `docs/superpowers/specs/2026-07-08-busx-tui-design.md` (§5 concurrency, §6 Elm layers, §8 Service page + key-hint bar, §14 phase 1).

---

## Conventions

- REUSE SPDX header on every new file: copyright `2026 Chen Linxuan <me@black-desk.cn>`, license `GPL-3.0-or-later`.
- Every commit message ends with a blank line then exactly: `Assisted-by: claude:glm-5.2`
- **Testing:** e2e-only. TUI logic is tested via `ratatui::backend::TestBackend` snapshots (`insta`) — drive keys through `update`, render to an in-memory buffer, compare to a golden snapshot. This is the "key + screen" e2e dimension for the TUI (per spec §13). The real terminal loop (needs a tty) is manual-smoke-tested, not in the suite.
- **API notes (confirm against the compiler — ratatui/crossterm move fast):**
  - ratatui 0.30: `Terminal::new(backend)`, `terminal.draw(|frame: &mut Frame| ...)`, `frame.area() -> Rect`, `frame.render_widget/widget_ref`, `frame.render_stateful_widget`. `widgets::{Block, Borders, List, ListItem, ListState, Paragraph}`, `layout::{Layout, Constraint, Direction}`, `text::Line`, `style::{Style, Modifier}`. `backend::{CrosstermBackend, TestBackend}`.
  - crossterm 0.29: `terminal::{enable_raw_mode, disable_raw_mode}`, `execute!(stdout, EnterAlternateScreen)` / `LeaveAlternateScreen`, `event::{poll, read, Event, KeyEvent, KeyCode, KeyEventKind}`. `From<KeyCode> for KeyEvent` exists, so `KeyCode::Down.into()` works in tests.
  - flume 0.12: `flume::unbounded() -> (Sender, Receiver)`, `Sender::send`, `Receiver::try_recv`.
  - `async_global_executor::spawn(future).detach()` — fire-and-forget a task.
  - When a method/import path differs in the resolved version, use the equivalent; keep behaviour and the snapshot output stable.

## File structure (after Phase 1)

- **Create** `src/tui/mod.rs` — `pub mod app/render/state/msg/update;` + re-exports of `run`, `State`, `Msg`, `update`, `render`.
- **Create** `src/tui/state.rs` — `State { screen, quit }`, `Screen::Service(ServiceScreen)`, `ServiceScreen { services, selected, loading, error }`.
- **Create** `src/tui/msg.rs` — `Msg`.
- **Create** `src/tui/update.rs` — `pub fn update(state: &mut State, msg: Msg)`.
- **Create** `src/tui/render.rs` — `pub fn render(frame: &mut Frame, state: &State)` + per-screen + key-hint.
- **Create** `src/tui/app.rs` — `App` (loop driver) + `pub fn run(user, system, address, verbose) -> Result<()>` + terminal guard.
- **Modify** `src/cli.rs` — `command: Option<Command>`.
- **Modify** `src/main.rs` — `mod tui;`; None ⇒ `tui::run`, Some ⇒ CLI dispatch.
- **Create** `tests/tui.rs` — TestBackend + insta snapshot tests.
- **Modify** `Cargo.toml` — add ratatui, crossterm, tui-tree-widget, tui-input, arboard, flume, futures; dev-dep insta.

---

## Task 1: Dependencies + `command: Option<Command>` + tui stub

**Files:** Modify `Cargo.toml`, `src/cli.rs`, `src/main.rs`; Create `src/tui/mod.rs`.

- [ ] **Step 1: Add dependencies**

In `Cargo.toml` `[dependencies]` add:

```toml
ratatui = "0.30"
crossterm = "0.29"
tui-tree-widget = "0.24"
tui-input = "0.15"
arboard = "3"
flume = "0.12"
futures = "0.3"
```

In `[dev-dependencies]` add:

```toml
insta = "1"
```

- [ ] **Step 2: Make the subcommand optional**

In `src/cli.rs`, change the field:

```rust
    #[command(subcommand)]
    pub command: Option<Command>,
```

(Leave `Command` and all other fields unchanged.)

- [ ] **Step 3: Create the tui module + a temporary stub**

`src/tui/mod.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interactive TUI (spec §5–§8). Built on the async `dbus::` core.

pub mod app;
pub mod msg;
pub mod render;
pub mod state;
pub mod update;

pub use app::run;
pub use msg::Msg;
pub use render::render;
pub use state::{Screen, ServiceScreen, State};
pub use update::update;
```

Create `src/tui/{app,msg,render,state,update}.rs` each with just the SPDX header (3 lines) so the module compiles empty. `src/tui/app.rs` gets a temporary stub:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TUI entry point + event loop (spec §5). Real loop lands in Task 5.

use crate::error::Result;

/// Launch the TUI. Temporary stub — replaced by the real loop in Task 5.
pub fn run(_user: bool, _system: bool, _address: Option<&str>, _verbose: bool) -> Result<()> {
    eprintln!("busx: TUI under construction (phase 1)");
    Ok(())
}
```

- [ ] **Step 4: Wire bare `busx` → TUI in main**

In `src/main.rs`: add `mod tui;` to the module block, and change the dispatch in `run`/`main` so an absent subcommand enters the TUI. Concretely, rename the existing `fn run(cli: Cli)` body into `fn run_command(cli: Cli, command: Command)` that matches on `command`, and dispatch in `main`:

```rust
    let cli = Cli::parse();
    let result = match cli.command {
        None => tui::run(cli.user, cli.system, cli.address.as_deref(), cli.verbose),
        Some(command) => run_command(cli, command),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("busx: {e}");
            e.exit_code()
        }
    }
```

(The `run_command` body is the existing `match cli.command { … }` dispatch, unchanged except it now matches the `command` parameter instead of `cli.command`.)

- [ ] **Step 5: Build + existing CLI e2e green**

Run: `cargo build && cargo test -q`
Expected: builds; all existing CLI e2e pass (the CLI path is untouched; bare `busx` now hits the stub).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/cli.rs src/main.rs src/tui/
git commit -m "feat(busx): bare busx enters TUI; add tui deps + stub

Assisted-by: claude:glm-5.2"
```

---

## Task 2: Pure render core — `State`/`Screen` + `render()` + first snapshot

**Files:** Modify `src/tui/state.rs`, `src/tui/render.rs`; Create `tests/tui.rs`.

- [ ] **Step 1: Write the failing snapshot test**

`tests/tui.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TUI snapshot tests (spec §13). Drive the pure `State`/`update` core, render
//! to a ratatui `TestBackend`, compare to an insta golden snapshot. No real bus.

use busx::dbus::types::ServiceInfo;
use busx::tui::{State, render};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Render `state` to a `w`×`h` buffer and return its text view for insta.
/// `TestBackend`'s `Display` is ratatui's readable `buffer_view` (text only, no
/// styling) — exactly what we want to snapshot.
fn render_to_string(state: &State, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, state)).unwrap();
    format!("{}", term.backend())
}

fn svc(name: &str, pid: Option<u64>, process: Option<&str>) -> ServiceInfo {
    ServiceInfo { name: name.into(), pid, process: process.map(Into::into) }
}

#[test]
fn service_screen_renders_populated_list() {
    let state = State::service(vec![
        svc("org.busx.Test", Some(1234), Some("dbus-daemon")),
        svc("org.busx.Other", None, None),
    ]);
    insta::assert_snapshot!(render_to_string(&state, 60, 8));
}
```

> This test imports `busx::dbus::types::ServiceInfo`, `busx::tui::{Msg, State, render, update}`. Those require `crate::dbus` and `crate::tui` to be accessible from the integration test. `crate::dbus` is currently a private `mod dbus;` in `main.rs` — to expose it (and `tui`) to integration tests, add a tiny `src/lib.rs` (see Step 4) so `busx` is both a lib and a bin, OR gate these tests behind the lib. Step 4 makes the crate a lib+bin.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test tui`
Expected: FAIL (compile error — `State::service` / `render` don't exist yet; `busx::tui` not exported).

- [ ] **Step 3: Implement `State` + `Screen` + `ServiceScreen`**

`src/tui/state.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure display state (spec §6). `render` reads only this — never IO handles.

use crate::dbus::types::ServiceInfo;

#[derive(Default)]
pub struct State {
    pub screen: Screen,
    pub quit: bool,
}

pub enum Screen {
    Service(ServiceScreen),
    // Objects / Interfaces / Interface / Detail / Result arrive in later phases.
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Service(ServiceScreen::default())
    }
}

#[derive(Default)]
pub struct ServiceScreen {
    pub services: Vec<ServiceInfo>,
    /// Index into `services` of the highlighted row.
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

impl State {
    /// Build a `State` showing a populated Service screen (for tests / default).
    pub fn service(services: Vec<ServiceInfo>) -> Self {
        State { screen: Screen::Service(ServiceScreen { services, selected: 0, loading: false, error: None }), quit: false }
    }
}
```

- [ ] **Step 4: Make the crate a lib+bin so integration tests reach `dbus`/`tui`**

Integration tests are a separate crate and can only call library code, so expose the shared modules via `src/lib.rs`. This is the cleanest move: every module's internal `crate::error` / `crate::dbus` / `crate::value` reference keeps resolving unchanged (now `crate` = the `busx` lib); only `src/main.rs`'s top-level changes.

Create `src/lib.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! busx library root: the modules the `busx` binary and the integration tests
//! share (the async `dbus` core, the `tui`, and helpers). Not a published
//! library — an internal code/test-sharing surface (spec §5 "不发布 lib" is
//! honoured: the package isn't exposed as a library API).

pub mod cli;
pub mod complete;
pub mod conn;
pub mod dbus;
pub mod error;
pub mod ops;
pub mod out;
pub mod tui;
pub mod value;
```

In `src/main.rs`: delete ALL the `mod <name>;` lines (`mod cli;`, `mod complete;`, `mod conn;`, `mod dbus;`, `mod error;`, `mod ops;`, `mod out;`, `mod tui;`, `mod value;`) and replace them with one import at the top:

```rust
use busx::{cli::{self, Cli, Command}, complete, error, ops, tui};
```

Then make two small body fixes in `main.rs` so references no longer go through the bin's `crate::`:
- `crate::complete::emit_script(shell)` → `complete::emit_script(shell)`
- (every other reference — `ops::list::run`, `complete::try_complete()`, `tui::run`, `error::Result`, `Cli::parse`, `Command::…` — already uses the bare path and resolves via the `use`.)

`complete.rs`, `ops/*`, `conn.rs`, `out.rs`, `value/`, `dbus/`, `tui/`, `error.rs` are **unchanged** — their internal `crate::` references now resolve to the lib. Verify: `cargo build` (lib + bin both compile) and `cargo test -q` (all existing CLI e2e still pass).

- [ ] **Step 5: Implement `render`**

`src/tui/render.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure rendering (spec §6, §8). `render` reads `&State` and draws — nothing else.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::tui::state::{Screen, ServiceScreen, State};

pub fn render(frame: &mut Frame, state: &State) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let (main, footer) = (chunks[0], chunks[1]);

    match &state.screen {
        Screen::Service(s) => render_service(frame, main, s),
    }
    render_keyhint(frame, footer);
}

fn render_service(frame: &mut Frame, area: ratatui::layout::Rect, s: &ServiceScreen) {
    let title = if s.loading {
        "Services (loading…)"
    } else {
        "Services"
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    if let Some(err) = &s.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }

    let items: Vec<ListItem> = s
        .services
        .iter()
        .map(|sv| {
            let pid = sv.pid.map(|p| p.to_string()).unwrap_or_default();
            let proc = sv.process.clone().unwrap_or_default();
            ListItem::new(Line::from(format!("{:<32} {:>7} {}", sv.name, pid, proc)))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    if !s.services.is_empty() {
        list_state.select(Some(s.selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_keyhint(frame: &mut Frame, area: ratatui::layout::Rect) {
    frame.render_widget(Paragraph::new("↑↓ select · Enter open · q quit · ? help"), area);
}
```

- [ ] **Step 6: Generate + accept the snapshot, then run the test**

First run (generates the golden snapshot):

```bash
INSTA_UPDATE=always cargo test --test tui
```

Expected: PASS (creates `tests/snapshots/tui__service_screen_renders_populated_list.snap`). Inspect the generated `.snap` — it should show a bordered "Services" list with the two rows and the bottom key-hint line. Then re-run normally to confirm it's pinned:

```bash
cargo test --test tui
```

Expected: PASS.

- [ ] **Step 7: Build + full e2e + commit**

Run: `cargo build && cargo test -q`
Expected: all green.

```bash
git add src/lib.rs src/main.rs src/tui/state.rs src/tui/render.rs tests/tui.rs tests/snapshots/ Cargo.toml Cargo.lock
git commit -m "feat(busx): tui render core + Service screen snapshot test

Assisted-by: claude:glm-5.2"
```

---

## Task 3: `Msg` + `update()` — key navigation (snapshot-driven)

**Files:** Modify `src/tui/msg.rs`, `src/tui/update.rs`, `tests/tui.rs`.

- [ ] **Step 1: Write the failing navigation tests**

Append to `tests/tui.rs` (after the `svc` helper and `render_to_string` from Task 2). Task 2 imported only `State`/`render`; add the rest here:

```rust
use busx::tui::{Msg, Screen, update};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn selected_of(state: &State) -> usize {
    match &state.screen {
        Screen::Service(s) => s.selected,
    }
}

#[test]
fn service_screen_down_arrow_moves_selection() {
    let mut state = State::service(vec![svc("a", None, None), svc("b", None, None), svc("c", None, None)]);
    assert_eq!(selected_of(&state), 0, "starts on row 0");
    update(&mut state, key(KeyCode::Down));
    assert_eq!(selected_of(&state), 1, "Down → row 1");
    // snapshot shows the highlight on the 2nd row
    insta::assert_snapshot!(render_to_string(&state, 40, 7));
}

#[test]
fn service_screen_up_clamps_at_top() {
    let mut state = State::service(vec![svc("a", None, None), svc("b", None, None)]);
    update(&mut state, key(KeyCode::Up));
    assert_eq!(selected_of(&state), 0, "Up at top stays at 0");
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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test tui`
Expected: FAIL (compile error — `Msg` / `update` not implemented).

- [ ] **Step 3: Implement `Msg`**

`src/tui/msg.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages fed to `update` (spec §6). Keys arrive from crossterm; data results
//! arrive from the async `dbus::` workers over the flume channel.

use crate::dbus::types::ServiceInfo;
use crossterm::event::{Event, KeyEvent};

pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    ServicesLoaded(Result<Vec<ServiceInfo>, String>),
}

impl From<Event> for Msg {
    fn from(ev: Event) -> Self {
        match ev {
            Event::Key(k) => Msg::Key(k),
            Event::Resize(w, h) => Msg::Resize(w, h),
            _ => Msg::Resize(0, 0), // mouse etc. are ignored for now
        }
    }
}
```

- [ ] **Step 4: Implement `update`**

`src/tui/update.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure state machine (spec §6). Mutates `State` from a `Msg`; performs no IO.

use crossterm::event::{KeyCode, KeyEventKind};

use crate::dbus::types::ServiceInfo;
use crate::tui::msg::Msg;
use crate::tui::state::{Screen, ServiceScreen, State};

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Key(k) => {
            // Quit is global.
            if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                state.quit = true;
                return;
            }
            // Ignore key-repeat / release events (some terminals send them).
            if k.kind != KeyEventKind::Press {
                return;
            }
            match &mut state.screen {
                Screen::Service(s) => update_service_key(s, k.code),
            }
        }
        Msg::Resize(_, _) => {}
        Msg::ServicesLoaded(res) => match &mut state.screen {
            Screen::Service(s) => {
                s.loading = false;
                match res {
                    Ok(services) => {
                        s.selected = s.selected.min(services.len().saturating_sub(1));
                        s.services = services;
                    }
                    Err(e) => s.error = Some(e),
                }
            }
        },
    }
}

fn update_service_key(s: &mut ServiceScreen, code: KeyCode) {
    if s.services.is_empty() {
        return;
    }
    let last = s.services.len() - 1;
    match code {
        KeyCode::Down | KeyCode::Char('j') => s.selected = (s.selected + 1).min(last),
        KeyCode::Up | KeyCode::Char('k') => s.selected = s.selected.checked_sub(1).unwrap_or(0),
        _ => {}
    }
}
```

- [ ] **Step 5: Generate + pin the new snapshot, run tests**

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test --test tui
```

Expected: PASS (a new snapshot `tui__service_screen_down_arrow_moves_selection.snap` is generated and pinned).

- [ ] **Step 6: Commit**

```bash
git add src/tui/msg.rs src/tui/update.rs tests/tui.rs tests/snapshots/
git commit -m "feat(busx): tui update core — key nav (↑↓/jk) + quit

Assisted-by: claude:glm-5.2"
```

---

## Task 4: Snapshot for the loading + error states

**Files:** Modify `tests/tui.rs`.

(These states are produced by `Msg::ServicesLoaded` which `update` already handles from Task 3; this task just locks their rendering into snapshots.)

- [ ] **Step 1: Add loading + error snapshot tests**

Append to `tests/tui.rs`:

```rust
#[test]
fn service_screen_loading_state() {
    let state = State {
        screen: busx::tui::Screen::Service(busx::tui::ServiceScreen {
            services: vec![], selected: 0, loading: true, error: None,
        }),
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 6));
}

#[test]
fn service_screen_error_state() {
    let state = State {
        screen: busx::tui::Screen::Service(busx::tui::ServiceScreen {
            services: vec![], selected: 0, loading: false,
            error: Some("org.freedesktop.DBus.Error.ServiceUnknown: no owner".into()),
        }),
        quit: false,
    };
    insta::assert_snapshot!(render_to_string(&state, 40, 6));
}
```

- [ ] **Step 2: Generate + pin snapshots**

```bash
INSTA_UPDATE=always cargo test --test tui && cargo test --test tui
```

Expected: PASS; the loading snapshot shows the "Services (loading…)" title; the error snapshot shows `error: …`.

- [ ] **Step 3: Commit**

```bash
git add tests/tui.rs tests/snapshots/
git commit -m "test(busx): snapshot tui Service loading + error states

Assisted-by: claude:glm-5.2"
```

---

## Task 5: Terminal run loop (glue) — real `busx` shows the live service list

**Files:** Modify `src/tui/app.rs`.

This is the untestable-by-snapshot glue (it drives a real tty). Verify with `cargo build` and a **manual** smoke test.

- [ ] **Step 1: Implement `App` (loop driver) + `run`**

`src/tui/app.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The terminal event loop (spec §5). Owns the async connection + flume channel
//! + `State`; drives crossterm input and redraws. Not snapshot-tested (needs a
//! real tty) — manual-smoke only.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use zbus::Connection;

use crate::dbus;
use crate::error::Result;
use crate::tui::msg::Msg;
use crate::tui::state::{Screen, ServiceScreen, State};
use crate::tui::{render, update};

type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Launch the TUI: connect to the bus, set up the terminal, run the loop, and
/// always restore the terminal on exit (via the guard).
pub fn run(user: bool, system: bool, address: Option<&str>, verbose: bool) -> Result<()> {
    let conn = async_global_executor::block_on(dbus::conn::connect(user, system, address, verbose))?;
    let (tx, rx) = flume::unbounded::<Msg>();

    // Kick off the initial service-list fetch.
    spawn_list_names(conn.clone(), tx.clone());

    let state = State {
        screen: Screen::Service(ServiceScreen { services: vec![], selected: 0, loading: true, error: None }),
        quit: false,
    };
    let mut app = App { conn, tx, rx, state };

    let mut terminal = setup_terminal()?;
    let result = app.main_loop(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

/// Spawn the service-list fetch on the async executor; deliver the result as a
/// `Msg::ServicesLoaded` back to the UI channel.
fn spawn_list_names(conn: Connection, tx: flume::Sender<Msg>) {
    async_global_executor::spawn(async move {
        let res = dbus::list::list_names(&conn, false, false, false).await;
        let _ = tx.send(Msg::ServicesLoaded(res.map_err(|e| e.to_string())));
    })
    .detach();
}

struct App {
    conn: Connection,
    tx: flume::Sender<Msg>,
    rx: flume::Receiver<Msg>,
    state: State,
}

impl App {
    fn main_loop(&mut self, terminal: &mut Tui) -> Result<()> {
        while !self.state.quit {
            terminal.draw(|f| render(f, &self.state))?;
            // Drain any worker results first.
            while let Ok(msg) = self.rx.try_recv() {
                update(&mut self.state, msg);
            }
            if self.state.quit {
                break;
            }
            // Block briefly for a key; the short timeout keeps worker results
            // flowing (max ~50 ms latency to a redraw after data arrives).
            if event::poll(Duration::from_millis(50))? {
                if let Ok(ev) = event::read() {
                    if let Some(msg) = non_mouse(ev) {
                        update(&mut self.state, msg);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Map a crossterm event to a `Msg`, dropping mouse/unsupported events.
fn non_mouse(ev: Event) -> Option<Msg> {
    match ev {
        Event::Key(k) => Some(Msg::Key(k)),
        Event::Resize(w, h) => Some(Msg::Resize(w, h)),
        _ => None,
    }
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
```

> Robustness: if `main_loop` returns early on an error (e.g. a draw failure), `restore_terminal` still runs (it's called unconditionally in `run`). For panic-safety during development, wrapping the body of `run` in `std::panic::catch_unwind` + restore is a future hardening; Phase 1 keeps the happy-path restore.

- [ ] **Step 2: Build + full snapshot/CLI suite**

Run: `cargo build && cargo test -q`
Expected: builds; all TUI snapshots + all CLI e2e pass.

- [ ] **Step 3: Manual smoke test**

Run the binary against the test bus to confirm it really launches. Start a throwaway session bus and point the TUI at it:

```bash
addr=$(dbus-daemon --session --print-address=1 --fork 2>/dev/null | head -1)   # or use an existing busx test bus
cargo run -q -- --address "$addr"   # bare busx → TUI
```

Expected: a fullscreen TUI appears, the service list fills in, ↑↓/jk move the highlight, `q` (or `Esc`) exits cleanly back to the shell with the terminal restored. (If you don't have a handy session bus, `cargo run -q -- --system` against the system bus works too — the list reflects real services.)

- [ ] **Step 4: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat(busx): tui run loop — bare busx shows live service list

Assisted-by: claude:glm-5.2"
```

---

## Self-review checklist

- **Spec coverage:** bare `busx` → TUI (§1/§2) ✓ (T1); Elm state/update/render pure split (§6) ✓ (T2/T3); concurrency — async task + flume + crossterm poll (§5) ✓ (T5); Service page list with name+pid+process + selection (§8) ✓ (T2/T3); bottom key-hint bar (§8) ✓ (T2); TestBackend snapshot testing (§13) ✓ (T2/T3/T4); initial service-list fetch via `dbus::list::list_names` ✓ (T5). Gaps deferred to later phases: navigation to Objects/Interfaces/Interface (phase 2), call/get/set (phase 3), monitors (phase 4), copy-as (phase 5).
- **Type consistency:** `State`/`Screen`/`ServiceScreen` defined T2, used T3/T4/T5. `Msg` defined T3, matched in T3/T5. `update`/`render` signatures stable. `State::service` constructor used in tests.
- **Placeholders:** none — each code step is complete and self-contained; the lib/bin restructure (T2 Step 4) is spelled out concretely.
- **Lib/bin restructure (T2 Step 4):** the biggest mechanical risk in Phase 1. The approach (lib.rs = `pub mod` for all; main.rs = thin bin `use busx::…`; module internals unchanged) is concrete, but the executor must confirm `cargo build` + all existing CLI e2e pass — if any module's `crate::` path doesn't resolve under the lib, that's the place to look.

---

## Roadmap — next plans (phases 2–6)

2. **Browse flow** — Objects (`tui-tree-widget`) → Interfaces (exclude `org.freedesktop.DBus.{Introspectable,Properties,Peer}`) → Interface (3 columns; property `GetAll` snapshot + `r` refresh); single-item auto-skip; breadcrumb + per-screen key-hint.
3. **Call / read / write** — method-call detail (per-arg `tui-input`) + result; property get/set detail + result; one-shot ops with loading state.
4. **Listen + cancel** — signal/property/method listen; result streaming; `Esc`-leaves-stops. *(Also migrates `ops/monitor.rs` to the async core and removes `src/conn.rs`.)*
5. **copy-as + clipboard** — dbus-send/busctl/qdbus/gdbus generation; `arboard`; copy-as popup preview.
6. **Polish** — `?` help overlay, error toasts, JSON view toggle, empty/edge states, snapshot coverage.
