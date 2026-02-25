use std::path::PathBuf;

use crate::tools::tmux::naming::session_name;

pub fn key(repo_key: &str, branch: &str) -> String {
    session_name(repo_key, branch)
}

pub fn codium_state_root() -> PathBuf {
    if let Ok(value) = std::env::var("XDG_STATE_HOME")
        && !value.trim().is_empty()
    {
        return PathBuf::from(value).join("task").join("codium");
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("task")
        .join("codium")
}

pub fn user_data_dir(repo_key: &str, branch: &str) -> PathBuf {
    codium_state_root().join(key(repo_key, branch))
}

#[cfg(test)]
mod tests {
    use super::{key, user_data_dir};

    #[test]
    fn key_matches_tmux_session_name() {
        assert_eq!(
            key("github.com/acme/tool", "feat/test.1"),
            "github_com_acme_tool-feat_test_1"
        );
    }

    #[test]
    fn user_data_dir_contains_sanitized_key() {
        let dir = user_data_dir("github.com/acme/tool", "feat/test.1");
        assert!(dir.ends_with("codium/github_com_acme_tool-feat_test_1"));
    }
}
