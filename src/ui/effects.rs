use super::{
    loader::{self, LoaderHandle},
    state::{UiAction, UiState},
    tasks::{clone_from_input, finish_selected, park_selected, resolve_create_repo},
};
use crate::{
    commands::detach as detach_cmd, error::Result, runtime::environment::RuntimeEnvironment,
};

/// Cancel the current loader and spawn a fresh one that scans both the
/// tasks and the repos in parallel. Rows are cleared immediately and
/// stream back in as each repo finishes.
pub(super) fn refresh_all(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) {
    let generation = state.begin_load();
    let new_handle = loader::spawn(context.clone(), state.task_repo_scope.clone(), generation);
    // Dropping the old handle sets its stop flag; workers exit between
    // repos without blocking this call.
    let _ = std::mem::replace(loader, new_handle);
}

pub(super) fn refresh_session_state(
    state: &UiState,
    task_card_details_refresh: &mut Option<LoaderHandle>,
) {
    if task_card_details_refresh.is_some() || state.task_rows.is_empty() {
        return;
    }

    let paths: Vec<_> = state.task_rows.iter().map(|row| row.path.clone()).collect();
    *task_card_details_refresh = Some(loader::spawn_task_card_details_refresh(paths));
}

pub(super) fn finish_and_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) -> Result<()> {
    finish_selected(context, state)?;
    refresh_all(context, state, loader);
    Ok(())
}

pub(super) fn park_and_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) -> Result<()> {
    park_selected(context, state)?;
    refresh_all(context, state, loader);
    Ok(())
}

pub(super) fn create_action(context: &RuntimeEnvironment, state: &UiState) -> Result<UiAction> {
    let branch = state.create_branch.trim();
    if branch.is_empty() {
        return Err(crate::error::Error::failed("Branch name cannot be empty"));
    }

    let repo = resolve_create_repo(context, state, state.task_repo_scope.as_deref())?;
    Ok(UiAction::Create {
        repo,
        branch: branch.to_string(),
    })
}

pub(super) fn clone_and_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) -> Result<String> {
    let cloned_repo = clone_from_input(context, &state.clone_input)?;
    refresh_all(context, state, loader);
    Ok(cloned_repo)
}

/// Toggle the detached worktree for the currently selected repo.
/// If a detached worktree already exists for the repo, removes it.
/// Otherwise, creates one.
/// Returns a human-readable status message on success.
pub(super) fn toggle_detach_and_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) -> Result<String> {
    let Some(row) = state.selected_repo_row().cloned() else {
        return Ok("No repo selected".to_string());
    };

    let repo_key_str = row.repo.to_string();
    let message = if row.is_detached {
        detach_cmd::remove(context, &repo_key_str, false)?;
        format!("Removed detached worktree for {repo_key_str}")
    } else {
        detach_cmd::add(context, &repo_key_str)?;
        format!("Added detached worktree for {repo_key_str}")
    };

    refresh_all(context, state, loader);
    Ok(message)
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::create_action;
    use crate::{
        runtime::environment::RuntimeEnvironment,
        ui::state::{UiAction, UiState},
    };

    fn test_env() -> RuntimeEnvironment {
        let base = env::temp_dir().join("task-rs-ui-effects-tests");
        let repos = base.join("repos");
        let wt = base.join("wt");
        let detached = base.join("detached");
        // create_dir_all is idempotent — safe across parallel test threads.
        fs::create_dir_all(&repos).unwrap();
        fs::create_dir_all(&wt).unwrap();
        RuntimeEnvironment::from_paths(&repos, &wt, &detached)
    }

    fn empty_state() -> UiState {
        UiState::new(vec![], vec![], None)
    }

    mod create_action_tests {
        use super::*;

        #[test]
        fn empty_branch_returns_error() {
            let ctx = test_env();
            let mut state = empty_state();
            state.create_branch = "".to_string();
            let err = create_action(&ctx, &state).expect_err("expected error for empty branch");
            assert!(
                err.to_string().contains("empty") || err.to_string().contains("cannot"),
                "error should mention empty branch: {err}"
            );
        }

        #[test]
        fn whitespace_only_branch_returns_error() {
            let ctx = test_env();
            let mut state = empty_state();
            state.create_branch = "   ".to_string();
            let err =
                create_action(&ctx, &state).expect_err("expected error for whitespace-only branch");
            assert!(
                err.to_string().contains("empty") || err.to_string().contains("cannot"),
                "error should mention empty branch: {err}"
            );
        }

        #[test]
        fn valid_branch_with_selected_task_returns_create_action() {
            use std::path::PathBuf;

            use crate::runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            };

            let ctx = test_env();
            let row = TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/acme/app"),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            };
            let mut state = UiState::new(vec![row], vec![], None);
            state.create_branch = "my-new-feature".to_string();

            let action = create_action(&ctx, &state).expect("create_action should succeed");
            match action {
                UiAction::Create { repo, branch } => {
                    assert_eq!(repo, "github.com/acme/app");
                    assert_eq!(branch, "my-new-feature");
                }
                other => panic!("expected Create action, got {:?}", other),
            }
        }

        #[test]
        fn branch_is_trimmed_before_use() {
            use std::path::PathBuf;

            use crate::runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            };

            let ctx = test_env();
            let row = TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/acme/app"),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            };
            let mut state = UiState::new(vec![row], vec![], None);
            state.create_branch = "  trimmed-branch  ".to_string();

            let action = create_action(&ctx, &state).expect("create_action should succeed");
            match action {
                UiAction::Create { branch, .. } => {
                    assert_eq!(branch, "trimmed-branch");
                }
                other => panic!("expected Create action, got {:?}", other),
            }
        }
    }
}
