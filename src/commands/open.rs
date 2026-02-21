use crate::layout::Layout;

pub fn run(layout: &Layout, repo_arg: &str, branch: &str) -> Result<(), String> {
    let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
    let worktree = layout.worktree_path(&repo_key, branch);
    if !worktree.join(".git").exists() {
        return Err(format!("Worktree not found: {}", worktree.display()));
    }
    super::launch_workspace(&repo_key, branch, &worktree)
}
