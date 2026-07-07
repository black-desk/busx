// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod cli;
mod complete;
mod conn;
mod error;
mod introspect;
mod ops;
mod out;
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
    match complete::try_complete() {
        Ok(true) => return std::process::ExitCode::SUCCESS,
        Ok(false) => {}
        Err(e) => {
            eprintln!("busx: {e}");
            return std::process::ExitCode::FAILURE;
        }
    }

    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("busx: {e}");
            e.exit_code()
        }
    }
}

fn run(cli: Cli) -> error::Result<()> {
    match cli.command {
        Command::List { unique, acquired, activatable } => ops::list::run(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            cli.json,
            unique,
            acquired,
            activatable,
        ),
        Command::Get { service, object, interface, props } => ops::property::get(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            cli.json,
            &service,
            &object,
            interface.as_deref(),
            &props,
        ),
        Command::Call { service, object, interface, method, signature, args } => ops::call::run(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            cli.json,
            &service,
            &object,
            &interface,
            &method,
            &signature,
            &args,
        ),
        Command::Set { service, object, interface, property, signature, value } => {
            ops::property::set(
                cli.user,
                cli.system,
                cli.address.as_deref(),
                cli.verbose,
                &service,
                &object,
                &interface,
                &property,
                &signature,
                &value,
            )
        }
        Command::Introspect { service, object, interface } => ops::introspect::run(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            cli.json,
            &service,
            &object,
            interface.as_deref(),
        ),
        Command::Tree { service } => ops::tree::run(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            cli.json,
            &service,
        ),
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
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            cli.json,
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
            crate::complete::emit_script(shell);
            Ok(())
        }
    }
}
