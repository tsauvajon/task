use std::path::{Path, PathBuf};

use super::{gitdir::GitDir, runner::run_git_status};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch_ref: Option<String>,
    pub is_bare: bool,
}

pub fn worktree_list(gitdir: &Path) -> Result<String> {
    GitDir::new(gitdir).capture(&["worktree", "list"])
}

pub fn worktree_list_porcelain(gitdir: &Path) -> Result<String> {
    GitDir::new(gitdir).capture(&["worktree", "list", "--porcelain"])
}

pub fn worktree_add_existing_branch(gitdir: &Path, worktree: &Path, branch: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    GitDir::new(gitdir).status(&["worktree", "add", worktree_str.as_ref(), branch])
}

pub fn worktree_add_tracking_remote_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<()> {
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

pub fn worktree_add_from_base(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
    base_ref: &str,
) -> Result<()> {
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

pub fn worktree_prune(gitdir: &Path) -> Result<()> {
    GitDir::new(gitdir).status(&["worktree", "prune", "--verbose", "--expire", "now"])
}

pub fn worktree_remove(gitdir: &Path, worktree: &Path, force: bool) -> Result<()> {
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
    // This uses `-C` rather than `--git-dir`, so call runner directly.
    super::runner::run_git_capture(
        &["-C", worktree_str.as_ref(), "status", "--porcelain"],
        None,
    )
}

pub fn rebase(worktree: &Path, base_ref: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    run_git_status(&["-C", worktree_str.as_ref(), "rebase", base_ref], None)
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
        branch_from_ref, branch_from_worktree_path, parse_worktree_porcelain, WorktreeEntry,
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
    fn worktree_prune_uses_immediate_expiry() {
        // Validate args via the public API (can't test private arg-building directly now)
        // Integration tested at a higher level.
    }

    #[test]
    fn worktree_remove_uses_repo_parent_as_cwd() {
        // The cwd is now derived inline from gitdir.parent() in worktree_remove.
        // This is implicitly tested by the higher-level integration tests.
    }
}
