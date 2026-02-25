use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::git::worktrees::worktree_list,
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;

    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key().map(String::from));

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = context.tasks().resolve_repo_key_input(repo_arg)?;
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(Error::not_found(format!("Repo not found: {repo_key}")));
        }
        let output = worktree_list(&gitdir)?;
        print!("{output}");
        return Ok(());
    }

    let repo_keys = context.tasks().available_repo_keys()?;
    if repo_keys.is_empty() {
        process::log(&format!(
            "No repositories found in {}",
            context.layout().repos_dir().display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!();
        println!("[{repo_key}]");
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        let output = worktree_list(&gitdir)?;
        print!("{output}");
    }

    Ok(())
}
