use std::{
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use dialoguer::{Input, theme::ColorfulTheme};
use serde::{Deserialize, Serialize};

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
    pub fn load_if_present() -> Result<Option<Self>, String> {
        let config_path = config_file_path()?;
        if !config_path.is_file() {
            return Ok(None);
        }

        load_config(&config_path).map(Some)
    }

    pub fn load_or_initialize() -> Result<Self, String> {
        let config_path = config_file_path()?;
        if config_path.is_file() {
            return load_config(&config_path);
        }

        if !is_interactive_terminal() {
            return Err(format!(
                "Missing config file at {}. Run task in an interactive terminal to initialize it (for example: 'task doctor --fix' or 'task bootstrap').",
                config_path.display()
            ));
        }

        bootstrap_config(&config_path)
    }
}

fn load_config(config_path: &Path) -> Result<TaskConfig, String> {
    let text = fs::read_to_string(config_path).map_err(|error| {
        format!(
            "Could not read config file {}: {error}",
            config_path.display()
        )
    })?;
    let parsed = toml::from_str::<TaskConfigFile>(&text).map_err(|error| {
        format!(
            "Could not parse config file {}: {error}",
            config_path.display()
        )
    })?;
    to_runtime_config(parsed)
}

fn bootstrap_config(config_path: &Path) -> Result<TaskConfig, String> {
    let home = home_dir()?;
    let default_repos = home.join("dev/repos");
    let default_wt = home.join("dev/wt");

    let repos_dir = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("repos_dir")
        .default(default_repos.display().to_string())
        .interact_text()
        .map_err(|error| error.to_string())?;

    let wt_dir = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("wt_dir")
        .default(default_wt.display().to_string())
        .interact_text()
        .map_err(|error| error.to_string())?;

    let config = to_runtime_config(TaskConfigFile {
        repos_dir,
        wt_dir,
        vscodium: None,
    })?;
    write_config(config_path, &config)?;
    Ok(config)
}

fn write_config(config_path: &Path, config: &TaskConfig) -> Result<(), String> {
    let parent = config_path.parent().ok_or_else(|| {
        format!(
            "Could not resolve parent directory for {}",
            config_path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;

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
    let text = toml::to_string_pretty(&file).map_err(|error| error.to_string())?;
    fs::write(config_path, text)
        .map_err(|error| format!("Could not write {}: {error}", config_path.display()))?;
    Ok(())
}

fn to_runtime_config(file: TaskConfigFile) -> Result<TaskConfig, String> {
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
                .collect::<Result<Vec<PathBuf>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(TaskConfig {
        repos_dir,
        wt_dir,
        codium_trusted_roots,
    })
}

fn expand_path(value: &str, home: &Path) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err("Config paths must not be empty".to_string());
    }

    if value == "~" {
        return Ok(home.to_path_buf());
    }

    if let Some(stripped) = value.strip_prefix("~/") {
        return Ok(home.join(stripped));
    }

    Ok(PathBuf::from(value))
}

pub fn config_file_path() -> Result<PathBuf, String> {
    Ok(config_dir_path()?.join("config.toml"))
}

pub fn config_dir_path() -> Result<PathBuf, String> {
    if let Ok(base) = std::env::var("XDG_CONFIG_HOME") {
        let base = PathBuf::from(base);
        return Ok(base.join("task"));
    }

    let home = home_dir()?;
    Ok(home.join(".config/task"))
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "HOME is not set".to_string())
}

pub fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{TaskConfigFile, VscodiumConfigFile, expand_path, to_runtime_config};

    #[test]
    fn expand_path_supports_tilde_prefix() {
        let home = Path::new("/tmp/home");
        assert_eq!(
            expand_path("~/dev/repos", home).unwrap(),
            home.join("dev/repos")
        );
    }

    #[test]
    fn expand_path_rejects_empty_values() {
        let home = Path::new("/tmp/home");
        assert!(expand_path("", home).is_err());
    }

    #[test]
    fn to_runtime_config_supports_legacy_file_without_vscodium_section() {
        let config = to_runtime_config(TaskConfigFile {
            repos_dir: "~/dev/repos".to_string(),
            wt_dir: "~/dev/wt".to_string(),
            vscodium: None,
        })
        .expect("runtime config");

        assert!(config.codium_trusted_roots.is_empty());
    }

    #[test]
    fn to_runtime_config_expands_vscodium_trusted_roots() {
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
}
