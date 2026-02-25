use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use super::run::{run_git_capture, run_git_status};
use crate::error::Result;

/// A bare git repository directory (`.git` or a bare repo path).
///
/// Wraps a `&Path` and provides methods that pre-pend `--git-dir <path>`
/// to every git invocation, eliminating the `to_string_lossy()` boilerplate
/// that otherwise repeats across `refs`, `worktrees`, and `repo`.
#[derive(Debug, Clone, Copy)]
pub struct GitDir<'a> {
    path: &'a Path,
}

impl<'a> GitDir<'a> {
    pub fn new(path: &'a Path) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        self.path
    }

    /// Run a git command with `--git-dir <self>` prepended, capturing stdout.
    pub fn capture(&self, args: &[&str]) -> Result<String> {
        let path_str = self.path_str();
        let mut full_args = vec!["--git-dir", path_str.as_ref()];
        full_args.extend_from_slice(args);
        run_git_capture(&full_args, None)
    }

    /// Run a git command with `--git-dir <self>` prepended, checking exit status.
    pub fn status(&self, args: &[&str]) -> Result<()> {
        let path_str = self.path_str();
        let mut full_args = vec!["--git-dir", path_str.as_ref()];
        full_args.extend_from_slice(args);
        run_git_status(&full_args, None)
    }

    /// Run a git command with `--git-dir <self>` prepended, checking exit status,
    /// and setting the working directory.
    pub fn status_in(&self, args: &[&str], cwd: &Path) -> Result<()> {
        let path_str = self.path_str();
        let mut full_args = vec!["--git-dir", path_str.as_ref()];
        full_args.extend_from_slice(args);
        run_git_status(&full_args, Some(cwd))
    }

    fn path_str(&self) -> Cow<'_, str> {
        self.path.to_string_lossy()
    }
}

impl<'a> From<&'a Path> for GitDir<'a> {
    fn from(path: &'a Path) -> Self {
        Self::new(path)
    }
}

impl<'a> From<&'a PathBuf> for GitDir<'a> {
    fn from(path: &'a PathBuf) -> Self {
        Self::new(path.as_path())
    }
}
