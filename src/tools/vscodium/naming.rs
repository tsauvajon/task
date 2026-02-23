use std::path::PathBuf;

use crate::tools::tmux;

pub fn task_key(repo_key: &str, branch: &str) -> String {
    tmux::session_name(repo_key, branch)
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

pub fn task_user_data_dir(repo_key: &str, branch: &str) -> PathBuf {
    codium_state_root().join(task_key(repo_key, branch))
}

#[cfg(test)]
mod tests {
    use super::{task_key, task_user_data_dir};

    #[test]
    fn task_key_matches_tmux_session_name() {
        assert_eq!(
            task_key("github.com/acme/tool", "feat/test.1"),
            "github_com_acme_tool-feat_test_1"
        );
    }

    #[test]
    fn task_user_data_dir_contains_sanitized_task_key() {
        let dir = task_user_data_dir("github.com/acme/tool", "feat/test.1");
        assert!(dir.ends_with("codium/github_com_acme_tool-feat_test_1"));
    }
}
