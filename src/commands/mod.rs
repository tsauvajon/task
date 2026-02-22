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
mod shared;
pub mod start;
pub mod ui;
pub mod worktrees;

use crate::cli::{Cli, Commands};
use crate::layout::Layout;

use shared::*;

pub fn run(cli: Cli) -> Result<(), String> {
    let layout = Layout::new(default_dev_root());

    match cli.command {
        None => ui::run(&layout, None),
        Some(Commands::Bootstrap) => bootstrap::run(&layout),
        Some(Commands::Doctor) => doctor::run(&layout),
        Some(Commands::Clone { repo_url, repo_key }) => clone::run(&layout, &repo_url, repo_key),
        Some(Commands::Start {
            repo,
            branch,
            base_ref,
        }) => start::run(&layout, &repo, &branch, base_ref.as_deref()),
        Some(Commands::Open { repo, branch }) => {
            open::run(&layout, repo.as_deref(), branch.as_deref())
        }
        Some(Commands::Park) => park::run(&layout),
        Some(Commands::Path { repo, branch }) => {
            path::run(&layout, repo.as_deref(), branch.as_deref())
        }
        Some(Commands::List { repo }) => list::run(&layout, repo.as_deref()),
        Some(Commands::Ui { repo }) => ui::run(&layout, repo.as_deref()),
        Some(Commands::Worktrees { repo }) => worktrees::run(&layout, repo.as_deref()),
        Some(Commands::Finish {
            repo,
            branch,
            force,
        }) => finish::run(&layout, repo.as_deref(), branch.as_deref(), force),
        Some(Commands::Prune { repo }) => prune::run(&layout, repo.as_deref()),
        Some(Commands::Check { worktree_path }) => check::run(worktree_path.as_deref()),
        Some(Commands::Completions { shell }) => completions::run(shell),
        Some(Commands::Complete { words }) => complete::run(&layout, &words),
    }
}
