use std::{
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::prelude::Position;

use self::{
    effects::{
        clone_and_refresh, create_action, finish_and_refresh, park_and_refresh, refresh_all,
        refresh_session_state, toggle_detach_and_refresh,
    },
    intent::{UiIntent, from_key},
    loader::LoaderHandle,
    render::render,
    state::{InputMode, MouseHit, UiAction, UiState, ViewMode},
    tasks::{FinishMode, TaskTopologyFingerprint, initial_repo_scope},
    terminal::TerminalGuard,
};
use crate::{
    error::{Error, Result},
    runtime::environment::RuntimeEnvironment,
};

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

/// Cadence of the background `OpenCode`-state refresher. Short enough to
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
    let mut task_topology_refresh: Option<LoaderHandle<TaskTopologyFingerprint>> = None;
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
    let mut task_topology_fingerprint: Option<TaskTopologyFingerprint> = None;
    let mut fingerprint_generation = state.load_generation;

    loop {
        // Drain background loader messages before each frame.
        while let Some(msg) = loader.try_recv() {
            state.apply_load_msg(msg);
        }
        drain_one_shot_loader(&mut opencode_refresh, state);
        drain_one_shot_loader(&mut task_card_details_refresh, state);
        drain_task_topology_refresh(
            context,
            state,
            &mut loader,
            &mut task_card_details_refresh,
            &mut task_topology_refresh,
            &mut task_topology_fingerprint,
        );

        if fingerprint_generation != state.load_generation {
            task_topology_fingerprint = None;
            task_topology_refresh = None;
            fingerprint_generation = state.load_generation;
        }

        // Start a new OpenCode refresh when the interval elapses and
        // nothing is in flight. Gated on having task rows — no point
        // paying for the sysinfo scan when the list is empty.
        maybe_spawn_task_topology_refresh(
            context,
            state,
            &mut task_topology_refresh,
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

        let Some(event) = next_terminal_event(state)? else {
            continue;
        };

        let (intent, source) = match event_action(state, &event) {
            EventAction::Intent(intent, source) => (intent, source),
            EventAction::Action(action) => return Ok(action),
            EventAction::Continue => continue,
        };
        if let Some(action) = apply_intent_with_source(context, state, &mut loader, intent, source)
        {
            return Ok(action);
        }
    }
}

enum EventAction {
    Intent(UiIntent, IntentSource),
    Action(UiAction),
    Continue,
}

fn next_terminal_event(state: &mut UiState) -> Result<Option<Event>> {
    if event::poll(TICK)? {
        return Ok(Some(event::read()?));
    }

    advance_spinner_if_loading(state);
    Ok(None)
}

const fn advance_spinner_if_loading(state: &mut UiState) {
    if state.task_load.is_loading() || state.repo_load.is_loading() {
        state.spinner_frame = state.spinner_frame.wrapping_add(1);
    }
}

fn event_action(state: &mut UiState, event: &Event) -> EventAction {
    if state.show_help {
        return handle_help_event(state, event).map_or(EventAction::Continue, EventAction::Action);
    }

    let (intent, source) = intent_from_event(state, event);
    EventAction::Intent(intent, source)
}

fn intent_from_event(state: &UiState, event: &Event) -> (UiIntent, IntentSource) {
    match event {
        Event::Key(key) => (from_key(state.mode, *key), IntentSource::Key),
        Event::Mouse(mouse) => (
            from_mouse(state, mouse.kind, mouse.column, mouse.row),
            source_for_mouse(mouse.kind),
        ),
        Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Resize(..) => {
            (UiIntent::Noop, IntentSource::Terminal)
        }
    }
}

fn handle_help_event(state: &mut UiState, event: &Event) -> Option<UiAction> {
    match event {
        Event::Key(key) => {
            if is_help_quit_key(key) {
                return Some(UiAction::Quit);
            }
            if is_help_dismiss_key(key) {
                state.show_help = false;
            }
        }
        Event::Mouse(mouse) => {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                let outside = state
                    .help_area
                    .is_none_or(|area| !area.contains(Position::from((mouse.column, mouse.row))));
                if outside {
                    state.show_help = false;
                }
            }
        }
        Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Resize(..) => {}
    }
    None
}

fn is_help_quit_key(key: &crossterm::event::KeyEvent) -> bool {
    is_control_char(key, 'c')
}

fn is_help_dismiss_key(key: &crossterm::event::KeyEvent) -> bool {
    if key.code == KeyCode::Esc {
        return true;
    }
    is_control_char(key, 'p')
}

fn is_control_char(key: &crossterm::event::KeyEvent, ch: char) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(ch)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentSource {
    Key,
    Mouse,
    Terminal,
}

