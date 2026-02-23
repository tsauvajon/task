use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::runtime::config::TaskConfig;
use crate::runtime::paths::WorkspacePaths;
use crate::runtime::process::ProcessRunner;
use crate::runtime::task_rows::TaskRow;
use crate::runtime::tasks::TaskResolver;

#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
    layout: WorkspacePaths,
    process: ProcessRunner,
    tasks: TaskResolver,
}

impl RuntimeEnvironment {
    pub fn new() -> Result<Self, String> {
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

    pub fn repos_dir(&self) -> &Path {
        self.layout.repos_dir()
    }

    pub fn wt_dir(&self) -> &Path {
        self.layout.wt_dir()
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

    pub fn ensure_layout(&self) -> Result<(), String> {
        self.tasks.ensure_layout()
    }

    pub fn available_repo_keys(&self) -> Result<Vec<String>, String> {
        self.tasks.available_repo_keys()
    }

    pub fn resolve_repo_key_input(&self, repo_arg: &str) -> Result<String, String> {
        self.tasks.resolve_repo_key_input(repo_arg)
    }

    pub fn clone_bare_repo(&self, repo_url: &str, repo_key: &str) -> Result<(), String> {
        self.tasks.clone_bare_repo(repo_url, repo_key)
    }

    pub fn ensure_repo_available(&self, repo_arg: &str, repo_key: &str) -> Result<(), String> {
        self.tasks.ensure_repo_available(repo_arg, repo_key)
    }

    pub fn launch_workspace(
        &self,
        repo_key: &str,
        branch: &str,
        path: &Path,
    ) -> Result<(), String> {
        self.tasks.launch_workspace(repo_key, branch, path)
    }

    pub fn repo_task_rows(
        &self,
        repo_key: &str,
        gitdir: &Path,
        open_sessions: &HashSet<String>,
    ) -> Result<Vec<TaskRow>, String> {
        self.tasks.repo_task_rows(repo_key, gitdir, open_sessions)
    }

    pub fn resolve_worktree_path(&self, repo_key: &str, branch: &str) -> PathBuf {
        self.tasks.resolve_worktree_path(repo_key, branch)
    }

    pub fn resolve_task_from_args(
        &self,
        args: &[String],
        usage: &str,
    ) -> Result<(String, String), String> {
        self.tasks.resolve_task_from_args(args, usage)
    }

    pub fn resolve_task_from_query(&self, query: &str) -> Result<(String, String), String> {
        self.tasks.resolve_task_from_query(query)
    }

    pub fn print_task_rows_table(&self, rows: &[TaskRow]) {
        self.tasks.print_task_rows_table(rows);
    }

    pub fn tmux_sessions(&self) -> HashSet<String> {
        self.tasks.tmux_sessions()
    }

    pub fn current_task_info(&self) -> Result<(String, String, PathBuf), String> {
        self.tasks.current_task_info()
    }

    pub fn current_repo_key(&self) -> Option<String> {
        self.tasks.current_repo_key()
    }

    pub fn resolve_repo_branch_inputs(
        &self,
        repo_arg: Option<&str>,
        branch_arg: Option<&str>,
    ) -> Result<(String, String), String> {
        self.tasks.resolve_repo_branch_inputs(repo_arg, branch_arg)
    }

    pub fn resolve_repo_input(&self, repo_arg: Option<&str>) -> Result<String, String> {
        self.tasks.resolve_repo_input(repo_arg)
    }

    pub fn command_exists(&self, name: &str) -> bool {
        self.process.command_exists(name)
    }

    pub fn run_capture(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<String, String> {
        self.process.run_capture(program, args, cwd)
    }

    pub fn run_status(
        &self,
        program: impl AsRef<OsStr>,
        args: &[&str],
        cwd: Option<&Path>,
    ) -> Result<(), String> {
        self.process.run_status(program, args, cwd)
    }

    pub fn log(&self, message: &str) {
        self.process.log(message);
    }

    pub fn warn(&self, message: &str) {
        self.process.warn(message);
    }
}

impl Default for RuntimeEnvironment {
    fn default() -> Self {
        Self::new().expect("runtime environment")
    }
}
