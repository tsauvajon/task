use std::fs;

use crate::layout::Layout;

pub fn run(
    layout: &Layout,
    repo_arg: &str,
    branch: &str,
    base_ref: Option<&str>,
) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
    super::ensure_repo_available(layout, repo_arg, &repo_key)?;

    let gitdir = layout.repo_gitdir_path(&repo_key);
    super::run_status(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "fetch",
            "--all",
            "--prune",
        ],
        None,
    )?;

    let base_ref = base_ref
        .map(|value| value.to_string())
        .unwrap_or_else(|| super::detect_default_base(&gitdir));

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

    if super::ref_exists(&gitdir, &format!("refs/heads/{branch}")) {
        super::run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "add",
                worktree.to_string_lossy().as_ref(),
                branch,
            ],
            None,
        )?;
    } else if super::ref_exists(&gitdir, &format!("refs/remotes/origin/{branch}")) {
        super::run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "add",
                "--track",
                "-b",
                branch,
                worktree.to_string_lossy().as_ref(),
                &format!("origin/{branch}"),
            ],
            None,
        )?;
    } else {
        if !super::rev_exists(&gitdir, &base_ref) {
            return Err(format!("Base ref not found: {base_ref}"));
        }
        super::run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "add",
                "-b",
                branch,
                worktree.to_string_lossy().as_ref(),
                &base_ref,
            ],
            None,
        )?;
    }

    super::launch_workspace(&repo_key, branch, &worktree)
}
