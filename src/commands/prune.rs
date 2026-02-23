use crate::runtime::RuntimeEnvironment;
use crate::tools::git::worktree_prune;

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<(), String> {
    let repo_arg = context.resolve_repo_input(repo_arg)?;
    let repo_key = context.resolve_repo_key_input(&repo_arg)?;
    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }
    worktree_prune(&gitdir)
}
