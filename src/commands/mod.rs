use clap::{Parser, Subcommand, ValueEnum};

pub mod bootstrap;
pub mod check;
pub mod clone;
pub mod complete;
pub mod completions;
pub mod coverage;
pub mod doctor;
pub mod finish;
pub mod list;
pub mod open;
pub mod park;
pub mod path;
pub mod prune;
pub mod rebase;
pub mod repo;
pub mod start;
pub mod ui;
pub mod worktrees;

use crate::{
    error::Result,
    runtime::{environment::RuntimeEnvironment, setup},
};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "task", about = "Task workflow helper")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    #[command(about = "Prepare workspace and asdf Node plugin")]
    Bootstrap,
    #[command(about = "Check toolchain/workspace health and optionally apply fixes")]
    Doctor {
        #[arg(long)]
        fix: bool,
    },
    #[command(about = "Manage repositories")]
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },
    #[command(about = "Create/open a task worktree")]
    Start {
        repo: String,
        branch: String,
        base_ref: Option<String>,
    },
    #[command(about = "Re-open a parked task")]
    Open {
        repo: Option<String>,
        branch: Option<String>,
    },
    #[command(about = "Park current task (stop tmux session)")]
    Park,
    #[command(about = "Print worktree path for a task")]
    Path {
        repo: Option<String>,
        branch: Option<String>,
    },
    #[command(about = "List tasks with open/parked status")]
    List { repo: Option<String> },
    #[command(about = "Open interactive TUI")]
    Ui { repo: Option<String> },
    #[command(about = "Show raw git worktree list output")]
    Worktrees { repo: Option<String> },
    #[command(about = "Remove a task worktree")]
    Finish {
        repo: Option<String>,
        branch: Option<String>,
        #[arg(long)]
        force: bool,
    },
    #[command(about = "Prune stale worktree metadata")]
    Prune { repo: Option<String> },
    #[command(about = "Run project checks for current task", alias = "done")]
    Check { worktree_path: Option<String> },
    #[command(about = "Run Rust test coverage via cargo-llvm-cov")]
    Coverage { worktree_path: Option<String> },
    #[command(about = "Rebase task branch onto a base ref")]
    Rebase { args: Vec<String> },
    #[command(about = "Generate shell completion scripts")]
    Completions { shell: CompletionShell },
    #[command(name = "__complete", hide = true, trailing_var_arg = true)]
    Complete { words: Vec<String> },
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum RepoCommand {
    #[command(about = "List repositories")]
    List,
    #[command(about = "Clone bare repo into configured repos directory")]
    Clone {
        repo_url: String,
        repo_key: Option<String>,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Fish,
    Zsh,
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Completions { shell }) => completions::run(shell),
        Some(Command::Complete { words }) => {
            let context = RuntimeEnvironment::try_new_if_configured()?;
            complete::run(context.as_ref(), &words)
        }
        command => run_with_context(command),
    }
}

fn run_with_context(command: Option<Command>) -> Result<()> {
    let context = RuntimeEnvironment::new()?;

    if should_auto_onboard(command.as_ref()) {
        setup::ensure_first_run_setup(&context)?;
    }

    match command {
        None => ui::run(&context, None),
        Some(Command::Bootstrap) => bootstrap::run(&context),
        Some(Command::Doctor { fix }) => doctor::run(&context, fix),
        Some(Command::Repo { command }) => repo::run(&context, command),
        Some(Command::Start {
            repo,
            branch,
            base_ref,
        }) => start::run(&context, &repo, &branch, base_ref.as_deref()),
        Some(Command::Open { repo, branch }) => {
            open::run(&context, repo.as_deref(), branch.as_deref())
        }
        Some(Command::Park) => park::run(&context),
        Some(Command::Path { repo, branch }) => {
            path::run(&context, repo.as_deref(), branch.as_deref())
        }
        Some(Command::List { repo }) => list::run(&context, repo.as_deref()),
        Some(Command::Ui { repo }) => ui::run(&context, repo.as_deref()),
        Some(Command::Worktrees { repo }) => worktrees::run(&context, repo.as_deref()),
        Some(Command::Finish {
            repo,
            branch,
            force,
        }) => finish::run(&context, repo.as_deref(), branch.as_deref(), force),
        Some(Command::Prune { repo }) => prune::run(&context, repo.as_deref()),
        Some(Command::Check { worktree_path }) => check::run(&context, worktree_path.as_deref()),
        Some(Command::Coverage { worktree_path }) => {
            coverage::run(&context, worktree_path.as_deref())
        }
        Some(Command::Rebase { args }) => rebase::run(&context, &args),
        Some(Command::Completions { .. }) | Some(Command::Complete { .. }) => {
            unreachable!("completion commands are handled before runtime initialization")
        }
    }
}

