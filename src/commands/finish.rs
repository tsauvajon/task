use std::fs;

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::{
        git::worktrees::{status_porcelain, worktree_prune, worktree_remove},
        tmux::workflow::finish_task_session,
        vscodium::workflow::cleanup_task_state,
    },
};

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
    force: bool,
) -> Result<()> {
    let (repo_key_raw, branch) = context
        .tasks()
        .resolve_repo_branch_inputs(repo_arg, branch_arg)?;
    let repo_key = context.tasks().resolve_repo_key_input(&repo_key_raw)?;
    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    let worktree = context.tasks().resolve_worktree_path(&repo_key, &branch);

    if !gitdir.is_dir() {
        return Err(Error::not_found(format!("Repo not found: {repo_key}")));
    }

    if !worktree.join(".git").exists() {
        process::warn(&format!(
            "Worktree metadata is stale for {}. Pruning stale entries.",
            worktree.display()
        ));
        worktree_prune(&gitdir)?;

        if worktree.exists() {
            let is_empty = fs::read_dir(&worktree)?.next().is_none();
            if is_empty {
                let _ = fs::remove_dir(&worktree);
            } else {
                process::warn(&format!(
                    "Left non-worktree directory in place: {}",
                    worktree.display()
                ));
            }
        }

        finish_task_session(&repo_key, &branch)?;
        if let Err(err) = cleanup_task_state(&repo_key, &branch) {
            process::warn(&format!(
                "Failed to remove task editor state for {repo_key} {branch}: {err}"
            ));
        }
        return Ok(());
    }

    if !force {
        let status = status_porcelain(&worktree)?;
        if !status.trim().is_empty() {
            return Err(Error::failed(
                "Worktree has uncommitted changes. Use --force if you really want to remove it.",
            ));
        }
    }

    worktree_remove(&gitdir, &worktree, force)?;

    if let Some(parent) = worktree.parent() {
        let _ = fs::remove_dir(parent);
    }

    finish_task_session(&repo_key, &branch)?;
    if let Err(err) = cleanup_task_state(&repo_key, &branch) {
        process::warn(&format!(
            "Failed to remove task editor state for {repo_key} {branch}: {err}"
        ));
    }

    Ok(())
}
