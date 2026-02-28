use crate::{
    error::Result,
    runtime::{environment::RuntimeEnvironment, process, RepoKey},
    tools::git::repo::{default_clone_url, parse_repo_input},
};

pub fn run(env: &RuntimeEnvironment, repo_url: &str, repo_key: Option<String>) -> Result<()> {
    env.tasks().ensure_layout()?;
    let parsed = parse_repo_input(repo_url);
    let clone_url = parsed
        .clone_url
        .unwrap_or_else(|| default_clone_url(repo_url));
    let repo_key = RepoKey::new(repo_key.unwrap_or(parsed.repo_key));
    env.tasks().clone_bare_repo(&clone_url, &repo_key)?;
    process::log(&format!("Repo key: {repo_key}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use crate::runtime::environment::RuntimeEnvironment;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-clone-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn env_for(repos_dir: &std::path::Path, wt_dir: &std::path::Path) -> RuntimeEnvironment {
        let detached_dir = repos_dir.parent().unwrap().join("detached");
        RuntimeEnvironment::from_paths(repos_dir, wt_dir, &detached_dir)
    }

    mod run {
        use super::*;

        #[test]
        fn propagates_error_for_unreachable_url() {
            let dir = TempDir::new("bad-url");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            // Use a URL that git can parse but will never resolve — clone must fail
            // and run() must propagate the error.
            let result = super::super::run(
                &env,
                "https://invalid.example.invalid/nonexistent/repo.git",
                None,
            );
            assert!(result.is_err(), "should fail for an unreachable URL");
        }

        #[test]
        fn propagates_error_for_explicit_key_with_unreachable_url() {
            let dir = TempDir::new("bad-url-with-key");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            let result = super::super::run(
                &env,
                "https://invalid.example.invalid/nonexistent/repo.git",
                Some("example.invalid/nonexistent/repo".to_string()),
            );
            assert!(
                result.is_err(),
                "should fail for an unreachable URL with explicit key"
            );
        }
    }
}
