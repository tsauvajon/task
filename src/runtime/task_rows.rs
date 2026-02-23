use std::path::{Path, PathBuf};

use crate::tools::git::worktrees::{WorktreeEntry, branch_from_ref, branch_from_worktree_path};
use crate::tools::tmux::naming::session_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub status: TaskStatus,
    pub repo: String,
    pub branch: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskStatus {
    Open,
    Parked,
}

pub fn build_task_rows(
    repo_key: &str,
    wt_dir: &Path,
    entries: &[WorktreeEntry],
    open_sessions: &[String],
) -> Vec<TaskRow> {
    let mut rows = Vec::new();

    for entry in entries {
        if entry.is_bare {
            continue;
        }

        let branch = branch_from_ref(entry.branch_ref.as_deref())
            .or_else(|| branch_from_worktree_path(wt_dir, repo_key, &entry.path))
            .or_else(|| {
                entry
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let session = session_name(repo_key, &branch);
        let status = if open_sessions.iter().any(|name| name == &session) {
            TaskStatus::Open
        } else {
            TaskStatus::Parked
        };

        rows.push(TaskRow {
            status,
            repo: repo_key.to_string(),
            branch,
            path: entry.path.clone(),
        });
    }

    rows
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::tools::git::worktrees::WorktreeEntry;

    use super::{TaskRow, TaskStatus, build_task_rows};

    #[test]
    fn build_task_rows_marks_open_and_parked_states() {
        let wt_dir = Path::new("/tmp/dev/wt");
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
        let rows = build_task_rows(
            "github.com/tsauvajon/task",
            wt_dir,
            &entries,
            &open_sessions,
        );

        assert_eq!(
            rows,
            vec![
                TaskRow {
                    status: TaskStatus::Open,
                    repo: "github.com/tsauvajon/task".to_string(),
                    branch: "rewrite-in-rust".to_string(),
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                },
                TaskRow {
                    status: TaskStatus::Parked,
                    repo: "github.com/tsauvajon/task".to_string(),
                    branch: "bump-deps".to_string(),
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
                },
            ]
        );
    }
}
