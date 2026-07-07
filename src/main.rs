mod cli;
mod conn;
mod error;
mod introspect;
mod ops;
mod out;
mod value;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};

fn main() -> std::process::ExitCode {
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
            unique,
            acquired,
            activatable,
        ),
        Command::Get { service, object, interface, props } => ops::property::get(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            &service,
            &object,
            interface.as_deref(),
            &props,
        ),
        Command::Call { service, object, interface, method, args } => ops::call::run(
            cli.user,
            cli.system,
            cli.address.as_deref(),
            cli.verbose,
            &service,
            &object,
            &interface,
            &method,
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
            &service,
            &object,
            interface.as_deref(),
        ),
        Command::Tree { .. } | Command::Monitor { .. } => {
            Err(error::Error::Msg("not yet implemented".into()))
        }
        Command::Completion { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "busx", &mut std::io::stdout());
            Ok(())
        }
        Command::Complete { .. } => Ok(()),
    }
}