fn from_mouse(state: &UiState, kind: MouseEventKind, column: u16, row: u16) -> UiIntent {
    match kind {
        MouseEventKind::ScrollDown => UiIntent::MoveNext,
        MouseEventKind::ScrollUp => UiIntent::MovePrev,
        MouseEventKind::Down(MouseButton::Left) if state.mode == InputMode::Normal => {
            match state.mouse_hit(column, row) {
                Some(MouseHit::Task { filtered_index }) => UiIntent::ClickTaskRow(filtered_index),
                Some(MouseHit::Repo { filtered_index }) => UiIntent::ClickRepoRow(filtered_index),
                None => UiIntent::Noop,
            }
        }
        MouseEventKind::Down(_)
        | MouseEventKind::Up(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Moved
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => UiIntent::Noop,
    }
}

const fn source_for_mouse(kind: MouseEventKind) -> IntentSource {
    match kind {
        MouseEventKind::ScrollDown
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight
        | MouseEventKind::Down(_) => IntentSource::Mouse,
        // Crossterm can emit these while the pointer passes over the
        // terminal. They are terminal noise for force-finish retry purposes.
        MouseEventKind::Up(_) | MouseEventKind::Drag(_) | MouseEventKind::Moved => {
            IntentSource::Terminal
        }
    }
}

/// Drop the current loader handle (cancels the worker) and spawn a fresh
/// one. The state's `load_generation` is bumped so any still-in-flight
/// messages from the old worker will be dropped by `apply_load_msg`.
fn restart_loader(context: &RuntimeEnvironment, state: &mut UiState, loader: &mut LoaderHandle) {
    let generation = state.begin_load();
    let new_handle = loader::spawn(context.clone(), state.task_repo_scope.clone(), generation);
    *loader = new_handle;
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

fn drain_task_topology_refresh(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    task_card_details_refresh: &mut Option<LoaderHandle>,
    handle: &mut Option<LoaderHandle<TaskTopologyFingerprint>>,
    current_fingerprint: &mut Option<TaskTopologyFingerprint>,
) {
    let Some(topology_loader) = handle.as_ref() else {
        return;
    };
    let next_fingerprint = match topology_loader.try_recv_result() {
        Ok(fingerprint) => fingerprint,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => {
            *handle = None;
            return;
        }
    };

    *handle = None;
    handle_task_topology_fingerprint(
        context,
        state,
        loader,
        task_card_details_refresh,
        current_fingerprint,
        next_fingerprint,
    );
}

fn handle_task_topology_fingerprint(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    task_card_details_refresh: &mut Option<LoaderHandle>,
    current_fingerprint: &mut Option<TaskTopologyFingerprint>,
    next_fingerprint: TaskTopologyFingerprint,
) {
    let Some(previous_fingerprint) = current_fingerprint.as_ref() else {
        *current_fingerprint = Some(next_fingerprint);
        return;
    };
    let repos_changed = previous_fingerprint.repos != next_fingerprint.repos;
    let sessions_changed = previous_fingerprint.sessions != next_fingerprint.sessions;
    *current_fingerprint = Some(next_fingerprint);

    if repos_changed {
        refresh_all(context, state, loader);
        state.set_message("Task list changed; refreshing…");
    } else if sessions_changed {
        refresh_session_state(state, task_card_details_refresh);
    }
}

fn maybe_spawn_task_topology_refresh(
    context: &RuntimeEnvironment,
    state: &UiState,
    handle: &mut Option<LoaderHandle<TaskTopologyFingerprint>>,
    last_refresh: &mut Instant,
    now: Instant,
) {
    if handle.is_some() {
        return;
    }
    if now.saturating_duration_since(*last_refresh) < TASK_TOPOLOGY_REFRESH_INTERVAL {
        return;
    }

    if state.task_load.is_loading() || state.repo_load.is_loading() {
        return;
    }
    if matches!(state.mode, InputMode::CreateTask | InputMode::CloneRepo) {
        return;
    }

    *handle = Some(loader::spawn_task_topology_refresh(
        context.clone(),
        state.task_repo_scope.clone(),
    ));
    *last_refresh = now;
}

/// Spawn a fresh `OpenCode` refresher when the interval has elapsed,
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
    maybe_spawn_task_path_refresh(
        state,
        handle,
        last_refresh,
        now,
        OPENCODE_REFRESH_INTERVAL,
        loader::spawn_opencode_refresh,
    );
}

fn maybe_spawn_task_card_details_refresh(
    state: &UiState,
    handle: &mut Option<LoaderHandle>,
    last_refresh: &mut Instant,
    now: Instant,
) {
    maybe_spawn_task_path_refresh(
        state,
        handle,
        last_refresh,
        now,
        TASK_CARD_DETAILS_REFRESH_INTERVAL,
        loader::spawn_task_card_details_refresh,
    );
}

fn maybe_spawn_task_path_refresh(
    state: &UiState,
    handle: &mut Option<LoaderHandle>,
    last_refresh: &mut Instant,
    now: Instant,
    interval: Duration,
    spawn: impl FnOnce(Vec<std::path::PathBuf>) -> LoaderHandle,
) {
    if handle.is_some() {
        return;
    }
    if now.saturating_duration_since(*last_refresh) < interval {
        return;
    }
    if state.task_rows.is_empty() {
        return;
    }

    let paths: Vec<_> = state.task_rows.iter().map(|row| row.path.clone()).collect();
    *handle = Some(spawn(paths));
    *last_refresh = now;
}

#[cfg(test)]
fn apply_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    intent: UiIntent,
) -> Option<UiAction> {
    apply_intent_with_source(context, state, loader, intent, IntentSource::Key)
}

fn apply_intent_with_source(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    intent: UiIntent,
    source: IntentSource,
) -> Option<UiAction> {
    clear_pending_force_finish_for_intent(state, intent, source);

    match intent {
        UiIntent::Quit => return Some(UiAction::Quit),
        UiIntent::SwitchView => apply_switch_view_intent(state),
        UiIntent::MoveNext
        | UiIntent::MovePrev
        | UiIntent::PageDown
        | UiIntent::PageUp
        | UiIntent::HalfPageDown
        | UiIntent::HalfPageUp
        | UiIntent::MoveFirst
        | UiIntent::MoveLast => apply_navigation_intent(state, intent),
        UiIntent::ToggleHelp => state.show_help = !state.show_help,
        UiIntent::OpenSelected => return activate_selected(context, state, loader),
        UiIntent::EnterFilterMode => apply_enter_filter_mode_intent(state),
        UiIntent::EnterCreateTaskMode => enter_create_task_mode(context, state, loader),
        UiIntent::EnterCloneMode => apply_enter_clone_mode_intent(state),
        UiIntent::FinishSelected => apply_finish_intent(context, state, loader),
        UiIntent::RefreshCurrentView => apply_refresh_current_view_intent(context, state, loader),
        UiIntent::ParkSelected => apply_park_intent(context, state, loader),
        UiIntent::ToggleDetach => apply_toggle_detach_intent(context, state, loader),
        UiIntent::ToggleSidebar => apply_toggle_sidebar_intent(state),
        UiIntent::ClearScope => apply_clear_scope_intent(context, state, loader),
        UiIntent::ClickTaskRow(filtered_index) => {
            return apply_click_task_intent(context, state, loader, filtered_index);
        }
        UiIntent::ClickRepoRow(filtered_index) => {
            return apply_click_repo_intent(context, state, loader, filtered_index);
        }
        UiIntent::FilterCancel => apply_filter_cancel_intent(state),
        UiIntent::FilterApply => apply_filter_apply_intent(state),
        UiIntent::FilterBackspace
        | UiIntent::FilterAppend(_)
        | UiIntent::InputStart
        | UiIntent::InputEnd
        | UiIntent::InputKillBackward
        | UiIntent::InputKillForward => apply_text_input_intent(state, intent),
        UiIntent::CreateCancel
        | UiIntent::CreateSubmit
        | UiIntent::CreateBackspace
        | UiIntent::CreateAppend(_) => return apply_create_intent(context, state, intent),
        UiIntent::CloneCancel
        | UiIntent::CloneSubmit
        | UiIntent::CloneBackspace
        | UiIntent::CloneAppend(_) => apply_clone_intent(context, state, loader, intent),
        UiIntent::UnboundKey | UiIntent::Noop => {}
    }

    None
}

fn apply_enter_filter_mode_intent(state: &mut UiState) {
    state.mode = InputMode::Filter;
    state.input_end();
    state.message = filter_mode_message(state.view);
}

fn apply_enter_clone_mode_intent(state: &mut UiState) {
    if state.view != ViewMode::Repos {
        return;
    }
    state.mode = InputMode::CloneRepo;
    state.clone_clear();
    state.set_message("Clone mode: type '<repo-url> [repo-key]'");
}

fn apply_refresh_current_view_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) {
    refresh_all(context, state, loader);
    state.message = refresh_message(state.view);
}

fn apply_filter_cancel_intent(state: &mut UiState) {
    state.mode = InputMode::Normal;
    state.set_message("Returned to normal mode");
}

