use std::fs;

use crate::{
    error::{Error, Result},
    runtime::{BranchName, environment::RuntimeEnvironment, process},
    tools::git::{
        refs::{detect_default_base, fetch_origin_refs, ref_exists, rev_exists},
        worktrees::{add_existing_branch, add_from_base, add_tracking_remote_branch},
    },
};

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: &str,
    branch: &str,
    base_ref: Option<&str>,
) -> Result<()> {
    context.tasks().ensure_layout()?;
    let repo_key = context.tasks().resolve_repo_key_input(repo_arg)?;
    context.tasks().ensure_repo_available(repo_arg, &repo_key)?;

    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref
        .map(str::to_string)
        .unwrap_or_else(|| detect_default_base(&gitdir));

    let branch_name = BranchName::new(branch);
    let worktree = context.layout().worktree_path(&repo_key, &branch_name);
    if worktree.exists() && !worktree.join(".git").exists() {
        return Err(Error::failed(format!(
            "Path exists but is not a git worktree: {}",
            worktree.display()
        )));
    }

    if worktree.join(".git").exists() {
        process::log(&format!(
            "Reusing existing worktree: {}",
            worktree.display()
        ));
        return context
            .tasks()
            .launch_workspace(&repo_key, &branch_name, &worktree);
    }

    if let Some(parent) = worktree.parent() {
        fs::create_dir_all(parent)?;
    }

    if ref_exists(&gitdir, &format!("refs/heads/{branch}")) {
        add_existing_branch(&gitdir, &worktree, branch)?;
    } else if ref_exists(&gitdir, &format!("refs/remotes/origin/{branch}")) {
        add_tracking_remote_branch(&gitdir, &worktree, branch)?;
    } else {
        if !rev_exists(&gitdir, &base_ref) {
            return Err(Error::not_found(format!("Base ref not found: {base_ref}")));
        }
        add_from_base(&gitdir, &worktree, branch, &base_ref)?;
    }

    context
        .tasks()
        .launch_workspace(&repo_key, &branch_name, &worktree)
}
