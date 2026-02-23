use clap::Parser;

use task::command_line::Cli;
use task::entrypoint;

fn main() {
    let cli = Cli::parse();
    let code = entrypoint::run_cli_and_exit_code(cli);
    std::process::exit(code);
}
