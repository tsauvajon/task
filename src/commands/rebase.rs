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
        [query] => Ok(RebaseInput::Query(query.clone())),
        [repo_arg, branch] => Ok(RebaseInput::RepoBranch {
            repo_arg: repo_arg.clone(),
            branch: branch.clone(),
        }),
        [repo_arg, branch, base_ref] => Ok(RebaseInput::RepoBranchBase {
            repo_arg: repo_arg.clone(),
            branch: branch.clone(),
            base_ref: base_ref.clone(),
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
            let args = vec!["feature/login".to_owned()];
            assert_eq!(
                parse_rebase_input(&args).unwrap(),
                RebaseInput::Query("feature/login".to_owned())
            );
        }

        #[test]
        fn handles_repo_branch() {
            let args = vec!["task".to_owned(), "feature/login".to_owned()];
            assert_eq!(
                parse_rebase_input(&args).unwrap(),
                RebaseInput::RepoBranch {
                    repo_arg: "task".to_owned(),
                    branch: "feature/login".to_owned(),
                }
            );
        }

        #[test]
        fn handles_repo_branch_base() {
            let args = vec![
                "task".to_owned(),
                "feature/login".to_owned(),
                "origin/main".to_owned(),
            ];
            assert_eq!(
                parse_rebase_input(&args).unwrap(),
                RebaseInput::RepoBranchBase {
                    repo_arg: "task".to_owned(),
                    branch: "feature/login".to_owned(),
                    base_ref: "origin/main".to_owned(),
                }
            );
        }

        #[test]
        fn rejects_too_many_args() {
            let args = vec![
                "task".to_owned(),
                "feature/login".to_owned(),
                "origin/main".to_owned(),
                "extra".to_owned(),
            ];
            let error = parse_rebase_input(&args).expect_err("must reject extra args");
            assert_eq!(
                error.to_string(),
                "Usage: task rebase [query] | [repo branch [base-ref]]"
            );
        }

        #[test]
        fn repo_branch_captures_correct_fields() {
            let args = vec!["myrepo".to_owned(), "feat/xyz".to_owned()];
            let input = parse_rebase_input(&args).unwrap();
            assert_eq!(
                input,
                RebaseInput::RepoBranch {
                    repo_arg: "myrepo".to_owned(),
                    branch: "feat/xyz".to_owned(),
                }
            );
        }

        #[test]
        fn repo_branch_base_captures_correct_fields() {
            let args = vec![
                "myrepo".to_owned(),
                "feat/xyz".to_owned(),
                "origin/develop".to_owned(),
            ];
            let input = parse_rebase_input(&args).unwrap();
            assert_eq!(
                input,
                RebaseInput::RepoBranchBase {
                    repo_arg: "myrepo".to_owned(),
                    branch: "feat/xyz".to_owned(),
                    base_ref: "origin/develop".to_owned(),
                }
            );
        }

        #[test]
        fn query_captures_slash_in_branch_name() {
            let args = vec!["feat/my-feature".to_owned()];
            let input = parse_rebase_input(&args).unwrap();
            assert_eq!(input, RebaseInput::Query("feat/my-feature".to_owned()));
        }

        #[test]
        fn rejects_five_args() {
            let args = vec![
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "d".to_owned(),
                "e".to_owned(),
            ];
            assert!(
                parse_rebase_input(&args).is_err(),
                "5 args should be rejected"
            );
        }

        #[test]
        fn error_message_contains_usage() {
            let args = vec![
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "d".to_owned(),
            ];
            let err = parse_rebase_input(&args).unwrap_err();
            assert!(
                err.to_string().contains("Usage:"),
                "error should contain usage hint: {err}"
            );
        }
    }
}
