use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    tools::{
        git::worktrees::{branch_from_ref, branch_from_worktree_path, WorktreeEntry},
        tmux::naming::session_name,
    },
    types::{BranchName, RepoKey},
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
    entries
        .iter()
        .filter(|e| !e.is_bare)
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

    use super::{build_task_rows, TaskRow, TaskStatus};
    use crate::{
        tools::git::worktrees::WorktreeEntry,
        types::{BranchName, RepoKey},
    };

    #[test]
    fn build_task_rows_marks_open_and_parked_states() {
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
}
