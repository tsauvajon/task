use clap::{Parser, Subcommand, ValueEnum};
use detach::DetachCommand;

use crate::{
    error::{Error, Result},
    runtime::environment::RuntimeEnvironment,
};

pub mod complete;
pub mod completions;
pub mod coverage;
pub mod detach;
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

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "task", about = "Task workflow helper", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    #[command(about = "Check toolchain/workspace health")]
    Doctor,
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
        #[arg(
            long,
            help = "Create the worktree without opening zellij/opencode/codium"
        )]
        no_open: bool,
    },
    #[command(about = "Re-open a parked task")]
    Open {
        repo: Option<String>,
        branch: Option<String>,
    },
    #[command(about = "Park current task (stop zellij session)")]
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
    #[command(about = "Remove task worktrees by task query")]
    Finish {
        #[arg(value_name = "TASK")]
        tasks: Vec<String>,
        #[arg(long)]
        force: bool,
    },
    #[command(about = "Run Rust test coverage via cargo-llvm-cov")]
    Coverage { worktree_path: Option<String> },
    #[command(about = "Rebase task branch onto a base ref")]
    Rebase { args: Vec<String> },
    #[command(about = "Manage detached worktrees (default branch, or pinned via [[detached]])")]
    Detach {
        #[command(subcommand)]
        command: DetachCommand,
    },
    #[command(about = "Generate shell completion scripts")]
    Completions { shell: CompletionShell },
    #[command(name = "__complete", hide = true, trailing_var_arg = true)]
    Complete {
        #[arg(allow_hyphen_values = true)]
        words: Vec<String>,
    },
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
    #[command(about = "Prune stale worktree metadata")]
    Prune { repo: Option<String> },
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
    let Some(command) = command else {
        return ui::run(&context, None);
    };

    match command {
        Command::Doctor => doctor::run(&context),
        Command::Start {
            repo,
            branch,
            base_ref,
            no_open,
        } => start::run(&context, &repo, &branch, base_ref.as_deref(), no_open),
        Command::Open { repo, branch } => open::run(&context, repo.as_deref(), branch.as_deref()),
        Command::Park => park::run(&context),
        Command::Path { repo, branch } => path::run(&context, repo.as_deref(), branch.as_deref()),
        Command::Finish { tasks, force } => finish::run(&context, &tasks, force),
        Command::Coverage { worktree_path } => coverage::run(&context, worktree_path.as_deref()),
        Command::Rebase { args } => rebase::run(&context, &args),
        passthrough @ (Command::Repo { .. }
        | Command::List { .. }
        | Command::Ui { .. }
        | Command::Detach { .. }) => run_context_passthrough(&context, passthrough),
        Command::Completions { .. } | Command::Complete { .. } => Err(Error::failed(
            "completion commands must be handled before runtime initialization",
        )),
    }
}

