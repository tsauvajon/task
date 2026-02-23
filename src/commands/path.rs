use crate::runtime::environment::RuntimeEnvironment;

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
) -> Result<(), String> {
    let (repo_arg, branch) = context.resolve_repo_branch_inputs(repo_arg, branch_arg)?;
    let repo_key = context.resolve_repo_key_input(&repo_arg)?;
    let worktree = context.resolve_worktree_path(&repo_key, &branch);
    println!("{}", worktree.display());
    Ok(())
}