fn apply_filter_apply_intent(state: &mut UiState) {
    state.mode = InputMode::Normal;
    state.message = filter_applied_message(state);
}

fn apply_switch_view_intent(state: &mut UiState) {
    let was_filter_mode = state.mode == InputMode::Filter;
    state.switch_view();
    if was_filter_mode {
        state.mode = InputMode::Filter;
        state.message = filter_mode_message(state.view);
    } else {
        state.message = switched_view_message(state.view);
    }
}

fn apply_navigation_intent(state: &mut UiState, intent: UiIntent) {
    if intent == UiIntent::MoveNext {
        state.move_next();
    } else if intent == UiIntent::MovePrev {
        state.move_prev();
    } else if intent == UiIntent::PageDown {
        state.move_page_down();
    } else if intent == UiIntent::PageUp {
        state.move_page_up();
    } else if intent == UiIntent::HalfPageDown {
        state.move_half_page_down();
    } else if intent == UiIntent::HalfPageUp {
        state.move_half_page_up();
    } else if intent == UiIntent::MoveFirst {
        state.move_first();
    } else if intent == UiIntent::MoveLast {
        state.move_last();
    }
}

fn apply_park_intent(context: &RuntimeEnvironment, state: &mut UiState, loader: &mut LoaderHandle) {
    if state.view != ViewMode::Tasks {
        state.set_message("Park is only available in Tasks view");
        return;
    }
    if let Err(err) = park_and_refresh(context, state, loader) {
        state.message = err.to_string();
    }
}

fn apply_toggle_detach_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) {
    if state.view != ViewMode::Repos {
        state.set_message("Detach toggle is only available in Repos view");
        return;
    }
    match toggle_detach_and_refresh(context, state, loader) {
        Ok(msg) => state.message = msg,
        Err(err) => state.message = err.to_string(),
    }
}

fn apply_toggle_sidebar_intent(state: &mut UiState) {
    let width = state.last_frame_width;
    state.toggle_sidebar(width);
    let message = if state.sidebar_visible(width) {
        "Sidebar shown"
    } else {
        "Sidebar hidden"
    };
    state.set_message(message);
}

fn apply_clear_scope_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) {
    if state.task_repo_scope.is_none() {
        return;
    }
    state.clear_repo_scope();
    restart_loader(context, state, loader);
    state.set_message("Returned to repos view");
}

fn apply_click_task_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    filtered_index: usize,
) -> Option<UiAction> {
    if state.view != ViewMode::Tasks {
        return None;
    }
    if state.task_selected == filtered_index {
        return activate_selected(context, state, loader);
    }
    state.select_task_filtered_index(filtered_index);
    None
}

fn apply_click_repo_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    filtered_index: usize,
) -> Option<UiAction> {
    if state.view != ViewMode::Repos {
        return None;
    }
    if state.repo_selected == filtered_index {
        enter_create_task_mode(context, state, loader);
        return None;
    }
    state.select_repo_filtered_index(filtered_index);
    None
}

fn apply_text_input_intent(state: &mut UiState, intent: UiIntent) {
    if intent == UiIntent::FilterBackspace {
        state.filter_backspace();
    } else if let UiIntent::FilterAppend(ch) = intent {
        state.filter_append(ch);
    } else if intent == UiIntent::InputStart {
        state.input_start();
    } else if intent == UiIntent::InputEnd {
        state.input_end();
    } else if intent == UiIntent::InputKillBackward {
        state.input_kill_backward();
    } else if intent == UiIntent::InputKillForward {
        state.input_kill_forward();
    }
}

fn apply_create_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    intent: UiIntent,
) -> Option<UiAction> {
    if intent == UiIntent::CreateCancel {
        state.mode = InputMode::Normal;
        state.set_message("Create cancelled");
        return None;
    }
    if intent == UiIntent::CreateSubmit {
        return match create_action(context, state) {
            Ok(action) => {
                state.create_clear();
                Some(action)
            }
            Err(err) => {
                state.message = err.to_string();
                None
            }
        };
    }
    if intent == UiIntent::CreateBackspace {
        state.create_backspace();
    } else if let UiIntent::CreateAppend(ch) = intent {
        state.create_append(ch);
    }
    None
}

fn apply_clone_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
    intent: UiIntent,
) {
    if intent == UiIntent::CloneCancel {
        state.mode = InputMode::Normal;
        state.set_message("Clone cancelled");
    } else if intent == UiIntent::CloneSubmit {
        match clone_and_refresh(context, state, loader) {
            Ok(repo_key) => {
                state.mode = InputMode::Normal;
                state.clone_clear();
                state.message = format!("Cloned repo: {repo_key}");
            }
            Err(err) => state.message = err.to_string(),
        }
    } else if intent == UiIntent::CloneBackspace {
        state.clone_backspace();
    } else if let UiIntent::CloneAppend(ch) = intent {
        state.clone_append(ch);
    }
}

fn filter_mode_message(view: ViewMode) -> String {
    match view {
        ViewMode::Tasks => "Filter mode: type to refine tasks".to_owned(),
        ViewMode::Repos => "Filter mode: type to refine repos".to_owned(),
    }
}

fn switched_view_message(view: ViewMode) -> String {
    match view {
        ViewMode::Tasks => "Switched to Tasks view".to_owned(),
        ViewMode::Repos => "Switched to Repos view".to_owned(),
    }
}

fn refresh_message(view: ViewMode) -> String {
    match view {
        ViewMode::Tasks => "Refreshing task list…".to_owned(),
        ViewMode::Repos => "Refreshing repo list…".to_owned(),
    }
}

fn filter_applied_message(state: &UiState) -> String {
    match state.view {
        ViewMode::Tasks => format!(
            "Filter applied: {} matches",
            state.task_filtered_indices.len()
        ),
        ViewMode::Repos => format!(
            "Filter applied: {} matches",
            state.repo_filtered_indices.len()
        ),
    }
}

const fn key_intent_clears_pending_force_finish(intent: UiIntent) -> bool {
    !matches!(intent, UiIntent::FinishSelected)
}

const PENDING_FORCE_FINISH_PROMPT: &str = "Press f again to force finish.";

fn clear_pending_force_finish_for_intent(
    state: &mut UiState,
    intent: UiIntent,
    source: IntentSource,
) {
    if source == IntentSource::Mouse {
        clear_pending_force_finish_and_prompt(state);
        return;
    }

    if source != IntentSource::Key || !key_intent_clears_pending_force_finish(intent) {
        return;
    }

    clear_pending_force_finish_and_prompt(state);
}

fn clear_pending_force_finish_and_prompt(state: &mut UiState) {
    if state.pending_force_finish.is_some() && state.message.contains(PENDING_FORCE_FINISH_PROMPT) {
        state.set_message("Ready");
    }
    state.clear_pending_force_finish();
}

