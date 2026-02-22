use std::io;

use clap::CommandFactory;
use clap_complete::{generate, shells};

use crate::cli::{Cli, CompletionShell};

pub fn run(shell: CompletionShell) -> Result<(), String> {
    let mut command = Cli::command();
    let name = command.get_name().to_string();

    match shell {
        CompletionShell::Bash => generate(shells::Bash, &mut command, name, &mut io::stdout()),
        CompletionShell::Fish => generate(shells::Fish, &mut command, name, &mut io::stdout()),
        CompletionShell::Zsh => generate(shells::Zsh, &mut command, name, &mut io::stdout()),
    }

    Ok(())
}
