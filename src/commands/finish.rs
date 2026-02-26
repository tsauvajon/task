use std::fs;

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::{
        git::worktrees::{prune, remove, status_porcelain},
        tmux::workflow::finish_session,
        vscodium::workflow::cleanup,
    },
};

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
    force: bool,
) -> Result<()> {
    let (repo_key_raw, branch) = context
        .tasks()
        .resolve_repo_branch_inputs(repo_arg, branch_arg)?;
    let repo_key = context.tasks().resolve_repo_key_input(&repo_key_raw)?;
    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    let worktree = context.tasks().resolve_worktree_path(&repo_key, &branch);

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
                    let _ = fs::remove_dir(&worktree);
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
                let _ = fs::remove_dir(parent);
            }
        }
    }

    finish_session(&repo_key, &branch)?;
    if let Err(err) = cleanup(&repo_key, &branch) {
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
    if !status.trim().is_empty() {
        return Err(Error::failed(
            "Worktree has uncommitted changes. Use --force if you really want to remove it.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    use super::{WorktreeState, classify_worktree_state};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-finish-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
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
            let _ = fs::remove_dir_all(&path);
            assert_eq!(classify_worktree_state(&path), WorktreeState::Stale);
        }
    }
}
