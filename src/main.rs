use clap::Parser;

use task::commands::{self, Cli};

fn main() {
    let cli = Cli::parse();
    let code = match commands::run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    };
    std::process::exit(code);
}
