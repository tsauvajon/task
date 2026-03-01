use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    runtime::{branch_name::BranchName, repo_key::RepoKey},
    tools::{
        git::worktrees::{WorktreeEntry, branch_from_ref, branch_from_worktree_path},
        tmux::naming::session_name,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub status: TaskStatus,
    pub repo: RepoKey,
    pub branch: BranchName,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskStatus {
    Open,
    Parked,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Parked => f.write_str("parked"),
        }
    }
}

pub fn build_task_rows(
    repo_key: &RepoKey,
    wt_dir: &Path,
    entries: &[WorktreeEntry],
    open_sessions: &[String],
) -> Vec<TaskRow> {
    // Canonicalize wt_dir so that path comparisons work correctly even when
    // the directory contains symlinks (e.g. macOS /var → /private/var).
    let real_wt_dir = std::fs::canonicalize(wt_dir).unwrap_or_else(|_| wt_dir.to_path_buf());
    let task_root = real_wt_dir.join(repo_key.as_str());
    entries
        .iter()
        .filter(|e| !e.is_bare)
        .filter(|e| {
            let real_path = std::fs::canonicalize(&e.path).unwrap_or_else(|_| e.path.clone());
            real_path.starts_with(&task_root) && real_path != task_root
        })
        .map(|entry| {
            let branch = branch_from_ref(entry.branch_ref.as_deref())
                .or_else(|| branch_from_worktree_path(wt_dir, repo_key, &entry.path))
                .or_else(|| {
                    entry
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "unknown".to_string());

            let session = session_name(repo_key, &branch);
            let status = if open_sessions.iter().any(|name| name == &session) {
                TaskStatus::Open
            } else {
                TaskStatus::Parked
            };

            TaskRow {
                status,
                repo: repo_key.clone(),
                branch: BranchName::new(branch),
                path: entry.path.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{TaskRow, TaskStatus, build_task_rows};
    use crate::{
        runtime::{BranchName, RepoKey},
        tools::git::worktrees::WorktreeEntry,
    };

    mod task_status {
        use super::*;

        #[test]
        fn display() {
            assert_eq!(TaskStatus::Open.to_string(), "open");
            assert_eq!(TaskStatus::Parked.to_string(), "parked");
        }

        #[test]
        fn ordering_open_before_parked() {
            assert!(TaskStatus::Open < TaskStatus::Parked);
            assert!(TaskStatus::Parked > TaskStatus::Open);
        }

        #[test]
        fn equality() {
            assert_eq!(TaskStatus::Open, TaskStatus::Open);
            assert_eq!(TaskStatus::Parked, TaskStatus::Parked);
            assert_ne!(TaskStatus::Open, TaskStatus::Parked);
        }
    }

    mod build_task_rows {
        use super::*;

        #[test]
        fn marks_open_and_parked_states() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                    branch_ref: Some("refs/heads/rewrite-in-rust".to_string()),
                    is_bare: false,
                },
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
                    branch_ref: Some("refs/heads/bump-deps".to_string()),
                    is_bare: false,
                },
            ];

            let open_sessions = vec!["github_com_tsauvajon_task-rewrite-in-rust".to_string()];
            let rows = build_task_rows(&repo_key, wt_dir, &entries, &open_sessions);

            assert_eq!(
                rows,
                vec![
                    TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/tsauvajon/task"),
                        branch: BranchName::new("rewrite-in-rust"),
                        path: "/tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                    },
                    TaskRow {
                        status: TaskStatus::Parked,
                        repo: RepoKey::new("github.com/tsauvajon/task"),
                        branch: BranchName::new("bump-deps"),
                        path: "/tmp/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
                    },
                ]
            );
        }

        #[test]
        fn filters_out_bare_entries() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![
                WorktreeEntry {
                    path: "/tmp/dev/repos/github.com/tsauvajon/task.git".into(),
                    branch_ref: None,
                    is_bare: true,
                },
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/main".into(),
                    branch_ref: Some("refs/heads/main".to_string()),
                    is_bare: false,
                },
            ];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].branch, BranchName::new("main"));
        }

        #[test]
        fn filters_out_entries_outside_task_root() {
            // Any worktree whose path is not under wt_dir/repo_key is excluded.
            // This covers detached snapshots, external checkouts, etc.
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/feat".into(),
                    branch_ref: Some("refs/heads/feat".to_string()),
                    is_bare: false,
                },
                // Detached snapshot under detached_dir — must be excluded.
                WorktreeEntry {
                    path: "/tmp/dev/detached/github.com/tsauvajon/task".into(),
                    branch_ref: None,
                    is_bare: false,
                },
                // Arbitrary external checkout — must be excluded.
                WorktreeEntry {
                    path: "/some/other/dir/mybranch".into(),
                    branch_ref: Some("refs/heads/mybranch".to_string()),
                    is_bare: false,
                },
            ];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert_eq!(
                rows.len(),
                1,
                "only the task-root worktree should be included"
            );
            assert_eq!(rows[0].branch, BranchName::new("feat"));
        }

        #[test]
        fn returns_empty_for_only_bare_entry() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![WorktreeEntry {
                path: "/tmp/dev/repos/github.com/tsauvajon/task.git".into(),
                branch_ref: None,
                is_bare: true,
            }];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert!(rows.is_empty());
        }

        #[test]
        fn empty_entries_returns_empty() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let rows = build_task_rows(&repo_key, wt_dir, &[], &[]);
            assert!(rows.is_empty());
        }

        #[test]
        fn multiple_open_sessions_match_correctly() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/branch-a".into(),
                    branch_ref: Some("refs/heads/branch-a".to_string()),
                    is_bare: false,
                },
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/branch-b".into(),
                    branch_ref: Some("refs/heads/branch-b".to_string()),
                    is_bare: false,
                },
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/branch-c".into(),
                    branch_ref: Some("refs/heads/branch-c".to_string()),
                    is_bare: false,
                },
            ];

            let open_sessions = vec![
                "github_com_tsauvajon_task-branch-a".to_string(),
                "github_com_tsauvajon_task-branch-c".to_string(),
            ];
            let rows = build_task_rows(&repo_key, wt_dir, &entries, &open_sessions);

            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].status, TaskStatus::Open);
            assert_eq!(rows[1].status, TaskStatus::Parked);
            assert_eq!(rows[2].status, TaskStatus::Open);
        }

        #[test]
        fn branch_from_worktree_path_fallback() {
            // branch_ref is None but path is under wt_dir/repo_key prefix:
            // branch is derived from the relative path.
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![WorktreeEntry {
                path: "/tmp/dev/wt/github.com/tsauvajon/task/feature-xyz".into(),
                branch_ref: None,
                is_bare: false,
            }];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].branch, BranchName::new("feature-xyz"));
        }

        #[test]
        fn repo_key_is_preserved() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![WorktreeEntry {
                path: "/tmp/dev/wt/github.com/tsauvajon/task/main".into(),
                branch_ref: Some("refs/heads/main".to_string()),
                is_bare: false,
            }];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert_eq!(rows[0].repo, repo_key);
        }

        #[test]
        fn path_is_preserved_from_entry() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let path = std::path::PathBuf::from("/tmp/dev/wt/github.com/tsauvajon/task/main");
            let entries = vec![WorktreeEntry {
                path: path.clone(),
                branch_ref: Some("refs/heads/main".to_string()),
                is_bare: false,
            }];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert_eq!(rows[0].path, path);
        }

        #[test]
        fn all_task_worktrees_are_included() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let repo_key = RepoKey::new("github.com/tsauvajon/task");
            let entries = vec![
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/main".into(),
                    branch_ref: Some("refs/heads/main".to_string()),
                    is_bare: false,
                },
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
                    branch_ref: Some("refs/heads/bump-deps".to_string()),
                    is_bare: false,
                },
            ];

            let rows = build_task_rows(&repo_key, wt_dir, &entries, &[]);
            assert_eq!(rows.len(), 2, "both task worktrees should be included");
        }
    }
}
