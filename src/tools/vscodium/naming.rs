use std::path::PathBuf;

use crate::tools::zellij::naming::session_name;

/// VSCodium profile key derived from repo key and stable worktree identity.
///
/// Uses the same sanitization as Zellij session names so that both tools
/// agree on the identity even after a Git branch rename.
#[must_use]
pub fn key(repo_key: &str, worktree_name: &str) -> String {
    session_name(repo_key, worktree_name)
}

#[must_use]
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

#[must_use]
pub fn user_data_dir(repo_key: &str, worktree_name: &str) -> PathBuf {
    codium_state_root().join(key(repo_key, worktree_name))
}

#[cfg(test)]
mod tests {
    use super::{codium_state_root_from, key, user_data_dir};

    mod key {
        use super::*;

        #[test]
        fn matches_zellij_session_name() {
            assert_eq!(
                key("github.com/acme/tool", "feat/test.1"),
                "github_com_acme_tool-feat_test_1"
            );
        }

        #[test]
        fn produces_non_empty_string() {
            assert!(!key("github.com/acme/repo", "branch").is_empty());
        }

        #[test]
        fn empty_inputs_produce_just_separator() {
            assert_eq!(key("", ""), "-");
        }
    }

    mod codium_state_root {
        use super::*;

        #[test]
        fn uses_xdg_state_home_when_set() {
            let root = codium_state_root_from(Some("/tmp/custom-state"), Some("/home/ignored"));
            assert_eq!(
                root,
                std::path::PathBuf::from("/tmp/custom-state/task/codium")
            );
        }

        #[test]
        fn falls_back_to_home_dot_local_state() {
            let root = codium_state_root_from(None, Some("/home/testuser"));
            assert_eq!(
                root,
                std::path::PathBuf::from("/home/testuser/.local/state/task/codium")
            );
        }

        #[test]
        fn ignores_blank_xdg_and_falls_back_to_home() {
            let root = codium_state_root_from(Some("  "), Some("/home/testuser"));
            assert_eq!(
                root,
                std::path::PathBuf::from("/home/testuser/.local/state/task/codium")
            );
        }

        #[test]
        fn uses_tmp_when_both_absent() {
            let root = codium_state_root_from(None, None);
            assert_eq!(
                root,
                std::path::PathBuf::from("/tmp/.local/state/task/codium")
            );
        }

        #[test]
        fn path_ends_with_task_codium() {
            let root = codium_state_root_from(None, Some("/home/user"));
            assert!(
                root.ends_with("task/codium"),
                "root should end with task/codium, got: {root:?}"
            );
        }
    }

    mod user_data_dir {
        use super::*;

        #[test]
        fn contains_sanitized_key() {
            let dir = user_data_dir("github.com/acme/tool", "feat/test.1");
            assert!(dir.ends_with("codium/github_com_acme_tool-feat_test_1"));
        }

        #[test]
        fn is_under_codium_state_root() {
            let dir = user_data_dir("github.com/acme/tool", "main");
            let dir_str = dir.to_string_lossy();
            assert!(
                dir_str.contains("codium"),
                "user_data_dir should contain 'codium' segment"
            );
        }
    }
}
