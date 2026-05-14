use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};

use self::{
    effects::{
        clone_and_refresh, create_action, finish_and_refresh, park_and_refresh, refresh_all,
        toggle_detach_and_refresh,
    },
    intent::{UiIntent, from_key},
    loader::LoaderHandle,
    render::render,
    state::{InputMode, UiAction, UiState, ViewMode},
    tasks::{TaskRefreshFingerprint, initial_repo_scope, task_refresh_fingerprint},
    terminal::TerminalGuard,
};
use crate::{error::Result, runtime::environment::RuntimeEnvironment};

mod effects;
mod intent;
mod loader;
mod render;
mod state;
mod tasks;
mod terminal;
mod theme;

/// How long `event::poll` waits before returning with no event. This is
/// also the spinner frame interval and the cadence at which loader
/// messages are drained from the channel.
const TICK: Duration = Duration::from_millis(100);

/// Cadence of the background OpenCode-state refresher. Short enough to
/// feel live while the user reads the Tasks view.
const OPENCODE_REFRESH_INTERVAL: Duration = Duration::from_millis(600);

const TASK_CARD_DETAILS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

const TASK_TOPOLOGY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;
    let task_repo_scope = initial_repo_scope(repo_arg);
    // Progressive loading: build an empty state, enter the terminal,
    // draw the first frame immediately, and only then spawn the loader.
    // The loader does *all* of the expensive work (Zellij snapshot, FS
    // walk, per-repo git) in the background so `task ui` shows up in
    // under a few milliseconds even on a workspace with ~150 bare repos.
    let mut state = UiState::new_empty_loading(task_repo_scope.clone());
    let generation = state.begin_load();
    let loader = loader::spawn(context.clone(), task_repo_scope, generation);

    let mut terminal = TerminalGuard::new()?;
    let _process_log_capture = ProcessLogCaptureGuard::new();
    let ui_result = run_event_loop(context, terminal.terminal_mut()?, &mut state, loader);

    match ui_result? {
        UiAction::Quit => Ok(()),
        UiAction::Open(row) => context.tasks().launch_workspace(&row.repo, &row.path),
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
    mut loader: LoaderHandle,
) -> Result<UiAction> {
    // Short-lived OpenCode refreshers run independently of the main
    // loader so they don't contend with per-repo git work. `Option`
    // because no refresher is active until the first tick deadline.
    let mut opencode_refresh: Option<LoaderHandle> = None;
    let mut task_card_details_refresh: Option<LoaderHandle> = None;
    // Back-date the "last refresh" timestamp so the first tick fires
    // immediately on startup. `checked_sub` guards against the case
    // where the platform's monotonic clock starts close to zero (e.g.
    // a freshly-booted box) — `Instant::now() - duration` would panic.
    let mut last_opencode_refresh = Instant::now()
        .checked_sub(OPENCODE_REFRESH_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut last_task_card_details_refresh = Instant::now()
        .checked_sub(TASK_CARD_DETAILS_REFRESH_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut last_task_topology_refresh = Instant::now()
        .checked_sub(TASK_TOPOLOGY_REFRESH_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut task_refresh_fingerprint: Option<TaskRefreshFingerprint> = None;
    let mut fingerprint_generation = state.load_generation;

    loop {
        // Drain background loader messages before each frame.
        while let Some(msg) = loader.try_recv() {
            state.apply_load_msg(msg);
        }
        drain_one_shot_loader(&mut opencode_refresh, state);
        drain_one_shot_loader(&mut task_card_details_refresh, state);

        if fingerprint_generation != state.load_generation {
            task_refresh_fingerprint = None;
            fingerprint_generation = state.load_generation;
        }

        // Start a new OpenCode refresh when the interval elapses and
        // nothing is in flight. Gated on having task rows — no point
        // paying for the sysinfo scan when the list is empty.
        maybe_auto_refresh_task_topology(
            context,
            state,
            &mut loader,
            &mut task_refresh_fingerprint,
            &mut last_task_topology_refresh,
            Instant::now(),
        );
        maybe_spawn_opencode_refresh(
            state,
            &mut opencode_refresh,
            &mut last_opencode_refresh,
            Instant::now(),
        );
        maybe_spawn_task_card_details_refresh(
            state,
            &mut task_card_details_refresh,
            &mut last_task_card_details_refresh,
            Instant::now(),
        );

        state.append_activity_lines(crate::runtime::process::take_captured_logs());
        terminal.draw(|frame| render(frame, &mut *state))?;

        // Tick-based polling lets us animate the spinner and consume
        // loader messages even when the user is idle.
        if !event::poll(TICK)? {
            // Advance the spinner only when a load is still in progress;
            // keeps the UI quiet once everything is loaded.
            if state.task_load.is_loading() || state.repo_load.is_loading() {
                state.spinner_frame = state.spinner_frame.wrapping_add(1);
            }
            continue;
        }

        let event = event::read()?;

        // While the commands overlay is open, only allow closing it or quitting.
        if state.show_help {
            match event {
                Event::Key(key) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        return Ok(UiAction::Quit);
                    }
                    if (key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('p'))
                        || key.code == KeyCode::Esc
                    {
                        state.show_help = false;
                    }
                }
                Event::Mouse(mouse) => {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        let outside = state
                            .help_area
                            .is_none_or(|a| !a.contains((mouse.column, mouse.row).into()));
                        if outside {
                            state.show_help = false;
                        }
                    }
                }
                Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Resize(..) => {}
            }
            continue;
        }

        let intent = match event {
            Event::Key(key) => from_key(state.mode, key),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => UiIntent::MoveNext,
                MouseEventKind::ScrollUp => UiIntent::MovePrev,
                MouseEventKind::Down(_)
                | MouseEventKind::Up(_)
                | MouseEventKind::Drag(_)
                | MouseEventKind::Moved
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight => UiIntent::Noop,
            },
            Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Resize(..) => {
                UiIntent::Noop
            }
        };
        if let Some(action) = apply_intent(context, state, &mut loader, intent)? {
            return Ok(action);
        }
    }
}

