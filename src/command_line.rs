use clap::{Parser, Subcommand, ValueEnum};

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
    #[command(about = "Check toolchain and workspace health")]
    Doctor,
    #[command(about = "Clone bare repo into ~/dev/repos")]
    Clone {
        repo_url: String,
        repo_key: Option<String>,
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
    #[command(about = "Interactive task dashboard")]
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
    #[command(about = "Rebase task branch onto a base ref")]
    Rebase { args: Vec<String> },
    #[command(about = "Generate shell completion scripts")]
    Completions { shell: CompletionShell },
    #[command(name = "__complete", hide = true, trailing_var_arg = true)]
    Complete { words: Vec<String> },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Fish,
    Zsh,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, CompletionShell};

    #[test]
    fn cli_parses_start_command() {
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
    fn cli_parses_park_command_without_args() {
        let cli = Cli::parse_from(["task", "park"]);
        assert_eq!(cli.command, Some(Command::Park));
    }

    #[test]
    fn cli_parses_completions_command() {
        let cli = Cli::parse_from(["task", "completions", "fish"]);
        assert_eq!(
            cli.command,
            Some(Command::Completions {
                shell: CompletionShell::Fish,
            })
        );
    }

    #[test]
    fn cli_parses_open_without_args() {
        let cli = Cli::parse_from(["task", "open"]);
        assert_eq!(
            cli.command,
            Some(Command::Open {
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
            Some(Command::Open {
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
            Some(Command::Rebase {
                args: vec!["goto".to_string(), "bump-deps".to_string()],
            })
        );
    }

    #[test]
    fn cli_parses_rebase_without_args() {
        let cli = Cli::parse_from(["task", "rebase"]);
        assert_eq!(cli.command, Some(Command::Rebase { args: Vec::new() }));
    }

    #[test]
    fn cli_parses_rebase_with_query_arg() {
        let cli = Cli::parse_from(["task", "rebase", "bump-deps"]);
        assert_eq!(
            cli.command,
            Some(Command::Rebase {
                args: vec!["bump-deps".to_string()],
            })
        );
    }

    #[test]
    fn cli_parses_finish_with_force_only() {
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
    fn cli_parses_prune_without_repo() {
        let cli = Cli::parse_from(["task", "prune"]);
        assert_eq!(cli.command, Some(Command::Prune { repo: None }));
    }

    #[test]
    fn cli_parses_check_command() {
        let cli = Cli::parse_from(["task", "check"]);
        assert_eq!(
            cli.command,
            Some(Command::Check {
                worktree_path: None,
            })
        );
    }

    #[test]
    fn cli_parses_ui_command() {
        let cli = Cli::parse_from(["task", "ui", "goto"]);
        assert_eq!(
            cli.command,
            Some(Command::Ui {
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
