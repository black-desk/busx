// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Terminal lifecycle and asynchronous D-Bus effects.

use std::ffi::OsString;
use std::io::{self, Stdout, Write};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::{FutureExt, StreamExt};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use zbus::Connection;

use crate::dbus;
use crate::dbus::conn::Bus;
use crate::error::Result;
use crate::tui::copy::CopyOp;
use crate::tui::msg::{Effect, Msg};
use crate::tui::render::render;
use crate::tui::state::{ActionResult, Context, ListenTarget, Page, State};
use crate::tui::update::{listen_rule, update};

type Tui = Terminal<CrosstermBackend<Stdout>>;

/// The guard is installed before entering raw mode so partial setup failures
/// and unwinding restore the terminal too.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            event::DisableMouseCapture,
            crossterm::cursor::Show
        );
    }
}

pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    show_standard_interfaces: bool,
) -> Result<()> {
    let (conn, bus) =
        async_global_executor::block_on(dbus::conn::connect_with_bus(user, system, address))?;
    let (tx, rx) = flume::bounded::<Msg>(1024);
    let mut state = State::new(bus, conn, show_standard_interfaces);
    let id = state.request_id();
    state.services_request = id;
    run_effect(Effect::Services(id, state.id), &state, tx.clone());
    let _guard = TerminalGuard;
    enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        event::EnableMouseCapture
    )?;
    let mut terminal: Tui = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    while !state.quit {
        let mut targets = vec![];
        let mut scroll = state.scroll;
        terminal.draw(|frame| render(frame, &state, &mut targets, &mut scroll))?;
        state.scroll = scroll;
        state.click_targets = targets;
        // Keep logical scrolling aligned with the rendered viewport, including
        // after End and when a watch was following the latest event.
        match state.page {
            Page::Browser => {
                if let Some(browser) = &mut state.browser {
                    browser.editor.scroll = scroll[2];
                }
            }
            Page::Watch => {
                if let Some(watch) = state.watch_mut() {
                    watch.scroll = scroll[2];
                }
            }
            Page::Services => {}
        }
        let msg = loop {
            // Give keyboard/mouse input priority over busy streams.
            if event::poll(Duration::ZERO)? {
                if let Some(msg) = terminal_event(event::read()?) {
                    break msg;
                }
            } else if let Ok(msg) = rx.try_recv() {
                break msg;
            } else if event::poll(Duration::from_millis(50))?
                && let Some(msg) = terminal_event(event::read()?)
            {
                break msg;
            }
        };
        if let Some(effect) = update(&mut state, msg) {
            let open_editor = matches!(effect, Effect::Editor(_, _));
            run_effect(effect, &state, tx.clone());
            if open_editor {
                terminal.clear()?;
            }
        }
    }
    Ok(())
}

fn terminal_event(event: Event) -> Option<Msg> {
    match event {
        Event::Key(key) => Some(Msg::Key(key)),
        Event::Mouse(mouse) => Some(Msg::Mouse(mouse)),
        Event::Resize(_, _) => Some(Msg::Resize),
        _ => None,
    }
}

