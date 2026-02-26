use std::path::PathBuf;

use crate::tools::tmux::naming::session_name;

pub fn key(repo_key: &str, branch: &str) -> String {
    session_name(repo_key, branch)
}

pub fn codium_state_root() -> PathBuf {
    let xdg = std::env::var("XDG_STATE_HOME").ok();
    let home = std::env::var("HOME").ok();
    codium_state_root_from(xdg.as_deref(), home.as_deref())
}

fn codium_state_root_from(xdg_state_home: Option<&str>, home: Option<&str>) -> PathBuf {
    if let Some(value) = xdg_state_home
        && !value.trim().is_empty()
    {
        return PathBuf::from(value).join("task").join("codium");
    }

    let home = home.unwrap_or("/tmp");
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
    use super::{codium_state_root_from, key, user_data_dir};

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

    #[test]
    fn codium_state_root_uses_xdg_state_home_when_set() {
        let root = codium_state_root_from(Some("/tmp/custom-state"), Some("/home/ignored"));
        assert_eq!(
            root,
            std::path::PathBuf::from("/tmp/custom-state/task/codium")
        );
    }

    #[test]
    fn codium_state_root_falls_back_to_home_dot_local_state() {
        let root = codium_state_root_from(None, Some("/home/testuser"));
        assert_eq!(
            root,
            std::path::PathBuf::from("/home/testuser/.local/state/task/codium")
        );
    }

    #[test]
    fn codium_state_root_ignores_blank_xdg_and_falls_back_to_home() {
        let root = codium_state_root_from(Some("  "), Some("/home/testuser"));
        assert_eq!(
            root,
            std::path::PathBuf::from("/home/testuser/.local/state/task/codium")
        );
    }

    #[test]
    fn codium_state_root_uses_tmp_when_both_absent() {
        let root = codium_state_root_from(None, None);
        assert_eq!(
            root,
            std::path::PathBuf::from("/tmp/.local/state/task/codium")
        );
    }
}
