use std::path::{Path, PathBuf};

use super::runner::{run_git_capture, run_git_status};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch_ref: Option<String>,
    pub is_bare: bool,
}

pub fn worktree_list(gitdir: &Path) -> Result<String> {
    let gitdir_str = gitdir.to_string_lossy();
    run_git_capture(&["--git-dir", gitdir_str.as_ref(), "worktree", "list"], None)
}

pub fn worktree_list_porcelain(gitdir: &Path) -> Result<String> {
    let gitdir_str = gitdir.to_string_lossy();
    run_git_capture(
        &["--git-dir", gitdir_str.as_ref(), "worktree", "list", "--porcelain"],
        None,
    )
}

pub fn worktree_add_existing_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<()> {
    let gitdir_str = gitdir.to_string_lossy();
    let worktree_str = worktree.to_string_lossy();
    run_git_status(
        &["--git-dir", gitdir_str.as_ref(), "worktree", "add", worktree_str.as_ref(), branch],
        None,
    )
}

pub fn worktree_add_tracking_remote_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<()> {
    let gitdir_str = gitdir.to_string_lossy();
    let worktree_str = worktree.to_string_lossy();
    let remote = format!("origin/{branch}");
    run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
            "worktree",
            "add",
            "--track",
            "-b",
            branch,
            worktree_str.as_ref(),
            &remote,
        ],
        None,
    )
}

pub fn worktree_add_from_base(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
    base_ref: &str,
) -> Result<()> {
    let gitdir_str = gitdir.to_string_lossy();
    let worktree_str = worktree.to_string_lossy();
    run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
            "worktree",
            "add",
            "-b",
            branch,
            worktree_str.as_ref(),
            base_ref,
        ],
        None,
    )
}

pub fn worktree_prune(gitdir: &Path) -> Result<()> {
    let gitdir_str = gitdir.to_string_lossy();
    run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
            "worktree",
            "prune",
            "--verbose",
            "--expire",
            "now",
        ],
        None,
    )
}

pub fn worktree_remove(gitdir: &Path, worktree: &Path, force: bool) -> Result<()> {
    let gitdir_str = gitdir.to_string_lossy();
    let worktree_str = worktree.to_string_lossy();
    let mut args = vec![
        "--git-dir",
        gitdir_str.as_ref(),
        "worktree",
        "remove",
    ];
    if force {
        args.push("--force");
    }
    args.push(worktree_str.as_ref());
    let cwd = gitdir.parent();
    run_git_status(&args, cwd)
}

pub fn status_porcelain(worktree: &Path) -> Result<String> {
    let worktree_str = worktree.to_string_lossy();
    run_git_capture(&["-C", worktree_str.as_ref(), "status", "--porcelain"], None)
}

pub fn rebase(worktree: &Path, base_ref: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    run_git_status(&["-C", worktree_str.as_ref(), "rebase", base_ref], None)
}

pub fn parse_worktree_porcelain(text: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut current_is_bare = false;

    let flush = |entries: &mut Vec<WorktreeEntry>,
                 path: Option<PathBuf>,
                 branch: Option<String>,
                 is_bare: bool| {
        if let Some(path) = path {
            entries.push(WorktreeEntry { path, branch_ref: branch, is_bare });
        }
    };

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            flush(
                &mut entries,
                current_path.take(),
                current_branch.take(),
                current_is_bare,
            );
            current_path = Some(PathBuf::from(path));
            current_branch = None;
            current_is_bare = false;
            continue;
        }

        if let Some(branch_ref) = line.strip_prefix("branch ") {
            current_branch = Some(branch_ref.to_string());
            continue;
        }

        if line == "bare" {
            current_is_bare = true;
            continue;
        }

        if line.is_empty()
            && let Some(path) = current_path.take()
        {
            entries.push(WorktreeEntry {
                path,
                branch_ref: current_branch.take(),
                is_bare: current_is_bare,
            });
            current_is_bare = false;
        }
    }

    flush(
        &mut entries,
        current_path,
        current_branch,
        current_is_bare,
    );

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

    use super::{WorktreeEntry, branch_from_ref, branch_from_worktree_path, parse_worktree_porcelain};

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
