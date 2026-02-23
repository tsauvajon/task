use crate::runtime::{RuntimeEnvironment, TaskRow};
use crate::tmux::{self, ParkResult};

use super::state::UiState;

pub(super) fn load_rows(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
) -> Result<Vec<TaskRow>, String> {
    let open_sessions = context.tmux_sessions();
    let mut rows = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.current_repo_key());

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = context.resolve_repo_key_input(repo_arg)?;
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }
        rows.extend(context.repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
    } else {
        for repo_key in context.available_repo_keys()? {
            let gitdir = context.layout().repo_gitdir_path(&repo_key);
            rows.extend(context.repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
        }
    }

    rows.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then(left.repo.cmp(&right.repo))
            .then(left.branch.cmp(&right.branch))
    });

    Ok(rows)
}

pub(super) fn park_selected(
    context: &RuntimeEnvironment,
    state: &mut UiState,
) -> Result<(), String> {
    let Some(row) = state.selected_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    if !tmux::is_available(context.process()) {
        return Err("tmux is not available. Run 'task list' to inspect tasks.".to_string());
    }

    match tmux::park_task(context.process(), &row.repo, &row.branch)? {
        ParkResult::Parked => state.message = format!("Parked task: {} {}", row.repo, row.branch),
        ParkResult::AlreadyParked => {
            state.message = format!("Task already parked: {} {}", row.repo, row.branch)
        }
    }

    Ok(())
}

pub(super) fn finish_selected(
    context: &RuntimeEnvironment,
    state: &mut UiState,
) -> Result<(), String> {
    let Some(row) = state.selected_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    crate::commands::finish::run(context, Some(&row.repo), Some(&row.branch), false)?;
    state.message = format!("Finished task: {} {}", row.repo, row.branch);
    Ok(())
}

pub(super) fn resolve_create_repo(
    context: &RuntimeEnvironment,
    state: &UiState,
    repo_arg: Option<&str>,
) -> Result<String, String> {
    if let Some(row) = state.selected_row() {
        return Ok(row.repo.clone());
    }

    if let Some(repo_arg) = repo_arg {
        return context.resolve_repo_input(Some(repo_arg));
    }

    context.resolve_repo_input(None)
}

fn status_rank(status: &str) -> u8 {
    match status {
        "open" => 0,
        "parked" => 1,
        _ => 2,
    }
}
