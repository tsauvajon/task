use crate::runtime::RuntimeEnvironment;
use crate::tools::opencode;

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    let mut missing = false;

    println!("DEV_ROOT: {}", context.dev_root().display());
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

    let dev_root = context.dev_root();
    if dev_root.join("repos").is_dir() && dev_root.join("wt").is_dir() {
        println!("[ok]      {} layout", dev_root.display());
    } else {
        println!("[missing] {} layout", dev_root.display());
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
