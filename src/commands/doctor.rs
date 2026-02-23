use crate::runtime::environment::RuntimeEnvironment;
use crate::tools::opencode;

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    let mut missing = false;

    println!("repos_dir: {}", context.repos_dir().display());
    println!("wt_dir: {}", context.wt_dir().display());
    for cmd in [
        "git", "tmux", "vim", "codium", "opencode", "nix", "direnv", "asdf",
    ] {
        if context.command_exists(cmd) {
            println!("[ok]      {cmd}");
        } else {
            println!("[missing] {cmd}");
            missing = true;
        }
    }

    if context.repos_dir().is_dir() && context.wt_dir().is_dir() {
        println!("[ok]      configured layout exists");
    } else {
        println!("[missing] configured layout does not exist");
        missing = true;
    }

    if context.command_exists("opencode") {
        if opencode::auth_storage_reachable(context.process()) {
            println!("[ok]      opencode auth storage reachable");
        } else {
            println!("[warn]    opencode auth storage not initialized yet");
        }
    }

    if missing {
        return Err("Doctor check found missing dependencies".to_string());
    }

    Ok(())
}
