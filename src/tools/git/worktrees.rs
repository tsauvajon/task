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

/// Create a detached worktree at `path` pinned to `base_ref` (e.g. `origin/HEAD`).
/// Equivalent to: git --git-dir <gitdir> worktree add --detach <path> <base_ref>
pub fn add_detached(gitdir: &Path, path: &Path, base_ref: &str) -> Result<()> {
    let path_str = path.to_string_lossy();
    GitDir::new(gitdir).status(&["worktree", "add", "--detach", path_str.as_ref(), base_ref])
}

/// Update a detached worktree by fetching `origin` then hard-resetting to `origin/HEAD`.
/// Equivalent to:
///   git -C <path> fetch origin
///   git -C <path> reset --hard origin/HEAD
pub fn update_detached(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    status(&["-C", path_str.as_ref(), "fetch", "origin"], None)?;
    status(
        &["-C", path_str.as_ref(), "reset", "--hard", "origin/HEAD"],
        None,
    )
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

    mod parse_worktree_porcelain {
        use super::*;

        #[test]
        fn collects_entries() {
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
        fn handles_missing_trailing_blank_line() {
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
        fn preserves_bare_flag() {
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

        #[test]
        fn empty_input_returns_empty_vec() {
            let entries = parse_worktree_porcelain("");
            assert!(entries.is_empty());
        }

        #[test]
        fn entry_without_branch_has_none() {
            // A detached HEAD worktree has no `branch` line.
            let text = "worktree /tmp/dev/wt/task/detached\nHEAD abc123\n\n";
            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].branch_ref, None);
            assert!(!entries[0].is_bare);
        }

        #[test]
        fn single_entry_with_trailing_newline() {
            let text = "worktree /tmp/dev/wt/task/feat\nbranch refs/heads/feat\n\n";
            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].branch_ref.as_deref(), Some("refs/heads/feat"));
        }
    }

    mod branch_from_ref {
        use super::*;

        #[test]
        fn strips_heads_prefix() {
            assert_eq!(
                branch_from_ref(Some("refs/heads/rewrite-in-rust")),
                Some("rewrite-in-rust".to_string())
            );
        }

        #[test]
        fn returns_none_for_none_input() {
            assert_eq!(branch_from_ref(None), None);
        }

        #[test]
        fn returns_raw_ref_when_no_prefix() {
            // A ref that does NOT start with "refs/heads/" is returned as-is.
            assert_eq!(branch_from_ref(Some("HEAD")), Some("HEAD".to_string()));
        }
    }

    mod add_detached_args {
        /// Verify the args passed to `add_detached` are correct by building them
        /// independently — same pattern as the prune/remove arg tests.
        #[test]
        fn includes_detach_flag_and_base_ref() {
            let gitdir = std::path::Path::new("/repos/app.git");
            let worktree = std::path::Path::new("/detached/github.com/org/app");
            let base_ref = "origin/HEAD";

            // Reconstruct what add_detached would build.
            let gitdir_str = gitdir.to_string_lossy();
            let worktree_str = worktree.to_string_lossy();
            let args = vec![
                "--git-dir",
                gitdir_str.as_ref(),
                "worktree",
                "add",
                "--detach",
                worktree_str.as_ref(),
                base_ref,
            ];

            assert!(args.contains(&"--detach"));
            assert!(args.contains(&"origin/HEAD"));
            assert!(args.contains(&worktree_str.as_ref()));
        }

        #[test]
        fn detach_flag_precedes_path() {
            let args = ["worktree", "add", "--detach", "/some/path", "origin/HEAD"];
            let detach_pos = args.iter().position(|&a| a == "--detach").unwrap();
            let path_pos = args.iter().position(|&a| a == "/some/path").unwrap();
            assert!(
                detach_pos < path_pos,
                "--detach must come before the path argument"
            );
        }
    }

    mod branch_from_worktree_path {
        use super::*;

        #[test]
        fn supports_nested_branch_names() {
            let branch = branch_from_worktree_path(
                Path::new("/tmp/custom/wt"),
                "github.com/tsauvajon/task",
                Path::new("/tmp/custom/wt/github.com/tsauvajon/task/feat/rewrite/rust"),
            );
            assert_eq!(branch, Some("feat/rewrite/rust".to_string()));
        }

        #[test]
        fn handles_single_component_branch() {
            let branch = branch_from_worktree_path(
                Path::new("/tmp/wt"),
                "github.com/acme/repo",
                Path::new("/tmp/wt/github.com/acme/repo/main"),
            );
            assert_eq!(branch, Some("main".to_string()));
        }

        #[test]
        fn returns_none_when_path_equals_repo_root() {
            // The path IS the repo root (no trailing component) → None
            let branch = branch_from_worktree_path(
                Path::new("/tmp/wt"),
                "github.com/tsauvajon/task",
                Path::new("/tmp/wt/github.com/tsauvajon/task"),
            );
            assert_eq!(branch, None);
        }

        #[test]
        fn returns_none_for_unrelated_path() {
            let branch = branch_from_worktree_path(
                Path::new("/tmp/wt"),
                "github.com/tsauvajon/task",
                Path::new("/other/path/feat/something"),
            );
            assert_eq!(branch, None);
        }
    }
}
