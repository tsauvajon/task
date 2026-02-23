use crate::runtime::RuntimeEnvironment;
use crate::tools::{asdf, nodejs};

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    context.log(&format!("Workspace root: {}", context.dev_root().display()));

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

    if nodejs::node_available(context.process()) && nodejs::corepack_available(context.process()) {
        let _ = nodejs::enable_corepack(context.process());
        context.log("Enabled corepack");
    }

    context.log("Bootstrap complete");
    Ok(())
}
