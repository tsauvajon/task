use crate::layout::Layout;

pub fn run(
    layout: &Layout,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
) -> Result<(), String> {
    let (repo_arg, branch) = super::resolve_repo_branch_inputs(layout, repo_arg, branch_arg)?;
    let repo_key = super::resolve_repo_key_input(layout, &repo_arg)?;
    let worktree = super::resolve_worktree_path(layout, &repo_key, &branch);
    println!("{}", worktree.display());
    Ok(())
}
