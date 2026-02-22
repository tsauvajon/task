pub mod bootstrap;
pub mod clean;
pub mod clone;
pub mod completions;
pub mod doctor;
pub mod done;
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
        Some(Commands::Open { repo, branch }) => open::run(&layout, &repo, &branch),
        Some(Commands::Park) => park::run(&layout),
        Some(Commands::Path { repo, branch }) => path::run(&layout, &repo, &branch),
        Some(Commands::List { repo }) => list::run(&layout, repo.as_deref()),
        Some(Commands::Ui { repo }) => ui::run(&layout, repo.as_deref()),
        Some(Commands::Worktrees { repo }) => worktrees::run(&layout, repo.as_deref()),
        Some(Commands::Clean {
            repo,
            branch,
            force,
        }) => clean::run(&layout, &repo, &branch, force),
        Some(Commands::Prune { repo }) => prune::run(&layout, &repo),
        Some(Commands::Done { worktree_path }) => done::run(worktree_path.as_deref()),
        Some(Commands::Completions { shell }) => completions::run(shell),
    }
}
