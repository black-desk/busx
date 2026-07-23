// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
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
            if let Err(e) = log::init_tui(verbose, log.as_deref()) {
                eprintln!("busx: warning: could not open TUI log file ({e}); logging disabled");
            }
            tui::run(user, system, address.as_deref(), show_standard_interfaces)
        }
        Some(command) => {
            // CLI diagnostics go to stderr; capture with `2> file`.
            log::init_cli(verbose);
            run_command(user, system, address, json, command)
        }
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("busx: {e}");
            // `-v`+ prints the full cause chain (walk .source()). Only the CLI
            // reaches here; the TUI surfaces errors inside its popup instead.
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
    command: Command,
) -> error::Result<()> {
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
        Command::Introspect {
            service,
            object,
            interface,
        } => ops::introspect::run(
            user,
            system,
            address.as_deref(),
            json,
            &service,
            &object,
            interface.as_deref(),
        ),
        Command::Monitor {
            services,
            interface,
            member,
            path,
            sender,
            r#match,
            all,
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
            all,
            limit_messages,
            timeout.as_deref(),
        ),
        Command::Completion { shell } => {
            complete::emit_script(shell);
            Ok(())
        }
    }
}