fn activate_selected(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) -> Option<UiAction> {
    match state.view {
        ViewMode::Tasks => state.selected_task_row().cloned().map(UiAction::Open),
        ViewMode::Repos => {
            if let Some(repo) = state.selected_repo_row().map(|row| row.repo.to_string()) {
                state.select_repo_for_tasks(repo);
                restart_loader(context, state, loader);
                state.set_message("Opened selected repository tasks");
            }
            None
        }
    }
}

fn enter_create_task_mode(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) {
    match state.view {
        ViewMode::Tasks => {
            state.mode = InputMode::CreateTask;
            state.create_clear();
            state.set_message("Create mode: type branch name");
        }
        ViewMode::Repos => {
            let Some(row) = state.selected_repo_row().cloned() else {
                state.set_message("No repo selected");
                return;
            };
            let repo_key_str = row.repo.to_string();
            state.task_repo_scope = Some(repo_key_str.clone());
            restart_loader(context, state, loader);
            state.mode = InputMode::CreateTask;
            state.create_clear();
            state.message = format!("Start task on {repo_key_str}: type branch name");
        }
    }
}

fn apply_finish_intent(
    context: &RuntimeEnvironment,
    state: &mut UiState,
    loader: &mut LoaderHandle,
) {
    if state.view != ViewMode::Tasks {
        state.clear_pending_force_finish();
        state.set_message("Finish is only available in Tasks view");
        return;
    }

    let mode = if state.pending_force_finish_matches_selected_task() {
        state.clear_pending_force_finish();
        FinishMode::Force
    } else {
        FinishMode::Normal
    };

    if let Err(err) = finish_and_refresh(context, state, loader, mode) {
        handle_finish_error(state, mode, &err);
    }
}

fn handle_finish_error(state: &mut UiState, mode: FinishMode, err: &Error) {
    if should_prompt_force_finish(state, mode, err) {
        state.message = format!("{PENDING_FORCE_FINISH_PROMPT} {err}");
        return;
    }

    state.clear_pending_force_finish();
    state.message = err.to_string();
}

