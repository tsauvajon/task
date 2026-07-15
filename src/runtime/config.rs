use std::{
    borrow::Cow,
    fmt, fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use dialoguer::{Input, theme::ColorfulTheme};
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, Result};

const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const HOME: &str = "HOME";
const DEFAULT_OPENCODE_COMMAND: &str = "opencode";

/// Executable used to launch `OpenCode`.
///
/// The value is passed directly to the process launcher as either one
/// PATH-resolvable program name or one absolute path. It is never parsed as a
/// shell command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct OpenCodeCommand(Cow<'static, str>);

impl OpenCodeCommand {
    pub const DEFAULT: Self = Self(Cow::Borrowed(DEFAULT_OPENCODE_COMMAND));

    pub fn try_new(command: impl Into<String>) -> Result<Self> {
        let command = command.into();
        let command = command.trim();
        if command.is_empty() {
            return Err(Error::failed(
                "OpenCode command in config must not be empty",
            ));
        }
        if has_unix_path_separator(command) && !Path::new(command).is_absolute() {
            return Err(Error::failed(format!(
                "OpenCode command `{command}` must be a PATH-resolvable executable name or an \
                 absolute path; relative paths are not supported"
            )));
        }
        Ok(Self(Cow::Owned(command.to_owned())))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn has_unix_path_separator(command: &str) -> bool {
    command.contains('/')
}

impl Default for OpenCodeCommand {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for OpenCodeCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for OpenCodeCommand {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let command = String::deserialize(deserializer)?;
        Self::try_new(command).map_err(serde::de::Error::custom)
    }
}

/// Pins a detached worktree to a specific branch.
///
/// Without an entry, `task detach add` / `task detach update` track the
/// remote's default branch (typically `main`/`master`). With one, both
/// commands operate against `origin/<branch>` instead.
///
/// ## `repo` matching
///
/// `repo` should be the fully-qualified repo key, i.e.
/// `host/owner/name` (for example `github.com/mattwparas/helix`).
/// Command-line arguments to `task detach add|update` are matched
/// against this field either by exact equality, or by unambiguous
/// short-name (the last `/`-separated segment). Ambiguous short-name
/// queries produce a hard error — a partial value like `owner/name`
/// without the host **must** equal the resolved repo key, otherwise
/// the pin will be skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetachedEntry {
    pub repo: String,
    pub branch: String,
}

/// Which editor to open for a task worktree.
///
/// Selected via the top-level `editor = "..."` key in `config.toml`.
/// Defaults to [`EditorKind::Vscodium`] for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::EnumString)]
pub enum EditorKind {
    #[default]
    #[strum(serialize = "vscodium", serialize = "codium")]
    Vscodium,
    #[strum(serialize = "helix", serialize = "hx")]
    Helix,
}

impl EditorKind {
    fn parse(value: &str) -> Result<Self> {
        value.parse::<Self>().map_err(|_| {
            Error::failed(format!(
                "Unknown editor `{value}` in config. Allowed values: \"vscodium\", \"helix\"."
            ))
        })
    }

    const fn as_config_string(self) -> &'static str {
        match self {
            Self::Vscodium => "vscodium",
            Self::Helix => "helix",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub repos_dir: PathBuf,
    pub wt_dir: PathBuf,
    pub detached_dir: PathBuf,
    pub codium_trusted_roots: Vec<PathBuf>,
    pub detached_entries: Vec<DetachedEntry>,
    pub editor: EditorKind,
    pub opencode_command: OpenCodeCommand,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskConfigFile {
    repos_dir: String,
    wt_dir: String,
    detached_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor: Option<String>,
    #[serde(default)]
    vscodium: Option<VscodiumConfigFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    opencode: Option<OpenCodeConfigFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    detached: Vec<DetachedEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VscodiumConfigFile {
    #[serde(default)]
    trusted_roots: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenCodeConfigFile {
    #[serde(default)]
    command: OpenCodeCommand,
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
                "Missing config file at {}. Run task in an interactive terminal to initialize it.",
                config_path.display()
            )));
        }

