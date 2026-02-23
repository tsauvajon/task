use crossterm::event::{self, Event, KeyCode, KeyModifiers};

use crate::runtime::RuntimeEnvironment;

use self::render::render;
use self::state::{InputMode, UiAction, UiState};
use self::tasks::{finish_selected, load_rows, park_selected, resolve_create_repo};
use self::terminal::{init_terminal, restore_terminal};

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

    let mut terminal = init_terminal()?;
    let ui_result = run_event_loop(context, repo_arg, &mut terminal, &mut state);
    let restore_result = restore_terminal(&mut terminal);
    restore_result?;

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
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(UiAction::Quit);
            }

            match state.mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(UiAction::Quit),
                    KeyCode::Down | KeyCode::Char('j') => state.move_next(),
                    KeyCode::Up | KeyCode::Char('k') => state.move_prev(),
                    KeyCode::Char('/') => {
                        state.mode = InputMode::Filter;
                        state.message = "Filter mode: type to refine tasks".to_string();
                    }
                    KeyCode::Char('?') => {
                        state.show_help = !state.show_help;
                    }
                    KeyCode::Char('c') => {
                        state.mode = InputMode::Create;
                        state.create_branch.clear();
                        state.message = "Create mode: type branch name".to_string();
                    }
                    KeyCode::Char('f') => {
                        finish_selected(context, state)?;
                        let rows = load_rows(context, repo_arg)?;
                        state.set_rows(rows);
                    }
                    KeyCode::Char('r') => {
                        let rows = load_rows(context, repo_arg)?;
                        state.set_rows(rows);
                        state.message = "Refreshed task list".to_string();
                    }
                    KeyCode::Char('p') => {
                        park_selected(context, state)?;
                        let rows = load_rows(context, repo_arg)?;
                        state.set_rows(rows);
                    }
                    KeyCode::Enter => {
                        if let Some(row) = state.selected_row() {
                            return Ok(UiAction::Open(row.clone()));
                        }
                    }
                    _ => {}
                },
                InputMode::Filter => match key.code {
                    KeyCode::Esc => {
                        state.mode = InputMode::Normal;
                        state.message = "Returned to normal mode".to_string();
                    }
                    KeyCode::Enter => {
                        state.mode = InputMode::Normal;
                        state.message =
                            format!("Filter applied: {} matches", state.filtered_indices.len());
                    }
                    KeyCode::Backspace => {
                        state.filter.pop();
                        state.apply_filter();
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.filter.clear();
                        state.apply_filter();
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.filter.push(ch);
                        state.apply_filter();
                    }
                    _ => {}
                },
                InputMode::Create => match key.code {
                    KeyCode::Esc => {
                        state.mode = InputMode::Normal;
                        state.message = "Create cancelled".to_string();
                    }
                    KeyCode::Enter => {
                        let branch = state.create_branch.trim();
                        if branch.is_empty() {
                            state.message = "Branch name cannot be empty".to_string();
                            continue;
                        }

                        let repo = resolve_create_repo(context, state, repo_arg)?;
                        return Ok(UiAction::Create {
                            repo,
                            branch: branch.to_string(),
                        });
                    }
                    KeyCode::Backspace => {
                        state.create_branch.pop();
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.create_branch.push(ch);
                    }
                    _ => {}
                },
            }
        }
    }
}
