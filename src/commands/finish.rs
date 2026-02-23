use std::fs;

use crate::runtime::RuntimeEnvironment;
use crate::tools::git::{status_porcelain, worktree_prune, worktree_remove};
use crate::tools::tmux;
use crate::tools::vscodium;

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
    force: bool,
) -> Result<(), String> {
    let (repo_arg, branch) = context.resolve_repo_branch_inputs(repo_arg, branch_arg)?;
    let repo_key = context.resolve_repo_key_input(&repo_arg)?;
    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    let worktree = context.resolve_worktree_path(&repo_key, &branch);

    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }

    if !worktree.join(".git").exists() {
        context.warn(&format!(
            "Worktree metadata is stale for {}. Pruning stale entries.",
            worktree.display()
        ));
        worktree_prune(&gitdir)?;

        if worktree.exists() {
            let is_empty = fs::read_dir(&worktree)
                .map_err(|error| error.to_string())?
                .next()
                .is_none();
            if is_empty {
                let _ = fs::remove_dir(&worktree);
            } else {
                context.warn(&format!(
                    "Left non-worktree directory in place: {}",
                    worktree.display()
                ));
            }
        }

        tmux::finish_task_session(context.process(), &repo_key, &branch)?;
        if let Err(error) = vscodium::cleanup_task_state(&repo_key, &branch) {
            context.warn(&format!(
                "Failed to remove task editor state for {repo_key} {branch}: {error}"
            ));
        }
        return Ok(());
    }

    if !force {
        let status = status_porcelain(&worktree)?;
        if !status.trim().is_empty() {
            return Err(
                "Worktree has uncommitted changes. Use --force if you really want to remove it."
                    .to_string(),
            );
        }
    }

    worktree_remove(&gitdir, &worktree, force)?;

    if let Some(parent) = worktree.parent() {
        let _ = fs::remove_dir(parent);
    }

    tmux::finish_task_session(context.process(), &repo_key, &branch)?;
    if let Err(error) = vscodium::cleanup_task_state(&repo_key, &branch) {
        context.warn(&format!(
            "Failed to remove task editor state for {repo_key} {branch}: {error}"
        ));
    }

    Ok(())
}
