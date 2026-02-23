use std::fs;

use crate::git::commands as git_commands;
use crate::runtime::RuntimeEnvironment;
use crate::tmux;

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
        git_commands::worktree_prune(&gitdir)?;

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
        return Ok(());
    }

    if !force {
        let status = git_commands::status_porcelain(&worktree)?;
        if !status.trim().is_empty() {
            return Err(
                "Worktree has uncommitted changes. Use --force if you really want to remove it."
                    .to_string(),
            );
        }
    }

    git_commands::worktree_remove(&gitdir, &worktree, force)?;

    if let Some(parent) = worktree.parent() {
        let _ = fs::remove_dir(parent);
    }

    tmux::finish_task_session(context.process(), &repo_key, &branch)?;

    Ok(())
}
