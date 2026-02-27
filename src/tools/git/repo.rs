use std::{fs, path::Path};

use super::{gitdir::GitDir, run::capture};
use crate::error::{Error, Result};

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

    let mut matches: Vec<String> = available_keys
        .iter()
        .filter(|key| {
            let base = key.rsplit('/').next().unwrap_or_default();
            *key == &normalized || base == normalized || key.ends_with(&format!("/{normalized}"))
        })
        .cloned()
        .collect();
    matches.sort();
    matches.dedup();

    if matches.len() == 1 {
        return ResolveResult::Resolved(matches.remove(0));
    }

    if matches.is_empty() {
        ResolveResult::Resolved(normalized)
    } else {
        ResolveResult::Ambiguous(matches)
    }
}

pub fn is_valid_bare_repo(gitdir: &Path) -> bool {
    if !gitdir.is_dir() {
        return false;
    }
    GitDir::new(gitdir)
        .capture(&["rev-parse", "--is-bare-repository"])
        .is_ok_and(|output| output.trim() == "true")
}

pub fn clone_bare_repo(repo_url: &str, gitdir: &Path) -> Result<()> {
    if gitdir.is_dir() {
        if is_valid_bare_repo(gitdir) {
            return Ok(());
        }
        fs::remove_dir_all(gitdir).map_err(|err| {
            Error::failed(format!(
                "Failed to remove invalid bare repo at {}: {err}",
                gitdir.display()
            ))
        })?;
    }

    if let Some(parent) = gitdir.parent() {
        fs::create_dir_all(parent)?;
    }

    capture(
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
mod private_tests {
    use super::{is_git_url, looks_like_host_path};

    mod is_git_url {
        use super::*;

        #[test]
        fn recognizes_http() {
            assert!(is_git_url("http://github.com/acme/repo.git"));
        }

        #[test]
        fn recognizes_https() {
            assert!(is_git_url("https://github.com/acme/repo.git"));
        }

        #[test]
        fn recognizes_ssh_scheme() {
            assert!(is_git_url("ssh://git@github.com/acme/repo.git"));
        }

        #[test]
        fn recognizes_git_at() {
            assert!(is_git_url("git@github.com:acme/repo.git"));
        }

        #[test]
        fn rejects_plain_host_path() {
            assert!(!is_git_url("github.com/acme/repo"));
        }

        #[test]
        fn rejects_short_name() {
            assert!(!is_git_url("myrepo"));
        }
    }

    mod looks_like_host_path {
        use super::*;

        #[test]
        fn matches_host_with_dot_and_path() {
            assert!(looks_like_host_path("github.com/acme/repo"));
        }

        #[test]
        fn rejects_host_without_path() {
            assert!(!looks_like_host_path("github.com"));
        }

        #[test]
        fn rejects_plain_name_without_dot() {
            assert!(!looks_like_host_path("myrepo/something"));
        }

        #[test]
        fn rejects_empty_string() {
            assert!(!looks_like_host_path(""));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{
        ResolveResult, default_clone_url, is_valid_bare_repo, normalize_repo_key, parse_repo_input,
        resolve_repo_query,
    };

    mod normalize_repo_key {
        use super::*;

        #[test]
        fn handles_git_urls() {
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
        fn handles_plain_keys() {
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
        fn handles_ssh_git_url() {
            assert_eq!(
                normalize_repo_key("ssh://git@github.com/tsauvajon/goto.git"),
                "github.com/tsauvajon/goto"
            );
        }
    }

    mod parse_repo_input {
        use super::*;

        #[test]
        fn keeps_clone_scheme() {
            let parsed = parse_repo_input("git@github.com:tsauvajon/goto.git");
            assert_eq!(parsed.repo_key, "github.com/tsauvajon/goto");
            assert_eq!(
                parsed.clone_url,
                Some("git@github.com:tsauvajon/goto.git".to_string())
            );
        }

        #[test]
        fn has_no_clone_url_for_plain_key() {
            let parsed = parse_repo_input("github.com/tsauvajon/goto");
            assert_eq!(parsed.repo_key, "github.com/tsauvajon/goto");
            assert_eq!(parsed.clone_url, None);
        }
    }

    mod default_clone_url {
        use super::*;

        #[test]
        fn keeps_protocol_urls() {
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
        fn prefixes_https_for_host_paths() {
            assert_eq!(
                default_clone_url("github.com/tsauvajon/goto"),
                "https://github.com/tsauvajon/goto"
            );
        }
    }

    mod normalize_repo_key_extra {
        use super::*;

        #[test]
        fn trims_whitespace() {
            assert_eq!(
                normalize_repo_key("  github.com/acme/repo  "),
                "github.com/acme/repo"
            );
        }

        #[test]
        fn strips_multiple_leading_slashes() {
            assert_eq!(
                normalize_repo_key("///github.com/acme/repo"),
                "github.com/acme/repo"
            );
        }
    }

    mod default_clone_url_extra {
        use super::*;

        #[test]
        fn returns_plain_string_unchanged_when_no_dot_in_host() {
            // A value like "myrepo" has no host dot → returned as-is.
            assert_eq!(default_clone_url("myrepo"), "myrepo");
        }
    }

    mod resolve_repo_query {
        use super::*;

        #[test]
        fn by_short_name_when_unique() {
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
        fn reports_ambiguity() {
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
        fn falls_through_to_resolved_when_no_matches() {
            // When there are no suffix matches the query is returned as
            // Resolved (treated as a new key the caller provides).
            let keys = vec!["github.com/acme/alpha".to_string()];
            let result = resolve_repo_query("github.com/acme/beta", &keys);
            assert_eq!(
                result,
                ResolveResult::Resolved("github.com/acme/beta".to_string())
            );
        }

        #[test]
        fn matches_full_key_exactly() {
            let keys = vec![
                "github.com/acme/alpha".to_string(),
                "github.com/acme/beta".to_string(),
            ];
            let result = resolve_repo_query("github.com/acme/alpha", &keys);
            assert_eq!(
                result,
                ResolveResult::Resolved("github.com/acme/alpha".to_string())
            );
        }
    }

    mod is_valid_bare_repo {
        use super::*;

        #[test]
        fn rejects_nonexistent_path() {
            let path = env::temp_dir().join("task-rs-nonexistent-bare-repo");
            let _ = fs::remove_dir_all(&path);
            assert!(!is_valid_bare_repo(&path));
        }

        #[test]
        fn rejects_empty_directory() {
            let path = env::temp_dir().join("task-rs-empty-bare-repo");
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create empty dir");

            assert!(!is_valid_bare_repo(&path));

            let _ = fs::remove_dir_all(&path);
        }

        #[test]
        fn accepts_real_bare_repo() {
            let path = env::temp_dir().join("task-rs-valid-bare-repo.git");
            let _ = fs::remove_dir_all(&path);

            let output = std::process::Command::new("git")
                .args(["init", "--bare", &path.to_string_lossy()])
                .output()
                .expect("git init --bare");
            assert!(output.status.success(), "git init --bare failed");

            assert!(is_valid_bare_repo(&path));

            let _ = fs::remove_dir_all(&path);
        }
    }
}
