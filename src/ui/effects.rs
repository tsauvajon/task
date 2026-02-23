use crate::runtime::RuntimeEnvironment;

use super::state::{UiAction, UiState};
use super::tasks::{finish_selected, load_rows, park_selected, resolve_create_repo};

pub(super) fn refresh_rows(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    state: &mut UiState,
) -> Result<(), String> {
    let rows = load_rows(context, repo_arg)?;
    state.set_rows(rows);
    Ok(())
}

pub(super) fn finish_and_refresh(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    state: &mut UiState,
) -> Result<(), String> {
    finish_selected(context, state)?;
    refresh_rows(context, repo_arg, state)
}

pub(super) fn park_and_refresh(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    state: &mut UiState,
) -> Result<(), String> {
    park_selected(context, state)?;
    refresh_rows(context, repo_arg, state)
}

pub(super) fn create_action(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    state: &UiState,
) -> Result<UiAction, String> {
    let branch = state.create_branch.trim();
    if branch.is_empty() {
        return Err("Branch name cannot be empty".to_string());
    }

    let repo = resolve_create_repo(context, state, repo_arg)?;
    Ok(UiAction::Create {
        repo,
        branch: branch.to_string(),
    })
}
