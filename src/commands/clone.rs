use crate::runtime::environment::RuntimeEnvironment;
use crate::tools::git::repo::parse_repo_input;

pub fn run(
    context: &RuntimeEnvironment,
    repo_url: &str,
    repo_key: Option<String>,
) -> Result<(), String> {
    context.ensure_layout()?;
    let parsed = parse_repo_input(repo_url);
    let clone_url = parsed
        .clone_url
        .unwrap_or_else(|| repo_url.trim().to_string());
    let repo_key = repo_key.unwrap_or(parsed.repo_key);
    context.clone_bare_repo(&clone_url, &repo_key)?;
    context.log(&format!("Repo key: {repo_key}"));
    Ok(())
}
