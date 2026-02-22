use crate::layout::Layout;
use crate::repo_key::normalize_repo_key;

pub fn run(layout: &Layout, repo_url: &str, repo_key: Option<String>) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let repo_key = repo_key.unwrap_or_else(|| normalize_repo_key(repo_url));
    super::clone_bare_repo(layout, repo_url, &repo_key)?;
    super::log(&format!("Repo key: {repo_key}"));
    Ok(())
}
