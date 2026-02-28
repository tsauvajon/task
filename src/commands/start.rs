use std::fs;

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process, BranchName},
    tools::git::{
        refs::{detect_default_base, fetch_origin_refs, ref_exists, rev_exists},
        worktrees::{add_existing_branch, add_from_base, add_tracking_remote_branch},
    },
};

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: &str,
    branch: &str,
    base_ref: Option<&str>,
) -> Result<()> {
    context.tasks().ensure_layout()?;
    let repo_key = context.tasks().resolve_repo_key_input(repo_arg)?;
    context.tasks().ensure_repo_available(repo_arg, &repo_key)?;

    let gitdir = context.layout().repo_gitdir_path(&repo_key);
    fetch_origin_refs(&gitdir)?;

    let base_ref = base_ref
        .map(str::to_string)
        .unwrap_or_else(|| detect_default_base(&gitdir));

    let branch_name = BranchName::new(branch);
    let worktree = context.layout().worktree_path(&repo_key, &branch_name);

    match classify_worktree_path(&worktree) {
        WorktreePathState::NotAWorktree => {
            return Err(Error::failed(format!(
                "Path exists but is not a git worktree: {}",
                worktree.display()
            )));
        }
        WorktreePathState::Existing => {
            process::log(&format!(
                "Reusing existing worktree: {}",
                worktree.display()
            ));
            return context
                .tasks()
                .launch_workspace(&repo_key, &branch_name, &worktree);
        }
        WorktreePathState::New => {}
    }

    if let Some(parent) = worktree.parent() {
        fs::create_dir_all(parent)?;
    }

    let strategy = resolve_branch_strategy(
        branch,
        &base_ref,
        |reference| ref_exists(&gitdir, reference),
        |revision| rev_exists(&gitdir, revision),
    )?;

    match strategy {
        BranchStrategy::ExistingLocal => add_existing_branch(&gitdir, &worktree, branch)?,
        BranchStrategy::TrackRemote => {
            add_tracking_remote_branch(&gitdir, &worktree, branch)?;
        }
        BranchStrategy::CreateFromBase { base } => {
            add_from_base(&gitdir, &worktree, branch, &base)?;
        }
    }

    context
        .tasks()
        .launch_workspace(&repo_key, &branch_name, &worktree)
}

/// Outcome of inspecting the target worktree path on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorktreePathState {
    /// Path exists but has no `.git` — something else lives there.
    NotAWorktree,
    /// Path exists with `.git` — reuse the existing worktree.
    Existing,
    /// Path does not exist — create a new worktree.
    New,
}

fn classify_worktree_path(worktree: &std::path::Path) -> WorktreePathState {
    if worktree.join(".git").exists() {
        WorktreePathState::Existing
    } else if worktree.exists() {
        WorktreePathState::NotAWorktree
    } else {
        WorktreePathState::New
    }
}

/// Which git operation to perform when creating the worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BranchStrategy {
    /// Branch exists locally — attach worktree to it.
    ExistingLocal,
    /// Branch exists on `origin` — create a tracking worktree.
    TrackRemote,
    /// Neither local nor remote — create from a base ref.
    CreateFromBase { base: String },
}