/// Drop the current loader handle (cancels the worker) and spawn a fresh
/// one. The state's `load_generation` is bumped so any still-in-flight
/// messages from the old worker will be dropped by `apply_load_msg`.
fn restart_loader(context: &RuntimeEnvironment, state: &mut UiState, loader: &mut LoaderHandle) {
    let generation = state.begin_load();
    let new_handle = loader::spawn(context.clone(), state.task_repo_scope.clone(), generation);
    let _ = std::mem::replace(loader, new_handle);
}

fn drain_one_shot_loader(handle: &mut Option<LoaderHandle>, state: &mut UiState) {
    let mut got_tick = false;
    if let Some(loader) = handle.as_ref() {
        while let Some(msg) = loader.try_recv() {
            got_tick = true;
            state.apply_load_msg(msg);
        }
    }
    if got_tick {
        *handle = None;
    }
}

fn maybe_auto_refresh_task_topology(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    current_fingerprint: &mut Option<TaskRefreshFingerprint>,
    last_refresh: &mut Instant,
    now: Instant,
) {
    if now.saturating_duration_since(*last_refresh) < TASK_TOPOLOGY_REFRESH_INTERVAL {
        return;
    }
    *last_refresh = now;

    if state.task_load.is_loading() || state.repo_load.is_loading() {
        return;
    }
    if matches!(state.mode, InputMode::CreateTask | InputMode::CloneRepo) {
        return;
    }

    let Ok(next_fingerprint) = task_refresh_fingerprint(context, state.task_repo_scope.as_deref())
    else {
        return;
    };
    let Some(previous_fingerprint) = current_fingerprint.replace(next_fingerprint.clone()) else {
        return;
    };
    if previous_fingerprint == next_fingerprint {
        return;
    }

    refresh_all(context, state, loader);
    state.message = "Task list changed; refreshing…".to_string();
}

/// Spawn a fresh OpenCode refresher when the interval has elapsed,
/// there is no refresher already in flight, and there is at least one
/// task row to classify. Dropping the previous handle (even if still
/// running) sets its stop flag so we never accumulate background work.
///
/// `now` is passed in so tests can drive the scheduler with a
/// controlled clock; production always passes `Instant::now()`.
fn maybe_spawn_opencode_refresh(
    state: &UiState,
    handle: &mut Option<LoaderHandle>,
    last_refresh: &mut Instant,
    now: Instant,
) {
    // A refresher is already running — let it finish before starting a
    // new one. The channel will drain into `apply_load_msg` on the next
    // frame.
    if handle.is_some() {
        return;
    }
    if now.saturating_duration_since(*last_refresh) < OPENCODE_REFRESH_INTERVAL {
        return;
    }
    if state.task_rows.is_empty() {
        return;
    }

    let paths: Vec<_> = state.task_rows.iter().map(|row| row.path.clone()).collect();
    *handle = Some(loader::spawn_opencode_refresh(paths));
    *last_refresh = now;
}

