// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event loop (spec §5). `run_loop` is backend- and event-source-agnostic so it
//! is exercised end-to-end with TestBackend + a scripted event iterator; the
//! real crossterm + flume wiring lives in `run`.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::Terminal;
use zbus::Connection;

use crate::dbus;
use crate::error::Result;
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::{ActionResult, State};
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
        while !self.state.quit {
            terminal.draw(|f| render(f, &self.state))?;
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
pub fn run(user: bool, system: bool, address: Option<&str>, verbose: bool) -> Result<()> {
    let conn = async_global_executor::block_on(dbus::conn::connect(user, system, address, verbose))?;
    let (tx, rx) = flume::unbounded::<Msg>();
    let on_effect = |effect: Effect| run_effect(effect, conn.clone(), tx.clone());
    on_effect(Effect::FetchServices); // initial service-list fetch

    let mut app = App { state: State::loading_service() };
    let mut terminal = setup_terminal()?;
    let result = app.run_loop(&mut terminal, CrosstermSource { rx }, on_effect);
    // Always try to restore the terminal; prefer the loop's result over a
    // restore failure (don't mask the real error), but warn on restore failure.
    if let Err(e) = restore_terminal(&mut terminal) {
        eprintln!("busx: warning: failed to restore terminal: {e}");
    }
    result
}

/// Perform a requested `Effect` against the bus; deliver the result as a `Msg`.
fn run_effect(effect: Effect, conn: Connection, tx: flume::Sender<Msg>) {
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
                let _ = tx.send(Msg::InterfacesLoaded(service, object, res.map_err(|e| e.to_string())));
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
        Effect::CallMethod { service, object, iface, method, signature, args } => {
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
        Effect::GetProperty { service, object, iface, property } => {
            async_global_executor::spawn(async move {
                let res = dbus::property::get_one(&conn, &service, &object, &iface, &property).await;
                let _ = tx.send(
                    Msg::ActionResult(res.map(|v| ActionResult::Get(pretty(&v))).map_err(|e| e.to_string())),
                );
            })
            .detach();
        }
        Effect::SetProperty { service, object, iface, property, signature, value } => {
            async_global_executor::spawn(async move {
                let res =
                    dbus::property::set(&conn, &service, &object, &iface, &property, &signature, &[
                        value,
                    ])
                    .await;
                let _ = tx
                    .send(Msg::ActionResult(res.map(|_| ActionResult::Set).map_err(|e| e.to_string())));
            })
            .detach();
        }
    }
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
                Err(e) => {
                    eprintln!("busx: warning: input poll failed: {e}");
                    return None; // can't read input — exit cleanly
                }
            }
            if let Ok(ev) = event::read() {
                if let Some(msg) = non_mouse(ev) {
                    return Some(msg);
                }
            }
        }
    }
}

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
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
