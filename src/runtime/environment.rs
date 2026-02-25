use std::path::Path;

use crate::{
    error::Result,
    runtime::{
        config::TaskConfig, paths::WorkspacePaths, process::ProcessRunner, tasks::TaskResolver,
    },
};

#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
    layout: WorkspacePaths,
    process: ProcessRunner,
    tasks: TaskResolver,
}

impl RuntimeEnvironment {
    pub fn new() -> Result<Self> {
        let config = TaskConfig::load_or_initialize()?;
        Ok(Self::from_config(config))
    }

    fn from_config(config: TaskConfig) -> Self {
        let layout = WorkspacePaths::new(config.repos_dir, config.wt_dir);
        let process = ProcessRunner;
        let tasks = TaskResolver::new(layout.clone(), process, config.codium_trusted_roots);
        Self {
            layout,
            process,
            tasks,
        }
    }

    pub fn try_new_if_configured() -> Result<Option<Self>> {
        let Some(config) = TaskConfig::load_if_present()? else {
            return Ok(None);
        };
        Ok(Some(Self::from_paths(config.repos_dir, config.wt_dir)))
    }

    pub fn from_paths(repos_dir: impl AsRef<Path>, wt_dir: impl AsRef<Path>) -> Self {
        let layout = WorkspacePaths::new(repos_dir, wt_dir);
        let process = ProcessRunner;
        let tasks = TaskResolver::new(layout.clone(), process, Vec::new());
        Self {
            layout,
            process,
            tasks,
        }
    }

    pub fn layout(&self) -> &WorkspacePaths {
        &self.layout
    }

    pub fn process(&self) -> ProcessRunner {
        self.process
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
