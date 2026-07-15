// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use busx::{
    cli::{Cli, Command},
    complete, error, ops, tui,
};
use clap::Parser;

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
        show_standard_interfaces,
        command,
    } = cli;
    let result = match command {
        None => tui::run(
            user,
            system,
            address.as_deref(),
            verbose,
            show_standard_interfaces,
        ),
        Some(command) => run_command(user, system, address, verbose, json, command),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("busx: {e}");
            e.exit_code()
        }
    }
}

fn run_command(
    user: bool,
    system: bool,
    address: Option<String>,
    verbose: bool,
    json: bool,
    command: Command,
) -> error::Result<()> {
    match command {
        Command::List {
            unique,
            acquired,
            activatable,
        } => ops::list::run(
            user,
            system,
            address.as_deref(),
            verbose,
            json,
            unique,
            acquired,
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
            verbose,
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
            verbose,
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
            verbose,
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
            verbose,
            json,
            &service,
            &object,
            interface.as_deref(),
        ),
        Command::Tree { service } => {
            ops::tree::run(user, system, address.as_deref(), verbose, json, &service)
        }
        Command::Monitor {
            services,
            interface,
            member,
            path,
            sender,
            r#match,
            signals,
            limit_messages,
            timeout,
        } => ops::monitor::run(
            user,
            system,
            address.as_deref(),
            verbose,
            json,
            services,
            interface,
            member,
            path,
            sender,
            r#match,
            signals,
            limit_messages,
            timeout.as_deref(),
        ),
        Command::Completion { shell } => {
            complete::emit_script(shell);
            Ok(())
        }
    }
}
