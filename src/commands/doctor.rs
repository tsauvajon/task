use crate::layout::Layout;

pub fn run(_layout: &Layout) -> Result<(), String> {
    let mut missing = false;

    println!("DEV_ROOT: {}", super::default_dev_root().display());
    for cmd in [
        "git", "tmux", "vim", "codium", "opencode", "nix", "direnv", "asdf",
    ] {
        if super::command_exists(cmd) {
            println!("[ok]      {cmd}");
        } else {
            println!("[missing] {cmd}");
            missing = true;
        }
    }

    let dev_root = super::default_dev_root();
    if dev_root.join("repos").is_dir() && dev_root.join("wt").is_dir() {
        println!("[ok]      {} layout", dev_root.display());
    } else {
        println!("[missing] {} layout", dev_root.display());
        missing = true;
    }

    if super::command_exists("opencode") {
        if super::run_status("opencode", &["auth", "list"], None).is_ok() {
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
