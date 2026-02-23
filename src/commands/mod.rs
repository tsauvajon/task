pub mod bootstrap;
pub mod check;
pub mod clone;
pub mod complete;
pub mod completions;
pub mod doctor;
pub mod finish;
pub mod list;
pub mod open;
pub mod park;
pub mod path;
pub mod prune;
pub mod rebase;
mod shared;
pub mod start;
pub mod ui;
pub mod worktrees;

use crate::command_line::{Cli, Command};
use crate::workspace_paths::WorkspacePaths;

use shared::*;

pub fn run(cli: Cli) -> Result<(), String> {
    let layout = WorkspacePaths::new(default_dev_root());

    match cli.command {
        None => ui::run(&layout, None),
        Some(Command::Bootstrap) => bootstrap::run(&layout),
        Some(Command::Doctor) => doctor::run(&layout),
        Some(Command::Clone { repo_url, repo_key }) => clone::run(&layout, &repo_url, repo_key),
        Some(Command::Start {
            repo,
            branch,
            base_ref,
        }) => start::run(&layout, &repo, &branch, base_ref.as_deref()),
        Some(Command::Open { repo, branch }) => {
            open::run(&layout, repo.as_deref(), branch.as_deref())
        }
        Some(Command::Park) => park::run(&layout),
        Some(Command::Path { repo, branch }) => {
            path::run(&layout, repo.as_deref(), branch.as_deref())
        }
        Some(Command::List { repo }) => list::run(&layout, repo.as_deref()),
        Some(Command::Ui { repo }) => ui::run(&layout, repo.as_deref()),
        Some(Command::Worktrees { repo }) => worktrees::run(&layout, repo.as_deref()),
        Some(Command::Finish {
            repo,
            branch,
            force,
        }) => finish::run(&layout, repo.as_deref(), branch.as_deref(), force),
        Some(Command::Prune { repo }) => prune::run(&layout, repo.as_deref()),
        Some(Command::Check { worktree_path }) => check::run(worktree_path.as_deref()),
        Some(Command::Rebase { args }) => rebase::run(&layout, &args),
        Some(Command::Completions { shell }) => completions::run(shell),
        Some(Command::Complete { words }) => complete::run(&layout, &words),
    }
}