fn run_context_passthrough(context: &RuntimeEnvironment, command: Command) -> Result<()> {
    match command {
        Command::List { repo } => list::run(context, repo.as_deref()),
        Command::Ui { repo } => ui::run(context, repo.as_deref()),
        Command::Repo { command } => repo::run(context, command),
        Command::Detach { command } => detach::run(context, command),
        Command::Doctor
        | Command::Start { .. }
        | Command::Open { .. }
        | Command::Park
        | Command::Path { .. }
        | Command::Finish { .. }
        | Command::Coverage { .. }
        | Command::Rebase { .. }
        | Command::Completions { .. }
        | Command::Complete { .. } => Err(Error::failed("internal command dispatch mismatch")),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, CompletionShell, DetachCommand, RepoCommand};

    mod cli_parsing {
        use super::*;

        #[test]
        fn parses_start_command() {
            let cli = Cli::parse_from(["task", "start", "goto", "bump-deps"]);
            assert_eq!(
                cli.command,
                Some(Command::Start {
                    repo: "goto".to_owned(),
                    branch: "bump-deps".to_owned(),
                    base_ref: None,
                    no_open: false,
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
                    repo: Some("goto".to_owned()),
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
                    args: vec!["goto".to_owned(), "bump-deps".to_owned()],
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
                    args: vec!["bump-deps".to_owned()],
                })
            );
        }

        #[test]
        fn parses_finish_with_force_only() {
            let cli = Cli::parse_from(["task", "finish", "--force"]);
            assert_eq!(
                cli.command,
                Some(Command::Finish {
                    tasks: Vec::new(),
                    force: true,
                })
            );
        }

        #[test]
        fn parses_repo_prune_without_repo() {
            let cli = Cli::parse_from(["task", "repo", "prune"]);
            assert_eq!(
                cli.command,
                Some(Command::Repo {
                    command: RepoCommand::Prune { repo: None },
                })
            );
        }

        #[test]
        fn parses_doctor_command() {
            let cli = Cli::parse_from(["task", "doctor"]);
            assert_eq!(cli.command, Some(Command::Doctor));
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
                    repo: Some("goto".to_owned()),
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
                        repo_url: "git@github.com:me/app.git".to_owned(),
                        repo_key: Some("app".to_owned()),
                    },
                })
            );
        }

        #[test]
        fn allows_no_command() {
            let cli = Cli::parse_from(["task"]);
            assert_eq!(cli.command, None);
        }

        #[test]
        fn parses_detach_add() {
            let cli = Cli::parse_from(["task", "detach", "add", "myrepo"]);
            assert_eq!(
                cli.command,
                Some(Command::Detach {
                    command: DetachCommand::Add {
                        repo: "myrepo".to_owned(),
                    },
                })
            );
        }

        #[test]
        fn parses_detach_update_with_repo() {
            let cli = Cli::parse_from(["task", "detach", "update", "myrepo"]);
            assert_eq!(
                cli.command,
                Some(Command::Detach {
                    command: DetachCommand::Update {
                        repo: Some("myrepo".to_owned()),
                    },
                })
            );
        }

        #[test]
        fn parses_detach_update_without_repo() {
            let cli = Cli::parse_from(["task", "detach", "update"]);
            assert_eq!(
                cli.command,
                Some(Command::Detach {
                    command: DetachCommand::Update { repo: None },
                })
            );
        }

        #[test]
        fn parses_detach_remove() {
            let cli = Cli::parse_from(["task", "detach", "remove", "myrepo"]);
            assert_eq!(
                cli.command,
                Some(Command::Detach {
                    command: DetachCommand::Remove {
                        repo: "myrepo".to_owned(),
                        force: false,
                    },
                })
            );
        }

        #[test]
        fn parses_detach_remove_force() {
            let cli = Cli::parse_from(["task", "detach", "remove", "myrepo", "--force"]);
            assert_eq!(
                cli.command,
                Some(Command::Detach {
                    command: DetachCommand::Remove {
                        repo: "myrepo".to_owned(),
                        force: true,
                    },
                })
            );
        }

        #[test]
        fn parses_detach_list() {
            let cli = Cli::parse_from(["task", "detach", "list"]);
            assert_eq!(
                cli.command,
                Some(Command::Detach {
                    command: DetachCommand::List,
                })
            );
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
                    repo: "goto".to_owned(),
                    branch: "bump-deps".to_owned(),
                    base_ref: Some("origin/main".to_owned()),
                    no_open: false,
                })
            );
        }

        #[test]
        fn parses_start_with_no_open_flag() {
            let cli = Cli::parse_from(["task", "start", "goto", "bump-deps", "--no-open"]);
            assert_eq!(
                cli.command,
                Some(Command::Start {
                    repo: "goto".to_owned(),
                    branch: "bump-deps".to_owned(),
                    base_ref: None,
                    no_open: true,
                })
            );
        }

        #[test]
        fn parses_start_with_base_ref_and_no_open_flag() {
            let cli = Cli::parse_from([
                "task",
                "start",
                "goto",
                "bump-deps",
                "origin/main",
                "--no-open",
            ]);
            assert_eq!(
                cli.command,
                Some(Command::Start {
                    repo: "goto".to_owned(),
                    branch: "bump-deps".to_owned(),
                    base_ref: Some("origin/main".to_owned()),
                    no_open: true,
                })
            );
        }

        #[test]
        fn parses_list_with_repo() {
            let cli = Cli::parse_from(["task", "list", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::List {
                    repo: Some("goto".to_owned()),
                })
            );
        }

        #[test]
        fn parses_list_without_repo() {
            let cli = Cli::parse_from(["task", "list"]);
            assert_eq!(cli.command, Some(Command::List { repo: None }));
        }

        #[test]
        fn parses_path_with_repo_and_branch() {
            let cli = Cli::parse_from(["task", "path", "goto", "bump-deps"]);
            assert_eq!(
                cli.command,
                Some(Command::Path {
                    repo: Some("goto".to_owned()),
                    branch: Some("bump-deps".to_owned()),
                })
            );
        }

        #[test]
        fn parses_finish_with_all_args() {
            let cli = Cli::parse_from(["task", "finish", "goto", "bump-deps", "--force"]);
            assert_eq!(
                cli.command,
                Some(Command::Finish {
                    tasks: vec!["goto".to_owned(), "bump-deps".to_owned()],
                    force: true,
                })
            );
        }

        #[test]
        fn parses_repo_prune_with_repo() {
            let cli = Cli::parse_from(["task", "repo", "prune", "goto"]);
            assert_eq!(
                cli.command,
                Some(Command::Repo {
                    command: RepoCommand::Prune {
                        repo: Some("goto".to_owned()),
                    },
                })
            );
        }

        #[test]
        fn parses_coverage_with_path() {
            let cli = Cli::parse_from(["task", "coverage", "/tmp/some/path"]);
            assert_eq!(
                cli.command,
                Some(Command::Coverage {
                    worktree_path: Some("/tmp/some/path".to_owned()),
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
                        repo_url: "git@github.com:me/app.git".to_owned(),
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
