use std::process::ExitCode;

use clap::Parser;
use task::commands::{self, Cli};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match commands::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if task::runtime::process::write_stderr_line(format_args!("error: {err}")).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}
