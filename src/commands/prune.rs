use crate::layout::Layout;

pub fn run(layout: &Layout, repo_arg: &str) -> Result<(), String> {
    let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }
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
    )
}