fn should_prompt_force_finish(state: &mut UiState, mode: FinishMode, err: &Error) -> bool {
    if mode != FinishMode::Normal {
        return false;
    }
    if !matches!(err, Error::DirtyWorktree) {
        return false;
    }
    state.set_pending_force_finish_to_selected_task()
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::Command,
    };

    use crossterm::event::{MouseButton, MouseEventKind};

    use super::{
        IntentSource, apply_intent, apply_intent_with_source, from_mouse, loader::LoaderHandle,
        source_for_mouse, state::UiAction,
    };
    use crate::{
        error::Error,
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

    fn task_row(repo: &str, branch: &str) -> crate::runtime::task_rows::TaskRow {
        use crate::{
            runtime::{BranchName, RepoKey, task_rows::TaskStatus},
            tools::opencode::status::OpenCodeState,
        };

        crate::runtime::task_rows::TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new(repo),
            branch: BranchName::new(branch),
            worktree_name: branch.to_owned(),
            path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
            opencode: OpenCodeState::None,
        }
    }

    fn repo_row(repo: &str) -> crate::ui::state::RepoRow {
        crate::ui::state::RepoRow {
            repo: crate::runtime::RepoKey::new(repo),
            open_tasks: 1,
            parked_tasks: 0,
            is_detached: false,
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-ui-mod-{name}"));
            _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            _ = fs::remove_dir_all(&self.0);
        }
    }

    fn init_bare_repo(path: &std::path::Path) {
        fs::create_dir_all(path).expect("create bare repo parent");
        let status = Command::new("git")
            .args(["init", "--bare"])
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git init --bare");
        assert!(status.success(), "git init --bare failed");
    }

    fn add_dirty_worktree(gitdir: &Path, worktree: &Path, branch: &crate::runtime::BranchName) {
        fs::create_dir_all(worktree.parent().expect("worktree parent")).unwrap();
        let status = Command::new("git")
            .args([
                "--git-dir",
                gitdir.to_str().expect("gitdir path"),
                "worktree",
                "add",
                "--orphan",
                "-b",
                branch.as_str(),
                worktree.to_str().expect("worktree path"),
            ])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git worktree add");
        assert!(status.success(), "git worktree add failed");
        fs::write(worktree.join("dirty.txt"), "dirty\n").unwrap();
    }

    fn dirty_finish_fixture(name: &str) -> (TempDir, RuntimeEnvironment, UiState, PathBuf) {
        use crate::{
            runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            },
            tools::opencode::status::OpenCodeState,
        };

        let dir = TempDir::new(name);
        let repos = dir.path().join("repos");
        let wt = dir.path().join("wt");
        let detached = dir.path().join("detached");
        let repo_key = RepoKey::new("github.com/acme/app");
        let branch = BranchName::new("dirty-task");
        let gitdir = repos.join("github.com/acme/app.git");
        let worktree = wt.join("github.com/acme/app/dirty-task");

        init_bare_repo(&gitdir);
        add_dirty_worktree(&gitdir, &worktree, &branch);

        let context = RuntimeEnvironment::from_paths(&repos, &wt, &detached);
        let row = TaskRow {
            status: TaskStatus::Open,
            repo: repo_key,
            branch,
            worktree_name: String::from("dirty-task"),
            path: worktree.clone(),
            opencode: OpenCodeState::None,
        };
        (
            dir,
            context,
            UiState::new(vec![row], vec![], None),
            worktree,
        )
    }

    fn two_dirty_finish_fixture(
        name: &str,
    ) -> (TempDir, RuntimeEnvironment, UiState, PathBuf, PathBuf) {
        use crate::{
            runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            },
            tools::opencode::status::OpenCodeState,
        };

        let dir = TempDir::new(name);
        let repos = dir.path().join("repos");
        let wt = dir.path().join("wt");
        let detached = dir.path().join("detached");
        let repo_key = RepoKey::new("github.com/acme/app");
        let branch_a = BranchName::new("dirty-a");
        let branch_b = BranchName::new("dirty-b");
        let gitdir = repos.join("github.com/acme/app.git");
        let worktree_a = wt.join("github.com/acme/app/dirty-a");
        let worktree_b = wt.join("github.com/acme/app/dirty-b");

        init_bare_repo(&gitdir);
        add_dirty_worktree(&gitdir, &worktree_a, &branch_a);
        add_dirty_worktree(&gitdir, &worktree_b, &branch_b);

        let rows = vec![
            TaskRow {
                status: TaskStatus::Open,
                repo: repo_key.clone(),
                branch: branch_a,
                worktree_name: String::from("dirty-a"),
                path: worktree_a.clone(),
                opencode: OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: repo_key,
                branch: branch_b,
                worktree_name: String::from("dirty-b"),
                path: worktree_b.clone(),
                opencode: OpenCodeState::None,
            },
        ];
        (
            dir,
            RuntimeEnvironment::from_paths(&repos, &wt, &detached),
            UiState::new(rows, vec![], None),
            worktree_a,
            worktree_b,
        )
    }

    // ── Quit ─────────────────────────────────────────────────────────────────

    #[test]
    fn quit_returns_quit_action() {
        let ctx = test_env();
        let mut state = empty_state();
        let result = apply_intent(&ctx, &mut state, &mut LoaderHandle::noop(), UiIntent::Quit);
        assert!(matches!(result, Some(UiAction::Quit)));
    }

    // ── Noop ─────────────────────────────────────────────────────────────────

    #[test]
    fn noop_returns_none() {
        let ctx = test_env();
        let mut state = empty_state();
        let result = apply_intent(&ctx, &mut state, &mut LoaderHandle::noop(), UiIntent::Noop);
        assert!(result.is_none());
    }

    // ── ToggleHelp ───────────────────────────────────────────────────────────

    #[test]
    fn toggle_help_flips_show_help() {
        let ctx = test_env();
        let mut state = empty_state();
        assert!(!state.show_help);
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ToggleHelp,
        );
        assert!(state.show_help);
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ToggleHelp,
        );
        assert!(!state.show_help);
    }

    // ── SwitchView ───────────────────────────────────────────────────────────

    #[test]
    fn switch_view_from_normal_mode_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        assert_eq!(state.view, ViewMode::Tasks);
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::SwitchView,
        );
        assert_eq!(state.view, ViewMode::Repos);
        assert_eq!(state.message, "Switched to Repos view");
    }

    #[test]
    fn switch_view_back_to_tasks_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::SwitchView,
        );
        assert_eq!(state.view, ViewMode::Tasks);
        assert_eq!(state.message, "Switched to Tasks view");
    }

    #[test]
    fn switch_view_in_filter_mode_preserves_filter_and_updates_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        // switch_view resets mode to Normal internally, then we force it back to Filter
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::SwitchView,
        );
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
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/c"),
                branch: BranchName::new("main"),
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/c"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        assert_eq!(state.task_selected, 0);
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MoveNext,
        );
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
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/c"),
                branch: BranchName::new("main"),
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/c"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 1;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MovePrev,
        );
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
                worktree_name: String::from("main"),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.visible_rows = 10;
        assert_eq!(state.task_selected, 0);
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::PageDown,
        );
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
                worktree_name: String::from("main"),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.visible_rows = 10;
        state.task_selected = 20;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::PageUp,
        );
        assert_eq!(state.task_selected, 10);
    }

    #[test]
    fn half_page_down_delegates_to_state() {
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
                worktree_name: String::from("main"),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.visible_rows = 10;

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::HalfPageDown,
        );

        assert_eq!(state.task_selected, 5);
    }

    #[test]
    fn half_page_up_delegates_to_state() {
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
                worktree_name: String::from("main"),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.visible_rows = 10;
        state.task_selected = 9;

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::HalfPageUp,
        );

        assert_eq!(state.task_selected, 4);
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
                worktree_name: String::from("main"),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 7;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MoveFirst,
        );
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
                worktree_name: String::from("main"),
                path: PathBuf::from(format!("/tmp/{i}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            })
            .collect();
        let mut state = UiState::new(rows, vec![], None);
        state.task_selected = 3;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::MoveLast,
        );
        assert_eq!(state.task_selected, 9);
    }

    // ── EnterFilterMode ──────────────────────────────────────────────────────

    #[test]
    fn enter_filter_mode_on_tasks_view() {
        let ctx = test_env();
        let mut state = empty_state();
        state.filter_text = String::from("abc");
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterFilterMode,
        );
        assert_eq!(state.mode, InputMode::Filter);
        assert_eq!(state.filter_cursor, state.filter_text.len());
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
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterFilterMode,
        );
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
        state.create_branch = String::from("leftover");
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCreateTaskMode,
        );
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
        state.create_branch = String::from("leftover");
        state.repo_selected = 1;

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCreateTaskMode,
        );
        assert!(result.is_none());
        assert_eq!(state.mode, InputMode::CreateTask);
        assert!(state.create_branch.is_empty(), "branch should be cleared");
        assert_eq!(
            state.task_repo_scope,
            Some(String::from("github.com/acme/ops"))
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
        );
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
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterCancel,
        );
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
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/a"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new("github.com/a/ops"),
                branch: BranchName::new("main"),
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/b"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            },
        ];
        let mut state = UiState::new(rows, vec![], None);
        state.mode = InputMode::Filter;
        state.filter_text = String::from("app");
        state.apply_task_filter();

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterApply,
        );
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
        state.filter_text = String::from("ops");
        state.apply_repo_filter();

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterApply,
        );
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
        state.mode = InputMode::Filter;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterAppend('x'),
        );
        assert_eq!(state.filter_text, "x");
    }

    #[test]
    fn filter_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        state.filter_text = String::from("ab");
        state.input_end();
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FilterBackspace,
        );
        assert_eq!(state.filter_text, "a");
    }

    #[test]
    fn filter_kill_backward_removes_text_before_cursor() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        state.filter_text = String::from("before-after");
        state.filter_cursor = "before-".len();
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::InputKillBackward,
        );
        assert_eq!(state.filter_text, "after");
        assert_eq!(state.filter_cursor, 0);
    }

    #[test]
    fn input_start_and_end_update_filter_cursor() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::Filter;
        state.filter_text = String::from("abc");
        state.input_end();

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::InputStart,
        );
        assert_eq!(state.filter_cursor, 0);

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::InputEnd,
        );
        assert_eq!(state.filter_cursor, 3);
    }

    // ── CreateCancel / CreateAppend / CreateBackspace ────────────────────────

    #[test]
    fn create_cancel_returns_to_normal() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CreateTask;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateCancel,
        );
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
        state.mode = InputMode::CreateTask;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('f'),
        );
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('e'),
        );
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('a'),
        );
        assert_eq!(state.create_branch, "fea");
        assert_eq!(state.create_cursor, 3);
    }

    #[test]
    fn create_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CreateTask;
        state.create_branch = String::from("fea");
        state.input_end();
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateBackspace,
        );
        assert_eq!(state.create_branch, "fe");
        assert_eq!(state.create_cursor, 2);
    }

    #[test]
    fn create_kill_forward_removes_text_after_cursor() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CreateTask;
        state.create_branch = String::from("some-branch");
        state.create_cursor = "some".len();
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::InputKillForward,
        );
        assert_eq!(state.create_branch, "some");
        assert_eq!(state.create_cursor, 4);
    }

    #[test]
    fn create_append_uses_cursor_position() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CreateTask;
        state.create_branch = String::from("ab");
        state.create_cursor = 1;

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateAppend('x'),
        );

        assert_eq!(state.create_branch, "axb");
        assert_eq!(state.create_cursor, 2);
    }

    // ── CloneCancel / CloneAppend / CloneBackspace ──────────────────────────

    #[test]
    fn clone_cancel_returns_to_normal() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CloneRepo;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneCancel,
        );
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
        state.mode = InputMode::CloneRepo;
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneAppend('g'),
        );
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneAppend('h'),
        );
        assert_eq!(state.clone_input, "gh");
        assert_eq!(state.clone_cursor, 2);
    }

    #[test]
    fn clone_backspace_removes_last_char() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CloneRepo;
        state.clone_input = String::from("gh");
        state.input_end();
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneBackspace,
        );
        assert_eq!(state.clone_input, "g");
        assert_eq!(state.clone_cursor, 1);
    }

    #[test]
    fn clone_kill_backward_removes_text_before_cursor() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CloneRepo;
        state.clone_input = String::from("repo-url");
        state.clone_cursor = "repo-".len();
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::InputKillBackward,
        );
        assert_eq!(state.clone_input, "url");
        assert_eq!(state.clone_cursor, 0);
    }

    #[test]
    fn clone_backspace_uses_cursor_position() {
        let ctx = test_env();
        let mut state = empty_state();
        state.mode = InputMode::CloneRepo;
        state.clone_input = String::from("aéb");
        state.clone_cursor = "aé".len();

        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CloneBackspace,
        );

        assert_eq!(state.clone_input, "ab");
        assert_eq!(state.clone_cursor, 1);
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
        );
        assert!(result.is_none());
        assert_eq!(state.mode, InputMode::Normal, "mode should stay Normal");
        assert_eq!(state.message, old_message, "message should not change");
    }

    #[test]
    fn enter_clone_mode_in_repos_view_enters_clone_mode() {
        let ctx = test_env();
        let mut state = empty_state();
        state.view = ViewMode::Repos;
        state.clone_input = String::from("leftover");
        _ = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::EnterCloneMode,
        );
        assert_eq!(state.mode, InputMode::CloneRepo);
        assert!(
            state.clone_input.is_empty(),
            "clone_input should be cleared"
        );
        assert_eq!(state.clone_cursor, 0);
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
        );
        assert!(result.is_none());
        assert!(
            state.message.contains("Tasks view"),
            "message should mention Tasks view: {}",
            state.message
        );
    }

    #[test]
    fn finish_dirty_task_sets_pending_force_finish() {
        let (_dir, ctx, mut state, worktree) = dirty_finish_fixture("dirty-pending");
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::FinishSelected,
        );

        assert!(result.is_none());
        assert!(worktree.exists(), "normal finish must leave dirty worktree");
        assert!(state.pending_force_finish_matches_selected_task());
        assert!(
            state.message.starts_with("Press f again"),
            "retry hint should lead narrow status message: {}",
            state.message
        );
        assert!(
            state.message.contains("Use --force") && state.message.contains("Press f again"),
            "message should contain force prompt: {}",
            state.message
        );
    }

    #[test]
    fn second_finish_on_pending_dirty_task_forces_and_refreshes() {
        let (_dir, ctx, mut state, worktree) = dirty_finish_fixture("dirty-force");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);

        assert!(!worktree.exists(), "forced finish should remove worktree");
        assert!(!state.pending_force_finish_matches_selected_task());
        assert!(
            state.task_load.is_loading(),
            "finish should refresh task list"
        );
        assert!(
            state.message.contains("Finished task"),
            "message should report finish: {}",
            state.message
        );
    }

    #[test]
    fn actionable_intent_clears_pending_force_finish() {
        let (_dir, ctx, mut state, worktree) = dirty_finish_fixture("dirty-clear");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        assert!(state.pending_force_finish_matches_selected_task());
        assert!(state.message.contains("Press f again"));

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::MoveNext);
        assert!(!state.pending_force_finish_matches_selected_task());
        assert_eq!(state.message, "Ready");

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        assert!(
            worktree.exists(),
            "finish after another action should be normal"
        );
        assert!(state.pending_force_finish_matches_selected_task());
    }

    #[test]
    fn unbound_key_clears_pending_force_finish() {
        let (_dir, ctx, mut state, worktree) = dirty_finish_fixture("dirty-unbound");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        assert!(state.pending_force_finish_matches_selected_task());

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::UnboundKey);
        assert!(!state.pending_force_finish_matches_selected_task());
        assert_eq!(state.message, "Ready");

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        assert!(
            worktree.exists(),
            "finish after unbound key should be normal"
        );
        assert!(state.pending_force_finish_matches_selected_task());
    }

    #[test]
    fn toggle_help_key_clears_pending_force_finish_prompt() {
        let (_dir, ctx, mut state, _worktree) = dirty_finish_fixture("dirty-help-toggle");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::ToggleHelp);

        assert!(state.pending_force_finish.is_none());
        assert_eq!(state.message, "Ready");
        assert!(state.show_help);
    }

    #[test]
    fn handle_finish_error_clears_pending_on_non_dirty_error() {
        let mut state = UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
        assert!(state.set_pending_force_finish_to_selected_task());
        state.message = format!(
            "{} {}",
            super::PENDING_FORCE_FINISH_PROMPT,
            Error::DirtyWorktree
        );

        super::handle_finish_error(
            &mut state,
            super::FinishMode::Normal,
            &Error::failed("boom"),
        );

        assert!(state.pending_force_finish.is_none());
        assert_eq!(state.message, "boom");
    }

    #[test]
    fn handle_finish_error_clears_pending_on_force_error() {
        let mut state = UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
        assert!(state.set_pending_force_finish_to_selected_task());

        super::handle_finish_error(&mut state, super::FinishMode::Force, &Error::DirtyWorktree);

        assert!(state.pending_force_finish.is_none());
        assert_eq!(state.message, Error::DirtyWorktree.to_string());
    }

    #[test]
    fn terminal_noise_does_not_clear_pending_force_finish() {
        let (_dir, ctx, mut state, _worktree) = dirty_finish_fixture("dirty-terminal-noop");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        assert!(
            apply_intent_with_source(
                &ctx,
                &mut state,
                &mut loader,
                UiIntent::Noop,
                IntentSource::Terminal,
            )
            .is_none()
        );

        assert!(state.pending_force_finish_matches_selected_task());
        assert!(state.message.contains("Press f again"));
    }

    #[test]
    fn passive_mouse_moved_does_not_clear_pending_force_finish() {
        let (_dir, ctx, mut state, _worktree) = dirty_finish_fixture("dirty-mouse-moved");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        let intent = from_mouse(&state, MouseEventKind::Moved, 0, 0);
        assert!(
            apply_intent_with_source(
                &ctx,
                &mut state,
                &mut loader,
                intent,
                source_for_mouse(MouseEventKind::Moved),
            )
            .is_none()
        );

        assert!(state.pending_force_finish_matches_selected_task());
        assert!(state.message.contains("Press f again"));
    }

    #[test]
    fn mouse_scroll_to_another_task_clears_pending_force_finish_prompt() {
        let (_dir, ctx, mut state, _worktree_a, _worktree_b) =
            two_dirty_finish_fixture("dirty-mouse-scroll-away");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        let intent = from_mouse(&state, MouseEventKind::ScrollDown, 0, 0);
        assert!(
            apply_intent_with_source(
                &ctx,
                &mut state,
                &mut loader,
                intent,
                source_for_mouse(MouseEventKind::ScrollDown),
            )
            .is_none()
        );

        assert!(state.pending_force_finish.is_none());
        assert_eq!(state.message, "Ready");
    }

    #[test]
    fn mouse_down_noop_clears_pending_force_finish_prompt() {
        let (_dir, ctx, mut state, _worktree) = dirty_finish_fixture("dirty-mouse-noop");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        let kind = MouseEventKind::Down(MouseButton::Left);
        let intent = from_mouse(&state, kind, 0, u16::MAX);
        assert!(
            apply_intent_with_source(
                &ctx,
                &mut state,
                &mut loader,
                intent,
                source_for_mouse(kind),
            )
            .is_none()
        );

        assert!(state.pending_force_finish.is_none());
        assert_eq!(state.message, "Ready");
    }

    #[test]
    fn mouse_selecting_another_task_prevents_force_finish_from_applying_to_it() {
        let (_dir, ctx, mut state, worktree_a, worktree_b) =
            two_dirty_finish_fixture("dirty-mouse-select-away");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        assert!(
            apply_intent_with_source(
                &ctx,
                &mut state,
                &mut loader,
                UiIntent::ClickTaskRow(1),
                IntentSource::Mouse,
            )
            .is_none()
        );
        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);

        assert!(
            worktree_a.exists(),
            "task A should remain dirty and armed only once"
        );
        assert!(
            worktree_b.exists(),
            "task B should not be force-finished after selecting away from A"
        );
        assert!(state.pending_force_finish_matches_selected_task());
        assert!(state.message.contains("Press f again"));
    }

    #[test]
    fn message_setting_intent_replaces_pending_force_finish_prompt() {
        let (_dir, ctx, mut state, _worktree) = dirty_finish_fixture("dirty-message-replace");
        let mut loader = LoaderHandle::noop();

        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::FinishSelected);
        _ = apply_intent(&ctx, &mut state, &mut loader, UiIntent::RefreshCurrentView);

        assert!(!state.pending_force_finish_matches_selected_task());
        assert_eq!(state.message, "Refreshing task list…");
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
        );
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
        );
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
        );
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
        );
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
            worktree_name: String::from("my-branch"),
            path: PathBuf::from("/tmp/a"),
            opencode: crate::tools::opencode::status::OpenCodeState::None,
        };
        let mut state = UiState::new(vec![row], vec![], None);
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::OpenSelected,
        );
        assert!(
            matches!(result, Some(UiAction::Open(_))),
            "should return Open action"
        );
    }

    #[test]
    fn mouse_down_on_task_hit_target_returns_task_click_intent() {
        let mut state = UiState::new(
            vec![
                task_row("github.com/acme/app", "main"),
                task_row("github.com/acme/app", "feature"),
            ],
            vec![],
            None,
        );
        state.register_task_mouse_hit_targets(ratatui::layout::Rect::new(0, 0, 80, 8), 0);

        assert_eq!(
            from_mouse(&state, MouseEventKind::Down(MouseButton::Left), 2, 4),
            UiIntent::ClickTaskRow(1)
        );
    }

    #[test]
    fn mouse_down_on_selected_task_hit_target_returns_task_click_intent() {
        let mut state = UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
        state.register_task_mouse_hit_targets(ratatui::layout::Rect::new(0, 0, 80, 4), 0);

        assert_eq!(
            from_mouse(&state, MouseEventKind::Down(MouseButton::Left), 2, 1),
            UiIntent::ClickTaskRow(0)
        );
    }

    #[test]
    fn mouse_down_on_repo_hit_target_returns_repo_click_intent() {
        let mut state = UiState::new(
            vec![],
            vec![
                repo_row("github.com/acme/app"),
                repo_row("github.com/acme/ops"),
            ],
            None,
        );
        state.view = ViewMode::Repos;
        state.register_repo_mouse_hit_targets(ratatui::layout::Rect::new(0, 0, 80, 3), 0);

        assert_eq!(
            from_mouse(&state, MouseEventKind::Down(MouseButton::Left), 2, 2),
            UiIntent::ClickRepoRow(1)
        );
    }

    #[test]
    fn mouse_down_outside_hit_targets_is_noop() {
        let mut state = UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
        state.register_task_mouse_hit_targets(ratatui::layout::Rect::new(0, 0, 80, 4), 0);

        assert_eq!(
            from_mouse(&state, MouseEventKind::Down(MouseButton::Left), 2, 10),
            UiIntent::Noop
        );
    }

    #[test]
    fn mouse_down_while_not_normal_mode_is_noop() {
        let mut state = UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
        state.mode = InputMode::Filter;
        state.register_task_mouse_hit_targets(ratatui::layout::Rect::new(0, 0, 80, 4), 0);

        assert_eq!(
            from_mouse(&state, MouseEventKind::Down(MouseButton::Left), 2, 1),
            UiIntent::Noop
        );
    }

    #[test]
    fn clicking_unselected_task_selects_it() {
        let ctx = test_env();
        let mut state = UiState::new(
            vec![
                task_row("github.com/acme/app", "main"),
                task_row("github.com/acme/app", "feature"),
            ],
            vec![],
            None,
        );

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ClickTaskRow(1),
        );

        assert!(result.is_none());
        assert_eq!(state.task_selected, 1);
    }

    #[test]
    fn clicking_unselected_repo_selects_it() {
        let ctx = test_env();
        let mut state = UiState::new(
            vec![],
            vec![
                repo_row("github.com/acme/app"),
                repo_row("github.com/acme/ops"),
            ],
            None,
        );
        state.view = ViewMode::Repos;

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ClickRepoRow(1),
        );

        assert!(result.is_none());
        assert_eq!(state.repo_selected, 1);
        assert_eq!(state.mode, InputMode::Normal);
    }

    #[test]
    fn task_click_in_repos_view_is_noop() {
        let ctx = test_env();
        let mut state = UiState::new(
            vec![task_row("github.com/acme/app", "main")],
            vec![repo_row("github.com/acme/app")],
            None,
        );
        state.view = ViewMode::Repos;

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ClickTaskRow(0),
        );

        assert!(result.is_none());
        assert_eq!(state.view, ViewMode::Repos);
        assert_eq!(state.task_selected, 0);
    }

    #[test]
    fn repo_click_in_tasks_view_is_noop() {
        let ctx = test_env();
        let mut state = UiState::new(
            vec![task_row("github.com/acme/app", "main")],
            vec![repo_row("github.com/acme/app")],
            None,
        );

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ClickRepoRow(0),
        );

        assert!(result.is_none());
        assert_eq!(state.view, ViewMode::Tasks);
        assert_eq!(state.repo_selected, 0);
    }

    #[test]
    fn clicking_selected_task_opens_it() {
        let ctx = test_env();
        let mut state = UiState::new(
            vec![task_row("github.com/acme/app", "feature")],
            vec![],
            None,
        );

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ClickTaskRow(0),
        );

        let opened_branch = match result {
            Some(UiAction::Open(row)) => Some(row.branch.to_string()),
            Some(UiAction::Quit | UiAction::Create { .. }) | None => None,
        };
        assert_eq!(opened_branch.as_deref(), Some("feature"));
    }

    #[test]
    fn clicking_selected_repo_enters_create_task_mode_scoped_to_repo() {
        let ctx = test_env();
        let mut state = UiState::new(
            vec![],
            vec![
                repo_row("github.com/acme/app"),
                repo_row("github.com/acme/ops"),
            ],
            None,
        );
        state.view = ViewMode::Repos;
        state.repo_selected = 1;
        state.create_branch = String::from("leftover");

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::ClickRepoRow(1),
        );

        assert!(result.is_none());
        assert_eq!(state.mode, InputMode::CreateTask);
        assert_eq!(
            state.task_repo_scope,
            Some(String::from("github.com/acme/ops"))
        );
        assert!(state.create_branch.is_empty());
    }

    #[test]
    fn enter_on_selected_repo_still_opens_repo_tasks() {
        let ctx = test_env();
        let mut state = UiState::new(vec![], vec![repo_row("github.com/acme/app")], None);
        state.view = ViewMode::Repos;

        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::OpenSelected,
        );

        assert!(result.is_none());
        assert_eq!(state.view, ViewMode::Tasks);
        assert_eq!(state.mode, InputMode::Normal);
        assert_eq!(
            state.task_repo_scope,
            Some(String::from("github.com/acme/app"))
        );
    }

    // ── CreateSubmit with empty branch ───────────────────────────────────────

    #[test]
    fn create_submit_with_empty_branch_sets_error_message() {
        let ctx = test_env();
        let mut state = empty_state();
        state.create_branch = String::from("  "); // whitespace only
        let result = apply_intent(
            &ctx,
            &mut state,
            &mut LoaderHandle::noop(),
            UiIntent::CreateSubmit,
        );
        assert!(result.is_none(), "should not return action on empty branch");
        assert!(
            state.message.contains("empty") || state.message.contains("cannot"),
            "message should mention empty branch: {}",
            state.message
        );
    }

    mod task_topology_refresh {
        use std::path::PathBuf;

        use super::{
            super::{TaskTopologyFingerprint, handle_task_topology_fingerprint},
            *,
        };
        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        };

        fn task_row(repo: &str, branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                worktree_name: branch.to_owned(),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            }
        }

        fn fingerprint(repo_entry: &str, session: &str) -> TaskTopologyFingerprint {
            TaskTopologyFingerprint {
                repos: vec![(RepoKey::new("github.com/acme/app"), repo_entry.to_owned())],
                sessions: vec![session.to_owned()],
            }
        }

        #[test]
        fn first_fingerprint_is_stored_without_refreshing() {
            let ctx = test_env();
            let mut state =
                UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
            let mut loader = LoaderHandle::noop();
            let mut card_refresh = None;
            let mut current = None;

            handle_task_topology_fingerprint(
                &ctx,
                &mut state,
                &mut loader,
                &mut card_refresh,
                &mut current,
                fingerprint("main", "session-a"),
            );

            assert!(current.is_some());
            assert_eq!(state.task_rows.len(), 1);
            assert!(card_refresh.is_none());
        }

        #[test]
        fn repo_change_triggers_full_refresh() {
            let ctx = test_env();
            let mut state =
                UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
            let mut loader = LoaderHandle::noop();
            let mut card_refresh = None;
            let mut current = Some(fingerprint("main", "session-a"));

            handle_task_topology_fingerprint(
                &ctx,
                &mut state,
                &mut loader,
                &mut card_refresh,
                &mut current,
                fingerprint("other", "session-a"),
            );

            assert!(state.task_rows.is_empty());
            assert!(state.task_load.is_loading());
            assert_eq!(state.message, "Task list changed; refreshing…");
        }

        #[test]
        fn identical_fingerprint_does_not_refresh() {
            let ctx = test_env();
            let mut state =
                UiState::new(vec![task_row("github.com/acme/app", "main")], vec![], None);
            let mut loader = LoaderHandle::noop();
            let mut card_refresh = None;
            let mut current = Some(fingerprint("main", "session-a"));

            handle_task_topology_fingerprint(
                &ctx,
                &mut state,
                &mut loader,
                &mut card_refresh,
                &mut current,
                fingerprint("main", "session-a"),
            );

            assert_eq!(state.task_rows.len(), 1);
            assert!(!state.task_load.is_loading());
            assert!(card_refresh.is_none());
        }

        #[test]
        fn sessions_only_change_preserves_selection_and_refreshes_card_details() {
            let ctx = test_env();
            let mut state = UiState::new(
                vec![
                    task_row("github.com/acme/app", "main"),
                    task_row("github.com/acme/app", "feature"),
                ],
                vec![],
                None,
            );
            state.task_selected = 1;
            let mut loader = LoaderHandle::noop();
            let mut card_refresh = None;
            let mut current = Some(fingerprint("main", "session-a"));

            handle_task_topology_fingerprint(
                &ctx,
                &mut state,
                &mut loader,
                &mut card_refresh,
                &mut current,
                fingerprint("main", "session-b"),
            );

            assert_eq!(state.task_rows.len(), 2);
            assert_eq!(state.task_selected, 1);
            assert!(card_refresh.is_some());
        }
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
                worktree_name: String::from("main"),
                path: PathBuf::from("/tmp/a/main"),
                opencode: crate::tools::opencode::status::OpenCodeState::None,
            };
            UiState::new(vec![row], vec![], None)
        }

        /// `now` that is strictly past the refresh interval from
        /// `last_refresh`, so the interval-gate branch never blocks
        /// when a test wants a spawn to happen.
        fn past_interval(last: Instant) -> Instant {
            last.checked_add(OPENCODE_REFRESH_INTERVAL)
                .and_then(|instant| instant.checked_add(std::time::Duration::from_millis(1)))
                .expect("instant overflow")
        }

        #[test]
        fn skips_when_handle_in_flight() {
            let state = state_with_one_task();
            let last = Instant::now()
                .checked_sub(OPENCODE_REFRESH_INTERVAL)
                .expect("instant underflow");
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
            let last = Instant::now()
                .checked_sub(OPENCODE_REFRESH_INTERVAL * 10)
                .unwrap();
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
            let last = Instant::now()
                .checked_sub(OPENCODE_REFRESH_INTERVAL * 10)
                .unwrap();
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
