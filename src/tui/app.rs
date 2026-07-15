// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event loop. `run_loop` is backend- and event-source-agnostic so it
//! is exercised end-to-end with TestBackend + a scripted event iterator; the
//! real crossterm + flume wiring lives in `run`.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use zbus::Connection;

use crate::dbus;
use crate::error::Result;
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{ActionResult, ListenTarget, State};
use crate::tui::{render, update};

/// Pretty-print an owned value (the common tail of call/get result rendering).
fn pretty(v: &zvariant::OwnedValue) -> String {
    crate::value::pretty::pretty(v)
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Loop driver: holds the display `State` and advances it from a stream of
/// `Msg`s. Built directly in tests; `run` builds it for production.
pub struct App {
    pub state: State,
}

impl App {
    /// Render, then consume one event, repeating until `state.quit` or the event
    /// source is exhausted. Generic over the backend so tests pass a TestBackend.
    ///
    /// Draws at the top of each iteration, so a non-quit mutation IS rendered on
    /// the next pass; a quit mutation exits without a final redraw (the screen is
    /// discarded when the terminal is torn down).
    ///
    /// `on_effect` performs any IO `update` requests (a fetch). It is injected so
    /// tests stay bus-free (they pass `|_| {}`); production closes over the
    /// connection + message channel.
    pub fn run_loop<B, F>(
        &mut self,
        terminal: &mut Terminal<B>,
        mut events: impl Iterator<Item = Msg>,
        mut on_effect: F,
    ) -> Result<()>
    where
        B: Backend,
        crate::error::Error: From<<B as Backend>::Error>,
        F: FnMut(Effect),
    {
        let mut targets: Vec<(ratatui::layout::Rect, crate::tui::ClickTarget)> = Vec::new();
        // Persisted per-list scroll offsets for the top screen's list(s),
        // threaded into `render` so the cursor doesn't snap back to the viewport
        // bottom each frame (see `render`). Like `targets`, this is loop-owned.
        // Reset on any navigation (screen-stack depth change) so a freshly
        // entered / returned-to screen starts at the top.
        let mut scroll = [0usize; 3];
        let mut prev_len = self.state.screens.len();
        while !self.state.quit {
            if self.state.screens.len() != prev_len {
                scroll = [0; 3];
                prev_len = self.state.screens.len();
            }
            terminal.draw(|f| render(f, &self.state, &mut targets, &mut scroll))?;
            // The draw closure's `&self.state` borrow ends when `draw` returns,
            // so storing into `click_targets` here is fine. `take` clears `targets`
            // for reuse next frame (no clone).
            self.state.click_targets = std::mem::take(&mut targets);
            match events.next() {
                Some(msg) => {
                    if let Some(effect) = update(&mut self.state, msg) {
                        on_effect(effect);
                    }
                }
                None => break, // scripted test source exhausted
            }
        }
        Ok(())
    }
}

/// Launch the TUI against the real terminal.
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    show_standard_interfaces: bool,
) -> Result<()> {
    let (conn, bus) = async_global_executor::block_on(dbus::conn::connect_with_bus(
        user, system, address, verbose,
    ))?;
    let (tx, rx) = flume::unbounded::<Msg>();
    let (user_arg, system_arg, address_arg) = (user, system, address.map(String::from));
    // `CopyToClipboard` is NOT a dbus op — intercept it before `run_effect`,
    // write via the most reliable available method, and send the result back as
    // `Msg::ClipboardResult` so it surfaces in the popup. NEVER prints to the
    // TTY (the TUI runs in raw mode + the alternate screen — any stray stdout
    // /stderr write corrupts the display). The clipboard tooling lives ONLY here
    // (never in `update`/`render`/tests).
    let on_effect = move |effect: Effect| match effect {
        Effect::CopyToClipboard(s) => {
            let res = write_to_clipboard(&s);
            let _ = tx.send(Msg::ClipboardResult(res));
        }
        other => run_effect(
            other,
            conn.clone(),
            tx.clone(),
            user_arg,
            system_arg,
            address_arg.as_deref(),
        ),
    };
    on_effect(Effect::FetchServices); // initial service-list fetch

    let mut app = App {
        state: State::loading_service(),
    };
    app.state.bus = bus;
    app.state.show_standard_interfaces = show_standard_interfaces;
    let mut terminal = setup_terminal()?;
    let result = app.run_loop(&mut terminal, CrosstermSource { rx }, on_effect);
    // Always try to restore the terminal; prefer the loop's result over a
    // restore failure (don't mask the real error), but warn on restore failure.
    if let Err(e) = restore_terminal(&mut terminal) {
        eprintln!("busx: warning: failed to restore terminal: {e}");
    }
    result
}