fn run_effect(effect: Effect, state: &State, tx: flume::Sender<Msg>) {
    if let Effect::Editor(id, text) = effect {
        let _ = tx.send(Msg::Editor(id, open_editor(&text)));
        return;
    }
    if let Effect::Clipboard(id, text) = effect {
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Clipboard(id, write_to_clipboard(&text)));
        });
        return;
    }
    if let Effect::Connect(id, bus) = effect {
        async_global_executor::spawn(async move {
            let result = match bus {
                Bus::Session => dbus::conn::connect_with_bus(true, false, None).await,
                Bus::System => dbus::conn::connect_with_bus(false, true, None).await,
                Bus::Other(address) => {
                    dbus::conn::connect_with_bus(false, false, Some(&address)).await
                }
            };
            let _ = tx.send_async(Msg::Connected(id, result)).await;
        })
        .detach();
        return;
    }
    let tab_id = match &effect {
        Effect::Services(_, tab_id)
        | Effect::Objects(_, tab_id, _)
        | Effect::Interface(_, tab_id, _)
        | Effect::Properties(_, tab_id, _)
        | Effect::Execute(_, tab_id, _, _)
        | Effect::Listen(_, tab_id, _, _) => *tab_id,
        Effect::Connect(_, _) | Effect::Clipboard(_, _) | Effect::Editor(_, _) => unreachable!(),
    };
    let Some(tab) = state.tabs.iter().find(|tab| tab.id == tab_id) else {
        return;
    };
    let conn = tab.connection.clone();
    let bus = tab.bus.clone();
    async_global_executor::spawn(async move {
        let msg = match effect {
            Effect::Services(id, _) => {
                Msg::Services(id, dbus::list::list_names(&conn, false, false).await)
            }
            Effect::Objects(id, _, service) => {
                Msg::Objects(id, dbus::tree::object_tree(&conn, &service).await)
            }
            Effect::Interface(id, _, context) => Msg::Interface(
                id,
                dbus::introspect::introspect(&conn, &context.service, &context.object).await,
            ),
            Effect::Properties(id, _, context) => {
                let result = match dbus::property::get_all(
                    &conn,
                    &context.service,
                    &context.object,
                    &context.interface,
                )
                .await
                {
                    Ok(values) => Ok(values),
                    Err(_) => {
                        dbus::property::get_all_by_one(
                            &conn,
                            &context.service,
                            &context.object,
                            &context.interface,
                        )
                        .await
                    }
                };
                Msg::Properties(id, result)
            }
            Effect::Execute(id, tab_id, context, op) => {
                let result = execute_op(&conn, &op).await;
                Msg::Action(id, tab_id, context, op, result)
            }
            Effect::Listen(id, _, context, target) => {
                let (cancel_tx, cancel_rx) = futures::channel::oneshot::channel();
                if tx.send_async(Msg::WatchReady(id, cancel_tx)).await.is_err() {
                    return;
                }
                // Cancellation covers connection and subscription setup, not
                // just the receive loop. A stopped watch cannot rearm later.
                let result = futures::select! {
                    result = listen(id, &conn, &bus, &context, &target, &tx).fuse() => result,
                    _ = cancel_rx.fuse() => Ok(()),
                };
                Msg::WatchEnded(id, result)
            }
            Effect::Connect(_, _) | Effect::Clipboard(_, _) | Effect::Editor(_, _) => {
                unreachable!()
            }
        };
        let _ = tx.send_async(msg).await;
    })
    .detach();
}

fn open_editor(text: &str) -> std::result::Result<(), String> {
    let mut file = tempfile::Builder::new()
        .prefix("busx-result-")
        .suffix(".txt")
        .tempfile()
        .map_err(|error| format!("create temporary result file: {error}"))?;
    let formatted = crate::value::pretty::format_for_editor(text);
    file.write_all(formatted.as_bytes())
        .map_err(|error| format!("write temporary result file: {error}"))?;
    file.flush()
        .map_err(|error| format!("flush temporary result file: {error}"))?;

    let editor = ["VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .unwrap_or_else(|| OsString::from("vi"));

    crossterm::terminal::disable_raw_mode()
        .map_err(|error| format!("leave raw mode for editor: {error}"))?;
    let suspended = crossterm::execute!(
        io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show
    );
    if let Err(error) = suspended {
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::cursor::Hide
        );
        let _ = crossterm::terminal::enable_raw_mode();
        return Err(format!("leave terminal for editor: {error}"));
    }

    let editor_result = Command::new(editor)
        .arg(file.path())
        .status()
        .map_err(|error| format!("run editor: {error}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("editor exited with {status}"))
            }
        });
    let restore_result = crossterm::execute!(
        io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::cursor::Hide
    )
    .map_err(|error| format!("return to terminal after editor: {error}"))
    .and_then(|_| {
        crossterm::terminal::enable_raw_mode()
            .map_err(|error| format!("restore raw mode after editor: {error}"))
    });
    editor_result.and(restore_result)
}

