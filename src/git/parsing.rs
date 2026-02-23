use std::path::PathBuf;

use crate::runtime::task_session_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInput {
    pub repo_key: String,
    pub clone_url: Option<String>,
}

pub fn parse_repo_input(input: &str) -> RepoInput {
    let trimmed = input.trim();
    let repo_key = normalize_repo_key(trimmed);
    let clone_url = if is_git_url(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    };

    RepoInput {
        repo_key,
        clone_url,
    }
}

pub fn normalize_repo_key(input: &str) -> String {
    let mut key = input.trim().to_string();

    if let Some((_, rest)) = key.split_once("://") {
        key = rest.to_string();
    }

    let first_slash = key.find('/').unwrap_or(key.len());
    let first_colon = key.find(':').unwrap_or(key.len());
    let boundary = first_slash.min(first_colon);
    if let Some(at_index) = key.find('@')
        && at_index < boundary
    {
        key = key[(at_index + 1)..].to_string();
    }

    if let Some(colon_index) = key.find(':')
        && key
            .find('/')
            .map(|slash_index| colon_index < slash_index)
            .unwrap_or(true)
    {
        key.replace_range(colon_index..=colon_index, "/");
    }

    while key.starts_with('/') {
        key.remove(0);
    }

    if let Some(stripped) = key.strip_suffix(".git") {
        return stripped.to_string();
    }

    key
}

fn is_git_url(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("ssh://")
        || input.starts_with("git@")
}

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

        let session = task_session_name(repo_key, &branch);
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

#[cfg(test)]
mod tests {
    use super::{
        TaskRow, WorktreeEntry, branch_from_ref, branch_from_worktree_path, build_task_rows,
        normalize_repo_key, parse_repo_input, parse_worktree_porcelain, repo_key_from_common_dir,
    };

    #[test]
    fn normalize_repo_key_handles_git_urls() {
        assert_eq!(
            normalize_repo_key("git@github.com:tsauvajon/goto.git"),
            "github.com/tsauvajon/goto"
        );
        assert_eq!(
            normalize_repo_key("https://github.com/tsauvajon/goto.git"),
            "github.com/tsauvajon/goto"
        );
    }

    #[test]
    fn normalize_repo_key_handles_plain_keys() {
        assert_eq!(
            normalize_repo_key("github.com/tsauvajon/goto.git"),
            "github.com/tsauvajon/goto"
        );
        assert_eq!(
            normalize_repo_key("/github.com/tsauvajon/goto"),
            "github.com/tsauvajon/goto"
        );
    }

    #[test]
    fn normalize_repo_key_handles_ssh_git_url() {
        assert_eq!(
            normalize_repo_key("ssh://git@github.com/tsauvajon/goto.git"),
            "github.com/tsauvajon/goto"
        );
    }

    #[test]
    fn parse_repo_input_keeps_clone_scheme() {
        let parsed = parse_repo_input("git@github.com:tsauvajon/goto.git");
        assert_eq!(parsed.repo_key, "github.com/tsauvajon/goto");
        assert_eq!(
            parsed.clone_url,
            Some("git@github.com:tsauvajon/goto.git".to_string())
        );
    }

    #[test]
    fn parse_repo_input_has_no_clone_url_for_plain_key() {
        let parsed = parse_repo_input("github.com/tsauvajon/goto");
        assert_eq!(parsed.repo_key, "github.com/tsauvajon/goto");
        assert_eq!(parsed.clone_url, None);
    }

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
    fn repo_key_from_common_dir_extracts_key() {
        let key = repo_key_from_common_dir("/tmp/dev/repos/github.com/tsauvajon/task.git");
        assert_eq!(key, Some("github.com/tsauvajon/task".to_string()));
    }

    #[test]
    fn branch_from_ref_strips_prefix() {
        assert_eq!(
            branch_from_ref(Some("refs/heads/rewrite-in-rust")),
            Some("rewrite-in-rust".to_string())
        );
    }

    #[test]
    fn build_task_rows_marks_open_and_parked_states() {
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
        let rows = build_task_rows("github.com/tsauvajon/task", &entries, &open_sessions);

        assert_eq!(
            rows,
            vec![
                TaskRow {
                    status: "open".to_string(),
                    repo: "github.com/tsauvajon/task".to_string(),
                    branch: "rewrite-in-rust".to_string(),
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                },
                TaskRow {
                    status: "parked".to_string(),
                    repo: "github.com/tsauvajon/task".to_string(),
                    branch: "bump-deps".to_string(),
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
                },
            ]
        );
    }
}