/// Pure decision function: given ref/rev existence predicates, decide which
/// strategy to use for worktree creation.
fn resolve_branch_strategy(
    branch: &str,
    base_ref: &str,
    ref_exists_fn: impl Fn(&str) -> bool,
    rev_exists_fn: impl Fn(&str) -> bool,
) -> Result<BranchStrategy> {
    if ref_exists_fn(&format!("refs/heads/{branch}")) {
        return Ok(BranchStrategy::ExistingLocal);
    }

    if ref_exists_fn(&format!("refs/remotes/origin/{branch}")) {
        return Ok(BranchStrategy::TrackRemote);
    }

    if !rev_exists_fn(base_ref) {
        return Err(Error::not_found(format!("Base ref not found: {base_ref}")));
    }

    Ok(BranchStrategy::CreateFromBase {
        base: base_ref.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    use super::{
        classify_worktree_path, resolve_branch_strategy, BranchStrategy, WorktreePathState,
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-start-{name}"));
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

    mod classify_worktree_path_tests {
        use super::*;

        #[test]
        fn existing_when_dot_git_present() {
            let dir = TempDir::new("wt-existing");
            fs::create_dir_all(dir.path().join(".git")).unwrap();
            assert_eq!(
                classify_worktree_path(dir.path()),
                WorktreePathState::Existing
            );
        }

        #[test]
        fn not_a_worktree_when_dir_exists_without_dot_git() {
            let dir = TempDir::new("wt-not-wt");
            assert_eq!(
                classify_worktree_path(dir.path()),
                WorktreePathState::NotAWorktree
            );
        }

        #[test]
        fn new_when_path_missing() {
            let path = env::temp_dir().join("task-rs-start-missing");
            let _ = fs::remove_dir_all(&path);
            assert_eq!(classify_worktree_path(&path), WorktreePathState::New);
        }
    }

    mod resolve_branch_strategy_tests {
        use super::*;

        #[test]
        fn prefers_local_branch_when_exists() {
            let result = resolve_branch_strategy(
                "feature",
                "origin/main",
                |r| r == "refs/heads/feature",
                |_| true,
            )
            .unwrap();
            assert_eq!(result, BranchStrategy::ExistingLocal);
        }

        #[test]
        fn falls_back_to_remote_tracking() {
            let result = resolve_branch_strategy(
                "feature",
                "origin/main",
                |r| r == "refs/remotes/origin/feature",
                |_| true,
            )
            .unwrap();
            assert_eq!(result, BranchStrategy::TrackRemote);
        }

        #[test]
        fn creates_from_base_when_no_existing_refs() {
            let result =
                resolve_branch_strategy("new-branch", "origin/main", |_| false, |_| true).unwrap();
            assert_eq!(
                result,
                BranchStrategy::CreateFromBase {
                    base: "origin/main".to_string()
                }
            );
        }

        #[test]
        fn errors_when_base_ref_not_found() {
            let result = resolve_branch_strategy("new-branch", "origin/main", |_| false, |_| false);
            let err = result.unwrap_err();
            assert!(err.to_string().contains("Base ref not found"));
        }

        #[test]
        fn local_takes_priority_over_remote() {
            // Both local and remote exist — local wins.
            let result = resolve_branch_strategy(
                "feature",
                "origin/main",
                |_| true, // all refs "exist"
                |_| true,
            )
            .unwrap();
            assert_eq!(result, BranchStrategy::ExistingLocal);
        }

        #[test]
        fn create_from_base_uses_supplied_base_ref() {
            let result =
                resolve_branch_strategy("new-branch", "origin/develop", |_| false, |_| true)
                    .unwrap();
            assert_eq!(
                result,
                BranchStrategy::CreateFromBase {
                    base: "origin/develop".to_string()
                }
            );
        }

        #[test]
        fn error_message_includes_base_ref_name() {
            let result =
                resolve_branch_strategy("new-branch", "origin/nonexistent", |_| false, |_| false);
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("origin/nonexistent"),
                "error should name the missing base: {}",
                err
            );
        }

        #[test]
        fn local_branch_check_activates_on_correct_ref_format() {
            use std::cell::Cell;
            let matched = Cell::new(false);
            let result = resolve_branch_strategy(
                "mybranch",
                "origin/main",
                |r| {
                    if r == "refs/heads/mybranch" {
                        matched.set(true);
                        true
                    } else {
                        false
                    }
                },
                |_| false,
            );
            assert!(matched.get(), "should have checked refs/heads/mybranch");
            assert_eq!(result.unwrap(), BranchStrategy::ExistingLocal);
        }

        #[test]
        fn remote_branch_check_activates_on_correct_ref_format() {
            use std::cell::Cell;
            let matched = Cell::new(false);
            let result = resolve_branch_strategy(
                "mybranch",
                "origin/main",
                |r| {
                    if r == "refs/remotes/origin/mybranch" {
                        matched.set(true);
                        true
                    } else {
                        false
                    }
                },
                |_| false,
            );
            assert!(
                matched.get(),
                "should have checked refs/remotes/origin/mybranch"
            );
            assert_eq!(result.unwrap(), BranchStrategy::TrackRemote);
        }
    }

    mod classify_worktree_path_extra {
        use super::*;

        #[test]
        fn dot_git_file_counts_as_existing_worktree() {
            // In linked worktrees `.git` is a file, not a directory —
            // classify_worktree_path only checks existence.
            let dir = TempDir::new("wt-git-file");
            fs::write(
                dir.path().join(".git"),
                "gitdir: ../../main/.git/worktrees/x",
            )
            .unwrap();
            assert_eq!(
                classify_worktree_path(dir.path()),
                WorktreePathState::Existing
            );
        }
    }
}
