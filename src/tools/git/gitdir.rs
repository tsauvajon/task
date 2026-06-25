use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use super::run::{capture, status};
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
    #[must_use]
    pub const fn new(path: &'a Path) -> Self {
        Self { path }
    }

    #[must_use]
    pub const fn path(&self) -> &Path {
        self.path
    }

    /// Run a git command with `--git-dir <self>` prepended, capturing stdout.
    pub fn capture(&self, args: &[&str]) -> Result<String> {
        self.with_git_dir_args(args, |full_args| capture(full_args, None))
    }

    /// Run a git command with `--git-dir <self>` prepended, checking exit status.
    pub fn status(&self, args: &[&str]) -> Result<()> {
        self.with_git_dir_args(args, |full_args| status(full_args, None))
    }

    /// Run a git command with `--git-dir <self>` prepended, checking exit status,
    /// and setting the working directory.
    pub fn status_in(&self, args: &[&str], cwd: &Path) -> Result<()> {
        self.with_git_dir_args(args, |full_args| status(full_args, Some(cwd)))
    }

    fn with_git_dir_args<T>(
        &self,
        args: &[&str],
        run: impl FnOnce(&[&str]) -> Result<T>,
    ) -> Result<T> {
        let path_str = self.path_str();
        let mut full_args = vec!["--git-dir", path_str.as_ref()];
        full_args.extend_from_slice(args);
        run(&full_args)
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

#[cfg(test)]
mod tests {
    use std::{env, process::Command};

    use super::GitDir;

    /// Create a temporary bare git repository and return its path.
    /// The directory is named with a random-ish suffix so parallel tests
    /// don't collide.
    fn make_bare_repo(name: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("task-rs-gitdir-{name}.git"));
        _ = std::fs::remove_dir_all(&dir);
        let status = Command::new("git")
            .args(["init", "--bare", dir.to_str().expect("valid utf-8 path")])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", env::temp_dir())
            .status()
            .expect("git must be available");
        assert!(status.success(), "git init --bare failed");
        dir
    }

    #[test]
    fn path_returns_the_wrapped_path() {
        let dir = env::temp_dir().join("task-rs-gitdir-path-check.git");
        let gd = GitDir::new(&dir);
        assert_eq!(gd.path(), dir.as_path());
    }

    #[test]
    fn from_path_ref_and_from_pathbuf_agree() {
        let dir = env::temp_dir().join("task-rs-gitdir-from-impls.git");
        let gd_from_ref = GitDir::from(dir.as_path());
        let gd_from_buf = GitDir::from(&dir);
        assert_eq!(gd_from_ref.path(), gd_from_buf.path());
    }

    #[test]
    fn capture_returns_git_dir_path() {
        let dir = make_bare_repo("capture-test");
        let gd = GitDir::new(&dir);
        let output = gd
            .capture(&["rev-parse", "--git-dir"])
            .expect("git rev-parse --git-dir should succeed");
        // git outputs "." when queried from inside the bare repo directory.
        assert!(!output.trim().is_empty(), "output should not be empty");
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_succeeds_on_valid_bare_repo() {
        let dir = make_bare_repo("status-test");
        let gd = GitDir::new(&dir);
        // `git rev-parse --git-dir` exits 0 on a valid repo.
        gd.status(&["rev-parse", "--git-dir"])
            .expect("status should succeed on a valid bare repo");
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_errors_on_nonexistent_path() {
        let dir = env::temp_dir().join("task-rs-gitdir-nonexistent-12345.git");
        _ = std::fs::remove_dir_all(&dir);
        let gd = GitDir::new(&dir);
        let result = gd.status(&["rev-parse", "--git-dir"]);
        assert!(result.is_err(), "should fail for a nonexistent git dir");
    }

    #[test]
    fn status_in_succeeds_on_valid_bare_repo_with_cwd() {
        let dir = make_bare_repo("status-in-test");
        let gd = GitDir::new(&dir);
        let parent = env::temp_dir();
        gd.status_in(&["rev-parse", "--git-dir"], &parent)
            .expect("status_in should succeed on a valid bare repo");
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_output_is_non_empty_for_bare_repo() {
        let dir = make_bare_repo("capture-nonempty");
        let gd = GitDir::new(&dir);
        let out = gd
            .capture(&["rev-parse", "--git-dir"])
            .expect("should succeed");
        assert!(
            !out.trim().is_empty(),
            "git rev-parse --git-dir should produce output"
        );
        _ = std::fs::remove_dir_all(&dir);
    }
}
