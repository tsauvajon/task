use crate::runtime::environment::RuntimeEnvironment;
use crate::tools::asdf;
use crate::tools::nodejs::runtime::{corepack_available, enable_corepack, node_available};

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    context.log(&format!("Repos dir: {}", context.repos_dir().display()));
    context.log(&format!("Worktrees dir: {}", context.wt_dir().display()));

    if !asdf::is_available(context.process()) {
        context.warn(
            "asdf not found. Install toolchain first (nix profile install path:~/flakes#toolchain).",
        );
    } else {
        if !asdf::has_nodejs_plugin(context.process()) {
            context.log("Installing asdf nodejs plugin");
            asdf::install_nodejs_plugin(context.process())?;
        }

        if let Err(error) = asdf::import_nodejs_release_keyring(context.process()) {
            context.warn(&format!("Could not import nodejs release keyring: {error}"));
        }

        if asdf::install_from_user_tool_versions(context.process())? {
            context.log("Installing runtimes from ~/.tool-versions");
        }
    }

    if node_available(context.process()) && corepack_available(context.process()) {
        let _ = enable_corepack(context.process());
        context.log("Enabled corepack");
    }

    context.log("Bootstrap complete");
    Ok(())
}