/// Copy `text` to the system clipboard. Tries the standard CLI clipboard tools
/// first (reliable across Wayland/X compositors — `wl-copy` for Wayland,
/// `xclip`/`xsel` for X), then `arboard` as a last-resort fallback. Returns
/// `Ok(())` on the first method that accepts the text, or an `Err` describing
/// why every method failed. NEVER prints — the TUI runs in raw mode + the
/// alternate screen, and any stdout/stderr write corrupts the display; the
/// caller surfaces the `Err` inside the popup via `Msg::ClipboardResult`.
///
/// We do not `.wait()` on the spawned tool: `wl-copy`/`xclip`/`xsel` may
/// daemonize themselves to hold the selection after their parent exits, so
/// writing `text` to stdin and dropping it (which closes the pipe) is correct.
fn write_to_clipboard(text: &str) -> std::result::Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Each entry: (program, args). Pipe `text` to stdin; success = the program
    // spawned and accepted it.
    let tools: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (prog, args) in tools {
        if let Ok(mut child) = Command::new(prog).args(*args).stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            // Don't .wait() — the tool may daemonize to hold the clipboard.
            return Ok(());
        }
        // spawn failed (tool not installed / not on PATH) → try the next.
    }
    // Last resort: arboard (pure-Rust, but compositor-quirky on some Wayland).
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.set_text(text).map_err(|e| format!("arboard: {e}")),
        Err(e) => Err(format!(
            "no wl-copy/xclip/xsel on PATH and arboard unavailable: {e}"
        )),
    }
}

/// Perform a requested `Effect` against the bus; deliver the result as a `Msg`.
///
/// `user`/`system`/`address` are only used by the Method-listen branch, which
/// builds its own dedicated connection (BecomeMonitor makes a connection
/// recv-only, so it cannot reuse the main one).
fn run_effect(
    effect: Effect,
    conn: Connection,
    tx: flume::Sender<Msg>,
    user: bool,
    system: bool,
    address: Option<&str>,
) {
    match effect {
        Effect::FetchServices => {
            async_global_executor::spawn(async move {
                let res = dbus::list::list_names(&conn, false, false, false).await;
                let _ = tx.send(Msg::ServicesLoaded(res.map_err(|e| e.to_string())));
            })
            .detach();
        }
        Effect::FetchObjects(service) => {
            async_global_executor::spawn(async move {
                let res = dbus::tree::object_tree(&conn, &service).await;
                let _ = tx.send(Msg::ObjectsLoaded(res.map_err(|e| e.to_string())));
            })
            .detach();
        }
        Effect::FetchInterfaces(service, object) => {
            async_global_executor::spawn(async move {
                let res = dbus::introspect::introspect(&conn, &service, &object).await;
                let _ = tx.send(Msg::InterfacesLoaded(
                    service,
                    object,
                    res.map_err(|e| e.to_string()),
                ));
            })
            .detach();
        }
        Effect::FetchProperties(service, object, iface) => {
            async_global_executor::spawn(async move {
                let res = dbus::property::get_all(&conn, &service, &object, &iface).await;
                let _ = tx.send(Msg::PropertiesLoaded(res.map_err(|e| e.to_string())));
            })
            .detach();
        }
        Effect::CallMethod {
            service,
            object,
            iface,
            method,
            signature,
            args,
        } => {
            async_global_executor::spawn(async move {
                let res = dbus::call::call_method(
                    &conn, &service, &object, &iface, &method, &signature, &args,
                )
                .await;
                let _ = tx.send(Msg::ActionResult(
                    res.map(|vs| ActionResult::Call(vs.iter().map(pretty).collect()))
                        .map_err(|e| e.to_string()),
                ));
            })
            .detach();
        }
        Effect::GetProperty {
            service,
            object,
            iface,
            property,
        } => {
            async_global_executor::spawn(async move {
                let res =
                    dbus::property::get_one(&conn, &service, &object, &iface, &property).await;
                let _ = tx.send(Msg::ActionResult(
                    res.map(|v| ActionResult::Get(pretty(&v)))
                        .map_err(|e| e.to_string()),
                ));
            })
            .detach();
        }
        Effect::SetProperty {
            service,
            object,
            iface,
            property,
            signature,
            value,
        } => {
            async_global_executor::spawn(async move {
                let res = dbus::property::set(
                    &conn,
                    &service,
                    &object,
                    &iface,
                    &property,
                    &signature,
                    &[value],
                )
                .await;
                let _ = tx.send(Msg::ActionResult(
                    res.map(|_| ActionResult::Set).map_err(|e| e.to_string()),
                ));
            })
            .detach();
        }
        Effect::Listen {
            service: _,
            object,
            iface,
            target,
        } => {
            // `address: Option<&str>` is not `'static`; own it for the spawned task.
            let address_owned = address.map(String::from);
            async_global_executor::spawn(async move {
                use futures::{FutureExt, StreamExt};
                let (cancel_tx, cancel_rx) = futures::channel::oneshot::channel::<()>();
                let _ = tx.send(Msg::ListenStarted(cancel_tx));
                let mut cancel_rx = cancel_rx.fuse();

                // Method listen: BecomeMonitor makes a connection
                // recv-only, so build a dedicated one and let the bus filter the
                // method's calls. `ListenStarted` is already sent above so the
                // cancel is wired for this branch too.
                if let ListenTarget::Method { .. } = &target {
                    // BecomeMonitor makes a connection recv-only — build a dedicated one.
                    let dedicated =
                        match dbus::conn::connect(user, system, address_owned.as_deref(), false)
                            .await
                        {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = tx.send(Msg::ActionResult(Err(format!(
                                    "listen: connect failed: {e}"
                                ))));
                                return;
                            }
                        };
                    let rule = match update::listen_rule(&iface, &object, &target) {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = tx.send(Msg::ActionResult(Err(e.to_string())));
                            return;
                        }
                    };
                    if let Err(e) =
                        crate::dbus::monitor::become_monitor(&dedicated, Some(&rule)).await
                    {
                        // Privileged op — some buses refuse it.
                        let _ = tx.send(Msg::ActionResult(Err(format!(
                            "BecomeMonitor refused: {e}"
                        ))));
                        return;
                    }
                    // The BecomeMonitor rule filters at the bus, so this stream
                    // yields only matching method_call messages — no client-side
                    // filtering or serial tracking needed.
                    let mut stream = zbus::MessageStream::from(&dedicated).fuse();
                    loop {
                        futures::select! {
                            msg = stream.next() => match msg {
                                Some(Ok(m)) => {
                                    let _ = tx.send(Msg::ListenMessage(
                                        crate::dbus::monitor::format_message(&m)));
                                }
                                Some(Err(_)) => {}   // drop a single malformed message
                                None => break,        // stream ended
                            },
                            _ = cancel_rx => break,   // Esc left the Result → stop
                        }
                    }
                    return;
                }

                // Signal / Property listen: subscribe via the match rule on the
                // main connection; PropertiesChanged is filtered client-side.
                let rule = match update::listen_rule(&iface, &object, &target) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Msg::ActionResult(Err(e.to_string())));
                        return;
                    }
                };
                let stream = match zbus::MessageStream::for_match_rule(rule, &conn, None).await {
                    Ok(s) => s.fuse(),
                    Err(e) => {
                        let _ = tx.send(Msg::ActionResult(Err(e.to_string())));
                        return;
                    }
                };
                let mut stream = stream;
                loop {
                    futures::select! {
                        msg = stream.next() => match msg {
                            Some(Ok(m)) => {
                                if listen_message_matches(&m, &target) {
                                    let _ = tx.send(Msg::ListenMessage(
                                        crate::dbus::monitor::format_message(&m)));
                                }
                            }
                            Some(Err(_)) => {}   // drop a single malformed message
                            None => break,        // stream ended
                        },
                        _ = cancel_rx => break,   // Esc left the Result → stop
                    }
                }
            })
            .detach();
        }
        // `CopyToClipboard` is intercepted by the `on_effect` closure in `run`
        // before it reaches here — `run_effect` never sees it (it has no bus
        // work). This arm exists only for match exhaustiveness; reaching it
        // would be a bug in the closure's routing, so it's a quiet no-op.
        Effect::CopyToClipboard(_) => {}
    }
}

