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
    use super::{codium_state_root, key, user_data_dir};

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
        let prev = std::env::var_os("XDG_STATE_HOME");
        // SAFETY: single-threaded test binary section; no other thread reads this var.
        unsafe {
            std::env::set_var("XDG_STATE_HOME", "/tmp/custom-state");
        }
        let root = codium_state_root();
        unsafe {
            match prev {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
        assert_eq!(
            root,
            std::path::PathBuf::from("/tmp/custom-state/task/codium")
        );
    }

    #[test]
    fn codium_state_root_falls_back_to_home_dot_local_state() {
        let prev_xdg = std::env::var_os("XDG_STATE_HOME");
        let prev_home = std::env::var_os("HOME");
        // SAFETY: single-threaded test binary section; no other thread reads these vars.
        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
            std::env::set_var("HOME", "/home/testuser");
        }

        let root = codium_state_root();

        // Restore
        unsafe {
            match prev_xdg {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }

        assert_eq!(
            root,
            std::path::PathBuf::from("/home/testuser/.local/state/task/codium")
        );
    }
}
