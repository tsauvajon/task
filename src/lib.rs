pub mod app;
pub mod cli;
pub mod commands;
pub mod layout;
pub mod repo_key;
pub mod session;
pub mod worktree;

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands, CompletionShell};
    use crate::layout::Layout;
    use crate::repo_key::{ResolveResult, normalize_repo_key, resolve_repo_key};
    use crate::session::session_name_for;
    use crate::worktree::{
        TaskRow, WorktreeEntry, branch_from_ref, branch_from_worktree_path, build_task_rows,
        parse_worktree_porcelain, repo_key_from_common_dir,
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
        let root = std::env::temp_dir().join("task-tests-dev-root");
        let layout = Layout::new(&root);
        assert_eq!(
            layout.repo_gitdir_path("github.com/tsauvajon/goto"),
            root.join("repos/github.com/tsauvajon/goto.git")
        );
        assert_eq!(
            layout.worktree_path("github.com/tsauvajon/goto", "bump-deps"),
            root.join("wt/github.com/tsauvajon/goto/bump-deps")
        );
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

    #[test]
    fn cli_parses_start_command() {
        let cli = Cli::parse_from(["task", "start", "goto", "bump-deps"]);
        assert_eq!(
            cli.command,
            Some(Commands::Start {
                repo: "goto".to_string(),
                branch: "bump-deps".to_string(),
                base_ref: None,
            })
        );
    }

    #[test]
    fn cli_parses_park_command_without_args() {
        let cli = Cli::parse_from(["task", "park"]);
        assert_eq!(cli.command, Some(Commands::Park));
    }

    #[test]
    fn cli_parses_completions_command() {
        let cli = Cli::parse_from(["task", "completions", "fish"]);
        assert_eq!(
            cli.command,
            Some(Commands::Completions {
                shell: CompletionShell::Fish,
            })
        );
    }

    #[test]
    fn cli_parses_open_without_args() {
        let cli = Cli::parse_from(["task", "open"]);
        assert_eq!(
            cli.command,
            Some(Commands::Open {
                repo: None,
                branch: None,
            })
        );
    }

    #[test]
    fn cli_parses_open_with_repo_only() {
        let cli = Cli::parse_from(["task", "open", "goto"]);
        assert_eq!(
            cli.command,
            Some(Commands::Open {
                repo: Some("goto".to_string()),
                branch: None,
            })
        );
    }

    #[test]
    fn cli_parses_rebase_command() {
        let cli = Cli::parse_from(["task", "rebase", "goto", "bump-deps"]);
        assert_eq!(
            cli.command,
            Some(Commands::Rebase {
                args: vec!["goto".to_string(), "bump-deps".to_string()],
            })
        );
    }

    #[test]
    fn cli_parses_rebase_without_args() {
        let cli = Cli::parse_from(["task", "rebase"]);
        assert_eq!(
            cli.command,
            Some(Commands::Rebase { args: Vec::new() })
        );
    }

    #[test]
    fn cli_parses_rebase_with_query_arg() {
        let cli = Cli::parse_from(["task", "rebase", "bump-deps"]);
        assert_eq!(
            cli.command,
            Some(Commands::Rebase {
                args: vec!["bump-deps".to_string()],
            })
        );
    }

    #[test]
    fn cli_parses_finish_with_force_only() {
        let cli = Cli::parse_from(["task", "finish", "--force"]);
        assert_eq!(
            cli.command,
            Some(Commands::Finish {
                repo: None,
                branch: None,
                force: true,
            })
        );
    }

    #[test]
    fn cli_parses_prune_without_repo() {
        let cli = Cli::parse_from(["task", "prune"]);
        assert_eq!(cli.command, Some(Commands::Prune { repo: None }));
    }

    #[test]
    fn cli_parses_check_command() {
        let cli = Cli::parse_from(["task", "check"]);
        assert_eq!(
            cli.command,
            Some(Commands::Check {
                worktree_path: None,
            })
        );
    }

    #[test]
    fn cli_parses_ui_command() {
        let cli = Cli::parse_from(["task", "ui", "goto"]);
        assert_eq!(
            cli.command,
            Some(Commands::Ui {
                repo: Some("goto".to_string()),
            })
        );
    }

    #[test]
    fn cli_allows_no_command() {
        let cli = Cli::parse_from(["task"]);
        assert_eq!(cli.command, None);
    }
}
