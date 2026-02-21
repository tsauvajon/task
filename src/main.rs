use clap::Parser;

use task::app;
use task::cli::Cli;

fn main() {
    let cli = Cli::parse();
    let code = app::run(cli);
    std::process::exit(code);
}
