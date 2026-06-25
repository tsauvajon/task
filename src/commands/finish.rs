use std::{collections::HashSet, fs};

use crate::{
    error::{Error, Result},
    runtime::{BranchName, RepoKey, environment::RuntimeEnvironment, process},
    tools::{
        git::worktrees::{self, prune, remove, status_porcelain},
        vscodium::workflow::cleanup,
        zellij::workflow::finish_session,
    },
};

pub fn run(context: &RuntimeEnvironment, tasks: &[String], force: bool) -> Result<()> {
    if tasks.is_empty() {
        let (repo_key, branch, _) = context.tasks().current_task_info()?;
        return run_resolved(context, &repo_key, &branch, force);
    }

    for (repo_key, branch) in resolve_task_targets(context, tasks)? {
        run_resolved(context, &repo_key, &branch, force)?;
    }

    Ok(())
}

fn resolve_task_targets(
    context: &RuntimeEnvironment,
    tasks: &[String],
) -> Result<Vec<(RepoKey, BranchName)>> {
    if let [repo_arg, branch_arg] = tasks
        && let Some(target) = resolve_explicit_repo_branch_target(context, repo_arg, branch_arg)?
    {
        return Ok(vec![target]);
    }

    let mut seen = HashSet::new();
    let mut targets = Vec::new();

    for task in tasks {
        let target = context.tasks().resolve_task_from_query(task)?;
        if seen.insert(target.clone()) {
            targets.push(target);
        }
    }

    Ok(targets)
}

fn resolve_explicit_repo_branch_target(
    context: &RuntimeEnvironment,
    repo_arg: &str,
    branch_arg: &str,
) -> Result<Option<(RepoKey, BranchName)>> {
    let repo_key = match context.tasks().resolve_repo_key_input(repo_arg) {
        Ok(repo_key) => repo_key,
        Err(Error::NotFound(_)) => return Ok(None),
        Err(err) => return Err(err),
    };
    let branch = BranchName::new(branch_arg);
    let worktree = context.tasks().resolve_worktree_path(&repo_key, &branch);

    Ok(worktree.exists().then_some((repo_key, branch)))
}

