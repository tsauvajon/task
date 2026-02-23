use crate::command_line::Cli;
use crate::commands;

pub fn run_cli_and_exit_code(cli: Cli) -> i32 {
    match commands::run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}
