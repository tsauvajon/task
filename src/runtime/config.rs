use std::{
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use dialoguer::{Input, theme::ColorfulTheme};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub repos_dir: PathBuf,
    pub wt_dir: PathBuf,
    pub codium_trusted_roots: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskConfigFile {
    repos_dir: String,
    wt_dir: String,
    #[serde(default)]
    vscodium: Option<VscodiumConfigFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VscodiumConfigFile {
    #[serde(default)]
    trusted_roots: Vec<String>,
}

impl TaskConfig {
    pub fn load_if_present() -> Result<Option<Self>> {
        let config_path = config_file_path()?;
        if !config_path.is_file() {
            return Ok(None);
        }
        load_config(&config_path).map(Some)
    }

    pub fn load_or_initialize() -> Result<Self> {
        let config_path = config_file_path()?;
        if config_path.is_file() {
            return load_config(&config_path);
        }

        if !is_interactive_terminal() {
            return Err(Error::failed(format!(
                "Missing config file at {}. Run task in an interactive terminal to initialize it (for example: 'task doctor --fix' or 'task bootstrap').",
                config_path.display()
            )));
        }

        bootstrap_config(&config_path)
    }
}

fn load_config(config_path: &Path) -> Result<TaskConfig> {
    let text = fs::read_to_string(config_path).map_err(|err| {
        Error::failed(format!(
            "Could not read config file {}: {err}",
            config_path.display()
        ))
    })?;
    let parsed = toml::from_str::<TaskConfigFile>(&text).map_err(|err| {
        Error::failed(format!(
            "Could not parse config file {}: {err}",
            config_path.display()
        ))
    })?;
    to_runtime_config(parsed)
}

fn bootstrap_config(config_path: &Path) -> Result<TaskConfig> {
    let home = home_dir()?;
    let default_repos = home.join("dev/repos");
    let default_wt = home.join("dev/wt");

    let repos_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("repos_dir")
        .default(default_repos.display().to_string())
        .interact_text()?;

    let wt_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("wt_dir")
        .default(default_wt.display().to_string())
        .interact_text()?;

    let config = to_runtime_config(TaskConfigFile {
        repos_dir,
        wt_dir,
        vscodium: None,
    })?;
    write_config(config_path, &config)?;
    Ok(config)
}

fn write_config(config_path: &Path, config: &TaskConfig) -> Result<()> {
    let parent = config_path.parent().ok_or_else(|| {
        Error::failed(format!(
            "Could not resolve parent directory for {}",
            config_path.display()
        ))
    })?;
    fs::create_dir_all(parent)
        .map_err(|err| Error::failed(format!("Could not create {}: {err}", parent.display())))?;

    let file = TaskConfigFile {
        repos_dir: config.repos_dir.display().to_string(),
        wt_dir: config.wt_dir.display().to_string(),
        vscodium: if config.codium_trusted_roots.is_empty() {
            None
        } else {
            Some(VscodiumConfigFile {
                trusted_roots: config
                    .codium_trusted_roots
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            })
        },
    };
    let text = toml::to_string_pretty(&file)?;
    fs::write(config_path, text).map_err(|err| {
        Error::failed(format!("Could not write {}: {err}", config_path.display()))
    })?;
    Ok(())
}

fn to_runtime_config(file: TaskConfigFile) -> Result<TaskConfig> {
    let home = home_dir()?;
    let repos_dir = expand_path(file.repos_dir.trim(), &home)?;
    let wt_dir = expand_path(file.wt_dir.trim(), &home)?;
    let codium_trusted_roots = file
        .vscodium
        .map(|config| {
            config
                .trusted_roots
                .iter()
                .map(|root| expand_path(root.trim(), &home))
                .collect::<Result<Vec<PathBuf>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(TaskConfig {
        repos_dir,
        wt_dir,
        codium_trusted_roots,
    })
}

fn expand_path(value: &str, home: &Path) -> Result<PathBuf> {
    if value.is_empty() {
        return Err(Error::failed("Config paths must not be empty"));
    }

    if value == "~" {
        return Ok(home.to_path_buf());
    }

    if let Some(stripped) = value.strip_prefix("~/") {
        return Ok(home.join(stripped));
    }

    Ok(PathBuf::from(value))
}

pub fn config_file_path() -> Result<PathBuf> {
    Ok(config_dir_path()?.join("config.toml"))
}

pub fn config_dir_path() -> Result<PathBuf> {
    if let Ok(base) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(base).join("task"));
    }
    Ok(home_dir()?.join(".config/task"))
}

fn home_dir() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| Error::failed("HOME is not set"))
}

pub fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{TaskConfigFile, VscodiumConfigFile, expand_path, to_runtime_config};

    mod expand_path {
        use super::*;

        #[test]
        fn supports_tilde_prefix() {
            let home = Path::new("/tmp/home");
            assert_eq!(
                expand_path("~/dev/repos", home).unwrap(),
                home.join("dev/repos")
            );
        }

        #[test]
        fn rejects_empty_values() {
            let home = Path::new("/tmp/home");
            assert!(expand_path("", home).is_err());
        }

        #[test]
        fn returns_home_for_tilde_alone() {
            let home = Path::new("/home/user");
            assert_eq!(expand_path("~", home).unwrap(), home);
        }

        #[test]
        fn returns_absolute_path_unchanged() {
            let home = Path::new("/home/user");
            assert_eq!(
                expand_path("/absolute/path", home).unwrap(),
                std::path::PathBuf::from("/absolute/path")
            );
        }

        #[test]
        fn returns_absolute_path_unchanged_without_tilde() {
            let home = Path::new("/home/user");
            assert_eq!(
                expand_path("/srv/repos", home).unwrap(),
                std::path::PathBuf::from("/srv/repos")
            );
        }
    }

    mod to_runtime_config {
        use super::*;

        #[test]
        fn supports_legacy_file_without_vscodium_section() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/dev/repos".to_string(),
                wt_dir: "~/dev/wt".to_string(),
                vscodium: None,
            })
            .expect("runtime config");

            assert!(config.codium_trusted_roots.is_empty());
        }

        #[test]
        fn expands_vscodium_trusted_roots() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/dev/repos".to_string(),
                wt_dir: "~/dev/wt".to_string(),
                vscodium: Some(VscodiumConfigFile {
                    trusted_roots: vec!["~/dev/wt/github.com/tsauvajon".to_string()],
                }),
            })
            .expect("runtime config");

            assert_eq!(config.codium_trusted_roots.len(), 1);
            assert!(config.codium_trusted_roots[0].ends_with("dev/wt/github.com/tsauvajon"));
        }

        #[test]
        fn expands_repos_and_wt_dirs() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/repos".to_string(),
                wt_dir: "~/wt".to_string(),
                vscodium: None,
            })
            .expect("runtime config");

            assert!(config.repos_dir.ends_with("repos"));
            assert!(config.wt_dir.ends_with("wt"));
        }
    }

    mod config_dir_path {
        use super::super::config_dir_path;

        #[test]
        fn uses_xdg_config_home_when_set() {
            // Temporarily set XDG_CONFIG_HOME for this test.
            let prev = std::env::var_os("XDG_CONFIG_HOME");
            // SAFETY: single-threaded test binary section; no other thread reads this var.
            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", "/tmp/custom-xdg");
            }
            let path = config_dir_path().expect("config_dir_path");
            // Restore
            unsafe {
                match prev {
                    Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                    None => std::env::remove_var("XDG_CONFIG_HOME"),
                }
            }
            assert_eq!(path, std::path::PathBuf::from("/tmp/custom-xdg/task"));
        }
    }
}