async fn execute_op(conn: &Connection, op: &CopyOp) -> Result<ActionResult> {
    match op {
        CopyOp::Call {
            service,
            object,
            iface,
            method,
            signature,
            args,
        } => {
            let values =
                dbus::call::call_method(conn, service, object, iface, method, signature, args)
                    .await?;
            Ok(ActionResult::Call(
                values
                    .iter()
                    .map(|value| crate::value::pretty::pretty(value))
                    .collect(),
            ))
        }
        CopyOp::Get {
            service,
            object,
            iface,
            property,
        } => {
            let value = dbus::property::get_one(conn, service, object, iface, property).await?;
            Ok(ActionResult::Get(crate::value::pretty::pretty(&value)))
        }
        CopyOp::Set {
            service,
            object,
            iface,
            property,
            signature,
            value,
        } => {
            dbus::property::set(conn, service, object, iface, property, signature, value).await?;
            Ok(ActionResult::Set)
        }
        CopyOp::Listen { .. } => Err(crate::error::Error::Msg(
            "Use Watch to start a subscription".into(),
        )),
    }
}

async fn listen(
    id: u64,
    conn: &Connection,
    bus: &Bus,
    context: &Context,
    target: &ListenTarget,
    tx: &flume::Sender<Msg>,
) -> Result<()> {
    let rule = listen_rule(context, target)?;
    if matches!(target, ListenTarget::Method(_)) {
        // Use the bus actually reached by the browser, including session-to-
        // system fallback, rather than trying the original CLI defaults again.
        let dedicated = match bus {
            Bus::Session => Connection::session().await?,
            Bus::System => Connection::system().await?,
            Bus::Other(address) => {
                zbus::connection::Builder::address(address.as_str())?
                    .build()
                    .await?
            }
        };
        let owner = zbus::fdo::DBusProxy::new(conn)
            .await?
            .get_name_owner(
                context
                    .service
                    .as_str()
                    .try_into()
                    .map_err(zbus::Error::from)?,
            )
            .await?;
        let mut resolved = context.clone();
        resolved.service = owner.to_string();
        let rule = listen_rule(&resolved, target)?;
        dbus::monitor::become_monitor(&dedicated, Some(&rule)).await?;
        let own_name = dedicated
            .unique_name()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let mut stream = zbus::MessageStream::from(&dedicated);
        let _ = tx.send_async(Msg::WatchStarted(id, rule.to_string())).await;
        while let Some(message) = stream.next().await {
            match message {
                Ok(message) if !dbus::monitor::is_become_monitor_noise(&message, &own_name) => {
                    if send_message(tx, id, &message).await.is_err() {
                        return Ok(());
                    }
                }
                Ok(_) => {}
                Err(error) => tracing::debug!("dropped malformed message: {error}"),
            }
        }
    } else {
        let rule_text = rule.to_string();
        let mut stream = zbus::MessageStream::for_match_rule(rule, conn, None).await?;
        let _ = tx.send_async(Msg::WatchStarted(id, rule_text)).await;
        while let Some(message) = stream.next().await {
            match message {
                Ok(message) if property_matches(&message, context, target) => {
                    if send_message(tx, id, &message).await.is_err() {
                        return Ok(());
                    }
                }
                Ok(_) => {}
                Err(error) => tracing::debug!("dropped malformed message: {error}"),
            }
        }
    }
    Ok(())
}

async fn send_message(
    tx: &flume::Sender<Msg>,
    id: u64,
    message: &zbus::Message,
) -> std::result::Result<(), ()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() % 86400;
    let timestamp = format!(
        "{:02}:{:02}:{:02}.{:03} UTC",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        now.subsec_millis()
    );
    tx.send_async(Msg::WatchMessage(
        id,
        format!("{timestamp}\n{}", dbus::monitor::format_message(message)),
    ))
    .await
    .map_err(|_| ())
}

fn property_matches(message: &zbus::Message, context: &Context, target: &ListenTarget) -> bool {
    let ListenTarget::Property(property) = target else {
        return true;
    };
    let Ok((interface, changed, invalidated)) = message.body().deserialize::<(
        String,
        std::collections::HashMap<String, zvariant::OwnedValue>,
        Vec<String>,
    )>() else {
        return false;
    };
    interface == context.interface
        && (changed.contains_key(property) || invalidated.contains(property))
}

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
        if let Ok(mut child) = Command::new(prog)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
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
