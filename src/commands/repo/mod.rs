use crate::{
    commands::{RepoCommand, clone},
    error::Result,
    runtime::{environment::RuntimeEnvironment, process},
};

pub fn run(context: &RuntimeEnvironment, command: RepoCommand) -> Result<()> {
    match command {
        RepoCommand::List => list(context),
        RepoCommand::Clone { repo_url, repo_key } => clone::run(context, &repo_url, repo_key),
    }
}

fn list(context: &RuntimeEnvironment) -> Result<()> {
    context.tasks().ensure_layout()?;
    let repo_keys = context.tasks().available_repo_keys()?;

    if repo_keys.is_empty() {
        process::log(&format!(
            "No repositories found in {}",
            context.layout().repos_dir().display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!("{repo_key}");
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
            let path = env::temp_dir().join(format!("task-rs-repo-list-{name}"));
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

    #[test]
    fn list_returns_ok_when_no_repos_exist() {
        let dir = TempDir::new("no-repos");
        let repos_dir = dir.path().join("repos");
        let wt_dir = dir.path().join("wt");
        fs::create_dir_all(&repos_dir).unwrap();
        fs::create_dir_all(&wt_dir).unwrap();

        let env = env_for(&repos_dir, &wt_dir);
        // Drive through run() with the List variant
        let result = super::list(&env);
        assert!(
            result.is_ok(),
            "list should succeed with no repos: {result:?}"
        );
    }

    #[test]
    fn list_returns_ok_with_a_bare_repo() {
        let dir = TempDir::new("one-repo");
        let repos_dir = dir.path().join("repos");
        let wt_dir = dir.path().join("wt");
        init_bare_repo(&repos_dir.join("github.com/me/app.git"));
        fs::create_dir_all(&wt_dir).unwrap();

        let env = env_for(&repos_dir, &wt_dir);
        let result = super::list(&env);
        assert!(
            result.is_ok(),
            "list should succeed with one repo: {result:?}"
        );
    }
}
