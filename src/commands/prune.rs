use crate::{
    error::{Error, Result},
    runtime::environment::RuntimeEnvironment,
    tools::git::worktrees::worktree_prune,
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    let repo_arg = context.tasks().resolve_repo_input(repo_arg)?;
    let repo_key = context.tasks().resolve_repo_key_input(&repo_arg)?;
    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(Error::not_found(format!("Repo not found: {repo_key}")));
    }
    worktree_prune(&gitdir)
}
