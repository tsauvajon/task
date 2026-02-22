use crate::layout::Layout;

pub fn run(layout: &Layout, args: &[String]) -> Result<(), String> {
    let (repo_key, branch, base_ref) = match args {
        [] => {
            let (repo_key, branch) = super::resolve_task_from_args(
                layout,
                args,
                "Usage: task rebase [query] | [repo branch [base-ref]]",
            )?;
            (repo_key, branch, None)
        }
        [query] => {
            let (repo_key, branch) = super::resolve_task_from_args(
                layout,
                std::slice::from_ref(query),
                "Usage: task rebase [query] | [repo branch [base-ref]]",
            )?;
            (repo_key, branch, None)
        }
        [repo_arg, branch] => {
            let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
            (repo_key, branch.to_string(), None)
        }
        [repo_arg, branch, base_ref] => {
            let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
            (repo_key, branch.to_string(), Some(base_ref.to_string()))
        }
        _ => {
            return Err("Usage: task rebase [query] | [repo branch [base-ref]]".to_string());
        }
    };

    let gitdir = layout.repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }

    let worktree = layout.worktree_path(&repo_key, &branch);
    if !worktree.join(".git").exists() {
        return Err(format!("Worktree not found: {}", worktree.display()));
    }

    super::fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref.unwrap_or_else(|| super::detect_default_base(&gitdir));
    if !super::rev_exists(&gitdir, &base_ref) {
        return Err(format!("Base ref not found: {base_ref}"));
    }

    super::log(&format!("Rebasing {repo_key} {branch} onto {base_ref}"));
    super::run_status(
        "git",
        &[
            "-C",
            worktree.to_string_lossy().as_ref(),
            "rebase",
            &base_ref,
        ],
        None,
    )
}
