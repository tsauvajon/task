use crate::{
    commands::{RepoCommand, clone},
    runtime::environment::RuntimeEnvironment,
};

pub fn run(context: &RuntimeEnvironment, command: RepoCommand) -> Result<(), String> {
    match command {
        RepoCommand::List => list(context),
        RepoCommand::Clone { repo_url, repo_key } => clone::run(context, &repo_url, repo_key),
    }
}

fn list(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    let repo_keys = context.available_repo_keys()?;

    if repo_keys.is_empty() {
        context.log(&format!(
            "No repositories found in {}",
            context.repos_dir().display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!("{repo_key}");
    }

    Ok(())
}
