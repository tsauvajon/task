use std::path::{Path, PathBuf};

use super::runner::{run_git_capture, run_git_status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch_ref: Option<String>,
    pub is_bare: bool,
}

pub fn worktree_list(gitdir: &Path) -> Result<String, String> {
    run_git_capture(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "list",
        ],
        None,
    )
}

pub fn worktree_list_porcelain(gitdir: &Path) -> Result<String, String> {
    run_git_capture(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "list",
            "--porcelain",
        ],
        None,
    )
}

pub fn worktree_add_existing_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<(), String> {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "add",
            worktree.to_string_lossy().as_ref(),
            branch,
        ],
        None,
    )
}

pub fn worktree_add_tracking_remote_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<(), String> {
    let remote = format!("origin/{branch}");
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "add",
            "--track",
            "-b",
            branch,
            worktree.to_string_lossy().as_ref(),
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
) -> Result<(), String> {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "add",
            "-b",
            branch,
            worktree.to_string_lossy().as_ref(),
            base_ref,
        ],
        None,
    )
}

pub fn worktree_prune(gitdir: &Path) -> Result<(), String> {
    let args = worktree_prune_args(gitdir);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git_status(&arg_refs, None)
}

pub fn worktree_remove(gitdir: &Path, worktree: &Path, force: bool) -> Result<(), String> {
    let mut args = vec![
        "--git-dir".to_string(),
        gitdir.to_string_lossy().to_string(),
        "worktree".to_string(),
        "remove".to_string(),
    ];
    if force {
        args.push("--force".to_string());
    }
    args.push(worktree.to_string_lossy().to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let cwd = worktree_remove_cwd(gitdir);
    run_git_status(&arg_refs, cwd.as_deref())
}

pub fn status_porcelain(worktree: &Path) -> Result<String, String> {
    run_git_capture(
        &[
            "-C",
            worktree.to_string_lossy().as_ref(),
            "status",
            "--porcelain",
        ],
        None,
    )
}

pub fn rebase(worktree: &Path, base_ref: &str) -> Result<(), String> {
    run_git_status(
        &[
            "-C",
            worktree.to_string_lossy().as_ref(),
            "rebase",
            base_ref,
        ],
        None,
    )
}

pub fn parse_worktree_porcelain(text: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut current_is_bare = false;

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(path) = current_path.take() {
                entries.push(WorktreeEntry {
                    path,
                    branch_ref: current_branch.take(),
                    is_bare: current_is_bare,
                });
            }
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

    if let Some(path) = current_path {
        entries.push(WorktreeEntry {
            path,
            branch_ref: current_branch,
            is_bare: current_is_bare,
        });
    }

    entries
}

pub fn branch_from_worktree_path(repo_key: &str, worktree_path: &str) -> Option<String> {
    let marker = format!("/wt/{repo_key}/");
    if let Some(index) = worktree_path.find(&marker) {
        let branch = &worktree_path[(index + marker.len())..];
        if !branch.is_empty() {
            return Some(branch.to_string());
        }
    }
    None
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

fn worktree_prune_args(gitdir: &Path) -> Vec<String> {
    vec![
        "--git-dir".to_string(),
        gitdir.to_string_lossy().to_string(),
        "worktree".to_string(),
        "prune".to_string(),
        "--verbose".to_string(),
        "--expire".to_string(),
        "now".to_string(),
    ]
}

fn worktree_remove_cwd(gitdir: &Path) -> Option<PathBuf> {
    gitdir.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        WorktreeEntry, branch_from_ref, branch_from_worktree_path, parse_worktree_porcelain,
        worktree_prune_args, worktree_remove_cwd,
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
            "github.com/tsauvajon/task",
            "/tmp/dev/wt/github.com/tsauvajon/task/feat/rewrite/rust",
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
        let args = worktree_prune_args(Path::new("/tmp/repos/github.com/acme/tool.git"));
        assert_eq!(
            args,
            vec![
                "--git-dir",
                "/tmp/repos/github.com/acme/tool.git",
                "worktree",
                "prune",
                "--verbose",
                "--expire",
                "now",
            ]
        );
    }

    #[test]
    fn worktree_remove_uses_repo_parent_as_cwd() {
        let cwd = worktree_remove_cwd(Path::new("/tmp/repos/github.com/acme/tool.git"));
        assert_eq!(
            cwd,
            Some(Path::new("/tmp/repos/github.com/acme").to_path_buf())
        );
    }
}
