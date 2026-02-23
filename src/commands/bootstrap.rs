use std::env;
use std::path::PathBuf;

use crate::runtime::RuntimeEnvironment;

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    context.log(&format!("Workspace root: {}", context.dev_root().display()));

    if context.command_exists("asdf") {
        let plugins = context
            .run_capture("asdf", &["plugin", "list"], None)
            .unwrap_or_default();
        if !plugins.lines().any(|line| line.trim() == "nodejs") {
            context.log("Installing asdf nodejs plugin");
            context.run_status(
                "asdf",
                &[
                    "plugin",
                    "add",
                    "nodejs",
                    "https://github.com/asdf-vm/asdf-nodejs.git",
                ],
                None,
            )?;
        }

        let home = env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        let asdf_data_dir = env::var("ASDF_DATA_DIR").unwrap_or_else(|_| format!("{home}/.asdf"));
        let import_script = PathBuf::from(asdf_data_dir)
            .join("plugins")
            .join("nodejs")
            .join("bin")
            .join("import-release-team-keyring");
        if import_script.exists()
            && let Err(error) = context.run_status(import_script.as_os_str(), &[], None)
        {
            context.warn(&format!("Could not import nodejs release keyring: {error}"));
        }

        let tool_versions = PathBuf::from(home).join(".tool-versions");
        if tool_versions.exists() {
            context.log("Installing runtimes from ~/.tool-versions");
            context.run_status("asdf", &["install"], None)?;
        }
    } else {
        context.warn(
            "asdf not found. Install toolchain first (nix profile install path:~/flakes#toolchain).",
        );
    }

    if context.command_exists("node") && context.command_exists("corepack") {
        let _ = context.run_status("corepack", &["enable"], None);
    }

    context.log("Bootstrap complete");
    Ok(())
}
