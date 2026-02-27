use std::path::Path;

use crate::{
    error::Result,
    runtime::{config::TaskConfig, paths::WorkspacePaths, tasks::TaskResolver},
};

#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
    layout: WorkspacePaths,
    tasks: TaskResolver,
}

impl RuntimeEnvironment {
    pub fn new() -> Result<Self> {
        let config = TaskConfig::load_or_initialize()?;
        Ok(Self::from_config(config))
    }

    fn from_config(config: TaskConfig) -> Self {
        let layout = WorkspacePaths::new(config.repos_dir, config.wt_dir);
        let tasks = TaskResolver::new(layout.clone(), config.codium_trusted_roots);
        Self { layout, tasks }
    }

    pub fn try_new_if_configured() -> Result<Option<Self>> {
        let Some(config) = TaskConfig::load_if_present()? else {
            return Ok(None);
        };
        Ok(Some(Self::from_paths(config.repos_dir, config.wt_dir)))
    }

    pub fn from_paths(repos_dir: impl AsRef<Path>, wt_dir: impl AsRef<Path>) -> Self {
        let layout = WorkspacePaths::new(repos_dir, wt_dir);
        let tasks = TaskResolver::new(layout.clone(), Vec::new());
        Self { layout, tasks }
    }

    pub fn layout(&self) -> &WorkspacePaths {
        &self.layout
    }

    pub fn tasks(&self) -> &TaskResolver {
        &self.tasks
    }
}

impl Default for RuntimeEnvironment {
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
            let env = RuntimeEnvironment::from_paths("/tmp/repos", "/tmp/wt");
            assert_eq!(env.layout().repos_dir(), std::path::Path::new("/tmp/repos"));
            assert_eq!(env.layout().wt_dir(), std::path::Path::new("/tmp/wt"));
        }

        #[test]
        fn exposes_tasks() {
            let env = RuntimeEnvironment::from_paths("/tmp/repos", "/tmp/wt");
            // TaskResolver is accessible – we exercise the accessor without
            // calling any I/O methods.
            let tasks = env.tasks();
            // layout() on the resolver should agree with the env layout.
            assert_eq!(tasks.layout().repos_dir(), env.layout().repos_dir());
        }

        #[test]
        fn accepts_pathbuf_input() {
            let repos = std::path::PathBuf::from("/srv/repos");
            let wt = std::path::PathBuf::from("/srv/wt");
            let env = RuntimeEnvironment::from_paths(&repos, &wt);
            assert_eq!(env.layout().repos_dir(), repos.as_path());
            assert_eq!(env.layout().wt_dir(), wt.as_path());
        }
    }
}
