use std::fs;

use crate::git::commands as git_commands;
use crate::runtime::session_name::task_session_name;
use crate::workspace_paths::WorkspacePaths;

pub fn run(
    layout: &WorkspacePaths,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
    force: bool,
) -> Result<(), String> {
    let (repo_arg, branch) = super::resolve_repo_branch_inputs(layout, repo_arg, branch_arg)?;
    let repo_key = super::resolve_repo_key_input(layout, &repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    let worktree = super::resolve_worktree_path(layout, &repo_key, &branch);

    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }

    if super::command_exists("tmux") {
        let session = task_session_name(&repo_key, &branch);
        if super::tmux_has_session(&session) {
            super::run_status("tmux", &["kill-session", "-t", &session], None)?;
        }
    }

    if !worktree.join(".git").exists() {
        super::warn(&format!(
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
                super::warn(&format!(
                    "Left non-worktree directory in place: {}",
                    worktree.display()
                ));
            }
        }
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

    Ok(())
}
