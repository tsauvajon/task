use crate::{
    commands::{RepoCommand, clone},
    error::Result,
    runtime::{environment::RuntimeEnvironment, process},
};

pub fn run(context: &RuntimeEnvironment, command: RepoCommand) -> Result<()> {
    match command {
        RepoCommand::List => list(context),
        RepoCommand::Clone { repo_url, repo_key } => clone::run(context, &repo_url, repo_key),
    }
}

fn list(context: &RuntimeEnvironment) -> Result<()> {
    context.tasks().ensure_layout()?;
    let repo_keys = context.tasks().available_repo_keys()?;

    if repo_keys.is_empty() {
        process::log(&format!(
            "No repositories found in {}",
            context.layout().repos_dir().display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!("{repo_key}");
    }

    Ok(())
}
