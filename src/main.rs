use std::process::ExitCode;

use clap::Parser;
use task::commands::{self, Cli};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match commands::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
