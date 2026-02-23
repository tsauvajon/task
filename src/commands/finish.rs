use std::fs;

use crate::layout::Layout;
use crate::session::session_name_for;

pub fn run(
    layout: &Layout,
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
        let session = session_name_for(&repo_key, &branch);
        if super::tmux_has_session(&session) {
            super::run_status("tmux", &["kill-session", "-t", &session], None)?;
        }
    }

    if !worktree.join(".git").exists() {
        super::warn(&format!(
            "Worktree metadata is stale for {}. Pruning stale entries.",
            worktree.display()
        ));
        super::run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "prune",
                "--verbose",
            ],
            None,
        )?;

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
        let status = super::run_capture(
            "git",
            &[
                "-C",
                worktree.to_string_lossy().as_ref(),
                "status",
                "--porcelain",
            ],
            None,
        )?;
        if !status.trim().is_empty() {
            return Err(
                "Worktree has uncommitted changes. Use --force if you really want to remove it."
                    .to_string(),
            );
        }
    }

    let mut args = vec![
        "--git-dir".to_string(),
        gitdir.to_string_lossy().to_string(),
        "worktree".to_string(),
        "remove".to_string(),
    ];
    if force {
        args.push("--force".to_string());
    }
    args.push(worktree.to_string_lossy().to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    super::run_status("git", &arg_refs, None)?;

    if let Some(parent) = worktree.parent() {
        let _ = fs::remove_dir(parent);
    }

    Ok(())
}
