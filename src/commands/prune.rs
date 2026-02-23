use crate::git::commands as git_commands;
use crate::layout::Layout;

pub fn run(layout: &Layout, repo_arg: Option<&str>) -> Result<(), String> {
    let repo_arg = super::resolve_repo_input(repo_arg)?;
    let repo_key = super::resolve_repo_key_input(layout, &repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }
    git_commands::worktree_prune(&gitdir)
}
