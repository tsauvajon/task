use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::git::parsing::TaskRow;
use crate::runtime::paths::WorkspacePaths;
use crate::runtime::process::ProcessRunner;
use crate::runtime::tasks::TaskResolver;

#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
    dev_root: PathBuf,
    layout: WorkspacePaths,
    process: ProcessRunner,
    tasks: TaskResolver,
}

impl RuntimeEnvironment {
    pub fn new() -> Self {
        Self::from_dev_root(Self::default_dev_root())
    }

    pub fn from_dev_root(dev_root: impl AsRef<Path>) -> Self {
        let dev_root = dev_root.as_ref().to_path_buf();
        let layout = WorkspacePaths::new(&dev_root);
        let process = ProcessRunner;
        let tasks = TaskResolver::new(layout.clone(), process);

        Self {
            dev_root,
            layout,
            process,
            tasks,
        }
    }

    pub fn default_dev_root() -> PathBuf {
        if let Ok(dev_root) = std::env::var("DEV_ROOT") {
            return PathBuf::from(dev_root);
        }

        let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
        PathBuf::from(home).join("dev")
    }

    pub fn dev_root(&self) -> &Path {
        &self.dev_root
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

    pub fn tmux_has_session(&self, session: &str) -> bool {
        self.tasks.tmux_has_session(session)
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
        Self::new()
    }
}