fn should_auto_onboard(command: Option<&Command>) -> bool {
    !matches!(
        command,
        Some(Command::Bootstrap) | Some(Command::Doctor { .. })
    )
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, CompletionShell, RepoCommand, should_auto_onboard};

    mod cli_parsing {
        use super::*;

        #[test]
        fn parses_start_command() {
            let cli = Cli::parse_from(["task", "start", "goto", "bump-deps"]);
            assert_eq!(
                cli.command,
                Some(Command::Start {
                    repo: "goto".to_string(),
                    branch: "bump-deps".to_string(),
                    base_ref: None,
                })
            );
        }

        #[test]
        fn parses_park_command_without_args() {
            let cli = Cli::parse_from(["task", "park"]);
            assert_eq!(cli.command, Some(Command::Park));
        }

        #[test]
        fn parses_completions_command() {
            let cli = Cli::parse_from(["task", "completions", "fish"]);
            assert_eq!(
                cli.command,
                Some(Command::Completions {
                    shell: CompletionShell::Fish,
                })
            );
        }

        #[test]
        fn parses_open_without_args() {
            let cli = Cli::parse_from(["task", "open"]);
            assert_eq!(
                cli.command,
                Some(Command::Open {
                    repo: None,
                    branch: None
                })
            );
        }

        #[test]
        fn parses_open_with_repo_only() {
            let cli = Cli::parse_from(["task", "open", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::Open {
                    repo: Some("goto".to_string()),
                    branch: None,
                })
            );
        }

        #[test]
        fn parses_rebase_command() {
            let cli = Cli::parse_from(["task", "rebase", "goto", "bump-deps"]);
            assert_eq!(
                cli.command,
                Some(Command::Rebase {
                    args: vec!["goto".to_string(), "bump-deps".to_string()],
                })
            );
        }

        #[test]
        fn parses_rebase_without_args() {
            let cli = Cli::parse_from(["task", "rebase"]);
            assert_eq!(cli.command, Some(Command::Rebase { args: Vec::new() }));
        }

        #[test]
        fn parses_rebase_with_query_arg() {
            let cli = Cli::parse_from(["task", "rebase", "bump-deps"]);
            assert_eq!(
                cli.command,
                Some(Command::Rebase {
                    args: vec!["bump-deps".to_string()],
                })
            );
        }

        #[test]
        fn parses_finish_with_force_only() {
            let cli = Cli::parse_from(["task", "finish", "--force"]);
            assert_eq!(
                cli.command,
                Some(Command::Finish {
                    repo: None,
                    branch: None,
                    force: true,
                })
            );
        }

        #[test]
        fn parses_prune_without_repo() {
            let cli = Cli::parse_from(["task", "prune"]);
            assert_eq!(cli.command, Some(Command::Prune { repo: None }));
        }

        #[test]
        fn parses_check_command() {
            let cli = Cli::parse_from(["task", "check"]);
            assert_eq!(
                cli.command,
                Some(Command::Check {
                    worktree_path: None
                })
            );
        }

        #[test]
        fn parses_doctor_fix_flag() {
            let cli = Cli::parse_from(["task", "doctor", "--fix"]);
            assert_eq!(cli.command, Some(Command::Doctor { fix: true }));
        }

        #[test]
        fn parses_doctor_without_fix_flag() {
            let cli = Cli::parse_from(["task", "doctor"]);
            assert_eq!(cli.command, Some(Command::Doctor { fix: false }));
        }

        #[test]
        fn parses_coverage_command() {
            let cli = Cli::parse_from(["task", "coverage"]);
            assert_eq!(
                cli.command,
                Some(Command::Coverage {
                    worktree_path: None
                })
            );
        }

        #[test]
        fn parses_ui_command() {
            let cli = Cli::parse_from(["task", "ui", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::Ui {
                    repo: Some("goto".to_string()),
                })
            );
        }

        #[test]
        fn parses_repo_list_command() {
            let cli = Cli::parse_from(["task", "repo", "list"]);
            assert_eq!(
                cli.command,
                Some(Command::Repo {
                    command: RepoCommand::List,
                })
            );
        }

        #[test]
        fn parses_repo_clone_command() {
            let cli =
                Cli::parse_from(["task", "repo", "clone", "git@github.com:me/app.git", "app"]);
            assert_eq!(
                cli.command,
                Some(Command::Repo {
                    command: RepoCommand::Clone {
                        repo_url: "git@github.com:me/app.git".to_string(),
                        repo_key: Some("app".to_string()),
                    },
                })
            );
        }

        #[test]
        fn allows_no_command() {
            let cli = Cli::parse_from(["task"]);
            assert_eq!(cli.command, None);
        }
    }

    mod auto_onboarding {
        use super::*;

        #[test]
        fn skips_bootstrap_and_doctor() {
            assert!(!should_auto_onboard(Some(&Command::Bootstrap)));
            assert!(!should_auto_onboard(Some(&Command::Doctor { fix: false })));
            assert!(should_auto_onboard(Some(&Command::Start {
                repo: "goto".to_string(),
                branch: "feature".to_string(),
                base_ref: None,
            })));
        }

        #[test]
        fn requires_onboard_for_none_command() {
            assert!(should_auto_onboard(None));
        }

        #[test]
        fn requires_onboard_for_list() {
            assert!(should_auto_onboard(Some(&Command::List { repo: None })));
        }

        #[test]
        fn requires_onboard_for_park() {
            assert!(should_auto_onboard(Some(&Command::Park)));
        }

        #[test]
        fn requires_onboard_for_finish() {
            assert!(should_auto_onboard(Some(&Command::Finish {
                repo: None,
                branch: None,
                force: false,
            })));
        }

        #[test]
        fn requires_onboard_for_check() {
            assert!(should_auto_onboard(Some(&Command::Check {
                worktree_path: None,
            })));
        }

        #[test]
        fn requires_onboard_for_open() {
            assert!(should_auto_onboard(Some(&Command::Open {
                repo: None,
                branch: None,
            })));
        }

        #[test]
        fn requires_onboard_for_prune() {
            assert!(should_auto_onboard(Some(&Command::Prune { repo: None })));
        }

        #[test]
        fn requires_onboard_for_rebase() {
            assert!(should_auto_onboard(Some(&Command::Rebase { args: vec![] })));
        }

        #[test]
        fn skips_doctor_with_fix_true() {
            assert!(!should_auto_onboard(Some(&Command::Doctor { fix: true })));
        }
    }

    mod cli_parsing_extra {
        use super::*;

        #[test]
        fn parses_start_with_base_ref() {
            let cli = Cli::parse_from(["task", "start", "goto", "bump-deps", "origin/main"]);
            assert_eq!(
                cli.command,
                Some(Command::Start {
                    repo: "goto".to_string(),
                    branch: "bump-deps".to_string(),
                    base_ref: Some("origin/main".to_string()),
                })
            );
        }

        #[test]
        fn parses_list_with_repo() {
            let cli = Cli::parse_from(["task", "list", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::List {
                    repo: Some("goto".to_string()),
                })
            );
        }

        #[test]
        fn parses_list_without_repo() {
            let cli = Cli::parse_from(["task", "list"]);
            assert_eq!(cli.command, Some(Command::List { repo: None }));
        }

        #[test]
        fn parses_worktrees_with_repo() {
            let cli = Cli::parse_from(["task", "worktrees", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::Worktrees {
                    repo: Some("goto".to_string()),
                })
            );
        }

        #[test]
        fn parses_path_with_repo_and_branch() {
            let cli = Cli::parse_from(["task", "path", "goto", "bump-deps"]);
            assert_eq!(
                cli.command,
                Some(Command::Path {
                    repo: Some("goto".to_string()),
                    branch: Some("bump-deps".to_string()),
                })
            );
        }

        #[test]
        fn parses_finish_with_all_args() {
            let cli = Cli::parse_from(["task", "finish", "goto", "bump-deps", "--force"]);
            assert_eq!(
                cli.command,
                Some(Command::Finish {
                    repo: Some("goto".to_string()),
                    branch: Some("bump-deps".to_string()),
                    force: true,
                })
            );
        }

        #[test]
        fn parses_prune_with_repo() {
            let cli = Cli::parse_from(["task", "prune", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::Prune {
                    repo: Some("goto".to_string()),
                })
            );
        }

        #[test]
        fn parses_coverage_with_path() {
            let cli = Cli::parse_from(["task", "coverage", "/tmp/some/path"]);
            assert_eq!(
                cli.command,
                Some(Command::Coverage {
                    worktree_path: Some("/tmp/some/path".to_string()),
                })
            );
        }

        #[test]
        fn parses_check_with_path() {
            let cli = Cli::parse_from(["task", "check", "/tmp/some/path"]);
            assert_eq!(
                cli.command,
                Some(Command::Check {
                    worktree_path: Some("/tmp/some/path".to_string()),
                })
            );
        }

        #[test]
        fn parses_completions_bash() {
            let cli = Cli::parse_from(["task", "completions", "bash"]);
            assert_eq!(
                cli.command,
                Some(Command::Completions {
                    shell: CompletionShell::Bash,
                })
            );
        }

        #[test]
        fn parses_completions_zsh() {
            let cli = Cli::parse_from(["task", "completions", "zsh"]);
            assert_eq!(
                cli.command,
                Some(Command::Completions {
                    shell: CompletionShell::Zsh,
                })
            );
        }

        #[test]
        fn parses_repo_clone_without_explicit_key() {
            let cli = Cli::parse_from(["task", "repo", "clone", "git@github.com:me/app.git"]);
            assert_eq!(
                cli.command,
                Some(Command::Repo {
                    command: RepoCommand::Clone {
                        repo_url: "git@github.com:me/app.git".to_string(),
                        repo_key: None,
                    },
                })
            );
        }

        #[test]
        fn parses_ui_without_repo() {
            let cli = Cli::parse_from(["task", "ui"]);
            assert_eq!(cli.command, Some(Command::Ui { repo: None }));
        }

        #[test]
        fn completion_shell_values_are_distinct() {
            assert_ne!(CompletionShell::Bash, CompletionShell::Fish);
            assert_ne!(CompletionShell::Bash, CompletionShell::Zsh);
            assert_ne!(CompletionShell::Fish, CompletionShell::Zsh);
        }
    }
}
