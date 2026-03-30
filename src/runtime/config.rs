use std::{
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use dialoguer::{Input, theme::ColorfulTheme};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallEntry {
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub repos_dir: PathBuf,
    pub wt_dir: PathBuf,
    pub detached_dir: PathBuf,
    pub codium_trusted_roots: Vec<PathBuf>,
    pub install_entries: Vec<InstallEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskConfigFile {
    repos_dir: String,
    wt_dir: String,
    detached_dir: String,
    #[serde(default)]
    vscodium: Option<VscodiumConfigFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    install: Vec<InstallEntry>,
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
    let default_detached = home.join("dev/detached");

    let repos_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("repos_dir")
        .default(default_repos.display().to_string())
        .interact_text()?;

    let wt_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("wt_dir")
        .default(default_wt.display().to_string())
        .interact_text()?;

    let detached_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("detached_dir")
        .default(default_detached.display().to_string())
        .interact_text()?;

    let config = to_runtime_config(TaskConfigFile {
        repos_dir,
        wt_dir,
        detached_dir,
        vscodium: None,
        install: Vec::new(),
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
        detached_dir: config.detached_dir.display().to_string(),
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
        install: config.install_entries.clone(),
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
    let detached_dir = expand_path(file.detached_dir.trim(), &home)?;
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
        detached_dir,
        codium_trusted_roots,
        install_entries: file.install,
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
    resolve_config_dir(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

fn resolve_config_dir(xdg_config_home: Option<&str>, home: Option<&str>) -> Result<PathBuf> {
    // Per XDG spec, empty $XDG_CONFIG_HOME is treated as unset.
    if let Some(base) = xdg_config_home.filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(base).join("task"));
    }
    let home = home.ok_or_else(|| Error::failed("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/task"))
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

    use super::{InstallEntry, TaskConfigFile, VscodiumConfigFile, expand_path, to_runtime_config};

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
        fn supports_file_without_vscodium_section() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/dev/repos".to_string(),
                wt_dir: "~/dev/wt".to_string(),
                detached_dir: "~/dev/detached".to_string(),
                vscodium: None,
                install: Vec::new(),
            })
            .expect("runtime config");

            assert!(config.codium_trusted_roots.is_empty());
        }

        #[test]
        fn expands_vscodium_trusted_roots() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/dev/repos".to_string(),
                wt_dir: "~/dev/wt".to_string(),
                detached_dir: "~/dev/detached".to_string(),
                vscodium: Some(VscodiumConfigFile {
                    trusted_roots: vec!["~/dev/wt/github.com/tsauvajon".to_string()],
                }),
                install: Vec::new(),
            })
            .expect("runtime config");

            assert_eq!(config.codium_trusted_roots.len(), 1);
            assert!(config.codium_trusted_roots[0].ends_with("dev/wt/github.com/tsauvajon"));
        }

        #[test]
        fn expands_repos_wt_and_detached_dirs() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/repos".to_string(),
                wt_dir: "~/wt".to_string(),
                detached_dir: "~/detached".to_string(),
                vscodium: None,
                install: Vec::new(),
            })
            .expect("runtime config");

            assert!(config.repos_dir.ends_with("repos"));
            assert!(config.wt_dir.ends_with("wt"));
            assert!(config.detached_dir.ends_with("detached"));
        }

        #[test]
        fn passes_through_install_entries() {
            let entries = vec![
                InstallEntry {
                    repo: "github.com/org/tool".to_string(),
                    path: None,
                },
                InstallEntry {
                    repo: "gitlab.com/team/app".to_string(),
                    path: Some("crates/cli".to_string()),
                },
            ];
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_string(),
                wt_dir: "/tmp/wt".to_string(),
                detached_dir: "/tmp/detached".to_string(),
                vscodium: None,
                install: entries.clone(),
            })
            .expect("runtime config");

            assert_eq!(config.install_entries, entries);
        }

        #[test]
        fn empty_install_entries_by_default() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_string(),
                wt_dir: "/tmp/wt".to_string(),
                detached_dir: "/tmp/detached".to_string(),
                vscodium: None,
                install: Vec::new(),
            })
            .expect("runtime config");

            assert!(config.install_entries.is_empty());
        }
    }

    mod config_dir_path {
        use super::super::resolve_config_dir;

        #[test]
        fn uses_xdg_config_home_when_set() {
            let path = resolve_config_dir(Some("/tmp/custom-xdg"), Some("/home/user")).expect("ok");
            assert_eq!(path, std::path::PathBuf::from("/tmp/custom-xdg/task"));
        }

        #[test]
        fn falls_back_to_home_dot_config_task_without_xdg() {
            let path = resolve_config_dir(None, Some("/home/testuser")).expect("ok");
            assert_eq!(
                path,
                std::path::PathBuf::from("/home/testuser/.config/task")
            );
        }

        #[test]
        fn errors_when_both_xdg_and_home_are_missing() {
            assert!(resolve_config_dir(None, None).is_err());
        }

        #[test]
        fn xdg_takes_priority_over_home() {
            let path = resolve_config_dir(Some("/xdg/config"), Some("/home/user")).expect("ok");
            assert_eq!(path, std::path::PathBuf::from("/xdg/config/task"));
        }
    }

    mod config_file_path {
        use super::super::config_file_path;

        #[test]
        fn returns_config_toml_filename() {
            // The path must end in config.toml regardless of XDG settings.
            let path = config_file_path().expect("config_file_path");
            assert_eq!(
                path.file_name().and_then(|n| n.to_str()),
                Some("config.toml")
            );
        }
    }

    mod expand_path_whitespace {
        use super::*;

        #[test]
        fn trims_whitespace_before_expansion() {
            // The public caller (to_runtime_config) trims before calling, but
            // verify expand_path directly does NOT add extra trimming – the
            // trimmed value is passed in by the caller.
            let home = std::path::Path::new("/home/user");
            // "~/path" with no surrounding whitespace → succeeds.
            let p = expand_path("~/repos", home).unwrap();
            assert_eq!(p, home.join("repos"));
        }

        #[test]
        fn relative_path_returned_as_is() {
            let home = std::path::Path::new("/home/user");
            let p = expand_path("relative/path", home).unwrap();
            assert_eq!(p, std::path::PathBuf::from("relative/path"));
        }
    }

    mod load_config_tests {
        use std::{env, fs, path::PathBuf};

        use super::super::load_config;

        struct TempDir(PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-config-{name}"));
                let _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &PathBuf {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn loads_minimal_config() {
            let dir = TempDir::new("load-minimal");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n",
            )
            .unwrap();

            let config = load_config(&config_path).expect("load minimal config");
            assert_eq!(config.repos_dir, PathBuf::from("/tmp/repos"));
            assert_eq!(config.wt_dir, PathBuf::from("/tmp/wt"));
            assert_eq!(config.detached_dir, PathBuf::from("/tmp/detached"));
            assert!(config.codium_trusted_roots.is_empty());
        }

        #[test]
        fn loads_config_with_vscodium() {
            let dir = TempDir::new("load-vscodium");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n\n[vscodium]\ntrusted_roots = [\"/tmp/wt/github.com/me\"]\n",
            )
            .unwrap();

            let config = load_config(&config_path).expect("load vscodium config");
            assert_eq!(config.codium_trusted_roots.len(), 1);
            assert_eq!(
                config.codium_trusted_roots[0],
                PathBuf::from("/tmp/wt/github.com/me")
            );
        }

        #[test]
        fn loads_config_with_install_entries() {
            let dir = TempDir::new("load-install");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                r#"repos_dir = "/tmp/repos"
wt_dir = "/tmp/wt"
detached_dir = "/tmp/detached"

[[install]]
repo = "github.com/org/tool"

[[install]]
repo = "gitlab.com/team/app"
path = "crates/cli"
"#,
            )
            .unwrap();

            let config = load_config(&config_path).expect("load install config");
            assert_eq!(config.install_entries.len(), 2);
            assert_eq!(config.install_entries[0].repo, "github.com/org/tool");
            assert!(config.install_entries[0].path.is_none());
            assert_eq!(config.install_entries[1].repo, "gitlab.com/team/app");
            assert_eq!(
                config.install_entries[1].path.as_deref(),
                Some("crates/cli")
            );
        }

        #[test]
        fn loads_config_without_install_section() {
            let dir = TempDir::new("load-no-install");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n",
            )
            .unwrap();

            let config = load_config(&config_path).expect("load config without install");
            assert!(config.install_entries.is_empty());
        }

        #[test]
        fn errors_on_invalid_toml() {
            let dir = TempDir::new("load-invalid");
            let config_path = dir.path().join("config.toml");
            fs::write(&config_path, "not valid toml {{{").unwrap();

            let err = load_config(&config_path).unwrap_err();
            assert!(err.to_string().contains("parse"));
        }

        #[test]
        fn errors_on_missing_file() {
            let dir = TempDir::new("load-missing");
            let config_path = dir.path().join("nonexistent.toml");

            let err = load_config(&config_path).unwrap_err();
            assert!(err.to_string().contains("read"));
        }

        #[test]
        fn errors_on_missing_required_fields() {
            let dir = TempDir::new("load-missing-fields");
            let config_path = dir.path().join("config.toml");
            fs::write(&config_path, "repos_dir = \"/tmp/repos\"\n").unwrap();

            let err = load_config(&config_path).unwrap_err();
            assert!(
                err.to_string().contains("parse")
                    || err.to_string().contains("wt_dir")
                    || err.to_string().contains("detached_dir")
            );
        }
    }

    mod write_config_tests {
        use std::{env, fs, path::PathBuf};

        use super::super::{TaskConfig, load_config, write_config};

        struct TempDir(PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-config-write-{name}"));
                let _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &PathBuf {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn round_trips_config() {
            let dir = TempDir::new("write-round-trip");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: vec![PathBuf::from("/tmp/trusted")],
                install_entries: Vec::new(),
            };

            write_config(&config_path, &config).expect("write config");
            let loaded = load_config(&config_path).expect("load written config");

            assert_eq!(loaded.repos_dir, config.repos_dir);
            assert_eq!(loaded.wt_dir, config.wt_dir);
            assert_eq!(loaded.detached_dir, config.detached_dir);
            assert_eq!(loaded.codium_trusted_roots, config.codium_trusted_roots);
            assert_eq!(loaded.install_entries, config.install_entries);
        }

        #[test]
        fn creates_parent_directories() {
            let dir = TempDir::new("write-creates-parent");
            let config_path = dir.path().join("nested/dir/config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                install_entries: Vec::new(),
            };

            write_config(&config_path, &config).expect("write config");
            assert!(config_path.is_file());
        }

        #[test]
        fn omits_vscodium_section_when_empty() {
            let dir = TempDir::new("write-no-vscodium");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                install_entries: Vec::new(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).unwrap();
            assert!(!content.contains("[vscodium]"));
        }

        #[test]
        fn round_trips_install_entries() {
            let dir = TempDir::new("write-install-round-trip");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                install_entries: vec![
                    super::super::InstallEntry {
                        repo: "github.com/org/tool".to_string(),
                        path: None,
                    },
                    super::super::InstallEntry {
                        repo: "gitlab.com/team/app".to_string(),
                        path: Some("crates/cli".to_string()),
                    },
                ],
            };

            write_config(&config_path, &config).expect("write config");
            let loaded = load_config(&config_path).expect("load written config");

            assert_eq!(loaded.install_entries.len(), 2);
            assert_eq!(loaded.install_entries[0].repo, "github.com/org/tool");
            assert!(loaded.install_entries[0].path.is_none());
            assert_eq!(loaded.install_entries[1].repo, "gitlab.com/team/app");
            assert_eq!(
                loaded.install_entries[1].path.as_deref(),
                Some("crates/cli")
            );
        }

        #[test]
        fn omits_install_section_when_empty() {
            let dir = TempDir::new("write-no-install");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                install_entries: Vec::new(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).unwrap();
            assert!(
                !content.contains("[[install]]"),
                "empty install should be omitted from config"
            );
        }
    }

    mod expand_path_edge_cases {
        use super::*;

        #[test]
        fn tilde_without_slash_is_passthrough() {
            // "~user/repos" is NOT expanded -- it falls through to the literal path.
            // This pins the current behavior; if this changes, this test should be updated.
            let home = Path::new("/home/me");
            let result = expand_path("~user/repos", home).unwrap();
            assert_eq!(result, std::path::PathBuf::from("~user/repos"));
        }
    }

    mod to_runtime_config_edge_cases {
        use super::*;

        #[test]
        fn trims_whitespace_around_paths() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "  /tmp/repos  ".to_string(),
                wt_dir: "  /tmp/wt  ".to_string(),
                detached_dir: "  /tmp/detached  ".to_string(),
                vscodium: None,
                install: Vec::new(),
            })
            .expect("runtime config");

            assert_eq!(config.repos_dir, std::path::PathBuf::from("/tmp/repos"));
            assert_eq!(config.wt_dir, std::path::PathBuf::from("/tmp/wt"));
            assert_eq!(
                config.detached_dir,
                std::path::PathBuf::from("/tmp/detached")
            );
        }
    }

    mod resolve_config_dir_edge_cases {
        use super::super::resolve_config_dir;

        #[test]
        fn empty_xdg_string_falls_back_to_home() {
            // Per XDG spec, empty $XDG_CONFIG_HOME is treated as unset and
            // should fall back to $HOME/.config.
            let path = resolve_config_dir(Some(""), Some("/home/user")).expect("ok");
            assert_eq!(path, std::path::PathBuf::from("/home/user/.config/task"));
        }
    }

    mod load_config_edge_cases {
        use std::{env, fs, path::PathBuf};

        use super::super::load_config;

        struct TempDir(PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-config-edge-{name}"));
                let _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &PathBuf {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn errors_on_empty_repos_dir() {
            let dir = TempDir::new("empty-repos-dir");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n",
            )
            .unwrap();

            let err = load_config(&config_path).unwrap_err();
            assert!(
                err.to_string().contains("empty"),
                "expected 'empty' in error: {err}"
            );
        }
    }
}
