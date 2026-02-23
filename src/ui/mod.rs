use crossterm::event::{self, Event};

use crate::runtime::RuntimeEnvironment;

use self::effects::{create_action, finish_and_refresh, park_and_refresh, refresh_rows};
use self::intent::{UiIntent, from_key};
use self::render::render;
use self::state::{InputMode, UiAction, UiState};
use self::tasks::load_rows;
use self::terminal::TerminalGuard;

mod effects;
mod intent;
mod render;
mod state;
mod tasks;
mod terminal;

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<(), String> {
    context.ensure_layout()?;
    let rows = load_rows(context, repo_arg)?;
    let mut state = UiState::new(rows);

    if state.rows.is_empty() {
        state.message = "No tasks found. Press q to quit.".to_string();
    }

    let mut terminal = TerminalGuard::new()?;
    let ui_result = run_event_loop(context, repo_arg, terminal.terminal_mut(), &mut state);

    match ui_result? {
        UiAction::Quit => Ok(()),
        UiAction::Open(row) => context.launch_workspace(&row.repo, &row.branch, &row.path),
        UiAction::Create { repo, branch } => {
            crate::commands::start::run(context, &repo, &branch, None)
        }
    }
}

fn run_event_loop(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    terminal: &mut terminal::AppTerminal,
    state: &mut UiState,
) -> Result<UiAction, String> {
    loop {
        terminal
            .draw(|frame| render(frame, state))
            .map_err(|e| e.to_string())?;

        let event = event::read().map_err(|e| e.to_string())?;
        if let Event::Key(key) = event {
            let intent = from_key(state.mode, key);
            if let Some(action) = apply_intent(context, repo_arg, state, intent)? {
                return Ok(action);
            }
        }
    }
}

fn apply_intent(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    state: &mut UiState,
    intent: UiIntent,
) -> Result<Option<UiAction>, String> {
    match intent {
        UiIntent::Quit => Ok(Some(UiAction::Quit)),
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
            if let Some(row) = state.selected_row() {
                return Ok(Some(UiAction::Open(row.clone())));
            }
            Ok(None)
        }
        UiIntent::EnterFilterMode => {
            state.mode = InputMode::Filter;
            state.message = "Filter mode: type to refine tasks".to_string();
            Ok(None)
        }
        UiIntent::EnterCreateMode => {
            state.mode = InputMode::Create;
            state.create_branch.clear();
            state.message = "Create mode: type branch name".to_string();
            Ok(None)
        }
        UiIntent::FinishSelected => {
            finish_and_refresh(context, repo_arg, state)?;
            Ok(None)
        }
        UiIntent::RefreshRows => {
            refresh_rows(context, repo_arg, state)?;
            state.message = "Refreshed task list".to_string();
            Ok(None)
        }
        UiIntent::ParkSelected => {
            park_and_refresh(context, repo_arg, state)?;
            Ok(None)
        }
        UiIntent::FilterCancel => {
            state.mode = InputMode::Normal;
            state.message = "Returned to normal mode".to_string();
            Ok(None)
        }
        UiIntent::FilterApply => {
            state.mode = InputMode::Normal;
            state.message = format!("Filter applied: {} matches", state.filtered_indices.len());
            Ok(None)
        }
        UiIntent::FilterBackspace => {
            state.filter.pop();
            state.apply_filter();
            Ok(None)
        }
        UiIntent::FilterClear => {
            state.filter.clear();
            state.apply_filter();
            Ok(None)
        }
        UiIntent::FilterAppend(ch) => {
            state.filter.push(ch);
            state.apply_filter();
            Ok(None)
        }
        UiIntent::CreateCancel => {
            state.mode = InputMode::Normal;
            state.message = "Create cancelled".to_string();
            Ok(None)
        }
        UiIntent::CreateSubmit => match create_action(context, repo_arg, state) {
            Ok(action) => Ok(Some(action)),
            Err(message) => {
                state.message = message;
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
        UiIntent::Noop => Ok(None),
    }
}
