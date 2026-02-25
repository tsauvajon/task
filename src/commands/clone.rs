use crate::{
    error::Result,
    runtime::{RepoKey, environment::RuntimeEnvironment, process},
    tools::git::repo::{default_clone_url, parse_repo_input},
};

pub fn run(env: &RuntimeEnvironment, repo_url: &str, repo_key: Option<String>) -> Result<()> {
    env.tasks().ensure_layout()?;
    let parsed = parse_repo_input(repo_url);
    let clone_url = parsed
        .clone_url
        .unwrap_or_else(|| default_clone_url(repo_url));
    let repo_key = RepoKey::new(repo_key.unwrap_or(parsed.repo_key));
    env.tasks().clone_bare_repo(&clone_url, &repo_key)?;
    process::log(&format!("Repo key: {repo_key}"));
    Ok(())
}
