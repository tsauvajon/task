use std::path::PathBuf;

use crate::session::session_name_for;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch_ref: Option<String>,
    pub is_bare: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub status: String,
    pub repo: String,
    pub branch: String,
    pub path: PathBuf,
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

pub fn repo_key_from_common_dir(common_dir: &str) -> Option<String> {
    let marker = "/repos/";
    let index = common_dir.find(marker)?;
    let mut key = common_dir[(index + marker.len())..].to_string();
    if key.ends_with(".git") {
        key.truncate(key.len() - 4);
    }
    if key.is_empty() {
        return None;
    }
    Some(key)
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

pub fn build_task_rows(
    repo_key: &str,
    entries: &[WorktreeEntry],
    open_sessions: &[String],
) -> Vec<TaskRow> {
    let mut rows = Vec::new();

    for entry in entries {
        if entry.is_bare {
            continue;
        }

        let path_text = entry.path.to_string_lossy().to_string();
        let branch = branch_from_ref(entry.branch_ref.as_deref())
            .or_else(|| branch_from_worktree_path(repo_key, &path_text))
            .or_else(|| {
                entry
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let session = session_name_for(repo_key, &branch);
        let status = if open_sessions.iter().any(|name| name == &session) {
            "open".to_string()
        } else {
            "parked".to_string()
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