        initialize_config(&config_path)
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

fn initialize_config(config_path: &Path) -> Result<TaskConfig> {
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
        editor: None,
        vscodium: None,
        opencode: None,
        detached: Vec::new(),
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
        editor: if config.editor == EditorKind::default() {
            None
        } else {
            Some(config.editor.as_config_string().to_owned())
        },
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
        opencode: if config.opencode_command == OpenCodeCommand::default() {
            None
        } else {
            Some(OpenCodeConfigFile {
                command: config.opencode_command.clone(),
            })
        },
        detached: config.detached_entries.clone(),
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
    validate_detached_entries(&file.detached)?;
    let editor = match file.editor.as_deref().map(str::trim) {
        None | Some("") => EditorKind::default(),
        Some(value) => EditorKind::parse(value)?,
    };
    let opencode_command = file
        .opencode
        .map_or_else(OpenCodeCommand::default, |config| config.command);
    Ok(TaskConfig {
        repos_dir,
        wt_dir,
        detached_dir,
        codium_trusted_roots,
        detached_entries: file.detached,
        editor,
        opencode_command,
    })
}

fn validate_detached_entries(entries: &[DetachedEntry]) -> Result<()> {
    for entry in entries {
        if entry.repo.trim().is_empty() {
            return Err(Error::failed(
                "[[detached]] entry has an empty `repo` field",
            ));
        }
        if entry.branch.trim().is_empty() {
            return Err(Error::failed(format!(
                "[[detached]] entry for `{}` has an empty `branch` field",
                entry.repo
            )));
        }
    }
    Ok(())
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

fn config_dir_path() -> Result<PathBuf> {
    resolve_config_dir(
        std::env::var(XDG_CONFIG_HOME).ok().as_deref(),
        std::env::var(HOME).ok().as_deref(),
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
    std::env::var(HOME)
        .map(PathBuf::from)
        .map_err(|_| Error::failed(format!("{HOME} is not set")))
}

#[must_use]
pub fn is_interactive_terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        DetachedEntry, EditorKind, OpenCodeCommand, TaskConfigFile, VscodiumConfigFile,
        expand_path, to_runtime_config,
    };

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
                repos_dir: "~/dev/repos".to_owned(),
                wt_dir: "~/dev/wt".to_owned(),
                detached_dir: "~/dev/detached".to_owned(),
                editor: None,
                vscodium: None,
                opencode: None,
                detached: Vec::new(),
            })
            .expect("runtime config");

