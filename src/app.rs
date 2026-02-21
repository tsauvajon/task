use crate::cli::Cli;
use crate::commands;

pub fn run(cli: Cli) -> i32 {
    match commands::run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}
