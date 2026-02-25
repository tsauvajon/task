use std::{
    env,
    path::{Path, PathBuf},
};

use crate::runtime::process::ProcessRunner;

use super::asdf_runner::{run_asdf_capture, run_asdf_status};

const NODEJS_PLUGIN_REPO: &str = "https://github.com/asdf-vm/asdf-nodejs.git";

pub fn is_available(process: ProcessRunner) -> bool {
    process.command_exists("asdf")
}

pub fn has_nodejs_plugin(_process: ProcessRunner) -> bool {
    list_plugins()
        .ok()
        .map(|plugins| plugins.lines().any(|line| line.trim() == "nodejs"))
        .unwrap_or(false)
}

pub fn install_nodejs_plugin(process: ProcessRunner) -> Result<(), String> {
    let _ = process; // availability already checked by caller
    run_asdf_status(&["plugin", "add", "nodejs", NODEJS_PLUGIN_REPO], None)
}

pub fn import_nodejs_release_keyring(process: ProcessRunner) -> Result<(), String> {
    let _ = process;
    let Some(script_path) = nodejs_release_keyring_script_path() else {
        return Ok(());
    };

    if !script_path.exists() {
        return Ok(());
    }

    // This runs a shell script, not asdf itself — keep using ProcessRunner.
    ProcessRunner.run_status(script_path.as_os_str(), &[], None)
}

pub fn install_from_user_tool_versions(process: ProcessRunner) -> Result<bool, String> {
    let Some(path) = user_tool_versions_path() else {
        return Ok(false);
    };
    if !path.exists() {
        return Ok(false);
    }

    install(None)?;
    let _ = process;
    Ok(true)
}

pub fn install_from_workspace_tool_versions(
    process: ProcessRunner,
    path: &Path,
) -> Result<bool, String> {
    if !path.join(".tool-versions").exists() {
        return Ok(false);
    }

    install(Some(path))?;
    let _ = process;
    Ok(true)
}

fn list_plugins() -> Result<String, String> {
    run_asdf_capture(&["plugin", "list"], None)
}

fn install(cwd: Option<&Path>) -> Result<(), String> {
    run_asdf_status(&["install"], cwd)
}

fn nodejs_release_keyring_script_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let asdf_data_dir = env::var("ASDF_DATA_DIR").unwrap_or_else(|_| format!("{home}/.asdf"));
    Some(
        PathBuf::from(asdf_data_dir)
            .join("plugins")
            .join("nodejs")
            .join("bin")
            .join("import-release-team-keyring"),
    )
}

fn user_tool_versions_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".tool-versions"))
}
