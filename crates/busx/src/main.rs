// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx` — a zbus-based dbus-send/busctl/qdbus replacement (binary crate).
//!
//! The module tree (`cli`, `dbus`, `ops`, `tui`, …) lives inside this binary
//! crate; there is no library target, so none of it is reachable from outside.

mod cli;
mod complete;
mod dbus;
mod error;
mod log;
mod ops;
mod out;
mod tui;
mod value;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> std::process::ExitCode {
    // Rust sets SIGPIPE to SIG_IGN at startup, so a downstream pipe reader
    // (e.g. `less`, `head`) closing early makes stdout writes return EPIPE,
    // which `println!` turns into a panic ("failed printing to stdout: Broken
    // pipe"). Restore the default disposition so we terminate via SIGPIPE like
    // ordinary Unix tools. Must run before any stdout output.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    // Shell-completion protocol: if `COMPLETE=<shell>` is set the shell is asking
    // us to produce candidates (or the registration script). Handle it before
    // any normal parsing and exit, mirroring `clap_complete`'s `CompleteEnv`.
    // Runs without a logging subscriber so completion stays silent.
    match complete::try_complete() {
        Ok(true) => return std::process::ExitCode::SUCCESS,
        Ok(false) => {}
        Err(e) => {
            eprintln!("busx: {e}");
            return std::process::ExitCode::FAILURE;
        }
    }

    let cli = Cli::parse();
    let Cli {
        user,
        system,
        address,
        verbose,
        json,
        log,
        show_standard_interfaces,
        command,
    } = cli;
    let result = match command {
        None => {
            // TUI owns the terminal in raw mode — diagnostics go to a log file,
            // never the TTY. If the file can't be opened, warn once (stderr is
            // still safe here, before raw mode) and run with logging disabled
            // rather than refusing to start.
            //
            // Keep `init_tui`'s WorkerGuard alive for the whole `tui::run`:
            // dropping it flushes and joins the non-blocking writer thread, so
            // if it were dropped here the log file would stay empty.
            let _guard = log::init_tui(verbose, log.as_deref());
            if let Err(e) = &_guard {
                eprintln!("busx: warning: could not open TUI log file ({e}); logging disabled");
            }
            tui::run(user, system, address.as_deref(), show_standard_interfaces)
        }
        Some(command) => {
            // CLI diagnostics go to stderr; capture with `2> file`.
            log::init_cli(verbose);
            run_command(
                user,
                system,
                address,
                json,
                show_standard_interfaces,
                log.is_some(),
                command,
            )
        }
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("busx: {e}");
            // `-v`+ prints the full cause chain (walk .source()). Both the CLI
            // and the TUI reach here — fatal TUI errors propagate up after the
            // terminal is restored; non-fatal TUI errors stay in their popup.
            if verbose > 0 {
                let mut source = std::error::Error::source(&e);
                while let Some(s) = source {
                    eprintln!("  caused by: {s}");
                    source = s.source();
                }
            }
            e.exit_code()
        }
    }
}

fn run_command(
    user: bool,
    system: bool,
    address: Option<String>,
    json: bool,
    show_standard_interfaces: bool,
    log: bool,
    command: Command,
) -> error::Result<()> {
    warn_ignored_globals(&command, json, show_standard_interfaces, log);
    match command {
        Command::List {
            no_unique,
            activatable,
        } => ops::list::run(
            user,
            system,
            address.as_deref(),
            json,
            no_unique,
            activatable,
        ),
        Command::Get {
            service,
            object,
            interface,
            props,
        } => ops::property::get(
            user,
            system,
            address.as_deref(),
            json,
            &service,
            &object,
            interface.as_deref(),
            &props,
        ),
        Command::Call {
            service,
            object,
            interface,
            method,
            signature,
            args,
        } => ops::call::run(
            user,
            system,
            address.as_deref(),
            json,
            &service,
            &object,
            &interface,
            &method,
            &signature,
            &args,
        ),
        Command::Set {
            service,
            object,
            interface,
            property,
            signature,
            value,
        } => ops::property::set(
            user,
            system,
            address.as_deref(),
            &service,
            &object,
            &interface,
            &property,
            &signature,
            &value,
        ),
        Command::Introspect { service, object } => {
            ops::introspect::run(user, system, address.as_deref(), &service, &object)
        }
        Command::Monitor {
            services,
            interface,
            member,
            path,
            sender,
            r#match,
            r#type,
            limit_messages,
            timeout,
        } => ops::monitor::run(
            user,
            system,
            address.as_deref(),
            json,
            services,
            interface,
            member,
            path,
            sender,
            r#match,
            r#type.map(|t| match t {
                cli::MonitorType::Signal => zbus::message::Type::Signal,
                cli::MonitorType::MethodCall => zbus::message::Type::MethodCall,
                cli::MonitorType::MethodReturn => zbus::message::Type::MethodReturn,
                cli::MonitorType::Error => zbus::message::Type::Error,
            }),
            limit_messages,
            timeout.as_deref(),
        ),
        Command::Tree { service } => ops::tree::run(
            user,
            system,
            address.as_deref(),
            json,
            show_standard_interfaces,
            &service,
        ),
        Command::Emit {
            destination,
            object,
            interface,
            member,
            signature,
            args,
        } => ops::emit::run(
            user,
            system,
            address.as_deref(),
            destination.as_deref(),
            &object,
            &interface,
            &member,
            &signature,
            &args,
        ),
        Command::Completion { shell } => {
            complete::emit_script(shell);
            Ok(())
        }
    }
}

/// Warn on stderr about every global flag the user passed but this subcommand
/// ignores. `--json`, `--show-standard-interfaces`, and `--log` are global, but
/// only some subcommands honour each; silently dropping a flag the user
/// explicitly passed is usually a buggy script, so surface it. Only warns when
/// the flag was actually passed (non-default).
fn warn_ignored_globals(command: &Command, json: bool, show_standard_interfaces: bool, log: bool) {
    let honors_json = matches!(
        command,
        Command::List { .. }
            | Command::Call { .. }
            | Command::Get { .. }
            | Command::Monitor { .. }
            | Command::Tree { .. }
    );
    let honors_show_standard_interfaces = matches!(command, Command::Tree { .. });

    if json && !honors_json {
        eprintln!("busx: warning: --json is ignored by this subcommand");
    }
    if show_standard_interfaces && !honors_show_standard_interfaces {
        eprintln!("busx: warning: --show-standard-interfaces is ignored by this subcommand");
    }
    if log {
        eprintln!(
            "busx: warning: --log is ignored by CLI subcommands (only the TUI writes a log file)"
        );
    }
}
