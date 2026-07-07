mod cli;
mod error;

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
        Command::List { .. } | Command::Tree { .. } | Command::Introspect { .. }
        | Command::Call { .. } | Command::Get { .. } | Command::Set { .. } | Command::Monitor { .. } => {
            Err(error::Error::Msg("not yet implemented".into()))
        }
        Command::Completion { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "busx", &mut std::io::stdout());
            Ok(())
        }
        Command::Complete { .. } => Ok(()),
    }
}