/// Client-side filter for a received listen message. Signals always pass; a
/// Property listen forwards only `PropertiesChanged` messages whose changed- or
/// invalidated-keys mention the watched property. Best-effort: on parse failure
/// the message passes through (don't kill the stream for one odd message).
fn listen_message_matches(m: &zbus::Message, target: &ListenTarget) -> bool {
    let ListenTarget::Property { property } = target else {
        return true; // Signal (Method never reaches here — returns early in the task)
    };
    let Ok((_, changed, invalidated)) = m.body().deserialize::<(
        String,
        std::collections::HashMap<String, zvariant::OwnedValue>,
        Vec<String>,
    )>() else {
        return true; // best-effort: can't parse → show it
    };
    changed.contains_key(property) || invalidated.iter().any(|k| k == property)
}

/// Production event source: drains the worker channel, and between messages
/// polls crossterm for keys (short timeout so worker results flow promptly).
struct CrosstermSource {
    rx: flume::Receiver<Msg>,
}

impl Iterator for CrosstermSource {
    type Item = Msg;

    fn next(&mut self) -> Option<Msg> {
        loop {
            if let Ok(msg) = self.rx.try_recv() {
                return Some(msg);
            }
            match event::poll(Duration::from_millis(50)) {
                Ok(false) => continue, // timeout: re-drain the channel
                Ok(true) => {}
                // Input poll failed (e.g. terminal gone). Don't print — we're
                // still in raw mode + the alternate screen, so any write would
                // corrupt the display. Just exit the loop; `restore_terminal`
                // runs next, then the process exits.
                Err(_) => return None,
            }
            if let Ok(ev) = event::read() {
                if let Some(msg) = non_mouse(ev) {
                    return Some(msg);
                }
            }
        }
    }
}

/// Map a crossterm event to a `Msg`. Mouse events are forwarded raw so
/// `update` can hit-test them against `state.click_targets`.
fn non_mouse(ev: Event) -> Option<Msg> {
    match ev {
        Event::Key(k) => Some(Msg::Key(k)),
        Event::Resize(w, h) => Some(Msg::Resize(w, h)),
        Event::Mouse(m) => Some(Msg::Mouse(m)),
        _ => None,
    }
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}
