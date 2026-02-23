use crate::git::parsing::parse_repo_input;
use crate::workspace_paths::WorkspacePaths;

pub fn run(
    layout: &WorkspacePaths,
    repo_url: &str,
    repo_key: Option<String>,
) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let parsed = parse_repo_input(repo_url);
    let clone_url = parsed
        .clone_url
        .unwrap_or_else(|| repo_url.trim().to_string());
    let repo_key = repo_key.unwrap_or(parsed.repo_key);
    super::clone_bare_repo(layout, &clone_url, &repo_key)?;
    super::log(&format!("Repo key: {repo_key}"));
    Ok(())
}
