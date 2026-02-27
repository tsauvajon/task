use std::path::PathBuf;

use crate::{error::Result, runtime::environment::RuntimeEnvironment};

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
) -> Result<()> {
    let worktree = resolve_output_path(
        repo_arg,
        branch_arg,
        |repo, branch| context.tasks().resolve_repo_branch_inputs(repo, branch),
        |repo| context.tasks().resolve_repo_key_input(repo),
        |repo_key, branch| context.tasks().resolve_worktree_path(repo_key, branch),
    )?;
    println!("{}", worktree.display());
    Ok(())
}

fn resolve_output_path<ResolveInputs, ResolveRepoKey, ResolveWorktree, RepoArg, RepoKey, Branch>(
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
    resolve_inputs: ResolveInputs,
    resolve_repo_key: ResolveRepoKey,
    resolve_worktree: ResolveWorktree,
) -> Result<PathBuf>
where
    ResolveInputs: FnOnce(Option<&str>, Option<&str>) -> Result<(RepoArg, Branch)>,
    ResolveRepoKey: FnOnce(&RepoArg) -> Result<RepoKey>,
    ResolveWorktree: FnOnce(&RepoKey, &Branch) -> PathBuf,
{
    let (repo_arg, branch) = resolve_inputs(repo_arg, branch_arg)?;
    let repo_key = resolve_repo_key(&repo_arg)?;
    Ok(resolve_worktree(&repo_key, &branch))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::resolve_output_path;
    use crate::error::Error;

    #[test]
    fn resolves_path_from_resolvers() {
        let resolved = resolve_output_path(
            Some("org/repo"),
            Some("feat"),
            |repo, branch| {
                assert_eq!(repo, Some("org/repo"));
                assert_eq!(branch, Some("feat"));
                Ok(("org/repo".to_string(), "feat".to_string()))
            },
            |repo| {
                assert_eq!(repo, "org/repo");
                Ok("org/repo".to_string())
            },
            |repo_key, branch| {
                assert_eq!(repo_key, "org/repo");
                assert_eq!(branch, "feat");
                PathBuf::from("/tmp/wt/org/repo/feat")
            },
        )
        .expect("path should resolve");

        assert_eq!(resolved, PathBuf::from("/tmp/wt/org/repo/feat"));
    }

    #[test]
    fn propagates_input_resolution_error() {
        let err = resolve_output_path(
            None,
            None,
            |_repo, _branch| Err(Error::failed("input error")),
            |_repo: &String| Ok("unused".to_string()),
            |_repo: &String, _branch: &String| PathBuf::from("unused"),
        )
        .expect_err("expected input error");

        assert!(err.to_string().contains("input error"));
    }

    #[test]
    fn propagates_repo_key_resolution_error() {
        let err = resolve_output_path(
            Some("org/repo"),
            Some("feat"),
            |_repo, _branch| Ok(("org/repo".to_string(), "feat".to_string())),
            |_repo| Err(Error::failed("repo key error")),
            |_repo: &String, _branch: &String| PathBuf::from("unused"),
        )
        .expect_err("expected repo key error");

        assert!(err.to_string().contains("repo key error"));
    }
}
