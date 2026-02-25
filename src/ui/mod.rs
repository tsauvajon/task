use crossterm::event::{self, Event};

use self::{
    effects::{
        clone_and_refresh, create_action, finish_and_refresh, park_and_refresh, refresh_repo_rows,
        refresh_task_rows,
    },
    intent::{from_key, UiIntent},
    render::render,
    state::{InputMode, UiAction, UiState, ViewMode},
    tasks::{initial_repo_scope, load_repo_rows, load_task_rows},
    terminal::TerminalGuard,
};
use crate::{error::Result, runtime::environment::RuntimeEnvironment};

mod effects;
mod intent;
mod render;
mod state;
mod tasks;
mod terminal;

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;
    let task_repo_scope = initial_repo_scope(context, repo_arg);
    let task_rows = load_task_rows(context, task_repo_scope.as_deref())?;
    let repo_rows = load_repo_rows(context)?;
    let mut state = UiState::new(task_rows, repo_rows, task_repo_scope);

    if state.task_rows.is_empty() {
        state.message = "No tasks found. Press q to quit.".to_string();
    }

    let mut terminal = TerminalGuard::new()?;
    let ui_result = run_event_loop(context, terminal.terminal_mut(), &mut state);

    match ui_result? {
        UiAction::Quit => Ok(()),
        UiAction::Open(row) => context
            .tasks()
            .launch_workspace(&row.repo, &row.branch, &row.path),
        UiAction::Create { repo, branch } => {
            crate::commands::start::run(context, &repo, &branch, None)
        }
    }
}

fn run_event_loop(
    context: &RuntimeEnvironment,
    terminal: &mut terminal::AppTerminal,
    state: &mut UiState,
) -> Result<UiAction> {
    loop {
        terminal.draw(|frame| render(frame, state))?;

        let event = event::read()?;
        if let Event::Key(key) = event {
            let intent = from_key(state.mode, key);
            if let Some(action) = apply_intent(context, state, intent)? {
                return Ok(action);
            }
        }
    }
}

fn apply_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    intent: UiIntent,
) -> Result<Option<UiAction>> {
    match intent {
        UiIntent::Quit => Ok(Some(UiAction::Quit)),
        UiIntent::SwitchView => {
            let was_filter_mode = state.mode == InputMode::Filter;
            state.switch_view();
            if was_filter_mode {
                state.mode = InputMode::Filter;
                state.message = match state.view {
                    ViewMode::Tasks => "Filter mode: type to refine tasks".to_string(),
                    ViewMode::Repos => "Filter mode: type to refine repos".to_string(),
                };
            } else {
                state.message = match state.view {
                    ViewMode::Tasks => "Switched to Tasks view".to_string(),
                    ViewMode::Repos => "Switched to Repos view".to_string(),
                };
            }
            Ok(None)
        }
        UiIntent::MoveNext => {
            state.move_next();
            Ok(None)
        }
        UiIntent::MovePrev => {
            state.move_prev();
            Ok(None)
        }
        UiIntent::ToggleHelp => {
            state.show_help = !state.show_help;
            Ok(None)
        }
        UiIntent::OpenSelected => {
            match state.view {
                ViewMode::Tasks => {
                    if let Some(row) = state.selected_task_row() {
                        return Ok(Some(UiAction::Open(row.clone())));
                    }
                }
                ViewMode::Repos => {
                    if let Some(repo) = state.selected_repo_row().map(|row| row.repo.clone()) {
                        state.select_repo_for_tasks(repo);
                        refresh_task_rows(context, state)?;
                        state.message = "Opened selected repository tasks".to_string();
                    }
                }
            }
            Ok(None)
        }
        UiIntent::EnterFilterMode => {
            state.mode = InputMode::Filter;
            state.message = match state.view {
                ViewMode::Tasks => "Filter mode: type to refine tasks".to_string(),
                ViewMode::Repos => "Filter mode: type to refine repos".to_string(),
            };
            Ok(None)
        }
        UiIntent::EnterCreateTaskMode => {
            match state.view {
                ViewMode::Tasks => {
                    state.mode = InputMode::CreateTask;
                    state.create_branch.clear();
                    state.message = "Create mode: type branch name".to_string();
                }
                ViewMode::Repos => {
                    state.mode = InputMode::CloneRepo;
                    state.clone_input.clear();
                    state.message = "Clone mode: type '<repo-url> [repo-key]'".to_string();
                }
            }
            Ok(None)
        }
        UiIntent::FinishSelected => {
            if state.view != ViewMode::Tasks {
                state.message = "Finish is only available in Tasks view".to_string();
                return Ok(None);
            }
            finish_and_refresh(context, state)?;
            Ok(None)
        }
        UiIntent::RefreshCurrentView => {
            match state.view {
                ViewMode::Tasks => {
                    refresh_task_rows(context, state)?;
                    state.message = "Refreshed task list".to_string();
                }
                ViewMode::Repos => {
                    refresh_repo_rows(context, state)?;
                    state.message = "Refreshed repo list".to_string();
                }
            }
            Ok(None)
        }
        UiIntent::ParkSelected => {
            if state.view != ViewMode::Tasks {
                state.message = "Park is only available in Tasks view".to_string();
                return Ok(None);
            }
            park_and_refresh(context, state)?;
            Ok(None)
        }
        UiIntent::FilterCancel => {
            state.mode = InputMode::Normal;
            state.message = "Returned to normal mode".to_string();
            Ok(None)
        }
        UiIntent::FilterApply => {
            state.mode = InputMode::Normal;
            state.message = match state.view {
                ViewMode::Tasks => {
                    format!(
                        "Filter applied: {} matches",
                        state.task_filtered_indices.len()
                    )
                }
                ViewMode::Repos => {
                    format!(
                        "Filter applied: {} matches",
                        state.repo_filtered_indices.len()
                    )
                }
            };
            Ok(None)
        }
        UiIntent::FilterBackspace => {
            state.filter_backspace();
            Ok(None)
        }
        UiIntent::FilterClear => {
            state.filter_clear();
            Ok(None)
        }
        UiIntent::FilterAppend(ch) => {
            state.filter_append(ch);
            Ok(None)
        }
        UiIntent::CreateCancel => {
            state.mode = InputMode::Normal;
            state.message = "Create cancelled".to_string();
            Ok(None)
        }
        UiIntent::CreateSubmit => match create_action(context, state) {
            Ok(action) => Ok(Some(action)),
            Err(err) => {
                state.message = err.to_string();
                Ok(None)
            }
        },
        UiIntent::CreateBackspace => {
            state.create_branch.pop();
            Ok(None)
        }
        UiIntent::CreateAppend(ch) => {
            state.create_branch.push(ch);
            Ok(None)
        }
        UiIntent::CloneCancel => {
            state.mode = InputMode::Normal;
            state.message = "Clone cancelled".to_string();
            Ok(None)
        }
        UiIntent::CloneSubmit => match clone_and_refresh(context, state) {
            Ok(repo_key) => {
                state.mode = InputMode::Normal;
                state.clone_input.clear();
                state.message = format!("Cloned repo: {repo_key}");
                Ok(None)
            }
            Err(err) => {
                state.message = err.to_string();
                Ok(None)
            }
        },
        UiIntent::CloneBackspace => {
            state.clone_input.pop();
            Ok(None)
        }
        UiIntent::CloneClear => {
            state.clone_input.clear();
            Ok(None)
        }
        UiIntent::CloneAppend(ch) => {
            state.clone_input.push(ch);
            Ok(None)
        }
        UiIntent::Noop => Ok(None),
    }
}
