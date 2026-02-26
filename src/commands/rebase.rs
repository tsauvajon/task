use crate::{
    error::{Error, Result},
    runtime::{BranchName, RepoKey, environment::RuntimeEnvironment, process},
    tools::git::{
        refs::{detect_default_base, fetch_origin_refs, rev_exists},
        worktrees::rebase,
    },
};

pub fn run(context: &RuntimeEnvironment, args: &[String]) -> Result<()> {
    let input = parse_rebase_input(args)?;
    let (repo_key, branch, base_ref) = resolve_rebase_target(context, input)?;

    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(Error::not_found(format!("Repo not found: {repo_key}")));
    }

    let worktree = context.tasks().resolve_worktree_path(&repo_key, &branch);
    if !worktree.join(".git").exists() {
        return Err(Error::not_found(format!(
            "Worktree not found: {}",
            worktree.display()
        )));
    }

    fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref.unwrap_or_else(|| detect_default_base(&gitdir));
    if !rev_exists(&gitdir, &base_ref) {
        return Err(Error::not_found(format!("Base ref not found: {base_ref}")));
    }

    process::log(&format!("Rebasing {repo_key} {branch} onto {base_ref}"));
    rebase(&worktree, &base_ref)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RebaseInput {
    CurrentTask,
    Query(String),
    RepoBranch {
        repo_arg: String,
        branch: String,
    },
    RepoBranchBase {
        repo_arg: String,
        branch: String,
        base_ref: String,
    },
}

fn parse_rebase_input(args: &[String]) -> Result<RebaseInput> {
    match args {
        [] => Ok(RebaseInput::CurrentTask),
        [query] => Ok(RebaseInput::Query(query.to_string())),
        [repo_arg, branch] => Ok(RebaseInput::RepoBranch {
            repo_arg: repo_arg.to_string(),
            branch: branch.to_string(),
        }),
        [repo_arg, branch, base_ref] => Ok(RebaseInput::RepoBranchBase {
            repo_arg: repo_arg.to_string(),
            branch: branch.to_string(),
            base_ref: base_ref.to_string(),
        }),
        _ => Err(Error::failed(
            "Usage: task rebase [query] | [repo branch [base-ref]]",
        )),
    }
}

fn resolve_rebase_target(
    context: &RuntimeEnvironment,
    input: RebaseInput,
) -> Result<(RepoKey, BranchName, Option<String>)> {
    match input {
        RebaseInput::CurrentTask => {
            let (repo_key, branch) = context.tasks().resolve_task_from_args(
                &[],
                "Usage: task rebase [query] | [repo branch [base-ref]]",
            )?;
            Ok((repo_key, branch, None))
        }
        RebaseInput::Query(query) => {
            let (repo_key, branch) = context.tasks().resolve_task_from_args(
                &[query],
                "Usage: task rebase [query] | [repo branch [base-ref]]",
            )?;
            Ok((repo_key, branch, None))
        }
        RebaseInput::RepoBranch { repo_arg, branch } => {
            let repo_key = context.tasks().resolve_repo_key_input(&repo_arg)?;
            Ok((repo_key, BranchName::new(branch), None))
        }
        RebaseInput::RepoBranchBase {
            repo_arg,
            branch,
            base_ref,
        } => {
            let repo_key = context.tasks().resolve_repo_key_input(&repo_arg)?;
            Ok((repo_key, BranchName::new(branch), Some(base_ref)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RebaseInput, parse_rebase_input};

    mod parse_rebase_input {
        use super::*;

        #[test]
        fn handles_current_task() {
            let args = Vec::<String>::new();
            assert_eq!(parse_rebase_input(&args).unwrap(), RebaseInput::CurrentTask);
        }

        #[test]
        fn handles_query() {
            let args = vec!["feature/login".to_string()];
            assert_eq!(
                parse_rebase_input(&args).unwrap(),
                RebaseInput::Query("feature/login".to_string())
            );
        }

        #[test]
        fn handles_repo_branch() {
            let args = vec!["task".to_string(), "feature/login".to_string()];
            assert_eq!(
                parse_rebase_input(&args).unwrap(),
                RebaseInput::RepoBranch {
                    repo_arg: "task".to_string(),
                    branch: "feature/login".to_string(),
                }
            );
        }

        #[test]
        fn handles_repo_branch_base() {
            let args = vec![
                "task".to_string(),
                "feature/login".to_string(),
                "origin/main".to_string(),
            ];
            assert_eq!(
                parse_rebase_input(&args).unwrap(),
                RebaseInput::RepoBranchBase {
                    repo_arg: "task".to_string(),
                    branch: "feature/login".to_string(),
                    base_ref: "origin/main".to_string(),
                }
            );
        }

        #[test]
        fn rejects_too_many_args() {
            let args = vec![
                "task".to_string(),
                "feature/login".to_string(),
                "origin/main".to_string(),
                "extra".to_string(),
            ];
            let error = parse_rebase_input(&args).expect_err("must reject extra args");
            assert_eq!(
                error.to_string(),
                "Usage: task rebase [query] | [repo branch [base-ref]]"
            );
        }
    }
}
