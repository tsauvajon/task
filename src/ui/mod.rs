use crossterm::event::{self, Event};

use self::{
    effects::{
        clone_and_refresh, create_action, finish_and_refresh, park_and_refresh, refresh_repo_rows,
        refresh_task_rows, toggle_detach_and_refresh,
    },
    intent::{UiIntent, from_key},
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
    let _process_log_capture = ProcessLogCaptureGuard::new();
    let ui_result = run_event_loop(context, terminal.terminal_mut(), &mut state);

    match ui_result? {
        UiAction::Quit => Ok(()),
        UiAction::Open(row) => context
            .tasks()
            .launch_workspace(&row.repo, &row.branch, &row.path),
        UiAction::Create { repo, branch } => {
            crate::commands::start::run(context, &repo, &branch, None, false)
        }
    }
}

struct ProcessLogCaptureGuard;

impl ProcessLogCaptureGuard {
    fn new() -> Self {
        crate::runtime::process::enable_log_capture();
        Self
    }
}

impl Drop for ProcessLogCaptureGuard {
    fn drop(&mut self) {
        crate::runtime::process::disable_log_capture();
    }
}

fn run_event_loop(
    context: &RuntimeEnvironment,
    terminal: &mut terminal::AppTerminal,
    state: &mut UiState,
) -> Result<UiAction> {
    loop {
        state.append_activity_lines(crate::runtime::process::take_captured_logs());
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
                    if let Some(repo) = state.selected_repo_row().map(|row| row.repo.to_string()) {
                        state.select_repo_for_tasks(repo);
                        match refresh_task_rows(context, state) {
                            Ok(()) => {
                                state.message = "Opened selected repository tasks".to_string()
                            }
                            Err(err) => state.message = err.to_string(),
                        }
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
            if let Err(err) = finish_and_refresh(context, state) {
                state.message = err.to_string();
            }
            Ok(None)
        }
        UiIntent::RefreshCurrentView => {
            match state.view {
                ViewMode::Tasks => match refresh_task_rows(context, state) {
                    Ok(()) => state.message = "Refreshed task list".to_string(),
                    Err(err) => state.message = err.to_string(),
                },
                ViewMode::Repos => match refresh_repo_rows(context, state) {
                    Ok(()) => state.message = "Refreshed repo list".to_string(),
                    Err(err) => state.message = err.to_string(),
                },
            }
            Ok(None)
        }
        UiIntent::ParkSelected => {
            if state.view != ViewMode::Tasks {
                state.message = "Park is only available in Tasks view".to_string();
                return Ok(None);
            }
            if let Err(err) = park_and_refresh(context, state) {
                state.message = err.to_string();
            }
            Ok(None)
        }
        UiIntent::ToggleDetach => {
            if state.view != ViewMode::Repos {
                state.message = "Detach toggle is only available in Repos view".to_string();
                return Ok(None);
            }
            match toggle_detach_and_refresh(context, state) {
                Ok(msg) => state.message = msg,
                Err(err) => state.message = err.to_string(),
            }
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

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{apply_intent, state::UiAction};
    use crate::{
        runtime::environment::RuntimeEnvironment,
        ui::{
            intent::UiIntent,
            state::{InputMode, UiState, ViewMode},
        },
    };

    fn test_env() -> RuntimeEnvironment {
        // Use a fixed dir that we only need to exist; create_dir_all is
        // idempotent and safe across parallel test threads.
        let base = env::temp_dir().join("task-rs-ui-mod-tests");
        let repos = base.join("repos");
        let wt = base.join("wt");
        let detached = base.join("detached");
        fs::create_dir_all(&repos).unwrap();
        fs::create_dir_all(&wt).unwrap();
        RuntimeEnvironment::from_paths(&repos, &wt, &detached)
    }

    fn empty_state() -> UiState {
        UiState::new(vec![], vec![], None)
    }

    // ── Quit ─────────────────────────────────────────────────────────────────

    #[test]
    fn quit_returns_quit_action() {
        let ctx = test_env();
        let mut state = empty_state();
        let result = apply_intent(&ctx, &mut state, UiIntent::Quit).unwrap();
        assert!(matches!(result, Some(UiAction::Quit)));
    }

    // ── Noop ─────────────────────────────────────────────────────────────────

    #[test]
    fn noop_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        let result = apply_intent(&ctx, &mut state, UiIntent::Noop).unwrap();
        assert!(result.is_none());
    }

    // ── ToggleHelp ───────────────────────────────────────────────────────────

    #[test]
    fn toggle_help_flips_show_help() {
        let ctx = test_env();
        let mut state = empty_state();
        assert!(!state.show_help);
        apply_intent(&ctx, &mut state, UiIntent::ToggleHelp).unwrap();
        assert!(state.show_help);
        apply_intent(&ctx, &mut state, UiIntent::ToggleHelp).unwrap();
        assert!(!state.show_help);
    }

    // ── SwitchView ───────────────────────────────────────────────────────────

    #[test]
    fn switch_view_from_normal_mode_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        assert_eq!(state.view, ViewMode::Tasks);
        apply_intent(&ctx, &mut state, UiIntent::SwitchView).unwrap();
        assert_eq!(state.view, ViewMode::Repos);
        assert_eq!(state.message, "Switched to Repos view");
    }

    #[test]
    fn switch_view_back_to_tasks_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        apply_intent(&ctx, &mut state, UiIntent::SwitchView).unwrap();
        assert_eq!(state.view, ViewMode::Tasks);
        assert_eq!(state.message, "Switched to Tasks view");
    }

    #[test]
    fn switch_view_in_filter_mode_preserves_filter_and_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        // switch_view resets mode to Normal internally, then we force it back to Filter
        apply_intent(&ctx, &mut state, UiIntent::SwitchView).unwrap();
        // After switch from Tasks→Repos in filter mode, mode stays Filter
        assert_eq!(state.mode, InputMode::Filter);
        assert!(
            state.message.contains("repos"),
            "message should mention repos: {}",
            state.message
        );
    }

    // ── MoveNext / MovePrev ──────────────────────────────────────────────────

    #[test]
    fn move_next_delegates_to_state() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows = vec![
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/b"),
                branch: BranchName::new("main"),
                path: PathBuf::from("/tmp/a"),
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/c"),
                branch: BranchName::new("main"),
                path: PathBuf::from("/tmp/c"),
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        assert_eq!(state.task_selected, 0);
        apply_intent(&ctx, &mut state, UiIntent::MoveNext).unwrap();
        assert_eq!(state.task_selected, 1);
    }

    #[test]
    fn move_prev_delegates_to_state() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows = vec![
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/b"),
                branch: BranchName::new("main"),
                path: PathBuf::from("/tmp/a"),
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/c"),
                branch: BranchName::new("main"),
                path: PathBuf::from("/tmp/c"),
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 1;
        apply_intent(&ctx, &mut state, UiIntent::MovePrev).unwrap();
        assert_eq!(state.task_selected, 0);
    }

    // ── EnterFilterMode ──────────────────────────────────────────────────────

    #[test]
    fn enter_filter_mode_on_tasks_view() {
        let ctx = test_env();
        let mut state = empty_state();
        apply_intent(&ctx, &mut state, UiIntent::EnterFilterMode).unwrap();
        assert_eq!(state.mode, InputMode::Filter);
        assert!(
            state.message.contains("tasks"),
            "message should mention tasks: {}",
            state.message
        );
    }

    #[test]
    fn enter_filter_mode_on_repos_view() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        apply_intent(&ctx, &mut state, UiIntent::EnterFilterMode).unwrap();
        assert_eq!(state.mode, InputMode::Filter);
        assert!(
            state.message.contains("repos"),
            "message should mention repos: {}",
            state.message
        );
    }

    // ── EnterCreateTaskMode ──────────────────────────────────────────────────

    #[test]
    fn enter_create_task_mode_in_tasks_view() {
        let ctx = test_env();
        let mut state = empty_state();
        state.create_branch = "leftover".to_string();
        apply_intent(&ctx, &mut state, UiIntent::EnterCreateTaskMode).unwrap();
        assert_eq!(state.mode, InputMode::CreateTask);
        assert!(state.create_branch.is_empty(), "branch should be cleared");
        assert!(
            state.message.contains("branch"),
            "message should mention branch: {}",
            state.message
        );
    }

    #[test]
    fn enter_create_mode_in_repos_view_enters_clone_mode() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        state.clone_input = "leftover".to_string();
        apply_intent(&ctx, &mut state, UiIntent::EnterCreateTaskMode).unwrap();
        assert_eq!(state.mode, InputMode::CloneRepo);
        assert!(
            state.clone_input.is_empty(),
            "clone_input should be cleared"
        );
        assert!(
            state.message.contains("Clone"),
            "message should mention Clone: {}",
            state.message
        );
    }

    // ── FilterCancel / FilterApply ───────────────────────────────────────────

    #[test]
    fn filter_cancel_returns_to_normal_mode() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        apply_intent(&ctx, &mut state, UiIntent::FilterCancel).unwrap();
        assert_eq!(state.mode, InputMode::Normal);
        assert!(
            state.message.contains("normal"),
            "message should confirm normal mode: {}",
            state.message
        );
    }

    #[test]
    fn filter_apply_returns_to_normal_and_reports_task_match_count() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows = vec![
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/app"),
                branch: BranchName::new("main"),
                path: PathBuf::from("/tmp/a"),
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/ops"),
                branch: BranchName::new("main"),
                path: PathBuf::from("/tmp/b"),
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        state.mode = InputMode::Filter;
        state.filter_text = "app".to_string();
        state.apply_task_filter();

        apply_intent(&ctx, &mut state, UiIntent::FilterApply).unwrap();
        assert_eq!(state.mode, InputMode::Normal);
        assert!(
            state.message.contains('1'),
            "message should mention 1 match: {}",
            state.message
        );
    }

    #[test]
    fn filter_apply_reports_repo_match_count_in_repos_view() {
        use crate::{runtime::RepoKey, ui::state::RepoRow};

        let ctx = test_env();
        let repo_rows = vec![
            RepoRow {
                repo: RepoKey::new("github.com/a/app"),
                open_tasks: 1,
                parked_tasks: 0,
                is_detached: false,
            },
            RepoRow {
                repo: RepoKey::new("github.com/a/ops"),
                open_tasks: 2,
                parked_tasks: 0,
                is_detached: false,
            },
        ];
        let mut state = UiState::new(vec![], repo_rows, None);
        state.view = ViewMode::Repos;
        state.mode = InputMode::Filter;
        state.filter_text = "ops".to_string();
        state.apply_repo_filter();

        apply_intent(&ctx, &mut state, UiIntent::FilterApply).unwrap();
        assert_eq!(state.mode, InputMode::Normal);
        assert!(
            state.message.contains('1'),
            "message should mention 1 match: {}",
            state.message
        );
    }

    // ── Filter text mutations ────────────────────────────────────────────────

    #[test]
    fn filter_append_adds_char() {
        let ctx = test_env();
        let mut state = empty_state();
        apply_intent(&ctx, &mut state, UiIntent::FilterAppend('x')).unwrap();
        assert_eq!(state.filter_text, "x");
    }

    #[test]
    fn filter_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.filter_text = "ab".to_string();
        apply_intent(&ctx, &mut state, UiIntent::FilterBackspace).unwrap();
        assert_eq!(state.filter_text, "a");
    }

    #[test]
    fn filter_clear_empties_filter() {
        let ctx = test_env();
        let mut state = empty_state();
        state.filter_text = "something".to_string();
        apply_intent(&ctx, &mut state, UiIntent::FilterClear).unwrap();
        assert_eq!(state.filter_text, "");
    }

    // ── CreateCancel / CreateAppend / CreateBackspace ────────────────────────

    #[test]
    fn create_cancel_returns_to_normal() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CreateTask;
        apply_intent(&ctx, &mut state, UiIntent::CreateCancel).unwrap();
        assert_eq!(state.mode, InputMode::Normal);
        assert!(
            state.message.contains("cancel"),
            "message should mention cancel: {}",
            state.message
        );
    }

    #[test]
    fn create_append_appends_char_to_branch() {
        let ctx = test_env();
        let mut state = empty_state();
        apply_intent(&ctx, &mut state, UiIntent::CreateAppend('f')).unwrap();
        apply_intent(&ctx, &mut state, UiIntent::CreateAppend('e')).unwrap();
        apply_intent(&ctx, &mut state, UiIntent::CreateAppend('a')).unwrap();
        assert_eq!(state.create_branch, "fea");
    }

    #[test]
    fn create_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.create_branch = "fea".to_string();
        apply_intent(&ctx, &mut state, UiIntent::CreateBackspace).unwrap();
        assert_eq!(state.create_branch, "fe");
    }

    // ── CloneCancel / CloneAppend / CloneBackspace / CloneClear ─────────────

    #[test]
    fn clone_cancel_returns_to_normal() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CloneRepo;
        apply_intent(&ctx, &mut state, UiIntent::CloneCancel).unwrap();
        assert_eq!(state.mode, InputMode::Normal);
        assert!(
            state.message.contains("cancel"),
            "message should mention cancel: {}",
            state.message
        );
    }

    #[test]
    fn clone_append_appends_chars() {
        let ctx = test_env();
        let mut state = empty_state();
        apply_intent(&ctx, &mut state, UiIntent::CloneAppend('g')).unwrap();
        apply_intent(&ctx, &mut state, UiIntent::CloneAppend('h')).unwrap();
        assert_eq!(state.clone_input, "gh");
    }

    #[test]
    fn clone_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.clone_input = "gh".to_string();
        apply_intent(&ctx, &mut state, UiIntent::CloneBackspace).unwrap();
        assert_eq!(state.clone_input, "g");
    }

    #[test]
    fn clone_clear_empties_input() {
        let ctx = test_env();
        let mut state = empty_state();
        state.clone_input = "something".to_string();
        apply_intent(&ctx, &mut state, UiIntent::CloneClear).unwrap();
        assert!(state.clone_input.is_empty());
    }

    // ── FinishSelected / ParkSelected guard in non-Tasks view ────────────────

    #[test]
    fn finish_selected_in_repos_view_sets_message_and_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        let result = apply_intent(&ctx, &mut state, UiIntent::FinishSelected).unwrap();
        assert!(result.is_none());
        assert!(
            state.message.contains("Tasks view"),
            "message should mention Tasks view: {}",
            state.message
        );
    }

    #[test]
    fn park_selected_in_repos_view_sets_message_and_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        let result = apply_intent(&ctx, &mut state, UiIntent::ParkSelected).unwrap();
        assert!(result.is_none());
        assert!(
            state.message.contains("Tasks view"),
            "message should mention Tasks view: {}",
            state.message
        );
    }

    // ── ToggleDetach ─────────────────────────────────────────────────────────

    #[test]
    fn toggle_detach_in_tasks_view_sets_message_and_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        // Default view is Tasks
        assert_eq!(state.view, ViewMode::Tasks);
        let result = apply_intent(&ctx, &mut state, UiIntent::ToggleDetach).unwrap();
        assert!(result.is_none());
        assert!(
            state.message.contains("Repos view"),
            "message should mention Repos view: {}",
            state.message
        );
    }

    #[test]
    fn toggle_detach_in_repos_view_with_no_selection_sets_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        // No repo rows → no selection → graceful message
        let result = apply_intent(&ctx, &mut state, UiIntent::ToggleDetach).unwrap();
        assert!(result.is_none());
        assert!(
            state.message.contains("No repo selected"),
            "message should mention 'No repo selected': {}",
            state.message
        );
    }

    // ── OpenSelected on Tasks view with no selection ─────────────────────────

    #[test]
    fn open_selected_on_empty_tasks_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        let result = apply_intent(&ctx, &mut state, UiIntent::OpenSelected).unwrap();
        assert!(
            result.is_none(),
            "should not return an action with no tasks"
        );
    }

    // ── OpenSelected on Tasks view with a selection ──────────────────────────

    #[test]
    fn open_selected_returns_open_action_with_selected_task() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let row = TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new("github.com/a/b"),
            branch: BranchName::new("my-branch"),
            path: PathBuf::from("/tmp/a"),
        };
        let mut state = UiState::new(vec![row], vec![], None);
        let result = apply_intent(&ctx, &mut state, UiIntent::OpenSelected).unwrap();
        assert!(
            matches!(result, Some(UiAction::Open(_))),
            "should return Open action"
        );
    }

    // ── CreateSubmit with empty branch ───────────────────────────────────────

    #[test]
    fn create_submit_with_empty_branch_sets_error_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.create_branch = "  ".to_string(); // whitespace only
        let result = apply_intent(&ctx, &mut state, UiIntent::CreateSubmit).unwrap();
        assert!(result.is_none(), "should not return action on empty branch");
        assert!(
            state.message.contains("empty") || state.message.contains("cannot"),
            "message should mention empty branch: {}",
            state.message
        );
    }
}
