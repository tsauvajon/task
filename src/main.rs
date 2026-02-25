use clap::Parser;
use task::commands::{self, Cli};

fn main() {
    let cli = Cli::parse();
    let code = match commands::run(cli) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    };
    std::process::exit(code);
}
