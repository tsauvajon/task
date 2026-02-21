pub mod app;
pub mod cli;
pub mod core;

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};
    use crate::core::{
        Layout, ResolveResult, TaskRow, WorktreeEntry, branch_from_ref, branch_from_worktree_path,
        build_task_rows, normalize_repo_key, parse_worktree_porcelain, repo_key_from_common_dir,
        resolve_repo_key, session_name_for,
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
    fn resolve_repo_key_by_short_name_when_unique() {
        let keys = vec![
            "github.com/tsauvajon/goto".to_string(),
            "github.com/tsauvajon/task".to_string(),
        ];

        let resolved = resolve_repo_key("goto", &keys);
        assert_eq!(
            resolved,
            ResolveResult::Resolved("github.com/tsauvajon/goto".to_string())
        );
    }

    #[test]
    fn resolve_repo_key_reports_ambiguity() {
        let keys = vec![
            "github.com/tsauvajon/goto".to_string(),
            "github.com/example/goto".to_string(),
        ];

        let resolved = resolve_repo_key("goto", &keys);
        assert_eq!(
            resolved,
            ResolveResult::Ambiguous(vec![
                "github.com/example/goto".to_string(),
                "github.com/tsauvajon/goto".to_string(),
            ])
        );
    }

    #[test]
    fn session_name_is_sanitized() {
        assert_eq!(
            session_name_for("github.com/tsauvajon/goto", "feat/test.1"),
            "github_com_tsauvajon_goto-feat_test_1"
        );
    }

    #[test]
    fn layout_builds_expected_paths() {
        let layout = Layout::new("/home/thomas/dev");
        assert_eq!(
            layout
                .repo_gitdir_path("github.com/tsauvajon/goto")
                .display()
                .to_string(),
            "/home/thomas/dev/repos/github.com/tsauvajon/goto.git"
        );
        assert_eq!(
            layout
                .worktree_path("github.com/tsauvajon/goto", "bump-deps")
                .display()
                .to_string(),
            "/home/thomas/dev/wt/github.com/tsauvajon/goto/bump-deps"
        );
    }

    #[test]
    fn parse_worktree_porcelain_collects_entries() {
        let text = "worktree /mnt/linux/dev/repos/github.com/tsauvajon/task.git\n\
bare\n\
\n\
worktree /mnt/linux/dev/wt/github.com/tsauvajon/task/rewrite-in-rust\n\
HEAD 0123456789abcdef\n\
branch refs/heads/rewrite-in-rust\n\n";

        let entries = parse_worktree_porcelain(text);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_bare);
        assert_eq!(
            entries[1],
            WorktreeEntry {
                path: "/mnt/linux/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                branch_ref: Some("refs/heads/rewrite-in-rust".to_string()),
                is_bare: false,
            }
        );
    }

    #[test]
    fn branch_from_worktree_path_supports_nested_branch_names() {
        let branch = branch_from_worktree_path(
            "github.com/tsauvajon/task",
            "/mnt/linux/dev/wt/github.com/tsauvajon/task/feat/rewrite/rust",
        );
        assert_eq!(branch, Some("feat/rewrite/rust".to_string()));
    }

    #[test]
    fn repo_key_from_common_dir_extracts_key() {
        let key = repo_key_from_common_dir("/mnt/linux/dev/repos/github.com/tsauvajon/task.git");
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
                path: "/mnt/linux/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                branch_ref: Some("refs/heads/rewrite-in-rust".to_string()),
                is_bare: false,
            },
            WorktreeEntry {
                path: "/mnt/linux/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
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
                    path: "/mnt/linux/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                },
                TaskRow {
                    status: "parked".to_string(),
                    repo: "github.com/tsauvajon/task".to_string(),
                    branch: "bump-deps".to_string(),
                    path: "/mnt/linux/dev/wt/github.com/tsauvajon/task/bump-deps".into(),
                },
            ]
        );
    }

    #[test]
    fn cli_parses_start_command() {
        let cli = Cli::parse_from(["task", "start", "goto", "bump-deps"]);
        assert_eq!(
            cli.command,
            Commands::Start {
                repo: "goto".to_string(),
                branch: "bump-deps".to_string(),
                base_ref: None,
            }
        );
    }

    #[test]
    fn cli_parses_park_command_without_args() {
        let cli = Cli::parse_from(["task", "park"]);
        assert_eq!(cli.command, Commands::Park);
    }
}
