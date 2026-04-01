use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::git::worktrees::list,
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;

    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key().map(String::from));

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = context.tasks().resolve_repo_key_input(repo_arg)?;
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(Error::not_found(format!("Repo not found: {repo_key}")));
        }
        let output = list(&gitdir)?;
        print!("{output}");
        return Ok(());
    }

    let repo_keys = context.tasks().available_repo_keys()?;
    if repo_keys.is_empty() {
        process::log(&format!(
            "No repositories found in {}",
            context.layout().repos_dir().display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!();
        println!("[{repo_key}]");
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        let output = list(&gitdir)?;
        print!("{output}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process::Command};

    use crate::runtime::environment::RuntimeEnvironment;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-worktrees-{name}"));
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

    fn init_bare_repo(path: &std::path::Path) {
        fs::create_dir_all(path).expect("create repo dir");
        let status = Command::new("git")
            .args(["init", "--bare"])
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git must be available");
        assert!(status.success(), "git init --bare failed");
    }

    fn env_for(repos_dir: &std::path::Path, wt_dir: &std::path::Path) -> RuntimeEnvironment {
        let detached_dir = repos_dir.parent().unwrap().join("detached");
        RuntimeEnvironment::from_paths(repos_dir, wt_dir, &detached_dir)
    }

    mod run {
        use super::*;

        #[test]
        fn returns_ok_when_no_repos_exist() {
            let dir = TempDir::new("no-repos");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            let result = super::super::run(&env, None);
            assert!(result.is_ok(), "should succeed with no repos: {result:?}");
        }

        #[test]
        fn returns_ok_for_bare_repo_with_no_worktrees() {
            let dir = TempDir::new("bare-no-wt");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            let result = super::super::run(&env, None);
            assert!(
                result.is_ok(),
                "should succeed with bare repo and no worktrees: {result:?}"
            );
        }

        #[test]
        fn errors_when_specific_repo_arg_does_not_exist() {
            let dir = TempDir::new("missing-repo");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            let result = super::super::run(&env, Some("github.com/me/nonexistent"));
            assert!(result.is_err(), "should fail for a missing repo");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("not found") || msg.contains("nonexistent"),
                "error should mention the missing repo: {msg}"
            );
        }

        #[test]
        fn returns_ok_for_explicit_repo_arg_that_exists() {
            let dir = TempDir::new("explicit-repo");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            let result = super::super::run(&env, Some("github.com/me/app"));
            assert!(
                result.is_ok(),
                "should succeed for an existing repo: {result:?}"
            );
        }
    }
}
