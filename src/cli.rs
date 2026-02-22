use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "task", about = "Task workflow helper")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Commands {
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
