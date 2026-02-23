use crate::git::{detect_default_base, fetch_origin_refs, rebase, rev_exists};
use crate::runtime::RuntimeEnvironment;

pub fn run(context: &RuntimeEnvironment, args: &[String]) -> Result<(), String> {
    let input = parse_rebase_input(args)?;
    let (repo_key, branch, base_ref) = resolve_rebase_target(context, input)?;

    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }

    let worktree = context.resolve_worktree_path(&repo_key, &branch);
    if !worktree.join(".git").exists() {
        return Err(format!("Worktree not found: {}", worktree.display()));
    }

    fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref.unwrap_or_else(|| detect_default_base(&gitdir));
    if !rev_exists(&gitdir, &base_ref) {
        return Err(format!("Base ref not found: {base_ref}"));
    }

    context.log(&format!("Rebasing {repo_key} {branch} onto {base_ref}"));
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

fn parse_rebase_input(args: &[String]) -> Result<RebaseInput, String> {
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
        _ => Err("Usage: task rebase [query] | [repo branch [base-ref]]".to_string()),
    }
}

fn resolve_rebase_target(
    context: &RuntimeEnvironment,
    input: RebaseInput,
) -> Result<(String, String, Option<String>), String> {
    match input {
        RebaseInput::CurrentTask => {
            let (repo_key, branch) = context.resolve_task_from_args(
                &[],
                "Usage: task rebase [query] | [repo branch [base-ref]]",
            )?;
            Ok((repo_key, branch, None))
        }
        RebaseInput::Query(query) => {
            let (repo_key, branch) = context.resolve_task_from_args(
                &[query],
                "Usage: task rebase [query] | [repo branch [base-ref]]",
            )?;
            Ok((repo_key, branch, None))
        }
        RebaseInput::RepoBranch { repo_arg, branch } => {
            let repo_key = context.resolve_repo_key_input(&repo_arg)?;
            Ok((repo_key, branch, None))
        }
        RebaseInput::RepoBranchBase {
            repo_arg,
            branch,
            base_ref,
        } => {
            let repo_key = context.resolve_repo_key_input(&repo_arg)?;
            Ok((repo_key, branch, Some(base_ref)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RebaseInput, parse_rebase_input};

    #[test]
    fn parse_rebase_input_handles_current_task() {
        let args = Vec::<String>::new();
        assert_eq!(parse_rebase_input(&args), Ok(RebaseInput::CurrentTask));
    }

    #[test]
    fn parse_rebase_input_handles_query() {
        let args = vec!["feature/login".to_string()];
        assert_eq!(
            parse_rebase_input(&args),
            Ok(RebaseInput::Query("feature/login".to_string()))
        );
    }

    #[test]
    fn parse_rebase_input_handles_repo_branch() {
        let args = vec!["task".to_string(), "feature/login".to_string()];
        assert_eq!(
            parse_rebase_input(&args),
            Ok(RebaseInput::RepoBranch {
                repo_arg: "task".to_string(),
                branch: "feature/login".to_string(),
            })
        );
    }

    #[test]
    fn parse_rebase_input_handles_repo_branch_base() {
        let args = vec![
            "task".to_string(),
            "feature/login".to_string(),
            "origin/main".to_string(),
        ];
        assert_eq!(
            parse_rebase_input(&args),
            Ok(RebaseInput::RepoBranchBase {
                repo_arg: "task".to_string(),
                branch: "feature/login".to_string(),
                base_ref: "origin/main".to_string(),
            })
        );
    }

    #[test]
    fn parse_rebase_input_rejects_too_many_args() {
        let args = vec![
            "task".to_string(),
            "feature/login".to_string(),
            "origin/main".to_string(),
            "extra".to_string(),
        ];
        let error = parse_rebase_input(&args).expect_err("must reject extra args");
        assert_eq!(
            error,
            "Usage: task rebase [query] | [repo branch [base-ref]]"
        );
    }
}
