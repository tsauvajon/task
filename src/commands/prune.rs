use std::path::Path;

use crate::{
    error::{Error, Result},
    runtime::environment::RuntimeEnvironment,
    tools::git::worktrees::prune,
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    let repo_arg = context.tasks().resolve_repo_input(repo_arg)?;
    let repo_key = context.tasks().resolve_repo_key_input(&repo_arg)?;
    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    ensure_repo_exists(&gitdir, &repo_key)?;
    prune(&gitdir)
}

fn ensure_repo_exists(gitdir: &Path, repo_key: &str) -> Result<()> {
    if gitdir.is_dir() {
        return Ok(());
    }
    Err(Error::not_found(format!("Repo not found: {repo_key}")))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::ensure_repo_exists;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("task-tests-{prefix}-{nanos}"))
    }

    #[test]
    fn accepts_existing_gitdir() {
        let dir = unique_temp_dir("prune-existing");
        fs::create_dir_all(&dir).expect("create temp dir");

        ensure_repo_exists(&dir, "my/repo").expect("existing directory should pass");

        fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_missing_gitdir_with_repo_name() {
        let missing = unique_temp_dir("prune-missing");
        let err = ensure_repo_exists(&missing, "my/repo").expect_err("expected missing repo error");
        assert!(err.to_string().contains("Repo not found: my/repo"));
    }
}
