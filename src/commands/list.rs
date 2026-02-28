use rayon::prelude::*;

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process, task_rows::TaskRow},
    tools::git,
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;
    let open_sessions = context.tasks().tmux_sessions();

    let mut rows: Vec<TaskRow> = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key().map(String::from));

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = context.tasks().resolve_repo_key_input(repo_arg)?;
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(Error::not_found(format!("Repo not found: {repo_key}")));
        }

        rows.extend(
            context
                .tasks()
                .repo_task_rows(&repo_key, &gitdir, &open_sessions)?,
        );
        if rows.is_empty() {
            process::log(&format!("No tasks found for {repo_key}"));
        } else {
            context.tasks().print_task_rows_table(&rows);
        }
        return Ok(());
    }

    // Resolve the nix store path for git before entering the parallel section:
    // the OnceLock inside NixRunner would otherwise block every rayon thread on
    // the first caller (~0.5s) while the rest stall idle.
    git::warmup();

    // Collect all (key, gitdir) pairs first (fast sequential scan), then
    // fan out all `git worktree list` subprocess calls in one flat parallel pass.
    let results: Vec<_> = context
        .tasks()
        .available_repos()?
        .into_par_iter()
        .map(|(repo_key, gitdir)| {
            let result = context
                .tasks()
                .repo_task_rows(&repo_key, &gitdir, &open_sessions);
            (repo_key, result)
        })
        .collect();

    let mut skipped_repos = Vec::new();
    for (repo_key, result) in results {
        match result {
            Ok(repo_rows) => rows.extend(repo_rows),
            Err(err) => skipped_repos.push((repo_key, err)),
        }
    }

    if rows.is_empty() {
        process::log(&format!(
            "No tasks found under {}",
            context.layout().wt_dir().display()
        ));
    } else {
        context.tasks().print_task_rows_table(&rows);
    }

    for (repo_key, err) in skipped_repos {
        process::warn(&format!("Skipping {repo_key}: {err}"));
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
            let path = env::temp_dir().join(format!("task-rs-list-{name}"));
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
        fn returns_ok_with_a_bare_repo_and_no_worktrees() {
            let dir = TempDir::new("bare-no-wt");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            fs::create_dir_all(&wt_dir).unwrap();

            let env = env_for(&repos_dir, &wt_dir);
            let result = super::super::run(&env, None);
            assert!(
                result.is_ok(),
                "should succeed with an empty bare repo: {result:?}"
            );
        }

        #[test]
        fn errors_when_specific_repo_arg_does_not_exist() {
            let dir = TempDir::new("missing-repo-arg");
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
                "error should mention the repo: {msg}"
            );
        }
    }
}