fn maybe_spawn_task_card_details_refresh(
    state: &UiState,
    handle: &mut Option<LoaderHandle>,
    last_refresh: &mut Instant,
    now: Instant,
) {
    if handle.is_some() {
        return;
    }
    if now.saturating_duration_since(*last_refresh) < TASK_CARD_DETAILS_REFRESH_INTERVAL {
        return;
    }
    if state.task_rows.is_empty() {
        return;
    }

    let paths: Vec<_> = state.task_rows.iter().map(|row| row.path.clone()).collect();
    *handle = Some(loader::spawn_task_card_details_refresh(paths));
    *last_refresh = now;
}

fn apply_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
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
        UiIntent::PageDown => {
            state.move_page_down();
            Ok(None)
        }
        UiIntent::PageUp => {
            state.move_page_up();
            Ok(None)
        }
        UiIntent::MoveFirst => {
            state.move_first();
            Ok(None)
        }
        UiIntent::MoveLast => {
            state.move_last();
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
                        restart_loader(context, state, loader);
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
                    let Some(row) = state.selected_repo_row().cloned() else {
                        state.message = "No repo selected".to_string();
                        return Ok(None);
                    };
                    let repo_key_str = row.repo.to_string();
                    state.task_repo_scope = Some(repo_key_str.clone());
                    restart_loader(context, state, loader);
                    state.mode = InputMode::CreateTask;
                    state.create_branch.clear();
                    state.message = format!("Start task on {repo_key_str}: type branch name");
                }
            }
            Ok(None)
        }
        UiIntent::EnterCloneMode => {
            if state.view != ViewMode::Repos {
                return Ok(None);
            }
            state.mode = InputMode::CloneRepo;
            state.clone_input.clear();
            state.message = "Clone mode: type '<repo-url> [repo-key]'".to_string();
            Ok(None)
        }
        UiIntent::FinishSelected => {
            if state.view != ViewMode::Tasks {
                state.message = "Finish is only available in Tasks view".to_string();
                return Ok(None);
            }
            if let Err(err) = finish_and_refresh(context, state, loader) {
                state.message = err.to_string();
            }
            Ok(None)
        }
        UiIntent::RefreshCurrentView => {
            refresh_all(context, state, loader);
            state.message = match state.view {
                ViewMode::Tasks => "Refreshing task list…".to_string(),
                ViewMode::Repos => "Refreshing repo list…".to_string(),
            };
            Ok(None)
        }
        UiIntent::ParkSelected => {
            if state.view != ViewMode::Tasks {
                state.message = "Park is only available in Tasks view".to_string();
                return Ok(None);
            }
            if let Err(err) = park_and_refresh(context, state, loader) {
                state.message = err.to_string();
            }
            Ok(None)
        }
        UiIntent::ToggleDetach => {
            if state.view != ViewMode::Repos {
                state.message = "Detach toggle is only available in Repos view".to_string();
                return Ok(None);
            }
            match toggle_detach_and_refresh(context, state, loader) {
                Ok(msg) => state.message = msg,
                Err(err) => state.message = err.to_string(),
            }
            Ok(None)
        }
        UiIntent::ToggleSidebar => {
            let width = state.last_frame_width;
            state.toggle_sidebar(width);
            state.message = if state.sidebar_visible(width) {
                "Sidebar shown".to_string()
            } else {
                "Sidebar hidden".to_string()
            };
            Ok(None)
        }

        UiIntent::ClearScope => {
            if state.task_repo_scope.is_some() {
                state.clear_repo_scope();
                restart_loader(context, state, loader);
                state.message = "Returned to repos view".to_string();
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
        UiIntent::CreateClear => {
            state.create_branch.clear();
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
        UiIntent::CloneSubmit => match clone_and_refresh(context, state, loader) {
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

    use super::{apply_intent, loader::LoaderHandle, state::UiAction};
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
        let result =
            apply_intent(&ctx, &mut state, &mut LoaderHandle::noop(), UiIntent::Quit).unwrap();
        assert!(matches!(result, Some(UiAction::Quit)));
    }

    // ── Noop ─────────────────────────────────────────────────────────────────

    #[test]
    fn noop_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        let result =
            apply_intent(&ctx, &mut state, &mut LoaderHandle::noop(), UiIntent::Noop).unwrap();
        assert!(result.is_none());
    }

    // ── ToggleHelp ───────────────────────────────────────────────────────────

    #[test]
    fn toggle_help_flips_show_help() {
        let ctx = test_env();
        let mut state = empty_state();
        assert!(!state.show_help);
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ToggleHelp,
        )
        .unwrap();
        assert!(state.show_help);
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ToggleHelp,
        )
        .unwrap();
        assert!(!state.show_help);
    }

    // ── SwitchView ───────────────────────────────────────────────────────────

    #[test]
    fn switch_view_from_normal_mode_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        assert_eq!(state.view, ViewMode::Tasks);
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::SwitchView,
        )
        .unwrap();
        assert_eq!(state.view, ViewMode::Repos);
        assert_eq!(state.message, "Switched to Repos view");
    }

    #[test]
    fn switch_view_back_to_tasks_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::SwitchView,
        )
        .unwrap();
        assert_eq!(state.view, ViewMode::Tasks);
        assert_eq!(state.message, "Switched to Tasks view");
    }

    #[test]
    fn switch_view_in_filter_mode_preserves_filter_and_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        // switch_view resets mode to Normal internally, then we force it back to Filter
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::SwitchView,
        )
        .unwrap();
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
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/c"),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/c"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        assert_eq!(state.task_selected, 0);
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MoveNext,
        )
        .unwrap();
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
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/c"),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/c"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 1;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MovePrev,
        )
        .unwrap();
        assert_eq!(state.task_selected, 0);
    }

    // ── PageDown / PageUp / MoveFirst / MoveLast ────────────────────────────

    #[test]
    fn page_down_delegates_to_state() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows: Vec<TaskRow> = (0..30)
            .map(|i| TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(format!("github.com/a/r{i}")),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.visible_rows = 10;
        assert_eq!(state.task_selected, 0);
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::PageDown,
        )
        .unwrap();
        assert_eq!(state.task_selected, 10);
    }

    #[test]
    fn page_up_delegates_to_state() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows: Vec<TaskRow> = (0..30)
            .map(|i| TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(format!("github.com/a/r{i}")),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.visible_rows = 10;
        state.task_selected = 20;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::PageUp,
        )
        .unwrap();
        assert_eq!(state.task_selected, 10);
    }

    #[test]
    fn move_first_delegates_to_state() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows: Vec<TaskRow> = (0..10)
            .map(|i| TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(format!("github.com/a/r{i}")),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 7;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MoveFirst,
        )
        .unwrap();
        assert_eq!(state.task_selected, 0);
    }

    #[test]
    fn move_last_delegates_to_state() {
        use std::path::PathBuf;

        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        let ctx = test_env();
        let rows: Vec<TaskRow> = (0..10)
            .map(|i| TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(format!("github.com/a/r{i}")),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 3;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MoveLast,
        )
        .unwrap();
        assert_eq!(state.task_selected, 9);
    }

    // ── EnterFilterMode ──────────────────────────────────────────────────────

    #[test]
    fn enter_filter_mode_on_tasks_view() {
        let ctx = test_env();
        let mut state = empty_state();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterFilterMode,
        )
        .unwrap();
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
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterFilterMode,
        )
        .unwrap();
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
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCreateTaskMode,
        )
        .unwrap();
        assert_eq!(state.mode, InputMode::CreateTask);
        assert!(state.create_branch.is_empty(), "branch should be cleared");
        assert!(
            state.message.contains("branch"),
            "message should mention branch: {}",
            state.message
        );
    }

    #[test]
    fn enter_create_task_mode_in_repos_view_scopes_to_selected_repo() {
        use crate::{runtime::RepoKey, ui::state::RepoRow};

        let ctx = test_env();
        let repo_rows = vec![
            RepoRow {
                repo: RepoKey::new("github.com/acme/app"),
                open_tasks: 1,
                parked_tasks: 0,
                is_detached: false,
            },
            RepoRow {
                repo: RepoKey::new("github.com/acme/ops"),
                open_tasks: 0,
                parked_tasks: 0,
                is_detached: false,
            },
        ];
        let mut state = UiState::new(vec![], repo_rows, None);
        state.view = ViewMode::Repos;
        state.create_branch = "leftover".to_string();
        state.repo_selected = 1;

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCreateTaskMode,
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(state.mode, InputMode::CreateTask);
        assert!(state.create_branch.is_empty(), "branch should be cleared");
        assert_eq!(
            state.task_repo_scope,
            Some("github.com/acme/ops".to_string())
        );
        assert!(
            state.message.contains("github.com/acme/ops"),
            "message should mention selected repo: {}",
            state.message
        );
    }

    #[test]
    fn enter_create_task_mode_in_repos_view_with_no_selection() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCreateTaskMode,
        )
        .unwrap();
        assert!(result.is_none());
        assert!(
            state.message.contains("No repo selected"),
            "message should mention 'No repo selected': {}",
            state.message
        );
    }

    // ── FilterCancel / FilterApply ───────────────────────────────────────────

    #[test]
    fn filter_cancel_returns_to_normal_mode() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterCancel,
        )
        .unwrap();
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
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/ops"),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/b"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        state.mode = InputMode::Filter;
        state.filter_text = "app".to_string();
        state.apply_task_filter();

        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterApply,
        )
        .unwrap();
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

        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterApply,
        )
        .unwrap();
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
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterAppend('x'),
        )
        .unwrap();
        assert_eq!(state.filter_text, "x");
    }

    #[test]
    fn filter_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.filter_text = "ab".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterBackspace,
        )
        .unwrap();
        assert_eq!(state.filter_text, "a");
    }

    #[test]
    fn filter_clear_empties_filter() {
        let ctx = test_env();
        let mut state = empty_state();
        state.filter_text = "something".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterClear,
        )
        .unwrap();
        assert_eq!(state.filter_text, "");
    }

    // ── CreateCancel / CreateAppend / CreateBackspace ────────────────────────

    #[test]
    fn create_cancel_returns_to_normal() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CreateTask;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateCancel,
        )
        .unwrap();
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
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('f'),
        )
        .unwrap();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('e'),
        )
        .unwrap();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('a'),
        )
        .unwrap();
        assert_eq!(state.create_branch, "fea");
    }

    #[test]
    fn create_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.create_branch = "fea".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateBackspace,
        )
        .unwrap();
        assert_eq!(state.create_branch, "fe");
    }

    #[test]
    fn create_clear_empties_branch() {
        let ctx = test_env();
        let mut state = empty_state();
        state.create_branch = "some-branch".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateClear,
        )
        .unwrap();
        assert!(state.create_branch.is_empty());
    }

    // ── CloneCancel / CloneAppend / CloneBackspace / CloneClear ─────────────

    #[test]
    fn clone_cancel_returns_to_normal() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CloneRepo;
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneCancel,
        )
        .unwrap();
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
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneAppend('g'),
        )
        .unwrap();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneAppend('h'),
        )
        .unwrap();
        assert_eq!(state.clone_input, "gh");
    }

    #[test]
    fn clone_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.clone_input = "gh".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneBackspace,
        )
        .unwrap();
        assert_eq!(state.clone_input, "g");
    }

    #[test]
    fn clone_clear_empties_input() {
        let ctx = test_env();
        let mut state = empty_state();
        state.clone_input = "something".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneClear,
        )
        .unwrap();
        assert!(state.clone_input.is_empty());
    }

    // ── EnterCloneMode ────────────────────────────────────────────────────

    #[test]
    fn enter_clone_mode_in_tasks_view_is_silent_noop() {
        let ctx = test_env();
        let mut state = empty_state();
        assert_eq!(state.view, ViewMode::Tasks);
        let old_message = state.message.clone();
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCloneMode,
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(state.mode, InputMode::Normal, "mode should stay Normal");
        assert_eq!(state.message, old_message, "message should not change");
    }

    #[test]
    fn enter_clone_mode_in_repos_view_enters_clone_mode() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        state.clone_input = "leftover".to_string();
        apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCloneMode,
        )
        .unwrap();
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

    // ── FinishSelected / ParkSelected guard in non-Tasks view ────────────────

    #[test]
    fn finish_selected_in_repos_view_sets_message_and_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FinishSelected,
        )
        .unwrap();
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
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ParkSelected,
        )
        .unwrap();
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
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ToggleDetach,
        )
        .unwrap();
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
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ToggleDetach,
        )
        .unwrap();
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
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::OpenSelected,
        )
        .unwrap();
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
            worktree_name: "my-branch".to_string(),
            path: PathBuf::from("/tmp/a"),
            opencode: crate::tools::opencode::status::OpenCodeState::None,
        };
        let mut state = UiState::new(vec![row], vec![], None);
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::OpenSelected,
        )
        .unwrap();
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
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateSubmit,
        )
        .unwrap();
        assert!(result.is_none(), "should not return action on empty branch");
        assert!(
            state.message.contains("empty") || state.message.contains("cannot"),
            "message should mention empty branch: {}",
            state.message
        );
    }

    mod opencode_refresh_scheduler {
        use std::{path::PathBuf, time::Instant};

        use super::{
            super::{OPENCODE_REFRESH_INTERVAL, maybe_spawn_opencode_refresh},
            *,
        };
        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        fn state_with_one_task() -> UiState {
            let row = TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/b"),
                branch: BranchName::new("main"),
                worktree_name: "main".to_string(),
                path: PathBuf::from("/tmp/a/main"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            };
            UiState::new(vec![row], vec![], None)
        }

        /// `now` that is strictly past the refresh interval from
        /// `last_refresh`, so the interval-gate branch never blocks
        /// when a test wants a spawn to happen.
        fn past_interval(last: Instant) -> Instant {
            last.checked_add(OPENCODE_REFRESH_INTERVAL + std::time::Duration::from_millis(1))
                .expect("instant overflow")
        }

        #[test]
        fn skips_when_handle_in_flight() {
            let state = state_with_one_task();
            let last = Instant::now() - OPENCODE_REFRESH_INTERVAL * 10;
            let mut last_refresh = last;
            let mut handle: Option<LoaderHandle> = Some(LoaderHandle::noop());
            let now = past_interval(last);

            maybe_spawn_opencode_refresh(&state, &mut handle, &mut last_refresh, now);

            // Existing handle must not be replaced; last_refresh untouched.
            assert!(handle.is_some());
            assert_eq!(last_refresh, last);
        }

        #[test]
        fn skips_before_interval_elapsed() {
            let state = state_with_one_task();
            let last = Instant::now();
            let mut last_refresh = last;
            let mut handle: Option<LoaderHandle> = None;
            // `now` is less than the interval after `last_refresh`.
            let now = last;

            maybe_spawn_opencode_refresh(&state, &mut handle, &mut last_refresh, now);

            assert!(handle.is_none());
            assert_eq!(last_refresh, last);
        }

        #[test]
        fn skips_when_task_rows_empty() {
            let state = UiState::new(vec![], vec![], None);
            let last = Instant::now() - OPENCODE_REFRESH_INTERVAL * 10;
            let mut last_refresh = last;
            let mut handle: Option<LoaderHandle> = None;
            let now = past_interval(last);

            maybe_spawn_opencode_refresh(&state, &mut handle, &mut last_refresh, now);

            assert!(handle.is_none());
            assert_eq!(
                last_refresh, last,
                "last_refresh should not advance when we didn't spawn",
            );
        }

        #[test]
        fn spawns_when_preconditions_met_and_bumps_last_refresh() {
            let state = state_with_one_task();
            let last = Instant::now() - OPENCODE_REFRESH_INTERVAL * 10;
            let mut last_refresh = last;
            let mut handle: Option<LoaderHandle> = None;
            let now = past_interval(last);

            maybe_spawn_opencode_refresh(&state, &mut handle, &mut last_refresh, now);

            assert!(handle.is_some(), "scheduler should spawn a refresher");
            assert_eq!(
                last_refresh, now,
                "last_refresh should advance to the injected `now`",
            );
        }
    }
}
