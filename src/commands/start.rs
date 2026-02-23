use std::fs;

use crate::git::{
    detect_default_base, fetch_origin_refs, ref_exists, rev_exists, worktree_add_existing_branch,
    worktree_add_from_base, worktree_add_tracking_remote_branch,
};
use crate::runtime::RuntimeEnvironment;

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: &str,
    branch: &str,
    base_ref: Option<&str>,
) -> Result<(), String> {
    context.ensure_layout()?;
    let repo_key = context.resolve_repo_key_input(repo_arg)?;
    context.ensure_repo_available(repo_arg, &repo_key)?;

    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref
        .map(|value| value.to_string())
        .unwrap_or_else(|| detect_default_base(&gitdir));

    let worktree = context.layout().worktree_path(&repo_key, branch);
    if worktree.exists() && !worktree.join(".git").exists() {
        return Err(format!(
            "Path exists but is not a git worktree: {}",
            worktree.display()
        ));
    }

    if worktree.join(".git").exists() {
        context.log(&format!(
            "Reusing existing worktree: {}",
            worktree.display()
        ));
        return context.launch_workspace(&repo_key, branch, &worktree);
    }

    if let Some(parent) = worktree.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if ref_exists(&gitdir, &format!("refs/heads/{branch}")) {
        worktree_add_existing_branch(&gitdir, &worktree, branch)?;
    } else if ref_exists(&gitdir, &format!("refs/remotes/origin/{branch}")) {
        worktree_add_tracking_remote_branch(&gitdir, &worktree, branch)?;
    } else {
        if !rev_exists(&gitdir, &base_ref) {
            return Err(format!("Base ref not found: {base_ref}"));
        }
        worktree_add_from_base(&gitdir, &worktree, branch, &base_ref)?;
    }

    context.launch_workspace(&repo_key, branch, &worktree)
}
