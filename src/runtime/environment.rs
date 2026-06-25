use std::path::Path;

use crate::{
    error::Result,
    runtime::{
        config::{DetachedEntry, EditorKind, TaskConfig, is_interactive_terminal},
        paths::WorkspacePaths,
        tasks::TaskResolver,
    },
};

#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
    layout: WorkspacePaths,
    tasks: TaskResolver,
    detached_entries: Vec<DetachedEntry>,
}

impl RuntimeEnvironment {
    pub fn new() -> Result<Self> {
        let config = TaskConfig::load_or_initialize()?;
        Ok(Self::from_config(config))
    }

    fn from_config(config: TaskConfig) -> Self {
        let layout = WorkspacePaths::new(config.repos_dir, config.wt_dir, config.detached_dir);
        let tasks = TaskResolver::new(
            layout.clone(),
            config.codium_trusted_roots,
            config.editor,
            is_interactive_terminal(),
        );
        Self {
            layout,
            tasks,
            detached_entries: config.detached_entries,
        }
    }

    pub fn try_new_if_configured() -> Result<Option<Self>> {
        let Some(config) = TaskConfig::load_if_present()? else {
            return Ok(None);
        };
        Ok(Some(Self::from_config(config)))
    }

    #[must_use]
    pub fn from_paths(
        repos_dir: impl AsRef<Path>,
        wt_dir: impl AsRef<Path>,
        detached_dir: impl AsRef<Path>,
    ) -> Self {
        let layout = WorkspacePaths::new(repos_dir, wt_dir, detached_dir);
        let tasks = TaskResolver::new(
            layout.clone(),
            Vec::new(),
            EditorKind::default(),
            is_interactive_terminal(),
        );
        Self {
            layout,
            tasks,
            detached_entries: Vec::new(),
        }
    }

    /// Attach detached entries to an environment (test-only helper).
    #[cfg(test)]
    #[must_use]
    pub fn with_detached_entries(mut self, entries: Vec<DetachedEntry>) -> Self {
        self.detached_entries = entries;
        self
    }

    #[must_use]
    pub const fn layout(&self) -> &WorkspacePaths {
        &self.layout
    }

    #[must_use]
    pub const fn tasks(&self) -> &TaskResolver {
        &self.tasks
    }

    #[must_use]
    pub fn detached_entries(&self) -> &[DetachedEntry] {
        &self.detached_entries
    }
}

impl Default for RuntimeEnvironment {
    #[expect(
        clippy::expect_used,
        reason = "Default cannot return initialization errors"
    )]
    fn default() -> Self {
        Self::new().expect("runtime environment")
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeEnvironment;

    mod from_paths {
        use super::*;

        #[test]
        fn exposes_layout() {
            let env = RuntimeEnvironment::from_paths("/tmp/repos", "/tmp/wt", "/tmp/detached");
            assert_eq!(env.layout().repos_dir(), std::path::Path::new("/tmp/repos"));
            assert_eq!(env.layout().wt_dir(), std::path::Path::new("/tmp/wt"));
            assert_eq!(
                env.layout().detached_dir(),
                std::path::Path::new("/tmp/detached")
            );
        }

        #[test]
        fn exposes_tasks() {
            let env = RuntimeEnvironment::from_paths("/tmp/repos", "/tmp/wt", "/tmp/detached");
            let tasks = env.tasks();
            assert_eq!(tasks.layout().repos_dir(), env.layout().repos_dir());
        }

        #[test]
        fn from_paths_defaults_editor_to_vscodium() {
            // `from_paths` is used by UI tests and by integration callers
            // that don't load a config file. If its default editor drifts,
            // code paths like `TaskResolver::open` that forward
            // `context.tasks().editor()` into `open_session` would break.
            use crate::runtime::config::EditorKind;
            let env = RuntimeEnvironment::from_paths("/tmp/repos", "/tmp/wt", "/tmp/detached");
            assert_eq!(env.tasks().editor(), EditorKind::default());
            assert_eq!(env.tasks().editor(), EditorKind::Vscodium);
        }

        #[test]
        fn accepts_pathbuf_input() {
            let repos = std::path::PathBuf::from("/srv/repos");
            let wt = std::path::PathBuf::from("/srv/wt");
            let detached = std::path::PathBuf::from("/srv/detached");
            let env = RuntimeEnvironment::from_paths(&repos, &wt, &detached);
            assert_eq!(env.layout().repos_dir(), repos.as_path());
            assert_eq!(env.layout().wt_dir(), wt.as_path());
            assert_eq!(env.layout().detached_dir(), detached.as_path());
        }
    }

    mod from_config {
        use super::*;
        use crate::runtime::config::{EditorKind, TaskConfig};

        #[test]
        fn preserves_codium_trusted_roots() {
            let config = TaskConfig {
                repos_dir: std::path::PathBuf::from("/tmp/repos"),
                wt_dir: std::path::PathBuf::from("/tmp/wt"),
                detached_dir: std::path::PathBuf::from("/tmp/detached"),
                codium_trusted_roots: vec![
                    std::path::PathBuf::from("/tmp/wt/github.com/me"),
                    std::path::PathBuf::from("/tmp/wt/github.com/team"),
                ],
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
            };
            let env = RuntimeEnvironment::from_config(config);
            assert_eq!(env.tasks().codium_trusted_roots().len(), 2);
            assert_eq!(
                env.tasks().codium_trusted_roots()[0],
                std::path::Path::new("/tmp/wt/github.com/me")
            );
            assert_eq!(
                env.tasks().codium_trusted_roots()[1],
                std::path::Path::new("/tmp/wt/github.com/team")
            );
        }

        #[test]
        fn layout_and_tasks_share_consistent_paths() {
            let config = TaskConfig {
                repos_dir: std::path::PathBuf::from("/tmp/repos"),
                wt_dir: std::path::PathBuf::from("/tmp/wt"),
                detached_dir: std::path::PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::default(),
            };
            let env = RuntimeEnvironment::from_config(config);
            assert_eq!(env.layout().repos_dir(), env.tasks().layout().repos_dir());
            assert_eq!(env.layout().wt_dir(), env.tasks().layout().wt_dir());
            assert_eq!(
                env.layout().detached_dir(),
                env.tasks().layout().detached_dir()
            );
        }

        #[test]
        fn propagates_helix_editor_kind_to_resolver() {
            let config = TaskConfig {
                repos_dir: std::path::PathBuf::from("/tmp/repos"),
                wt_dir: std::path::PathBuf::from("/tmp/wt"),
                detached_dir: std::path::PathBuf::from("/tmp/detached"),
                codium_trusted_roots: Vec::new(),
                detached_entries: Vec::new(),
                editor: EditorKind::Helix,
            };
            let env = RuntimeEnvironment::from_config(config);
            assert_eq!(env.tasks().editor(), EditorKind::Helix);
        }
    }
}