pub(crate) fn run_resolved(
    context: &RuntimeEnvironment,
    repo_key: &RepoKey,
    branch: &BranchName,
    force: bool,
) -> Result<()> {
    let gitdir = context.layout().repo_gitdir_path(repo_key);
    let worktree = context.tasks().resolve_worktree_path(repo_key, branch);

    if !gitdir.is_dir() {
        return Err(Error::not_found(format!("Repo not found: {repo_key}")));
    }

    let state = classify_worktree_state(&worktree);
    match state {
        WorktreeState::Stale => {
            process::warn(&format!(
                "Worktree metadata is stale for {}. Pruning stale entries.",
                worktree.display()
            ));
            prune(&gitdir)?;

            if worktree.exists() {
                let is_empty = fs::read_dir(&worktree)?.next().is_none();
                if is_empty {
                    drop(fs::remove_dir(&worktree));
                } else {
                    process::warn(&format!(
                        "Left non-worktree directory in place: {}",
                        worktree.display()
                    ));
                }
            }
        }
        WorktreeState::Live => {
            check_dirty_worktree(force, &worktree)?;
            remove(&gitdir, &worktree, force)?;

            if let Some(parent) = worktree.parent() {
                drop(fs::remove_dir(parent));
            }
        }
    }

    let wt_name = worktrees::worktree_name(context.layout().wt_dir(), repo_key, &worktree);
    finish_session(repo_key, &wt_name, &gitdir)?;
    if let Err(err) = cleanup(repo_key, &wt_name) {
        process::warn(&format!(
            "Failed to remove task editor state for {repo_key} {branch}: {err}"
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorktreeState {
    /// `.git` marker missing — worktree metadata is stale.
    Stale,
    /// `.git` marker present — live worktree.
    Live,
}

fn classify_worktree_state(worktree: &std::path::Path) -> WorktreeState {
    if worktree.join(".git").exists() {
        WorktreeState::Live
    } else {
        WorktreeState::Stale
    }
}

fn check_dirty_worktree(force: bool, worktree: &std::path::Path) -> Result<()> {
    if force {
        return Ok(());
    }
    let status = status_porcelain(worktree)?;
    if is_status_dirty(&status) {
        return Err(Error::DirtyWorktree);
    }
    Ok(())
}

/// Returns `true` when a `git status --porcelain` output indicates uncommitted
/// changes (i.e. the output is non-empty after trimming).
fn is_status_dirty(status: &str) -> bool {
    !status.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    use super::{WorktreeState, classify_worktree_state, is_status_dirty};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-finish-{name}"));
            _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            _ = fs::remove_dir_all(&self.0);
        }
    }

    mod classify_worktree {
        use super::*;

        #[test]
        fn live_when_dot_git_exists() {
            let dir = TempDir::new("live");
            fs::create_dir_all(dir.path().join(".git")).unwrap();
            assert_eq!(classify_worktree_state(dir.path()), WorktreeState::Live);
        }

        #[test]
        fn stale_when_dot_git_missing() {
            let dir = TempDir::new("stale");
            assert_eq!(classify_worktree_state(dir.path()), WorktreeState::Stale);
        }

        #[test]
        fn stale_when_directory_does_not_exist() {
            let path = env::temp_dir().join("task-rs-finish-nonexistent");
            _ = fs::remove_dir_all(&path);
            assert_eq!(classify_worktree_state(&path), WorktreeState::Stale);
        }
    }

    mod dirty_status {
        use super::*;

        #[test]
        fn empty_output_is_clean() {
            assert!(!is_status_dirty(""));
        }

        #[test]
        fn whitespace_only_is_clean() {
            assert!(!is_status_dirty("   \n  "));
        }

        #[test]
        fn modified_file_is_dirty() {
            assert!(is_status_dirty(" M src/main.rs\n"));
        }

        #[test]
        fn untracked_file_is_dirty() {
            assert!(is_status_dirty("?? new_file.txt\n"));
        }

        #[test]
        fn multiple_changes_are_dirty() {
            assert!(is_status_dirty("M  src/lib.rs\n?? scratch.txt\n"));
        }
    }

    mod check_dirty_worktree_force {
        use std::path::Path;

        use super::super::check_dirty_worktree;

        #[test]
        fn force_true_always_succeeds_without_reading_status() {
            // Pass a nonexistent path — if it tries to run git it would fail.
            let bogus = Path::new("/nonexistent/path/that/cannot/exist");
            let result = check_dirty_worktree(true, bogus);
            assert!(
                result.is_ok(),
                "force=true should bypass dirty check: {result:?}"
            );
        }
    }

    mod is_status_dirty_edge_cases {
        use super::super::is_status_dirty;

        #[test]
        fn single_newline_is_clean() {
            assert!(!is_status_dirty("\n"));
        }

        #[test]
        fn tab_only_is_clean() {
            assert!(!is_status_dirty("\t"));
        }

        #[test]
        fn deleted_file_marker_is_dirty() {
            assert!(is_status_dirty("D  deleted_file.rs\n"));
        }

        #[test]
        fn added_file_marker_is_dirty() {
            assert!(is_status_dirty("A  new_file.rs\n"));
        }

        #[test]
        fn renamed_file_marker_is_dirty() {
            assert!(is_status_dirty("R  old.rs -> new.rs\n"));
        }
    }

    mod worktree_state_classification {
        use std::{env, fs};

        use super::super::{WorktreeState, classify_worktree_state};

        struct TempDir(std::path::PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-finish-state-{name}"));
                _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn dot_git_file_also_counts_as_live() {
            // In a linked worktree, .git is a file, not a directory.
            // Our classify function just checks `.git` exists — file or dir.
            let dir = TempDir::new("git-file");
            fs::write(dir.0.join(".git"), "gitdir: ../main/.git/worktrees/feat")
                .expect("write .git file");
            assert_eq!(
                classify_worktree_state(&dir.0),
                WorktreeState::Live,
                ".git file should count as live worktree"
            );
        }
    }

    mod resolve_explicit_repo_branch {
        use super::{super::resolve_explicit_repo_branch_target, *};
        use crate::runtime::{BranchName, RepoKey, environment::RuntimeEnvironment};

        fn environment_for(dir: &TempDir) -> RuntimeEnvironment {
            fs::create_dir_all(dir.path().join("repos")).unwrap();
            fs::create_dir_all(dir.path().join("wt")).unwrap();
            fs::create_dir_all(dir.path().join("detached")).unwrap();
            RuntimeEnvironment::from_paths(
                dir.path().join("repos"),
                dir.path().join("wt"),
                dir.path().join("detached"),
            )
        }

        #[test]
        fn returns_target_when_dot_git_missing() {
            let dir = TempDir::new("explicit-stale");
            let context = environment_for(&dir);
            fs::create_dir_all(dir.path().join("wt/github.com/me/app/feature-x")).unwrap();

            let target =
                resolve_explicit_repo_branch_target(&context, "github.com/me/app", "feature-x")
                    .unwrap();

            assert_eq!(
                target,
                Some((
                    RepoKey::new("github.com/me/app"),
                    BranchName::new("feature-x")
                ))
            );
        }

        #[test]
        fn returns_target_when_dot_git_exists() {
            let dir = TempDir::new("explicit-live");
            let context = environment_for(&dir);
            fs::create_dir_all(dir.path().join("wt/github.com/me/app/feature-y/.git")).unwrap();

            let target =
                resolve_explicit_repo_branch_target(&context, "github.com/me/app", "feature-y")
                    .unwrap();

            assert_eq!(
                target,
                Some((
                    RepoKey::new("github.com/me/app"),
                    BranchName::new("feature-y")
                ))
            );
        }

        #[test]
        fn returns_none_when_worktree_path_is_absent() {
            let dir = TempDir::new("explicit-absent");
            let context = environment_for(&dir);

            let target =
                resolve_explicit_repo_branch_target(&context, "github.com/me/app", "feature-z")
                    .unwrap();

            assert_eq!(target, None);
        }
    }
}
