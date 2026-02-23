use std::fs;

use crate::git::commands as git_commands;
use crate::workspace_paths::WorkspacePaths;

pub fn run(
    layout: &WorkspacePaths,
    repo_arg: &str,
    branch: &str,
    base_ref: Option<&str>,
) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
    super::ensure_repo_available(layout, repo_arg, &repo_key)?;

    let gitdir = layout.repo_gitdir_path(&repo_key);
    git_commands::fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref
        .map(|value| value.to_string())
        .unwrap_or_else(|| git_commands::detect_default_base(&gitdir));

    let worktree = layout.worktree_path(&repo_key, branch);
    if worktree.exists() && !worktree.join(".git").exists() {
        return Err(format!(
            "Path exists but is not a git worktree: {}",
            worktree.display()
        ));
    }

    if worktree.join(".git").exists() {
        super::log(&format!(
            "Reusing existing worktree: {}",
            worktree.display()
        ));
        return super::launch_workspace(&repo_key, branch, &worktree);
    }

    if let Some(parent) = worktree.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if git_commands::ref_exists(&gitdir, &format!("refs/heads/{branch}")) {
        git_commands::worktree_add_existing_branch(&gitdir, &worktree, branch)?;
    } else if git_commands::ref_exists(&gitdir, &format!("refs/remotes/origin/{branch}")) {
        git_commands::worktree_add_tracking_remote_branch(&gitdir, &worktree, branch)?;
    } else {
        if !git_commands::rev_exists(&gitdir, &base_ref) {
            return Err(format!("Base ref not found: {base_ref}"));
        }
        git_commands::worktree_add_from_base(&gitdir, &worktree, branch, &base_ref)?;
    }

    super::launch_workspace(&repo_key, branch, &worktree)
}