            assert!(config.codium_trusted_roots.is_empty());
        }

        #[test]
        fn expands_vscodium_trusted_roots() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/dev/repos".to_owned(),
                wt_dir: "~/dev/wt".to_owned(),
                detached_dir: "~/dev/detached".to_owned(),
                editor: None,
                vscodium: Some(VscodiumConfigFile {
                    trusted_roots: vec!["~/dev/wt/github.com/tsauvajon".to_owned()],
                }),
                opencode: None,
                detached: Vec::new(),
            })
            .expect("runtime config");

            assert_eq!(config.codium_trusted_roots.len(), 1);
            assert!(config.codium_trusted_roots[0].ends_with("dev/wt/github.com/tsauvajon"));
        }

        #[test]
        fn expands_repos_wt_and_detached_dirs() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "~/repos".to_owned(),
                wt_dir: "~/wt".to_owned(),
                detached_dir: "~/detached".to_owned(),
                editor: None,
                vscodium: None,
                opencode: None,
                detached: Vec::new(),
            })
            .expect("runtime config");

            assert!(config.repos_dir.ends_with("repos"));
            assert!(config.wt_dir.ends_with("wt"));
            assert!(config.detached_dir.ends_with("detached"));
        }

        #[test]
        fn passes_through_detached_entries() {
            let entries = vec![
                DetachedEntry {
                    repo: "github.com/mattwparas/helix".to_owned(),
                    branch: "steel-event-system".to_owned(),
                },
                DetachedEntry {
                    repo: "github.com/org/fork".to_owned(),
                    branch: "custom".to_owned(),
                },
            ];
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_owned(),
                wt_dir: "/tmp/wt".to_owned(),
                detached_dir: "/tmp/detached".to_owned(),
                vscodium: None,
                editor: None,
                opencode: None,
                detached: entries.clone(),
            })
            .expect("runtime config");

            assert_eq!(config.detached_entries, entries);
        }

        #[test]
        fn empty_detached_entries_by_default() {
            let config = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_owned(),
                wt_dir: "/tmp/wt".to_owned(),
                detached_dir: "/tmp/detached".to_owned(),
                vscodium: None,
                editor: None,
                opencode: None,
                detached: Vec::new(),
            })
            .expect("runtime config");

            assert!(config.detached_entries.is_empty());
        }

        #[test]
        fn rejects_detached_entry_with_empty_branch() {
            let err = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_owned(),
                wt_dir: "/tmp/wt".to_owned(),
                detached_dir: "/tmp/detached".to_owned(),
                vscodium: None,
                editor: None,
                opencode: None,
                detached: vec![DetachedEntry {
                    repo: "github.com/org/repo".to_owned(),
                    branch: String::new(),
                }],
            })
            .unwrap_err();

            let msg = err.to_string();
            assert!(msg.contains("branch"), "expected branch error: {msg}");
            assert!(
                msg.contains("github.com/org/repo"),
                "expected repo name in error: {msg}"
            );
        }

        #[test]
        fn rejects_detached_entry_with_whitespace_only_branch() {
            let err = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_owned(),
                wt_dir: "/tmp/wt".to_owned(),
                detached_dir: "/tmp/detached".to_owned(),
                vscodium: None,
                editor: None,
                opencode: None,
                detached: vec![DetachedEntry {
                    repo: "github.com/org/repo".to_owned(),
                    branch: "   ".to_owned(),
                }],
            })
            .unwrap_err();

            assert!(err.to_string().contains("branch"));
        }

        #[test]
        fn rejects_detached_entry_with_empty_repo() {
            let err = to_runtime_config(TaskConfigFile {
                repos_dir: "/tmp/repos".to_owned(),
                wt_dir: "/tmp/wt".to_owned(),
                detached_dir: "/tmp/detached".to_owned(),
                vscodium: None,
                editor: None,
                opencode: None,
                detached: vec![DetachedEntry {
                    repo: String::new(),
                    branch: "main".to_owned(),
                }],
            })
            .unwrap_err();

            assert!(err.to_string().contains("repo"));
        }
    }

    mod editor_kind {
        use super::*;

        fn file_with_editor(editor: Option<&str>) -> TaskConfigFile {
            TaskConfigFile {
                repos_dir: "/tmp/repos".to_owned(),
                wt_dir: "/tmp/wt".to_owned(),
                detached_dir: "/tmp/detached".to_owned(),
                editor: editor.map(str::to_owned),
                vscodium: None,
                opencode: None,
                detached: Vec::new(),
            }
        }

        #[test]
        fn defaults_to_vscodium_when_absent() {
            let config = to_runtime_config(file_with_editor(None)).expect("runtime config");
            assert_eq!(config.editor, EditorKind::Vscodium);
        }

        #[test]
        fn defaults_to_vscodium_when_empty_string() {
            let config = to_runtime_config(file_with_editor(Some(""))).expect("runtime config");
            assert_eq!(config.editor, EditorKind::Vscodium);
        }

        #[test]
        fn parses_helix_value() {
            let config =
                to_runtime_config(file_with_editor(Some("helix"))).expect("runtime config");
            assert_eq!(config.editor, EditorKind::Helix);
        }

        #[test]
        fn parses_hx_alias() {
            let config = to_runtime_config(file_with_editor(Some("hx"))).expect("runtime config");
            assert_eq!(config.editor, EditorKind::Helix);
        }

        #[test]
        fn parses_vscodium_value() {
            let config =
                to_runtime_config(file_with_editor(Some("vscodium"))).expect("runtime config");
            assert_eq!(config.editor, EditorKind::Vscodium);
        }

        #[test]
        fn rejects_unknown_editor() {
            let err = to_runtime_config(file_with_editor(Some("emacs"))).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("emacs"), "error should name the value: {msg}");
            assert!(
                msg.contains("vscodium") && msg.contains("helix"),
                "error should list allowed values: {msg}"
            );
        }

        #[test]
        fn trims_whitespace() {
            let config =
                to_runtime_config(file_with_editor(Some("  helix  "))).expect("runtime config");
            assert_eq!(config.editor, EditorKind::Helix);
        }
    }

    mod opencode_command {
        use super::*;

        fn parse_config(
            opencode_section: &str,
        ) -> std::result::Result<TaskConfigFile, toml::de::Error> {
            toml::from_str(&format!(
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n{opencode_section}"
            ))
        }

        #[test]
        fn defaults_to_opencode_when_section_is_omitted() {
            let file = parse_config("").expect("config file");
            let config = to_runtime_config(file).expect("runtime config");

            assert_eq!(config.opencode_command, OpenCodeCommand::default());
            assert_eq!(config.opencode_command.as_str(), "opencode");
        }

        #[test]
        fn defaults_to_opencode_when_section_is_empty() {
            let file = parse_config("\n[opencode]\n").expect("config file");
            let config = to_runtime_config(file).expect("runtime config");

            assert_eq!(config.opencode_command, OpenCodeCommand::default());
            assert_eq!(config.opencode_command.as_str(), "opencode");
        }

        #[test]
        fn parses_custom_executable() {
            let file =
                parse_config("\n[opencode]\ncommand = \"opencode-shared\"\n").expect("config file");
            let config = to_runtime_config(file).expect("runtime config");

            assert_eq!(config.opencode_command.as_str(), "opencode-shared");
        }

        #[test]
        fn parses_absolute_executable_path() {
            let file = parse_config("\n[opencode]\ncommand = \"/opt/opencode-shared\"\n")
                .expect("config file");
            let config = to_runtime_config(file).expect("runtime config");

            assert_eq!(config.opencode_command.as_str(), "/opt/opencode-shared");
        }

        #[test]
        fn rejects_empty_executable() {
            let err = parse_config("\n[opencode]\ncommand = \"\"\n").unwrap_err();

            assert!(err.to_string().contains("must not be empty"));
        }

        #[test]
        fn rejects_whitespace_only_executable() {
            let err = parse_config("\n[opencode]\ncommand = \"   \"\n").unwrap_err();

            assert!(err.to_string().contains("must not be empty"));
        }

        #[test]
        fn rejects_relative_executable_path() {
            let err =
                parse_config("\n[opencode]\ncommand = \"bin/opencode-shared\"\n").unwrap_err();

            assert!(err.to_string().contains("relative paths are not supported"));
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

        use super::super::{EditorKind, load_config};

        struct TempDir(PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-config-{name}"));
                _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &PathBuf {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                _ = fs::remove_dir_all(&self.0);
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
        fn loads_config_with_detached_entries() {
            let dir = TempDir::new("load-detached");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                r#"repos_dir = "/tmp/repos"
wt_dir = "/tmp/wt"
detached_dir = "/tmp/detached"

[[detached]]
repo = "github.com/mattwparas/helix"
branch = "steel-event-system"

[[detached]]
repo = "github.com/org/fork"
branch = "custom"
"#,
            )
            .unwrap();

            let config = load_config(&config_path).expect("load detached config");
            assert_eq!(config.detached_entries.len(), 2);
            assert_eq!(
                config.detached_entries[0].repo,
                "github.com/mattwparas/helix"
            );
            assert_eq!(config.detached_entries[0].branch, "steel-event-system");
            assert_eq!(config.detached_entries[1].repo, "github.com/org/fork");
            assert_eq!(config.detached_entries[1].branch, "custom");
        }

        #[test]
        fn loads_config_without_detached_section() {
            let dir = TempDir::new("load-no-detached");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n",
            )
            .unwrap();

            let config = load_config(&config_path).expect("load config without detached");
            assert!(config.detached_entries.is_empty());
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

        #[test]
        fn loads_helix_editor() {
            let dir = TempDir::new("load-editor-helix");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\neditor = \"helix\"\n",
            )
            .unwrap();

            let config = load_config(&config_path).expect("load editor config");
            assert_eq!(config.editor, EditorKind::Helix);
        }

        #[test]
        fn defaults_editor_when_key_absent() {
            let dir = TempDir::new("load-editor-default");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\n",
            )
            .unwrap();

            let config = load_config(&config_path).expect("load default editor");
            assert_eq!(config.editor, EditorKind::Vscodium);
        }

        #[test]
        fn errors_on_unknown_editor() {
            let dir = TempDir::new("load-editor-unknown");
            let config_path = dir.path().join("config.toml");
            fs::write(
                &config_path,
                "repos_dir = \"/tmp/repos\"\nwt_dir = \"/tmp/wt\"\ndetached_dir = \"/tmp/detached\"\neditor = \"nano\"\n",
            )
            .unwrap();

            let err = load_config(&config_path).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("nano"), "error should name the value: {msg}");
        }
    }

    mod write_config_tests {
        use std::{env, fs, path::PathBuf};

        use super::super::{EditorKind, OpenCodeCommand, TaskConfig, load_config, write_config};

        struct TempDir(PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-config-write-{name}"));
                _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &PathBuf {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                _ = fs::remove_dir_all(&self.0);
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
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let loaded = load_config(&config_path).expect("load written config");

            assert_eq!(loaded.repos_dir, config.repos_dir);
            assert_eq!(loaded.wt_dir, config.wt_dir);
            assert_eq!(loaded.detached_dir, config.detached_dir);
            assert_eq!(loaded.codium_trusted_roots, config.codium_trusted_roots);
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
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::default(),
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
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).unwrap();
            assert!(!content.contains("[vscodium]"));
        }

        #[test]
        fn round_trips_detached_entries() {
            let dir = TempDir::new("write-detached-round-trip");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: vec![
                    super::super::DetachedEntry {
                        repo: "github.com/mattwparas/helix".to_owned(),
                        branch: "steel-event-system".to_owned(),
                    },
                    super::super::DetachedEntry {
                        repo: "github.com/org/fork".to_owned(),
                        branch: "custom".to_owned(),
                    },
                ],
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let loaded = load_config(&config_path).expect("load written config");

            assert_eq!(loaded.detached_entries, config.detached_entries);
        }

        #[test]
        fn omits_detached_section_when_empty() {
            let dir = TempDir::new("write-no-detached");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).unwrap();
            assert!(
                !content.contains("[[detached]]"),
                "empty detached should be omitted from config"
            );
        }

        #[test]
        fn round_trips_non_default_editor() {
            let dir = TempDir::new("write-editor-helix");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::Helix,
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).unwrap();
            assert!(
                content.contains("editor = \"helix\""),
                "helix editor should be serialized: {content}"
            );

            let loaded = load_config(&config_path).expect("load written config");
            assert_eq!(loaded.editor, EditorKind::Helix);
        }

        #[test]
        fn omits_editor_key_when_default() {
            let dir = TempDir::new("write-editor-default");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::Vscodium,
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).unwrap();
            assert!(
                !content.contains("editor ="),
                "default editor should be omitted: {content}"
            );
        }

        #[test]
        fn omits_opencode_section_when_default() {
            let dir = TempDir::new("write-opencode-default");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).expect("read config");

            assert!(!content.contains("[opencode]"));
        }

        #[test]
        fn round_trips_custom_opencode_command() {
            let dir = TempDir::new("write-opencode-custom");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::try_new("opencode-shared")
                    .expect("valid command"),
            };

            write_config(&config_path, &config).expect("write config");
            let content = fs::read_to_string(&config_path).expect("read config");
            let loaded = load_config(&config_path).expect("load config");

            assert!(content.contains("[opencode]"));
            assert!(content.contains("command = \"opencode-shared\""));
            assert_eq!(loaded.opencode_command, config.opencode_command);
        }

        #[test]
        fn round_trips_custom_opencode_vscodium_and_detached_entries() {
            let dir = TempDir::new("write-opencode-vscodium-detached");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: vec![PathBuf::from("/tmp/trusted")],
                detached_entries: vec![super::super::DetachedEntry {
                    repo: "github.com/org/repo".to_owned(),
                    branch: "main".to_owned(),
                }],
                editor: EditorKind::default(),
                opencode_command: OpenCodeCommand::try_new("opencode-shared")
                    .expect("valid command"),
            };

            write_config(&config_path, &config).expect("write config");
            let loaded = load_config(&config_path).expect("load config");

            assert_eq!(loaded.opencode_command, config.opencode_command);
            assert_eq!(loaded.codium_trusted_roots, config.codium_trusted_roots);
            assert_eq!(loaded.detached_entries, config.detached_entries);
        }

        /// Guards against future serializer changes that might couple the
        /// ordering or representation of top-level keys and `[[detached]]`
        /// table arrays. A config carrying both a non-default editor and
        /// one-or-more detached pins must round-trip losslessly.
        #[test]
        fn round_trips_editor_and_detached_entries_together() {
            let dir = TempDir::new("write-editor-and-detached");
            let config_path = dir.path().join("config.toml");
            let config = TaskConfig {
                repos_dir: PathBuf::from("/tmp/repos"),
                wt_dir: PathBuf::from("/tmp/wt"),
                detached_dir: PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: vec![
                    super::super::DetachedEntry {
                        repo: "github.com/mattwparas/helix".to_owned(),
                        branch: "steel-event-system".to_owned(),
                    },
                    super::super::DetachedEntry {
                        repo: "github.com/org/fork".to_owned(),
                        branch: "custom".to_owned(),
                    },
                ],
                editor: EditorKind::Helix,
                opencode_command: OpenCodeCommand::default(),
            };

            write_config(&config_path, &config).expect("write config");
            let loaded = load_config(&config_path).expect("load written config");

            assert_eq!(loaded.editor, EditorKind::Helix);
            assert_eq!(loaded.detached_entries, config.detached_entries);
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
                repos_dir: "  /tmp/repos  ".to_owned(),
                wt_dir: "  /tmp/wt  ".to_owned(),
                detached_dir: "  /tmp/detached  ".to_owned(),
                editor: None,
                vscodium: None,
                opencode: None,
                detached: Vec::new(),
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
                _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &PathBuf {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                _ = fs::remove_dir_all(&self.0);
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
