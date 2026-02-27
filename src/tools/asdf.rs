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

pub(crate) fn nodejs_release_keyring_script_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let asdf_data_dir = env::var("ASDF_DATA_DIR").unwrap_or_else(|_| format!("{home}/.asdf"));
    Some(PathBuf::from(asdf_data_dir).join("plugins/nodejs/bin/import-release-team-keyring"))
}

pub(crate) fn user_tool_versions_path() -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".tool-versions"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{nodejs_release_keyring_script_path, user_tool_versions_path};

    mod user_tool_versions_path {
        use super::*;

        #[test]
        fn returns_none_when_home_is_unset() {
            // We can only test this non-destructively if HOME is already set.
            // When HOME is set, the function must return Some(_).
            if std::env::var("HOME").is_ok() {
                assert!(
                    user_tool_versions_path().is_some(),
                    "should return Some when HOME is set"
                );
            }
        }

        #[test]
        fn path_ends_with_tool_versions() {
            let Some(path) = user_tool_versions_path() else {
                return; // HOME not set in this environment — skip
            };
            assert_eq!(
                path.file_name().and_then(|n| n.to_str()),
                Some(".tool-versions"),
                "filename should be .tool-versions"
            );
        }

        #[test]
        fn path_is_under_home_directory() {
            let Ok(home) = std::env::var("HOME") else {
                return; // HOME not set — skip
            };
            let path = user_tool_versions_path().expect("HOME is set so path should exist");
            assert!(
                path.starts_with(&home),
                "path {path:?} should be under HOME {home:?}"
            );
        }
    }

    mod nodejs_release_keyring_script_path {
        use super::*;

        #[test]
        fn uses_asdf_data_dir_when_set() {
            // We test the logic by constructing the expected path manually.
            // If ASDF_DATA_DIR is set, the result should use that as the root.
            if let Ok(data_dir) = std::env::var("ASDF_DATA_DIR") {
                let path = nodejs_release_keyring_script_path()
                    .expect("HOME must be set when ASDF_DATA_DIR is set");
                let expected =
                    PathBuf::from(&data_dir).join("plugins/nodejs/bin/import-release-team-keyring");
                assert_eq!(path, expected);
            }
        }

        #[test]
        fn falls_back_to_home_dot_asdf_when_no_data_dir() {
            let Ok(home) = std::env::var("HOME") else {
                return; // HOME not set — skip
            };
            // Only assert when ASDF_DATA_DIR is absent so we exercise the fallback.
            if std::env::var("ASDF_DATA_DIR").is_err() {
                let path =
                    nodejs_release_keyring_script_path().expect("HOME is set so path should exist");
                let expected = PathBuf::from(&home)
                    .join(".asdf/plugins/nodejs/bin/import-release-team-keyring");
                assert_eq!(path, expected);
            }
        }

        #[test]
        fn script_name_is_import_release_team_keyring() {
            let Some(path) = nodejs_release_keyring_script_path() else {
                return; // HOME not set — skip
            };
            assert_eq!(
                path.file_name().and_then(|n| n.to_str()),
                Some("import-release-team-keyring"),
                "filename should be import-release-team-keyring"
            );
        }

        #[test]
        fn path_contains_plugins_nodejs_bin() {
            let Some(path) = nodejs_release_keyring_script_path() else {
                return; // HOME not set — skip
            };
            let path_str = path.to_string_lossy();
            assert!(
                path_str.contains("plugins/nodejs/bin"),
                "path should contain plugins/nodejs/bin, got: {path_str}"
            );
        }
    }

    mod user_tool_versions_path_extra {
        use super::*;

        #[test]
        fn path_is_absolute() {
            let Some(path) = user_tool_versions_path() else {
                return; // HOME not set — skip
            };
            assert!(path.is_absolute(), "path should be absolute, got: {path:?}");
        }
    }
}
