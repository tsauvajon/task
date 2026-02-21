use std::env;
use std::path::PathBuf;

use crate::layout::Layout;

pub fn run(layout: &Layout) -> Result<(), String> {
    super::ensure_layout(layout)?;
    super::log(&format!(
        "Workspace root: {}",
        super::default_dev_root().display()
    ));

    if super::command_exists("asdf") {
        let plugins = super::run_capture("asdf", &["plugin", "list"], None).unwrap_or_default();
        if !plugins.lines().any(|line| line.trim() == "nodejs") {
            super::log("Installing asdf nodejs plugin");
            super::run_status(
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
            && let Err(error) = super::run_status(import_script.as_os_str(), &[], None)
        {
            super::warn(&format!("Could not import nodejs release keyring: {error}"));
        }

        let tool_versions = PathBuf::from(home).join(".tool-versions");
        if tool_versions.exists() {
            super::log("Installing runtimes from ~/.tool-versions");
            super::run_status("asdf", &["install"], None)?;
        }
    } else {
        super::warn(
            "asdf not found. Install toolchain first (nix profile install path:~/flakes#toolchain).",
        );
    }

    if super::command_exists("node") && super::command_exists("corepack") {
        let _ = super::run_status("corepack", &["enable"], None);
    }

    super::log("Bootstrap complete");
    Ok(())
}
