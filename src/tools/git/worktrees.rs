use std::path::{Path, PathBuf};

use super::{
    gitdir::GitDir,
    run::{capture, status},
};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch_ref: Option<String>,
    pub is_bare: bool,
}

pub fn list(gitdir: &Path) -> Result<String> {
    GitDir::new(gitdir).capture(&["worktree", "list"])
}

pub fn list_porcelain(gitdir: &Path) -> Result<String> {
    GitDir::new(gitdir).capture(&["worktree", "list", "--porcelain"])
}

pub fn add_existing_branch(gitdir: &Path, worktree: &Path, branch: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    GitDir::new(gitdir).status(&["worktree", "add", worktree_str.as_ref(), branch])
}

pub fn add_tracking_remote_branch(gitdir: &Path, worktree: &Path, branch: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    let remote = format!("origin/{branch}");
    GitDir::new(gitdir).status(&[
        "worktree",
        "add",
        "--track",
        "-b",
        branch,
        worktree_str.as_ref(),
        &remote,
    ])
}

pub fn add_from_base(gitdir: &Path, worktree: &Path, branch: &str, base_ref: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    GitDir::new(gitdir).status(&[
        "worktree",
        "add",
        "-b",
        branch,
        worktree_str.as_ref(),
        base_ref,
    ])
}

pub fn prune(gitdir: &Path) -> Result<()> {
    GitDir::new(gitdir).status(&["worktree", "prune", "--verbose", "--expire", "now"])
}

pub fn remove(gitdir: &Path, worktree: &Path, force: bool) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_str.as_ref());
    let cwd = gitdir.parent().unwrap_or(gitdir);
    GitDir::new(gitdir).status_in(&args, cwd)
}

pub fn status_porcelain(worktree: &Path) -> Result<String> {
    let worktree_str = worktree.to_string_lossy();
    // This uses `-C` rather than `--git-dir`, so call run directly.
    capture(
        &["-C", worktree_str.as_ref(), "status", "--porcelain"],
        None,
    )
}

pub fn rebase(worktree: &Path, base_ref: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    status(&["-C", worktree_str.as_ref(), "rebase", base_ref], None)
}

pub fn parse_worktree_porcelain(text: &str) -> Vec<WorktreeEntry> {
    #[derive(Default)]
    struct Builder {
        path: Option<PathBuf>,
        branch_ref: Option<String>,
        is_bare: bool,
    }

    impl Builder {
        fn flush(&mut self) -> Option<WorktreeEntry> {
            let path = self.path.take()?;
            Some(WorktreeEntry {
                path,
                branch_ref: self.branch_ref.take(),
                is_bare: std::mem::replace(&mut self.is_bare, false),
            })
        }
    }

    let mut entries = Vec::new();
    let mut builder = Builder::default();

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(entry) = builder.flush() {
                entries.push(entry);
            }
            builder.path = Some(PathBuf::from(path));
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            builder.branch_ref = Some(branch_ref.to_string());
        } else if line == "bare" {
            builder.is_bare = true;
        } else if line.is_empty()
            && let Some(entry) = builder.flush()
        {
            entries.push(entry);
        }
    }

    if let Some(entry) = builder.flush() {
        entries.push(entry);
    }

    entries
}

pub fn branch_from_worktree_path(
    wt_dir: &Path,
    repo_key: &str,
    worktree_path: &Path,
) -> Option<String> {
    let relative = worktree_path.strip_prefix(wt_dir.join(repo_key)).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(relative.to_string_lossy().into_owned())
}

pub fn branch_from_ref(branch_ref: Option<&str>) -> Option<String> {
    let branch_ref = branch_ref?;
    Some(
        branch_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(branch_ref)
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        WorktreeEntry, branch_from_ref, branch_from_worktree_path, parse_worktree_porcelain,
    };

    #[test]
    fn parse_worktree_porcelain_collects_entries() {
        let text = "worktree /tmp/dev/repos/github.com/tsauvajon/task.git\n\
bare\n\
\n\
worktree /tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust\n\
HEAD 0123456789abcdef\n\
branch refs/heads/rewrite-in-rust\n\n";

        let entries = parse_worktree_porcelain(text);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_bare);
        assert_eq!(
            entries[1],
            WorktreeEntry {
                path: "/tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                branch_ref: Some("refs/heads/rewrite-in-rust".to_string()),
                is_bare: false,
            }
        );
    }

    #[test]
    fn branch_from_worktree_path_supports_nested_branch_names() {
        let branch = branch_from_worktree_path(
            Path::new("/tmp/custom/wt"),
            "github.com/tsauvajon/task",
            Path::new("/tmp/custom/wt/github.com/tsauvajon/task/feat/rewrite/rust"),
        );
        assert_eq!(branch, Some("feat/rewrite/rust".to_string()));
    }

    #[test]
    fn branch_from_ref_strips_prefix() {
        assert_eq!(
            branch_from_ref(Some("refs/heads/rewrite-in-rust")),
            Some("rewrite-in-rust".to_string())
        );
    }

    #[test]
    fn branch_from_worktree_path_returns_none_when_path_equals_repo_root() {
        // The path IS the repo root (no trailing component) → None
        let branch = branch_from_worktree_path(
            Path::new("/tmp/wt"),
            "github.com/tsauvajon/task",
            Path::new("/tmp/wt/github.com/tsauvajon/task"),
        );
        assert_eq!(branch, None);
    }

    #[test]
    fn branch_from_worktree_path_returns_none_for_unrelated_path() {
        // Path that does not start with wt_dir/repo_key → None
        let branch = branch_from_worktree_path(
            Path::new("/tmp/wt"),
            "github.com/tsauvajon/task",
            Path::new("/other/path/feat/something"),
        );
        assert_eq!(branch, None);
    }

    #[test]
    fn branch_from_ref_returns_none_for_none_input() {
        assert_eq!(branch_from_ref(None), None);
    }

    #[test]
    fn branch_from_ref_returns_raw_ref_when_no_prefix() {
        // A ref that does NOT start with "refs/heads/" is returned as-is.
        assert_eq!(branch_from_ref(Some("HEAD")), Some("HEAD".to_string()));
    }

    #[test]
    fn parse_worktree_porcelain_handles_missing_trailing_blank_line() {
        // If the porcelain output ends without a trailing blank line the last
        // entry should still be flushed by the end-of-input flush path.
        let text = "worktree /tmp/dev/wt/github.com/tsauvajon/task/bump\n\
branch refs/heads/bump";

        let entries = parse_worktree_porcelain(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            WorktreeEntry {
                path: "/tmp/dev/wt/github.com/tsauvajon/task/bump".into(),
                branch_ref: Some("refs/heads/bump".to_string()),
                is_bare: false,
            }
        );
    }

    #[test]
    fn parse_worktree_porcelain_skips_bare_entries_from_non_bare_list() {
        let text = "worktree /tmp/dev/repos/task.git\n\
bare\n\
\n\
worktree /tmp/dev/wt/task/main\n\
branch refs/heads/main\n\
\n";
        let entries = parse_worktree_porcelain(text);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_bare);
        assert!(!entries[1].is_bare);
    }
}
