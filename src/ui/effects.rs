use crate::runtime::environment::RuntimeEnvironment;

use super::state::{UiAction, UiState};
use super::tasks::{
    clone_from_input, finish_selected, load_repo_rows, load_task_rows, park_selected,
    resolve_create_repo,
};

pub(super) fn refresh_task_rows(
    context: &RuntimeEnvironment,
    state: &mut UiState,
) -> Result<(), String> {
    let rows = load_task_rows(context, state.task_repo_scope.as_deref())?;
    state.set_task_rows(rows);
    Ok(())
}

pub(super) fn refresh_repo_rows(
    context: &RuntimeEnvironment,
    state: &mut UiState,
) -> Result<(), String> {
    let rows = load_repo_rows(context)?;
    state.set_repo_rows(rows);
    Ok(())
}

pub(super) fn finish_and_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
) -> Result<(), String> {
    finish_selected(context, state)?;
    refresh_task_rows(context, state)
}

pub(super) fn park_and_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
) -> Result<(), String> {
    park_selected(context, state)?;
    refresh_task_rows(context, state)
}

pub(super) fn create_action(
    context: &RuntimeEnvironment,
    state: &UiState,
) -> Result<UiAction, String> {
    let branch = state.create_branch.trim();
    if branch.is_empty() {
        return Err("Branch name cannot be empty".to_string());
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
) -> Result<String, String> {
    let cloned_repo = clone_from_input(context, &state.clone_input)?;
    refresh_repo_rows(context, state)?;
    if let Some(repo) = state.task_repo_scope.as_deref()
        && repo == cloned_repo
    {
        refresh_task_rows(context, state)?;
    }
    Ok(cloned_repo)
}
