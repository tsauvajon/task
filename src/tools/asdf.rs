use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{
    error::Result,
    runtime::{nix_store::NixRunner, process::ManagedTool},
};

static ASDF: NixRunner = NixRunner::new(ManagedTool::Asdf);

fn capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    ASDF.capture(args, cwd)
}

fn status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    ASDF.status(args, cwd)
}

const NODEJS_PLUGIN_REPO: &str = "https://github.com/asdf-vm/asdf-nodejs.git";

pub fn is_available() -> bool {
    crate::runtime::process::command_exists("asdf")
}

pub fn has_nodejs_plugin() -> bool {
    if !is_available() {
        return false;
    }
    list_plugins().is_ok_and(|plugins| plugins.lines().any(|line| line.trim() == "nodejs"))
}

pub fn install_nodejs_plugin() -> Result<()> {
    status(&["plugin", "add", "nodejs", NODEJS_PLUGIN_REPO], None)
}

pub fn import_nodejs_release_keyring() -> Result<()> {
    let Some(script_path) = nodejs_release_keyring_script_path() else {
        return Ok(());
    };

    if !script_path.exists() {
        return Ok(());
    }

    // This runs a shell script, not asdf itself — keep using process::run_status.
    crate::runtime::process::run_status(script_path.as_os_str(), &[], None)
}

pub fn install_from_user_tool_versions() -> Result<bool> {
    let Some(path) = user_tool_versions_path() else {
        return Ok(false);
    };
    if !path.exists() {
        return Ok(false);
    }
    install(None)?;
    Ok(true)
}

pub fn install_from_workspace_tool_versions(path: &Path) -> Result<bool> {
    if !path.join(".tool-versions").exists() {
        return Ok(false);
    }
    install(Some(path))?;
    Ok(true)
}

fn list_plugins() -> Result<String> {
    capture(&["plugin", "list"], None)
}

fn install(cwd: Option<&Path>) -> Result<()> {
    status(&["install"], cwd)
}

fn nodejs_release_keyring_script_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let asdf_data_dir = env::var("ASDF_DATA_DIR").unwrap_or_else(|_| format!("{home}/.asdf"));
    Some(PathBuf::from(asdf_data_dir).join("plugins/nodejs/bin/import-release-team-keyring"))
}

fn user_tool_versions_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".tool-versions"))
}
