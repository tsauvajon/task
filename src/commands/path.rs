use crate::layout::Layout;

pub fn run(layout: &Layout, repo_arg: &str, branch: &str) -> Result<(), String> {
    let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
    println!("{}", layout.worktree_path(&repo_key, branch).display());
    Ok(())
}
