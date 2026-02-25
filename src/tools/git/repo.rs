use std::{fs, path::Path};

use super::runner::run_git_capture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInput {
    pub repo_key: String,
    pub clone_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    Resolved(String),
    Ambiguous(Vec<String>),
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

pub fn default_clone_url(input: &str) -> String {
    let trimmed = input.trim();
    if is_git_url(trimmed) {
        return trimmed.to_string();
    }

    if looks_like_host_path(trimmed) {
        return format!("https://{trimmed}");
    }

    trimmed.to_string()
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

pub fn resolve_repo_query(query: &str, available_keys: &[String]) -> ResolveResult {
    let normalized = normalize_repo_key(query);

    if available_keys.iter().any(|key| key == &normalized) {
        return ResolveResult::Resolved(normalized);
    }

    let mut matches = Vec::new();
    for key in available_keys {
        let base = key.rsplit('/').next().unwrap_or_default();
        if key == &normalized || base == normalized || key.ends_with(&format!("/{normalized}")) {
            matches.push(key.clone());
        }
    }
    matches.sort();
    matches.dedup();

    if matches.is_empty() {
        return ResolveResult::Resolved(normalized);
    }

    if matches.len() == 1 {
        return ResolveResult::Resolved(matches[0].clone());
    }

    ResolveResult::Ambiguous(matches)
}

pub fn is_valid_bare_repo(gitdir: &Path) -> bool {
    if !gitdir.is_dir() {
        return false;
    }

    let gitdir_str = gitdir.to_string_lossy();
    run_git_capture(
        &[
            "--git-dir",
            &gitdir_str,
            "rev-parse",
            "--is-bare-repository",
        ],
        None,
    )
    .is_ok_and(|output| output.trim() == "true")
}

pub fn clone_bare_repo(repo_url: &str, gitdir: &Path) -> Result<(), String> {
    if gitdir.is_dir() {
        if is_valid_bare_repo(gitdir) {
            return Ok(());
        }
        fs::remove_dir_all(gitdir).map_err(|e| {
            format!(
                "Failed to remove invalid bare repo at {}: {e}",
                gitdir.display()
            )
        })?;
    }

    if let Some(parent) = gitdir.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    run_git_capture(
        &[
            "clone",
            "--bare",
            repo_url,
            gitdir.to_string_lossy().as_ref(),
        ],
        None,
    )
    .map(|_| ())
}

fn is_git_url(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("ssh://")
        || input.starts_with("git@")
}

fn looks_like_host_path(input: &str) -> bool {
    let mut parts = input.split('/');
    let host = parts.next().unwrap_or_default();
    let has_path = parts.next().is_some();
    host.contains('.') && has_path
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{
        ResolveResult, default_clone_url, is_valid_bare_repo, normalize_repo_key, parse_repo_input,
        resolve_repo_query,
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
    fn default_clone_url_keeps_protocol_urls() {
        assert_eq!(
            default_clone_url("git@github.com:tsauvajon/goto.git"),
            "git@github.com:tsauvajon/goto.git"
        );
        assert_eq!(
            default_clone_url("https://github.com/tsauvajon/goto.git"),
            "https://github.com/tsauvajon/goto.git"
        );
    }

    #[test]
    fn default_clone_url_prefixes_https_for_host_paths() {
        assert_eq!(
            default_clone_url("github.com/tsauvajon/goto"),
            "https://github.com/tsauvajon/goto"
        );
    }

    #[test]
    fn resolve_repo_query_by_short_name_when_unique() {
        let keys = vec![
            "github.com/tsauvajon/goto".to_string(),
            "github.com/tsauvajon/task".to_string(),
        ];

        let resolved = resolve_repo_query("goto", &keys);
        assert_eq!(
            resolved,
            ResolveResult::Resolved("github.com/tsauvajon/goto".to_string())
        );
    }

    #[test]
    fn resolve_repo_query_reports_ambiguity() {
        let keys = vec![
            "github.com/tsauvajon/goto".to_string(),
            "github.com/example/goto".to_string(),
        ];

        let resolved = resolve_repo_query("goto", &keys);
        assert_eq!(
            resolved,
            ResolveResult::Ambiguous(vec![
                "github.com/example/goto".to_string(),
                "github.com/tsauvajon/goto".to_string(),
            ])
        );
    }

    #[test]
    fn is_valid_bare_repo_rejects_nonexistent_path() {
        let path = env::temp_dir().join("task-rs-nonexistent-bare-repo");
        let _ = fs::remove_dir_all(&path);
        assert!(!is_valid_bare_repo(&path));
    }

    #[test]
    fn is_valid_bare_repo_rejects_empty_directory() {
        let path = env::temp_dir().join("task-rs-empty-bare-repo");
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create empty dir");

        assert!(!is_valid_bare_repo(&path));

        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn is_valid_bare_repo_accepts_real_bare_repo() {
        let path = env::temp_dir().join("task-rs-valid-bare-repo.git");
        let _ = fs::remove_dir_all(&path);

        // Create a real bare repo using git init --bare
        let output = std::process::Command::new("git")
            .args(["init", "--bare", &path.to_string_lossy()])
            .output()
            .expect("git init --bare");
        assert!(output.status.success(), "git init --bare failed");

        assert!(is_valid_bare_repo(&path));

        let _ = fs::remove_dir_all(&path);
    }
}
