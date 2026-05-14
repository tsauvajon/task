use std::{collections::HashMap, path::PathBuf};

use ratatui::layout::Rect;

use crate::{
    runtime::{BranchName, RepoKey, task_rows::TaskRow},
    tools::{git::worktrees::WorktreeDiff, opencode::status::OpenCodeState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputMode {
    Normal,
    Filter,
    CreateTask,
    CloneRepo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewMode {
    Tasks,
    Repos,
}

/// User preference for the right-hand sidebar (actions + activity panels).
///
/// Mirrors OpenCode's `"auto" | "hide"` signal: `Auto` lets the sidebar
/// appear when the terminal is wide enough, `Hide` suppresses it. A
/// separate explicit-open flag on `UiState` can force the sidebar on
/// even in narrow terminals.
///
/// Threshold matches OpenCode: `width > 120`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarMode {
    Auto,
    Hide,
}

/// Width threshold (exclusive) above which the sidebar auto-shows in
/// `SidebarMode::Auto`. Matches OpenCode's `wide = width > 120`.
pub(super) const SIDEBAR_AUTO_WIDTH_THRESHOLD: u16 = 120;

/// Fixed width of the right-hand sidebar when visible. Matches
/// OpenCode, which reserves 42 columns for its sidebar independent of
/// terminal width (so the main content always grows/shrinks with the
/// window instead of the sidebar).
pub(super) const SIDEBAR_WIDTH: u16 = 42;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepoRow {
    pub(super) repo: RepoKey,
    pub(super) open_tasks: usize,
    pub(super) parked_tasks: usize,
    pub(super) is_detached: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TaskCardDetails {
    pub(super) diff: WorktreeDiff,
    pub(super) session_title: Option<String>,
    pub(super) last_activity_ms: Option<i64>,
}

/// Progress of a background load. `total` is `None` while the
/// repo-enumeration scan is still running in the loader thread — the
/// status bar renders `… / ?` in that window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LoadPhase {
    Idle,
    Loading { done: usize, total: Option<usize> },
}

impl LoadPhase {
    pub(super) fn is_loading(&self) -> bool {
        matches!(self, LoadPhase::Loading { .. })
    }
}

/// Messages sent by the background loader thread to the UI event loop.
///
/// `generation` is the loader-generation counter; the main thread bumps it
/// on every `refresh` and drops any message whose `generation` does not
/// match the current one. This keeps stale workers from corrupting state
/// after a refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LoadMsg {
    /// Sent once the loader has enumerated all cloned repos and knows the
    /// total count for both scans.
    ScanStarted {
        generation: u64,
        total: usize,
    },
    TaskRowsForRepo {
        generation: u64,
        repo: RepoKey,
        rows: Vec<TaskRow>,
    },
    /// One repo finished its task-scan (success or error). Advances the
    /// `done` counter on the tasks `LoadPhase::Loading`.
    TaskRepoDone {
        generation: u64,
    },
    /// All task-scan workers have returned. Flips `task_load` to `Idle`.
    TasksComplete {
        generation: u64,
    },
    RepoRow {
        generation: u64,
        row: RepoRow,
    },
    /// All repo-scan workers have returned. Flips `repo_load` to `Idle`.
    RepoRowsDone {
        generation: u64,
    },
    /// A per-repo git call failed. Surfaces a muted entry in the activity
    /// panel and bumps `skipped_repos_count`.
    RepoError {
        generation: u64,
        repo: RepoKey,
        err: String,
    },
    /// Periodic refresh of OpenCode session states for every tracked task
    /// row. Sent by [`crate::ui::loader::spawn_opencode_refresh`] on a short
    /// cadence while the Tasks view is visible.
    ///
    /// Unlike the other load messages this one is deliberately *not*
    /// tagged with a `generation`: its payload is keyed by worktree
    /// path, so a stale tick can only update rows that still exist in
    /// the current scope. Dropping a stale tick would introduce a
    /// ~600ms staleness window after every full refresh for no safety
    /// gain.
    ///
    /// Path-uniqueness premise: this is safe only because every task
    /// worktree path in the workspace is uniquely keyed by
    /// `(repo, branch)` (see `<wt_dir>/<repo_key>/<branch>` in
    /// `RuntimeEnvironment`). If a future layout change ever reused a
    /// path across worktrees, an in-flight tick from a previous scope
    /// could write the wrong row's state.
    OpenCodeTick {
        states: Vec<(PathBuf, OpenCodeState)>,
    },
    /// Periodic refresh of card-only metadata for every tracked task
    /// row. Path-keyed like `OpenCodeTick`, so stale ticks can only
    /// update rows that still exist in the current scope.
    TaskCardDetailsTick {
        details: Vec<(PathBuf, TaskCardDetails)>,
    },
}

impl LoadMsg {
    /// Generation stamp used by the UI to ignore messages from a
    /// superseded loader. `OpenCodeTick` has no generation because it
    /// is safe to apply regardless (see the variant docs).
    pub(super) fn generation(&self) -> Option<u64> {
        match self {
            Self::ScanStarted { generation, .. }
            | Self::TaskRowsForRepo { generation, .. }
            | Self::TaskRepoDone { generation }
            | Self::TasksComplete { generation }
            | Self::RepoRow { generation, .. }
            | Self::RepoRowsDone { generation }
            | Self::RepoError { generation, .. } => Some(*generation),
            Self::OpenCodeTick { .. } | Self::TaskCardDetailsTick { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum UiAction {
    Quit,
    Open(TaskRow),
    Create { repo: String, branch: String },
}

#[derive(Debug, Clone)]
pub(super) struct UiState {
    pub(super) task_rows: Vec<TaskRow>,
    pub(super) task_card_details: HashMap<PathBuf, TaskCardDetails>,
    pub(super) task_filtered_indices: Vec<usize>,
    pub(super) task_selected: usize,
    pub(super) repo_rows: Vec<RepoRow>,
    pub(super) repo_filtered_indices: Vec<usize>,
    pub(super) repo_selected: usize,
    pub(super) filter_text: String,
    pub(super) task_repo_scope: Option<String>,
    pub(super) create_branch: String,
    pub(super) clone_input: String,
    pub(super) activity_lines: Vec<String>,
    pub(super) view: ViewMode,
    pub(super) mode: InputMode,
    pub(super) message: String,
    pub(super) show_help: bool,
    pub(super) help_area: Option<Rect>,
    pub(super) visible_rows: usize,
    /// Progress of the background tasks-scan.
    pub(super) task_load: LoadPhase,
    /// Progress of the background repo-scan.
    pub(super) repo_load: LoadPhase,
    /// Animated spinner frame counter; advanced each tick by the event loop.
    pub(super) spinner_frame: u8,
    /// Current loader generation. Incremented on every refresh so stale
    /// messages from an old loader worker are dropped by `apply_load_msg`.
    pub(super) load_generation: u64,
    /// Count of repos that produced an error during the current load. Reset
    /// at the start of each refresh.
    pub(super) skipped_repos_count: usize,
    /// Identity of the repo that was selected when the current load started.
    /// Used by `insert_repo_row_sorted` to re-anchor the cursor on the same
    /// repo once it streams back in. Cleared after a successful re-anchor or
    /// when the load finishes without the repo reappearing.
    pub(super) pending_repo_selection: Option<RepoKey>,
    /// Identity of the task row (repo + branch) that was selected when the
    /// current load started. Mirrors `pending_repo_selection` for the tasks
    /// view.
    pub(super) pending_task_selection: Option<(RepoKey, BranchName)>,
    /// Sidebar preference. `Auto` shows the sidebar when the terminal is
    /// wide (width > 120); `Hide` suppresses it. Toggled manually with
    /// `b` in Normal mode.
    pub(super) sidebar_mode: SidebarMode,
    /// Explicit-open override. When `true`, the sidebar is shown even on
    /// narrow terminals. Set by the manual toggle when opening from a
    /// hidden state, cleared when toggling to hide.
    pub(super) sidebar_open: bool,
    /// Width of the last rendered frame, captured by `render_body`. Used
    /// by the `ToggleSidebar` intent handler to decide which direction
    /// the toggle should flip without re-querying the terminal.
    pub(super) last_frame_width: u16,
}

impl UiState {
    pub(super) fn new(
        task_rows: Vec<TaskRow>,
        repo_rows: Vec<RepoRow>,
        task_repo_scope: Option<String>,
    ) -> Self {
        let mut state = Self {
            task_rows,
            task_card_details: HashMap::new(),
            task_filtered_indices: Vec::new(),
            task_selected: 0,
            repo_rows,
            repo_filtered_indices: Vec::new(),
            repo_selected: 0,
            filter_text: String::new(),
            task_repo_scope,
            create_branch: String::new(),
            clone_input: String::new(),
            activity_lines: Vec::new(),
            view: ViewMode::Tasks,
            mode: InputMode::Normal,
            message: "Ready".to_string(),
            show_help: false,
            help_area: None,
            visible_rows: 20,
            task_load: LoadPhase::Idle,
            repo_load: LoadPhase::Idle,
            spinner_frame: 0,
            load_generation: 0,
            skipped_repos_count: 0,
            pending_repo_selection: None,
            pending_task_selection: None,
            sidebar_mode: SidebarMode::Auto,
            sidebar_open: false,
            last_frame_width: 0,
        };
        state.apply_filters();
        state
    }

    /// Whether the right-hand sidebar should be rendered at the given
    /// terminal width.
    ///
    /// Mirrors OpenCode's visibility rule:
    /// - explicit-open flag wins;
    /// - otherwise `Auto` + wide terminal shows the sidebar;
    /// - `Hide` always suppresses it (unless explicitly opened).
    pub(super) fn sidebar_visible(&self, width: u16) -> bool {
        if self.sidebar_open {
            return true;
        }
        matches!(self.sidebar_mode, SidebarMode::Auto) && width > SIDEBAR_AUTO_WIDTH_THRESHOLD
    }

    /// Flip the sidebar between shown and hidden, using the given
    /// terminal width to decide the starting state.
    ///
    /// - Currently visible → transition to `Hide` + clear explicit-open
    ///   so the sidebar stays hidden even on wide terminals until the
    ///   user toggles back.
    /// - Currently hidden → transition to `Auto` and force
    ///   explicit-open so the sidebar appears even on narrow terminals.
    pub(super) fn toggle_sidebar(&mut self, width: u16) {
        if self.sidebar_visible(width) {
            self.sidebar_mode = SidebarMode::Hide;
            self.sidebar_open = false;
        } else {
            self.sidebar_mode = SidebarMode::Auto;
            self.sidebar_open = true;
        }
    }

    /// Construct an empty state ready for a background load. Used by the
    /// progressive UI path: we enter the terminal and draw the first frame
    /// before any rows have been collected.
    pub(super) fn new_empty_loading(task_repo_scope: Option<String>) -> Self {
        let mut state = Self::new(Vec::new(), Vec::new(), task_repo_scope);
        state.task_load = LoadPhase::Loading {
            done: 0,
            total: None,
        };
        state.repo_load = LoadPhase::Loading {
            done: 0,
            total: None,
        };
        state.message = "Loading…".to_string();
        state
    }

    /// Apply a message from the background loader, advancing progress and
    /// inserting rows in sorted order. Messages whose `generation` does
    /// not match the current one are ignored (they come from a superseded
    /// loader).
    ///
    /// `OpenCodeTick` is an exception: it carries no generation (see
    /// [`LoadMsg::OpenCodeTick`] docs) and is applied unconditionally.
    /// Its payload is keyed by path, so a stale tick can only update
    /// rows that survived the current scope's refresh.
    pub(super) fn apply_load_msg(&mut self, msg: LoadMsg) {
        if let Some(generation) = msg.generation()
            && generation != self.load_generation
        {
            return;
        }

        match msg {
            LoadMsg::ScanStarted { total, .. } => {
                if let LoadPhase::Loading {
                    total: task_total, ..
                } = &mut self.task_load
                {
                    *task_total = Some(total);
                }
                if let LoadPhase::Loading {
                    total: repo_total, ..
                } = &mut self.repo_load
                {
                    *repo_total = Some(total);
                }
            }
            LoadMsg::TaskRowsForRepo { rows, .. } => {
                if !rows.is_empty() {
                    self.insert_task_rows_sorted(rows);
                }
            }
            LoadMsg::TaskRepoDone { .. } => {
                if let LoadPhase::Loading { done, .. } = &mut self.task_load {
                    *done += 1;
                }
            }
            LoadMsg::TasksComplete { .. } => {
                self.task_load = LoadPhase::Idle;
                // Load is over: if the pending task never came back (e.g.
                // the worktree was removed), drop the pending identity so
                // it cannot re-anchor a later unrelated refresh.
                self.pending_task_selection = None;
            }
            LoadMsg::RepoRow { row, .. } => {
                self.insert_repo_row_sorted(row);
                if let LoadPhase::Loading { done, .. } = &mut self.repo_load {
                    *done += 1;
                }
            }
            LoadMsg::RepoRowsDone { .. } => {
                self.repo_load = LoadPhase::Idle;
                // Load is over: if the pending repo never came back (e.g.
                // it was deleted), drop the pending identity so it cannot
                // re-anchor a later unrelated refresh.
                self.pending_repo_selection = None;
            }
            LoadMsg::RepoError { repo, err, .. } => {
                self.skipped_repos_count += 1;
                self.append_activity_lines(vec![format!("{repo}: {err}")]);
            }
            LoadMsg::OpenCodeTick { states, .. } => {
                self.apply_opencode_states(&states);
            }
            LoadMsg::TaskCardDetailsTick { details } => {
                self.apply_task_card_details(&details);
            }
        }
    }

    /// Write fresh OpenCode states into the current `task_rows`, matched
    /// by worktree path. Unknown paths are silently ignored so
    /// out-of-band refreshes can race with a full reload without
    /// corrupting state.
    pub(super) fn apply_opencode_states(&mut self, states: &[(PathBuf, OpenCodeState)]) {
        if states.is_empty() || self.task_rows.is_empty() {
            return;
        }
        for (path, state) in states {
            if let Some(row) = self.task_rows.iter_mut().find(|row| &row.path == path) {
                row.opencode = *state;
            }
        }
    }

    pub(super) fn apply_task_card_details(&mut self, details: &[(PathBuf, TaskCardDetails)]) {
        if details.is_empty() || self.task_rows.is_empty() {
            return;
        }
        let mut changed = false;
        for (path, detail) in details {
            if self.task_rows.iter().any(|row| &row.path == path) {
                self.task_card_details.insert(path.clone(), detail.clone());
                changed = true;
            }
        }
        if changed && !self.filter_text.is_empty() {
            self.apply_task_filter();
        }
    }

    pub(super) fn task_card_details_for(&self, row: &TaskRow) -> TaskCardDetails {
        self.task_card_details
            .get(&row.path)
            .cloned()
            .unwrap_or_default()
    }

    /// Bump `load_generation` and return the new value. Use this when
    /// starting a new load so the caller can stamp it into the loader
    /// thread; any in-flight messages from the previous generation will
    /// be ignored by `apply_load_msg`.
    pub(super) fn begin_load(&mut self) -> u64 {
        // Remember the identities of the currently-selected rows so that
        // streaming inserts can re-anchor the cursor to the same repo/task
        // once it arrives. Without this, the cursor would latch onto
        // whichever row happened to return first from the background
        // loader, which looks random to the user.
        self.pending_repo_selection = self.selected_repo_row().map(|row| row.repo.clone());
        self.pending_task_selection = self
            .selected_task_row()
            .map(|row| (row.repo.clone(), row.branch.clone()));

        self.load_generation = self.load_generation.wrapping_add(1);
        self.task_rows.clear();
        self.task_card_details.clear();
        self.repo_rows.clear();
        self.task_selected = 0;
        self.repo_selected = 0;
        self.skipped_repos_count = 0;
        self.task_load = LoadPhase::Loading {
            done: 0,
            total: None,
        };
        self.repo_load = LoadPhase::Loading {
            done: 0,
            total: None,
        };
        self.apply_filters();
        self.load_generation
    }

    /// Insert task rows while preserving the global sort order
    /// `(status, repo, branch)`.
    ///
    /// Cursor behavior:
    /// - If `pending_task_selection` is `Some` (a load is in flight and
    ///   we captured the user's pre-refresh selection), we keep the
    ///   cursor locked onto that identity for the whole load. Found →
    ///   anchor cursor on it; missing → leave the cursor at index 0
    ///   while we wait for it. The pending identity is cleared by
    ///   `RepoRowsDone` / `TasksComplete` (or by the next `begin_load`).
    /// - Otherwise preserve the live selection by re-locating its
    ///   identity in the new list (falls back to index 0 if it was
    ///   filtered out or the row disappeared entirely).
    pub(super) fn insert_task_rows_sorted(&mut self, rows: Vec<TaskRow>) {
        let live_identity = self
            .selected_task_row()
            .map(|row| (row.repo.clone(), row.branch.clone()));
        let pending_identity = self.pending_task_selection.clone();

        self.task_rows.extend(rows);
        self.task_rows.sort_by(|left, right| {
            left.status
                .cmp(&right.status)
                .then(left.repo.cmp(&right.repo))
                .then(left.branch.cmp(&right.branch))
        });
        self.apply_task_filter();

        if let Some(pending) = pending_identity {
            // Keep pending set for the duration of the load so every
            // subsequent row re-anchors. Clearing it here would let
            // later rows fall through to the live-identity branch and
            // drift the cursor as the sort order changes.
            self.task_selected = self.find_task_index(&pending.0, &pending.1).unwrap_or(0);
        } else if self.task_load.is_loading() {
            // No pending identity means this is an initial load with no
            // prior selection. Pin the cursor to the top of the current
            // sorted list instead of chasing whichever row arrived first.
            self.task_selected = 0;
        } else if let Some((repo, branch)) = live_identity {
            self.task_selected = self.find_task_index(&repo, &branch).unwrap_or(0);
        }
    }

    fn find_task_index(&self, repo: &RepoKey, branch: &BranchName) -> Option<usize> {
        self.task_filtered_indices.iter().position(|&idx| {
            let Some(row) = self.task_rows.get(idx) else {
                return false;
            };
            &row.repo == repo && &row.branch == branch
        })
    }

    /// Insert one repo row, preserving sort order used elsewhere
    /// (open desc, parked desc, detached desc, repo asc).
    ///
    /// Mirrors `insert_task_rows_sorted` for cursor behavior: while a
    /// pending pre-refresh identity is set, we keep re-anchoring the
    /// cursor to it on every row (even after it first appears, since
    /// later rows may shift its sort position). Outside a pending load
    /// we preserve the live selection. An initial load with no pending
    /// identity keeps the cursor at the top.
    pub(super) fn insert_repo_row_sorted(&mut self, row: RepoRow) {
        let live_identity = self.selected_repo_row().map(|row| row.repo.clone());
        let pending_identity = self.pending_repo_selection.clone();

        self.repo_rows.push(row);
        self.repo_rows.sort_by(|left, right| {
            right
                .open_tasks
                .cmp(&left.open_tasks)
                .then(right.parked_tasks.cmp(&left.parked_tasks))
                .then(right.is_detached.cmp(&left.is_detached))
                .then(left.repo.cmp(&right.repo))
        });
        self.apply_repo_filter();

        if let Some(pending) = pending_identity {
            // Keep pending set for the duration of the load so every
            // subsequent row re-anchors. Clearing it on first match
            // would let later rows fall through to the live-identity
            // branch, which can snap the cursor to 0 as the sort order
            // changes.
            self.repo_selected = self.find_repo_index(&pending).unwrap_or(0);
        } else if self.repo_load.is_loading() {
            // Initial load with no prior selection: pin to the top
            // rather than chasing whichever row arrived first.
            self.repo_selected = 0;
        } else if let Some(repo) = live_identity {
            self.repo_selected = self.find_repo_index(&repo).unwrap_or(0);
        }
    }

    fn find_repo_index(&self, repo: &RepoKey) -> Option<usize> {
        self.repo_filtered_indices
            .iter()
            .position(|&idx| self.repo_rows.get(idx).is_some_and(|row| &row.repo == repo))
    }

    pub(super) fn apply_filters(&mut self) {
        self.apply_task_filter();
        self.apply_repo_filter();
    }

    pub(super) fn apply_task_filter(&mut self) {
        let tokens: Vec<String> = self
            .filter_text
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .collect();
        self.task_filtered_indices = self
            .task_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                if tokens.is_empty() {
                    return true;
                }

                let session_title = self
                    .task_card_details
                    .get(&row.path)
                    .and_then(|details| details.session_title.as_deref())
                    .unwrap_or_default();
                let haystack = format!(
                    "{} {} {} {}",
                    row.repo.to_lowercase(),
                    row.branch.to_lowercase(),
                    row.path.to_string_lossy().to_lowercase(),
                    session_title.to_lowercase(),
                );
                tokens.iter().all(|token| haystack.contains(token.as_str()))
            })
            .map(|(index, _)| index)
            .collect();

        if self.task_selected >= self.task_filtered_indices.len() {
            self.task_selected = self.task_filtered_indices.len().saturating_sub(1);
        }
    }

    pub(super) fn selected_task_row(&self) -> Option<&TaskRow> {
        let index = *self.task_filtered_indices.get(self.task_selected)?;
        self.task_rows.get(index)
    }

    pub(super) fn selected_repo_row(&self) -> Option<&RepoRow> {
        let index = *self.repo_filtered_indices.get(self.repo_selected)?;
        self.repo_rows.get(index)
    }

    pub(super) fn apply_repo_filter(&mut self) {
        let tokens: Vec<String> = self
            .filter_text
            .split_whitespace()
            .map(|token| token.to_lowercase())
            .collect();
        self.repo_filtered_indices = self
            .repo_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                if tokens.is_empty() {
                    return true;
                }

                let repo = row.repo.to_lowercase();
                tokens.iter().all(|token| repo.contains(token))
            })
            .map(|(index, _)| index)
            .collect();

        if self.repo_selected >= self.repo_filtered_indices.len() {
            self.repo_selected = self.repo_filtered_indices.len().saturating_sub(1);
        }
    }

    pub(super) fn move_next(&mut self) {
        match self.view {
            ViewMode::Tasks => {
                if self.task_filtered_indices.is_empty() {
                    return;
                }
                self.task_selected = (self.task_selected + 1) % self.task_filtered_indices.len();
            }
            ViewMode::Repos => {
                if self.repo_filtered_indices.is_empty() {
                    return;
                }
                self.repo_selected = (self.repo_selected + 1) % self.repo_filtered_indices.len();
            }
        }
    }

    pub(super) fn move_prev(&mut self) {
        match self.view {
            ViewMode::Tasks => {
                if self.task_filtered_indices.is_empty() {
                    return;
                }
                self.task_selected = if self.task_selected == 0 {
                    self.task_filtered_indices.len() - 1
                } else {
                    self.task_selected - 1
                };
            }
            ViewMode::Repos => {
                if self.repo_filtered_indices.is_empty() {
                    return;
                }
                self.repo_selected = if self.repo_selected == 0 {
                    self.repo_filtered_indices.len() - 1
                } else {
                    self.repo_selected - 1
                };
            }
        }
    }

    pub(super) fn move_page_down(&mut self) {
        let (selected, len) = match self.view {
            ViewMode::Tasks => (&mut self.task_selected, self.task_filtered_indices.len()),
            ViewMode::Repos => (&mut self.repo_selected, self.repo_filtered_indices.len()),
        };
        if len == 0 {
            return;
        }
        *selected = (*selected + self.visible_rows).min(len - 1);
    }

    pub(super) fn move_page_up(&mut self) {
        let (selected, len) = match self.view {
            ViewMode::Tasks => (&mut self.task_selected, self.task_filtered_indices.len()),
            ViewMode::Repos => (&mut self.repo_selected, self.repo_filtered_indices.len()),
        };
        if len == 0 {
            return;
        }
        *selected = selected.saturating_sub(self.visible_rows);
    }

    pub(super) fn move_first(&mut self) {
        match self.view {
            ViewMode::Tasks => self.task_selected = 0,
            ViewMode::Repos => self.repo_selected = 0,
        }
    }

    pub(super) fn move_last(&mut self) {
        let (selected, len) = match self.view {
            ViewMode::Tasks => (&mut self.task_selected, self.task_filtered_indices.len()),
            ViewMode::Repos => (&mut self.repo_selected, self.repo_filtered_indices.len()),
        };
        *selected = len.saturating_sub(1);
    }

    pub(super) fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows;
    }

    #[cfg(test)]
    pub(super) fn set_task_rows(&mut self, rows: Vec<TaskRow>) {
        self.task_rows = rows;
        self.apply_task_filter();
    }

    #[cfg(test)]
    pub(super) fn set_repo_rows(&mut self, rows: Vec<RepoRow>) {
        self.repo_rows = rows;
        self.apply_repo_filter();
    }

    pub(super) fn switch_view(&mut self) {
        self.mode = InputMode::Normal;
        self.view = match self.view {
            ViewMode::Tasks => ViewMode::Repos,
            ViewMode::Repos => ViewMode::Tasks,
        };
    }

    pub(super) fn filter_backspace(&mut self) {
        self.filter_text.pop();
        self.apply_filters();
    }

    pub(super) fn filter_clear(&mut self) {
        self.filter_text.clear();
        self.apply_filters();
    }

    pub(super) fn filter_append(&mut self, ch: char) {
        self.filter_text.push(ch);
        self.apply_filters();
    }

    pub(super) fn select_repo_for_tasks(&mut self, repo: String) {
        self.task_repo_scope = Some(repo);
        self.view = ViewMode::Tasks;
        self.mode = InputMode::Normal;
    }

    pub(super) fn clear_repo_scope(&mut self) {
        self.task_repo_scope = None;
        self.task_selected = 0;
        self.view = ViewMode::Repos;
        self.apply_filters();
    }

    pub(super) fn append_activity_lines(&mut self, lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }

        self.activity_lines.extend(lines);
        const MAX_ACTIVITY_LINES: usize = 8;
        if self.activity_lines.len() > MAX_ACTIVITY_LINES {
            let extra = self.activity_lines.len() - MAX_ACTIVITY_LINES;
            self.activity_lines.drain(0..extra);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{RepoRow, TaskCardDetails, UiState};
    use crate::runtime::task_rows::{TaskRow, TaskStatus};

    fn repo_row(repo: &str, open_tasks: usize, parked_tasks: usize) -> RepoRow {
        use crate::runtime::RepoKey;
        RepoRow {
            repo: RepoKey::new(repo),
            open_tasks,
            parked_tasks,
            is_detached: false,
        }
    }

    fn sample_task_row() -> TaskRow {
        use crate::{
            runtime::{BranchName, RepoKey},
            tools::opencode::status::OpenCodeState,
        };
        TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new("github.com/acme/app"),
            branch: BranchName::new("main"),
            worktree_name: "main".to_string(),
            path: PathBuf::from("/tmp/dev/wt/github.com/acme/app/main"),
            opencode: OpenCodeState::None,
        }
    }

    fn task_row_for_repo(repo: &str) -> TaskRow {
        use crate::{
            runtime::{BranchName, RepoKey},
            tools::opencode::status::OpenCodeState,
        };
        TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new(repo),
            branch: BranchName::new("main"),
            worktree_name: "main".to_string(),
            path: PathBuf::from(format!("/tmp/dev/wt/{repo}/main")),
            opencode: OpenCodeState::None,
        }
    }

    mod repo_filter {
        use super::*;

        #[test]
        fn matches_repo_name_only() {
            let mut state = UiState::new(
                vec![sample_task_row()],
                vec![
                    repo_row("github.com/acme/app", 1, 2),
                    repo_row("github.com/acme/ops", 9, 9),
                ],
                None,
            );

            state.filter_text = "app".to_string();
            state.apply_repo_filter();

            assert_eq!(state.repo_filtered_indices, vec![0]);
            assert_eq!(
                state.selected_repo_row().map(|row| row.repo.as_str()),
                Some("github.com/acme/app")
            );
        }

        #[test]
        fn selection_clamps_when_filter_reduces_results() {
            let mut state = UiState::new(
                vec![sample_task_row()],
                vec![
                    repo_row("github.com/acme/app", 1, 0),
                    repo_row("github.com/acme/ops", 2, 0),
                    repo_row("github.com/acme/docs", 3, 0),
                ],
                None,
            );

            state.repo_selected = 2;
            state.filter_text = "ops".to_string();
            state.apply_repo_filter();

            assert_eq!(state.repo_filtered_indices, vec![1]);
            assert_eq!(state.repo_selected, 0);
            assert_eq!(
                state.selected_repo_row().map(|row| row.repo.as_str()),
                Some("github.com/acme/ops")
            );
        }

        #[test]
        fn matches_all_space_separated_tokens() {
            let mut state = UiState::new(
                vec![sample_task_row()],
                vec![
                    repo_row("github.com/tsauvajon/goto", 1, 0),
                    repo_row("github.com/other/repo", 1, 0),
                ],
                None,
            );

            state.filter_text = "gith tsa go".to_string();
            state.apply_repo_filter();

            assert_eq!(state.repo_filtered_indices, vec![0]);
            assert_eq!(
                state.selected_repo_row().map(|row| row.repo.as_str()),
                Some("github.com/tsauvajon/goto")
            );
        }

        #[test]
        fn matches_host_fragment() {
            let mut state = UiState::new(
                vec![sample_task_row()],
                vec![
                    repo_row("github.com/acme/app", 1, 0),
                    repo_row("gitlab.com/acme/app", 1, 0),
                ],
                None,
            );

            state.filter_text = "gitlab".to_string();
            state.apply_repo_filter();

            assert_eq!(state.repo_filtered_indices, vec![1]);
            assert_eq!(
                state.selected_repo_row().map(|row| row.repo.as_str()),
                Some("gitlab.com/acme/app")
            );
        }
    }

    mod filter_text {
        use super::*;

        #[test]
        fn is_shared_between_task_and_repo_views() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                ],
                vec![
                    repo_row("github.com/acme/app", 1, 0),
                    repo_row("github.com/acme/ops", 1, 0),
                ],
                None,
            );

            state.filter_text = "app".to_string();
            state.apply_filters();

            assert_eq!(state.task_filtered_indices, vec![0]);
            assert_eq!(state.repo_filtered_indices, vec![0]);
        }

        #[test]
        fn append_updates_filter_and_reapplies() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                ],
                vec![],
                None,
            );
            state.filter_append('o');
            state.filter_append('p');
            state.filter_append('s');
            assert_eq!(state.filter_text, "ops");
            assert_eq!(state.task_filtered_indices, vec![1]);
        }

        #[test]
        fn backspace_removes_last_char_and_reapplies() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                ],
                vec![],
                None,
            );
            state.filter_text = "ops".to_string();
            state.apply_filters();
            assert_eq!(state.task_filtered_indices.len(), 1);

            state.filter_backspace(); // "op"
            state.filter_backspace(); // "o"
            state.filter_backspace(); // ""
            assert_eq!(state.filter_text, "");
            // Empty filter → all rows visible
            assert_eq!(state.task_filtered_indices.len(), 2);
        }

        #[test]
        fn clear_resets_and_shows_all_rows() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                ],
                vec![],
                None,
            );
            state.filter_text = "app".to_string();
            state.apply_filters();
            assert_eq!(state.task_filtered_indices.len(), 1);

            state.filter_clear();
            assert_eq!(state.filter_text, "");
            assert_eq!(state.task_filtered_indices.len(), 2);
        }
    }

    mod navigation {
        use super::*;

        #[test]
        fn move_next_advances_task_selection() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/a"),
                    task_row_for_repo("github.com/acme/b"),
                ],
                vec![],
                None,
            );
            assert_eq!(state.task_selected, 0);
            state.move_next();
            assert_eq!(state.task_selected, 1);
        }

        #[test]
        fn move_next_wraps_to_first_item() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/a"),
                    task_row_for_repo("github.com/acme/b"),
                ],
                vec![],
                None,
            );
            state.task_selected = 1;
            state.move_next(); // wraps back to 0
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_prev_decrements_task_selection() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/a"),
                    task_row_for_repo("github.com/acme/b"),
                ],
                vec![],
                None,
            );
            state.task_selected = 1;
            state.move_prev();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_prev_wraps_to_last_item() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/a"),
                    task_row_for_repo("github.com/acme/b"),
                ],
                vec![],
                None,
            );
            state.move_prev(); // wraps from 0 to last item
            assert_eq!(state.task_selected, 1);
        }

        #[test]
        fn move_next_does_nothing_on_empty_list() {
            let mut state = UiState::new(vec![], vec![], None);
            state.move_next(); // must not panic
            assert_eq!(state.task_selected, 0);
        }
    }

    mod switch_view {
        use super::*;

        #[test]
        fn toggles_between_tasks_and_repos() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], vec![], None);
            assert_eq!(state.view, ViewMode::Tasks);
            state.switch_view();
            assert_eq!(state.view, ViewMode::Repos);
            state.switch_view();
            assert_eq!(state.view, ViewMode::Tasks);
        }

        #[test]
        fn resets_mode_to_normal() {
            use super::super::{InputMode, ViewMode};
            let mut state = UiState::new(vec![], vec![], None);
            state.mode = InputMode::Filter;
            state.switch_view();
            assert_eq!(state.mode, InputMode::Normal);
            assert_eq!(state.view, ViewMode::Repos);
        }
    }

    mod set_rows {
        use super::*;

        #[test]
        fn set_task_rows_replaces_rows_and_reapplies_filter() {
            let mut state =
                UiState::new(vec![task_row_for_repo("github.com/acme/app")], vec![], None);
            state.filter_text = "ops".to_string();
            state.apply_filters();
            assert_eq!(state.task_filtered_indices.len(), 0);

            state.set_task_rows(vec![
                task_row_for_repo("github.com/acme/ops"),
                task_row_for_repo("github.com/acme/app"),
            ]);
            // "ops" filter now matches one of the two new rows
            assert_eq!(state.task_filtered_indices.len(), 1);
        }

        #[test]
        fn set_repo_rows_replaces_repo_rows_and_reapplies_filter() {
            let mut state = UiState::new(vec![], vec![repo_row("github.com/acme/app", 1, 0)], None);
            state.filter_text = "ops".to_string();
            state.apply_repo_filter();
            assert_eq!(state.repo_filtered_indices.len(), 0);

            state.set_repo_rows(vec![repo_row("github.com/acme/ops", 2, 0)]);
            assert_eq!(state.repo_filtered_indices.len(), 1);
        }
    }

    mod selected_row {
        use super::*;

        #[test]
        fn selected_task_row_returns_none_on_empty_list() {
            let state = UiState::new(vec![], vec![], None);
            assert!(state.selected_task_row().is_none());
        }

        #[test]
        fn selected_repo_row_returns_none_on_empty_list() {
            let state = UiState::new(vec![], vec![], None);
            assert!(state.selected_repo_row().is_none());
        }

        #[test]
        fn selected_task_row_returns_first_by_default() {
            let state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/a"),
                    task_row_for_repo("github.com/acme/b"),
                ],
                vec![],
                None,
            );
            assert_eq!(
                state.selected_task_row().map(|r| r.repo.to_string()),
                Some("github.com/acme/a".to_string())
            );
        }

        #[test]
        fn selected_repo_row_returns_first_by_default() {
            let state = UiState::new(
                vec![],
                vec![
                    repo_row("github.com/acme/a", 1, 0),
                    repo_row("github.com/acme/b", 2, 0),
                ],
                None,
            );
            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/acme/a")
            );
        }
    }

    mod select_repo_for_tasks {
        use super::*;

        #[test]
        fn sets_scope_and_switches_to_tasks_view() {
            use super::super::{InputMode, ViewMode};
            let mut state = UiState::new(vec![], vec![], None);
            state.view = ViewMode::Repos;
            state.mode = InputMode::Filter;
            state.select_repo_for_tasks("github.com/acme/app".to_string());
            assert_eq!(
                state.task_repo_scope,
                Some("github.com/acme/app".to_string())
            );
            assert_eq!(state.view, ViewMode::Tasks);
            assert_eq!(state.mode, InputMode::Normal);
        }
    }

    mod clear_repo_scope {
        use std::path::PathBuf;

        use super::*;
        use crate::{
            runtime::{BranchName, RepoKey, task_rows::TaskStatus},
            tools::opencode::status::OpenCodeState,
        };

        fn task_row(repo: &str, branch: &str) -> crate::runtime::task_rows::TaskRow {
            crate::runtime::task_rows::TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                worktree_name: branch.to_string(),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
                opencode: OpenCodeState::None,
            }
        }

        #[test]
        fn clears_scope_and_resets_selection() {
            let mut state = UiState::new(
                vec![
                    task_row("github.com/org/a", "feat-1"),
                    task_row("github.com/org/b", "feat-2"),
                ],
                vec![],
                Some("github.com/org/a".to_string()),
            );
            state.task_selected = 1;

            state.clear_repo_scope();

            assert!(state.task_repo_scope.is_none());
            assert_eq!(state.task_selected, 0);
            assert_eq!(state.task_filtered_indices.len(), 2);
            assert_eq!(state.view, super::super::ViewMode::Repos);
        }

        #[test]
        fn noop_when_already_unscoped() {
            let mut state =
                UiState::new(vec![task_row("github.com/org/a", "feat-1")], vec![], None);

            state.clear_repo_scope();

            assert!(state.task_repo_scope.is_none());
            assert_eq!(state.task_filtered_indices.len(), 1);
        }
    }

    mod task_filter_matching {
        use super::*;

        #[test]
        fn matches_by_branch_name() {
            use std::path::PathBuf;

            use crate::{
                runtime::{BranchName, RepoKey, task_rows::TaskStatus},
                tools::opencode::status::OpenCodeState,
            };

            let mut state = UiState::new(
                vec![
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("feature-x"),
                        worktree_name: "feature-x".to_string(),
                        path: PathBuf::from("/tmp/a"),
                        opencode: OpenCodeState::None,
                    },
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("main"),
                        worktree_name: "main".to_string(),
                        path: PathBuf::from("/tmp/b"),
                        opencode: OpenCodeState::None,
                    },
                ],
                vec![],
                None,
            );
            state.filter_text = "feature".to_string();
            state.apply_task_filter();
            assert_eq!(state.task_filtered_indices, vec![0]);
        }

        #[test]
        fn matches_by_path() {
            use std::path::PathBuf;

            use crate::{
                runtime::{BranchName, RepoKey, task_rows::TaskStatus},
                tools::opencode::status::OpenCodeState,
            };

            let mut state = UiState::new(
                vec![
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("main"),
                        worktree_name: "main".to_string(),
                        path: PathBuf::from("/projects/special/path"),
                        opencode: OpenCodeState::None,
                    },
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("main"),
                        worktree_name: "main".to_string(),
                        path: PathBuf::from("/other/path"),
                        opencode: OpenCodeState::None,
                    },
                ],
                vec![],
                None,
            );
            state.filter_text = "special".to_string();
            state.apply_task_filter();
            assert_eq!(state.task_filtered_indices, vec![0]);
        }

        #[test]
        fn matches_by_session_title_when_card_details_arrive() {
            let row = task_row_for_repo("github.com/acme/app");
            let path = row.path.clone();
            let mut state = UiState::new(
                vec![row, task_row_for_repo("github.com/acme/other")],
                vec![],
                None,
            );
            state.filter_text = "compact cards".to_string();

            state.apply_task_filter();
            assert!(state.task_filtered_indices.is_empty());

            state.apply_task_card_details(&[(
                path,
                TaskCardDetails {
                    session_title: Some("Ship compact task cards".to_string()),
                    ..TaskCardDetails::default()
                },
            )]);

            assert_eq!(state.task_filtered_indices, vec![0]);
        }

        #[test]
        fn empty_filter_shows_all_tasks() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/a"),
                    task_row_for_repo("github.com/acme/b"),
                    task_row_for_repo("github.com/acme/c"),
                ],
                vec![],
                None,
            );
            state.filter_text = String::new();
            state.apply_task_filter();
            assert_eq!(state.task_filtered_indices.len(), 3);
        }

        #[test]
        fn filter_is_case_insensitive() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/ACME/App"),
                    task_row_for_repo("github.com/other/repo"),
                ],
                vec![],
                None,
            );
            state.filter_text = "acme".to_string();
            state.apply_task_filter();
            assert_eq!(state.task_filtered_indices, vec![0]);
        }
    }

    mod repo_navigation {
        use super::*;

        #[test]
        fn move_next_advances_repo_selection() {
            use super::super::ViewMode;
            let mut state = UiState::new(
                vec![],
                vec![
                    repo_row("github.com/acme/a", 1, 0),
                    repo_row("github.com/acme/b", 2, 0),
                ],
                None,
            );
            state.view = ViewMode::Repos;
            assert_eq!(state.repo_selected, 0);
            state.move_next();
            assert_eq!(state.repo_selected, 1);
        }

        #[test]
        fn move_next_wraps_to_first_repo() {
            use super::super::ViewMode;
            let mut state = UiState::new(
                vec![],
                vec![
                    repo_row("github.com/acme/a", 1, 0),
                    repo_row("github.com/acme/b", 2, 0),
                ],
                None,
            );
            state.view = ViewMode::Repos;
            state.repo_selected = 1;
            state.move_next(); // wraps back to 0
            assert_eq!(state.repo_selected, 0);
        }

        #[test]
        fn move_prev_decrements_repo_selection() {
            use super::super::ViewMode;
            let mut state = UiState::new(
                vec![],
                vec![
                    repo_row("github.com/acme/a", 1, 0),
                    repo_row("github.com/acme/b", 2, 0),
                ],
                None,
            );
            state.view = ViewMode::Repos;
            state.repo_selected = 1;
            state.move_prev();
            assert_eq!(state.repo_selected, 0);
        }

        #[test]
        fn move_prev_wraps_to_last_repo() {
            use super::super::ViewMode;
            let mut state = UiState::new(
                vec![],
                vec![
                    repo_row("github.com/acme/a", 1, 0),
                    repo_row("github.com/acme/b", 2, 0),
                ],
                None,
            );
            state.view = ViewMode::Repos;
            state.move_prev(); // wraps from 0 to last item
            assert_eq!(state.repo_selected, 1);
        }

        #[test]
        fn move_next_does_nothing_on_empty_repo_list() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], vec![], None);
            state.view = ViewMode::Repos;
            state.move_next(); // must not panic
            assert_eq!(state.repo_selected, 0);
        }
    }

    mod initial_state {
        use super::*;

        #[test]
        fn new_state_has_ready_message() {
            let state = UiState::new(vec![], vec![], None);
            assert_eq!(state.message, "Ready");
        }

        #[test]
        fn new_state_starts_in_tasks_view_normal_mode() {
            use super::super::{InputMode, ViewMode};
            let state = UiState::new(vec![], vec![], None);
            assert_eq!(state.view, ViewMode::Tasks);
            assert_eq!(state.mode, InputMode::Normal);
        }

        #[test]
        fn new_state_show_help_is_false() {
            let state = UiState::new(vec![], vec![], None);
            assert!(!state.show_help);
        }

        #[test]
        fn task_repo_scope_is_stored() {
            let state = UiState::new(vec![], vec![], Some("github.com/acme/app".to_string()));
            assert_eq!(
                state.task_repo_scope,
                Some("github.com/acme/app".to_string())
            );
        }

        #[test]
        fn default_visible_rows_is_twenty() {
            let state = UiState::new(vec![], vec![], None);
            assert_eq!(state.visible_rows, 20);
        }
    }

    mod visible_rows {
        use super::*;

        #[test]
        fn set_visible_rows_updates_field() {
            let mut state = UiState::new(vec![], vec![], None);
            state.set_visible_rows(42);
            assert_eq!(state.visible_rows, 42);
        }
    }

    mod sidebar {
        use super::{
            super::{SIDEBAR_AUTO_WIDTH_THRESHOLD, SidebarMode},
            *,
        };

        #[test]
        fn defaults_to_auto_mode_and_closed() {
            let state = UiState::new(vec![], vec![], None);
            assert_eq!(state.sidebar_mode, SidebarMode::Auto);
            assert!(!state.sidebar_open);
        }

        #[test]
        fn auto_visible_above_threshold() {
            let state = UiState::new(vec![], vec![], None);
            assert!(state.sidebar_visible(SIDEBAR_AUTO_WIDTH_THRESHOLD + 1));
        }

        #[test]
        fn auto_hidden_at_threshold() {
            // OpenCode uses strict `>` — width exactly at 120 is narrow.
            let state = UiState::new(vec![], vec![], None);
            assert!(!state.sidebar_visible(SIDEBAR_AUTO_WIDTH_THRESHOLD));
        }

        #[test]
        fn auto_hidden_below_threshold() {
            let state = UiState::new(vec![], vec![], None);
            assert!(!state.sidebar_visible(80));
        }

        #[test]
        fn hide_mode_hides_even_on_wide_terminals() {
            let mut state = UiState::new(vec![], vec![], None);
            state.sidebar_mode = SidebarMode::Hide;
            assert!(!state.sidebar_visible(200));
        }

        #[test]
        fn explicit_open_shows_on_narrow_terminals() {
            let mut state = UiState::new(vec![], vec![], None);
            state.sidebar_open = true;
            assert!(state.sidebar_visible(40));
        }

        #[test]
        fn explicit_open_overrides_hide_mode() {
            let mut state = UiState::new(vec![], vec![], None);
            state.sidebar_mode = SidebarMode::Hide;
            state.sidebar_open = true;
            assert!(state.sidebar_visible(200));
        }

        #[test]
        fn toggle_from_wide_visible_hides_and_sticks() {
            let mut state = UiState::new(vec![], vec![], None);
            assert!(state.sidebar_visible(200));
            state.toggle_sidebar(200);
            assert!(!state.sidebar_visible(200));
            assert_eq!(state.sidebar_mode, SidebarMode::Hide);
            assert!(!state.sidebar_open);
        }

        #[test]
        fn toggle_from_narrow_auto_forces_open() {
            let mut state = UiState::new(vec![], vec![], None);
            assert!(!state.sidebar_visible(80));
            state.toggle_sidebar(80);
            assert!(state.sidebar_visible(80));
            assert_eq!(state.sidebar_mode, SidebarMode::Auto);
            assert!(state.sidebar_open);
        }

        #[test]
        fn toggle_from_hide_reopens_even_on_narrow() {
            let mut state = UiState::new(vec![], vec![], None);
            state.sidebar_mode = SidebarMode::Hide;
            assert!(!state.sidebar_visible(80));
            state.toggle_sidebar(80);
            assert!(state.sidebar_visible(80));
        }

        #[test]
        fn toggle_round_trip_returns_to_visible_state() {
            let mut state = UiState::new(vec![], vec![], None);
            assert!(state.sidebar_visible(200));
            state.toggle_sidebar(200); // hide
            state.toggle_sidebar(200); // back to visible
            assert!(state.sidebar_visible(200));
        }
    }

    mod page_navigation {
        use super::*;

        fn many_task_rows(n: usize) -> Vec<TaskRow> {
            (0..n)
                .map(|i| task_row_for_repo(&format!("github.com/acme/repo-{i}")))
                .collect()
        }

        fn many_repo_rows(n: usize) -> Vec<RepoRow> {
            (0..n)
                .map(|i| repo_row(&format!("github.com/acme/repo-{i}"), 1, 0))
                .collect()
        }

        #[test]
        fn page_down_advances_by_visible_rows() {
            let mut state = UiState::new(many_task_rows(30), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 0;
            state.move_page_down();
            assert_eq!(state.task_selected, 10);
        }

        #[test]
        fn page_down_clamps_at_last_item() {
            let mut state = UiState::new(many_task_rows(30), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 25;
            state.move_page_down();
            assert_eq!(state.task_selected, 29);
        }

        #[test]
        fn page_down_already_at_last_stays_put() {
            let mut state = UiState::new(many_task_rows(30), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 29;
            state.move_page_down();
            assert_eq!(state.task_selected, 29);
        }

        #[test]
        fn page_down_on_empty_list_is_noop() {
            let mut state = UiState::new(vec![], vec![], None);
            state.visible_rows = 10;
            state.move_page_down();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn page_down_fewer_items_than_page_size() {
            let mut state = UiState::new(many_task_rows(5), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 0;
            state.move_page_down();
            assert_eq!(state.task_selected, 4);
        }

        #[test]
        fn page_up_moves_back_by_visible_rows() {
            let mut state = UiState::new(many_task_rows(30), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 20;
            state.move_page_up();
            assert_eq!(state.task_selected, 10);
        }

        #[test]
        fn page_up_clamps_at_zero() {
            let mut state = UiState::new(many_task_rows(30), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 5;
            state.move_page_up();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn page_up_already_at_first_stays_put() {
            let mut state = UiState::new(many_task_rows(30), vec![], None);
            state.visible_rows = 10;
            state.task_selected = 0;
            state.move_page_up();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn page_up_on_empty_list_is_noop() {
            let mut state = UiState::new(vec![], vec![], None);
            state.visible_rows = 10;
            state.move_page_up();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn page_down_advances_repo_selection() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], many_repo_rows(30), None);
            state.view = ViewMode::Repos;
            state.visible_rows = 10;
            state.repo_selected = 0;
            state.move_page_down();
            assert_eq!(state.repo_selected, 10);
        }

        #[test]
        fn page_up_clamps_repo_selection_at_zero() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], many_repo_rows(30), None);
            state.view = ViewMode::Repos;
            state.visible_rows = 10;
            state.repo_selected = 5;
            state.move_page_up();
            assert_eq!(state.repo_selected, 0);
        }
    }

    mod first_last_navigation {
        use super::*;

        fn many_task_rows(n: usize) -> Vec<TaskRow> {
            (0..n)
                .map(|i| task_row_for_repo(&format!("github.com/acme/repo-{i}")))
                .collect()
        }

        fn many_repo_rows(n: usize) -> Vec<RepoRow> {
            (0..n)
                .map(|i| repo_row(&format!("github.com/acme/repo-{i}"), 1, 0))
                .collect()
        }

        #[test]
        fn move_first_jumps_to_zero() {
            let mut state = UiState::new(many_task_rows(10), vec![], None);
            state.task_selected = 7;
            state.move_first();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_first_already_at_zero_stays_put() {
            let mut state = UiState::new(many_task_rows(10), vec![], None);
            state.task_selected = 0;
            state.move_first();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_first_on_empty_list_is_noop() {
            let mut state = UiState::new(vec![], vec![], None);
            state.move_first();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_last_jumps_to_end() {
            let mut state = UiState::new(many_task_rows(10), vec![], None);
            state.task_selected = 3;
            state.move_last();
            assert_eq!(state.task_selected, 9);
        }

        #[test]
        fn move_last_already_at_end_stays_put() {
            let mut state = UiState::new(many_task_rows(10), vec![], None);
            state.task_selected = 9;
            state.move_last();
            assert_eq!(state.task_selected, 9);
        }

        #[test]
        fn move_last_on_empty_list_is_noop() {
            let mut state = UiState::new(vec![], vec![], None);
            state.move_last();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_first_jumps_to_first_repo() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], many_repo_rows(10), None);
            state.view = ViewMode::Repos;
            state.repo_selected = 7;
            state.move_first();
            assert_eq!(state.repo_selected, 0);
        }

        #[test]
        fn move_last_jumps_to_last_repo() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], many_repo_rows(10), None);
            state.view = ViewMode::Repos;
            state.repo_selected = 3;
            state.move_last();
            assert_eq!(state.repo_selected, 9);
        }
    }

    mod activity_lines {
        use super::*;

        #[test]
        fn empty_input_is_noop() {
            let mut state = UiState::new(vec![], vec![], None);
            state.activity_lines = vec!["existing".to_string()];
            state.append_activity_lines(vec![]);
            assert_eq!(state.activity_lines, vec!["existing"]);
        }

        #[test]
        fn appends_lines() {
            let mut state = UiState::new(vec![], vec![], None);
            state.append_activity_lines(vec!["line-1".to_string(), "line-2".to_string()]);
            assert_eq!(state.activity_lines, vec!["line-1", "line-2"]);
        }

        #[test]
        fn drains_oldest_when_exceeding_cap() {
            let mut state = UiState::new(vec![], vec![], None);
            state.activity_lines = (0..6).map(|i| format!("old-{i}")).collect();
            state.append_activity_lines((0..5).map(|i| format!("new-{i}")).collect());
            // 6 + 5 = 11, drain 3 oldest → keep last 8
            assert_eq!(state.activity_lines.len(), 8);
            assert_eq!(state.activity_lines[0], "old-3");
            assert_eq!(state.activity_lines[3], "new-0");
            assert_eq!(state.activity_lines[7], "new-4");
        }

        #[test]
        fn exactly_at_cap_does_not_drain() {
            let mut state = UiState::new(vec![], vec![], None);
            state.activity_lines = (0..5).map(|i| format!("old-{i}")).collect();
            state.append_activity_lines((0..3).map(|i| format!("new-{i}")).collect());
            // 5 + 3 = 8, exactly at cap
            assert_eq!(state.activity_lines.len(), 8);
            assert_eq!(state.activity_lines[0], "old-0");
            assert_eq!(state.activity_lines[7], "new-2");
        }
    }

    mod navigation_edge_cases {
        use super::*;

        #[test]
        fn move_prev_on_empty_task_list_is_noop() {
            let mut state = UiState::new(vec![], vec![], None);
            state.move_prev();
            assert_eq!(state.task_selected, 0);
        }

        #[test]
        fn move_prev_on_empty_repo_list_is_noop() {
            use super::super::ViewMode;
            let mut state = UiState::new(vec![], vec![], None);
            state.view = ViewMode::Repos;
            state.move_prev();
            assert_eq!(state.repo_selected, 0);
        }

        #[test]
        fn single_element_list_navigation_is_stable() {
            let mut state =
                UiState::new(vec![task_row_for_repo("github.com/acme/app")], vec![], None);
            assert_eq!(state.task_selected, 0);
            state.move_next();
            assert_eq!(state.task_selected, 0);
            state.move_prev();
            assert_eq!(state.task_selected, 0);
        }
    }

    mod task_filter_clamping {
        use super::*;

        #[test]
        fn selection_clamps_when_filter_reduces_results() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                    task_row_for_repo("github.com/acme/docs"),
                ],
                vec![],
                None,
            );

            state.task_selected = 2;
            state.filter_text = "ops".to_string();
            state.apply_task_filter();

            assert_eq!(state.task_filtered_indices, vec![1]);
            assert_eq!(state.task_selected, 0);
        }
    }

    mod filter_edge_cases {
        use super::*;

        #[test]
        fn backspace_on_empty_filter_is_noop() {
            let mut state =
                UiState::new(vec![task_row_for_repo("github.com/acme/app")], vec![], None);
            assert!(state.filter_text.is_empty());
            state.filter_backspace(); // must not panic
            assert!(state.filter_text.is_empty());
            assert_eq!(state.task_filtered_indices.len(), 1);
        }

        #[test]
        fn whitespace_only_repo_filter_shows_all() {
            let mut state = UiState::new(
                vec![],
                vec![
                    repo_row("github.com/acme/app", 1, 0),
                    repo_row("github.com/acme/ops", 1, 0),
                ],
                None,
            );
            state.filter_text = "   ".to_string();
            state.apply_repo_filter();

            assert_eq!(state.repo_filtered_indices.len(), 2);
        }

        #[test]
        fn task_filter_splits_on_whitespace_like_repo_filter() {
            // Both task and repo filters use multi-token AND: "acme app"
            // splits into ["acme", "app"] and both must match somewhere in
            // the combined repo+branch+path.
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                    task_row_for_repo("github.com/other/app"),
                ],
                vec![],
                None,
            );
            state.filter_text = "acme app".to_string();
            state.apply_task_filter();

            // Only the row containing both "acme" AND "app" matches
            assert_eq!(state.task_filtered_indices, vec![0]);
        }

        #[test]
        fn task_filter_whitespace_only_shows_all() {
            let mut state = UiState::new(
                vec![
                    task_row_for_repo("github.com/acme/app"),
                    task_row_for_repo("github.com/acme/ops"),
                ],
                vec![],
                None,
            );
            state.filter_text = "   ".to_string();
            state.apply_task_filter();

            assert_eq!(state.task_filtered_indices.len(), 2);
        }
    }

    mod load_phase_reducer {
        use super::*;
        use crate::{
            runtime::RepoKey,
            ui::state::{LoadMsg, LoadPhase},
        };

        fn loading_state() -> UiState {
            UiState::new_empty_loading(None)
        }

        #[test]
        fn new_empty_loading_starts_in_loading_without_total() {
            let state = loading_state();
            assert_eq!(
                state.task_load,
                LoadPhase::Loading {
                    done: 0,
                    total: None
                }
            );
            assert_eq!(
                state.repo_load,
                LoadPhase::Loading {
                    done: 0,
                    total: None
                }
            );
            assert_eq!(state.load_generation, 0);
        }

        #[test]
        fn scan_started_fills_total_for_both_phases() {
            let mut state = loading_state();
            state.apply_load_msg(LoadMsg::ScanStarted {
                generation: 0,
                total: 42,
            });
            assert_eq!(
                state.task_load,
                LoadPhase::Loading {
                    done: 0,
                    total: Some(42)
                }
            );
            assert_eq!(
                state.repo_load,
                LoadPhase::Loading {
                    done: 0,
                    total: Some(42)
                }
            );
        }

        #[test]
        fn task_repo_done_increments_done() {
            let mut state = loading_state();
            state.apply_load_msg(LoadMsg::ScanStarted {
                generation: 0,
                total: 3,
            });
            state.apply_load_msg(LoadMsg::TaskRepoDone { generation: 0 });
            state.apply_load_msg(LoadMsg::TaskRepoDone { generation: 0 });
            assert_eq!(
                state.task_load,
                LoadPhase::Loading {
                    done: 2,
                    total: Some(3)
                }
            );
        }

        #[test]
        fn tasks_complete_flips_to_idle() {
            let mut state = loading_state();
            state.apply_load_msg(LoadMsg::TasksComplete { generation: 0 });
            assert_eq!(state.task_load, LoadPhase::Idle);
            assert_eq!(
                state.repo_load,
                LoadPhase::Loading {
                    done: 0,
                    total: None
                }
            );
        }

        #[test]
        fn repo_rows_done_flips_repo_to_idle_only() {
            let mut state = loading_state();
            state.apply_load_msg(LoadMsg::RepoRowsDone { generation: 0 });
            assert_eq!(state.repo_load, LoadPhase::Idle);
            assert!(matches!(state.task_load, LoadPhase::Loading { .. }));
        }

        #[test]
        fn task_rows_for_repo_appends_and_keeps_sort() {
            let mut state = loading_state();
            let r1 = task_row_for_repo("github.com/b/app");
            let r2 = task_row_for_repo("github.com/a/app");
            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation: 0,
                repo: RepoKey::new("github.com/b/app"),
                rows: vec![r1],
            });
            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation: 0,
                repo: RepoKey::new("github.com/a/app"),
                rows: vec![r2],
            });
            assert_eq!(state.task_rows.len(), 2);
            assert_eq!(state.task_rows[0].repo.as_str(), "github.com/a/app");
            assert_eq!(state.task_rows[1].repo.as_str(), "github.com/b/app");
        }

        #[test]
        fn repo_row_insert_keeps_sort_and_bumps_done() {
            let mut state = loading_state();
            state.apply_load_msg(LoadMsg::ScanStarted {
                generation: 0,
                total: 2,
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation: 0,
                row: repo_row("github.com/z/zebra", 0, 0),
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation: 0,
                row: repo_row("github.com/a/alpha", 0, 0),
            });
            assert_eq!(state.repo_rows[0].repo.as_str(), "github.com/a/alpha");
            assert_eq!(state.repo_rows[1].repo.as_str(), "github.com/z/zebra");
            assert_eq!(
                state.repo_load,
                LoadPhase::Loading {
                    done: 2,
                    total: Some(2)
                }
            );
        }

        #[test]
        fn messages_with_stale_generation_are_ignored() {
            let mut state = loading_state();
            // Pretend a refresh just happened — next generation is 1.
            state.begin_load();
            assert_eq!(state.load_generation, 1);
            // Stale message from an old worker.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation: 0,
                row: repo_row("github.com/a/app", 1, 0),
            });
            assert!(state.repo_rows.is_empty(), "stale row must be dropped");
        }

        #[test]
        fn repo_error_surfaces_in_activity_and_counter() {
            let mut state = loading_state();
            state.apply_load_msg(LoadMsg::RepoError {
                generation: 0,
                repo: RepoKey::new("github.com/acme/broken"),
                err: "git failed".to_string(),
            });
            assert_eq!(state.skipped_repos_count, 1);
            assert!(
                state
                    .activity_lines
                    .iter()
                    .any(|line| line.contains("broken") && line.contains("git failed")),
                "activity lines should contain the repo error: {:?}",
                state.activity_lines
            );
        }

        #[test]
        fn selection_follows_by_identity_through_streaming_inserts() {
            // Seed the table with one row, select it, then stream in rows
            // from earlier-sorting repos. Selection must track the same
            // (repo, branch) instead of staying on index 0.
            let mut state =
                UiState::new(vec![task_row_for_repo("github.com/m/middle")], vec![], None);
            assert_eq!(state.task_selected, 0);
            state.insert_task_rows_sorted(vec![task_row_for_repo("github.com/a/alpha")]);
            let selected = state
                .selected_task_row()
                .expect("selection should exist after insert");
            assert_eq!(selected.repo.as_str(), "github.com/m/middle");
        }

        #[test]
        fn begin_load_bumps_generation_and_resets_rows() {
            let mut state = UiState::new(
                vec![task_row_for_repo("github.com/a/app")],
                vec![repo_row("github.com/a/app", 1, 0)],
                None,
            );
            state.skipped_repos_count = 5;
            let gen_before = state.load_generation;
            let new_gen = state.begin_load();
            assert_eq!(new_gen, gen_before.wrapping_add(1));
            assert!(state.task_rows.is_empty());
            assert!(state.repo_rows.is_empty());
            assert_eq!(state.skipped_repos_count, 0);
            assert!(matches!(state.task_load, LoadPhase::Loading { .. }));
            assert!(matches!(state.repo_load, LoadPhase::Loading { .. }));
        }
    }

    mod streaming_cursor {
        use super::*;
        use crate::{
            runtime::{BranchName, RepoKey, task_rows::TaskStatus},
            tools::opencode::status::OpenCodeState,
            ui::state::{LoadMsg, RepoRow},
        };

        fn task_row(repo: &str, branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                worktree_name: branch.to_string(),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
                opencode: OpenCodeState::None,
            }
        }

        fn detached_repo_row(repo: &str, open: usize) -> RepoRow {
            RepoRow {
                repo: RepoKey::new(repo),
                open_tasks: open,
                parked_tasks: 0,
                is_detached: false,
            }
        }

        /// Initial load: no prior identity; rows arrive in an arbitrary
        /// order. The cursor should stay pinned to index 0 of the sorted
        /// list rather than chasing the first-arriving row.
        #[test]
        fn initial_load_keeps_cursor_at_top_of_sorted_list() {
            let mut state = UiState::new_empty_loading(None);
            let generation = state.begin_load();

            // "c" arrives first, then "a", then "b". Sort is by open desc,
            // then repo asc. All have open=1, so sort is purely alphabetical.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/c", 1),
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/a", 1),
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/b", 1),
            });

            // Cursor is at index 0 — which after sort is "a", not "c".
            assert_eq!(state.repo_selected, 0);
            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/a"),
            );
        }

        /// Refresh: the user had a non-top repo selected. After rows stream
        /// back in arbitrary order, the cursor should land on the same repo.
        #[test]
        fn refresh_preserves_selected_repo_when_it_still_exists() {
            let mut state = UiState::new(
                vec![],
                vec![
                    detached_repo_row("github.com/org/a", 3),
                    detached_repo_row("github.com/org/b", 2),
                    detached_repo_row("github.com/org/c", 1),
                ],
                None,
            );
            state.repo_selected = 1; // "b"
            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/b"),
            );

            let generation = state.begin_load();

            // Stream in a different order.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/c", 1),
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/a", 3),
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/b", 2),
            });
            state.apply_load_msg(LoadMsg::RepoRowsDone { generation });

            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/b"),
            );
            assert!(state.pending_repo_selection.is_none());
        }

        /// Refresh: selected repo was deleted. Cursor should fall back to
        /// the top of the (now-smaller) list.
        #[test]
        fn refresh_falls_back_to_top_when_selected_repo_disappears() {
            let mut state = UiState::new(
                vec![],
                vec![
                    detached_repo_row("github.com/org/a", 3),
                    detached_repo_row("github.com/org/b", 2),
                    detached_repo_row("github.com/org/c", 1),
                ],
                None,
            );
            state.repo_selected = 1; // "b"

            let generation = state.begin_load();

            // "b" no longer exists.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/a", 3),
            });
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/c", 1),
            });
            state.apply_load_msg(LoadMsg::RepoRowsDone { generation });

            assert_eq!(state.repo_selected, 0);
            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/a"),
            );
            assert!(state.pending_repo_selection.is_none());
        }

        /// Same scenario as above, but for tasks.
        #[test]
        fn refresh_preserves_selected_task_when_it_still_exists() {
            let mut state = UiState::new(
                vec![
                    task_row("github.com/org/a", "feat-1"),
                    task_row("github.com/org/b", "feat-2"),
                    task_row("github.com/org/c", "feat-3"),
                ],
                vec![],
                None,
            );
            state.task_selected = 1; // org/b feat-2

            let generation = state.begin_load();

            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/c"),
                rows: vec![task_row("github.com/org/c", "feat-3")],
            });
            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/a"),
                rows: vec![task_row("github.com/org/a", "feat-1")],
            });
            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/b"),
                rows: vec![task_row("github.com/org/b", "feat-2")],
            });
            state.apply_load_msg(LoadMsg::TasksComplete { generation });

            let selected = state.selected_task_row().expect("selection exists");
            assert_eq!(selected.repo.as_str(), "github.com/org/b");
            assert_eq!(selected.branch.as_str(), "feat-2");
            assert!(state.pending_task_selection.is_none());
        }

        #[test]
        fn refresh_falls_back_to_top_when_selected_task_disappears() {
            let mut state = UiState::new(
                vec![
                    task_row("github.com/org/a", "feat-1"),
                    task_row("github.com/org/b", "feat-2"),
                ],
                vec![],
                None,
            );
            state.task_selected = 1;

            let generation = state.begin_load();

            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/a"),
                rows: vec![task_row("github.com/org/a", "feat-1")],
            });
            state.apply_load_msg(LoadMsg::TasksComplete { generation });

            assert_eq!(state.task_selected, 0);
            let selected = state.selected_task_row().expect("selection exists");
            assert_eq!(selected.repo.as_str(), "github.com/org/a");
            assert!(state.pending_task_selection.is_none());
        }

        /// The pending identity is kept for the whole load so every
        /// subsequent row re-anchors on it. `RepoRowsDone` clears it.
        #[test]
        fn pending_repo_identity_is_cleared_on_repo_rows_done() {
            let mut state = UiState::new(
                vec![],
                vec![
                    detached_repo_row("github.com/org/a", 3),
                    detached_repo_row("github.com/org/b", 2),
                ],
                None,
            );
            state.repo_selected = 1; // "b"

            let generation = state.begin_load();
            assert!(state.pending_repo_selection.is_some());

            // Anchor row arrives. Pending stays set so later inserts
            // keep re-anchoring to it.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/b", 2),
            });
            assert!(state.pending_repo_selection.is_some());

            state.apply_load_msg(LoadMsg::RepoRowsDone { generation });
            assert!(state.pending_repo_selection.is_none());
        }

        /// Regression: once the pending repo arrives mid-stream, later
        /// `RepoRow` inserts must not snap the cursor back to 0 while the
        /// load is still in progress.
        #[test]
        fn later_repo_rows_do_not_reset_cursor_after_pending_reattaches() {
            let mut state = UiState::new(
                vec![],
                vec![
                    detached_repo_row("github.com/org/a", 1),
                    detached_repo_row("github.com/org/b", 1),
                    detached_repo_row("github.com/org/c", 1),
                    detached_repo_row("github.com/org/d", 1),
                ],
                None,
            );
            state.repo_selected = 1; // "b"

            let generation = state.begin_load();

            // "c" arrives before the pending row.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/c", 1),
            });
            // Pending row "b" arrives second and must anchor the cursor.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/b", 1),
            });
            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/b"),
                "cursor should be on pending row after it arrives",
            );
            // Pending stays set so later rows keep re-anchoring.
            assert!(state.pending_repo_selection.is_some());

            // "a" then "d" arrive later in the same load; cursor must
            // stay on "b" instead of snapping to index 0.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/a", 1),
            });
            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/b"),
                "cursor must stay on pending row after another row arrives",
            );

            state.apply_load_msg(LoadMsg::RepoRow {
                generation,
                row: detached_repo_row("github.com/org/d", 1),
            });
            state.apply_load_msg(LoadMsg::RepoRowsDone { generation });

            assert_eq!(
                state.selected_repo_row().map(|r| r.repo.as_str()),
                Some("github.com/org/b"),
                "cursor must remain on pending row at end of load",
            );
        }

        /// Regression mirror for tasks: once the pending task arrives
        /// mid-stream, subsequent `TaskRowsForRepo` inserts must not
        /// reset the cursor.
        #[test]
        fn later_task_rows_do_not_reset_cursor_after_pending_reattaches() {
            let mut state = UiState::new(
                vec![
                    task_row("github.com/org/a", "feat-1"),
                    task_row("github.com/org/b", "feat-2"),
                    task_row("github.com/org/c", "feat-3"),
                    task_row("github.com/org/d", "feat-4"),
                ],
                vec![],
                None,
            );
            state.task_selected = 1; // "b/feat-2"

            let generation = state.begin_load();

            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/c"),
                rows: vec![task_row("github.com/org/c", "feat-3")],
            });
            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/b"),
                rows: vec![task_row("github.com/org/b", "feat-2")],
            });
            let selected = state.selected_task_row().expect("selection exists");
            assert_eq!(selected.repo.as_str(), "github.com/org/b");
            // Pending stays set so later rows keep re-anchoring.
            assert!(state.pending_task_selection.is_some());

            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/a"),
                rows: vec![task_row("github.com/org/a", "feat-1")],
            });
            let selected = state.selected_task_row().expect("selection exists");
            assert_eq!(
                selected.repo.as_str(),
                "github.com/org/b",
                "cursor must stay on pending task after another row arrives",
            );

            state.apply_load_msg(LoadMsg::TaskRowsForRepo {
                generation,
                repo: RepoKey::new("github.com/org/d"),
                rows: vec![task_row("github.com/org/d", "feat-4")],
            });
            state.apply_load_msg(LoadMsg::TasksComplete { generation });

            let selected = state.selected_task_row().expect("selection exists");
            assert_eq!(
                selected.repo.as_str(),
                "github.com/org/b",
                "cursor must remain on pending task at end of load",
            );
        }
    }

    mod opencode_tick {
        use super::*;
        use crate::{
            runtime::{BranchName, RepoKey, task_rows::TaskStatus},
            tools::{git::worktrees::WorktreeDiff, opencode::status::OpenCodeState},
            ui::state::{LoadMsg, TaskCardDetails},
        };

        fn row(repo: &str, branch: &str, path: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                worktree_name: branch.to_string(),
                path: PathBuf::from(path),
                opencode: OpenCodeState::None,
            }
        }

        #[test]
        fn load_msg_generation_returns_none_for_tick() {
            // Ticks are path-keyed and safe to apply regardless of
            // generation, so they report no generation at all.
            let msg = LoadMsg::OpenCodeTick { states: vec![] };
            assert_eq!(msg.generation(), None);

            let msg = LoadMsg::TaskCardDetailsTick { details: vec![] };
            assert_eq!(msg.generation(), None);
        }

        #[test]
        fn load_msg_generation_returns_some_for_regular_messages() {
            assert_eq!(
                LoadMsg::TaskRepoDone { generation: 7 }.generation(),
                Some(7),
            );
        }

        #[test]
        fn apply_opencode_states_is_noop_on_empty_states() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            state.task_rows[0].opencode = OpenCodeState::Busy;
            state.apply_opencode_states(&[]);
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Busy);
        }

        #[test]
        fn apply_opencode_states_is_noop_on_empty_task_rows() {
            let mut state = UiState::new(vec![], vec![], None);
            state.apply_opencode_states(&[(PathBuf::from("/tmp/a"), OpenCodeState::Idle)]);
            assert!(state.task_rows.is_empty());
        }

        #[test]
        fn apply_opencode_states_updates_matching_row_by_path() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            state.apply_opencode_states(&[(PathBuf::from("/tmp/a/main"), OpenCodeState::Idle)]);
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Idle);
        }

        #[test]
        fn apply_opencode_states_ignores_unknown_paths() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            state.task_rows[0].opencode = OpenCodeState::Gone;
            state.apply_opencode_states(&[(
                PathBuf::from("/nonexistent/path"),
                OpenCodeState::Hung,
            )]);
            // The known row is untouched.
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Gone);
        }

        #[test]
        fn apply_opencode_states_updates_multiple_rows_in_one_call() {
            let mut state = UiState::new(
                vec![
                    row("github.com/a/app", "main", "/tmp/a/main"),
                    row("github.com/b/app", "main", "/tmp/b/main"),
                    row("github.com/c/app", "main", "/tmp/c/main"),
                ],
                vec![],
                None,
            );
            state.apply_opencode_states(&[
                (PathBuf::from("/tmp/a/main"), OpenCodeState::Busy),
                (PathBuf::from("/tmp/c/main"), OpenCodeState::Hung),
            ]);
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Busy);
            // Unmatched row stays at its default.
            assert_eq!(state.task_rows[1].opencode, OpenCodeState::None);
            assert_eq!(state.task_rows[2].opencode, OpenCodeState::Hung);
        }

        #[test]
        fn later_tick_overwrites_earlier_state_for_same_path() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            state.apply_opencode_states(&[(PathBuf::from("/tmp/a/main"), OpenCodeState::Busy)]);
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Busy);
            state.apply_opencode_states(&[(PathBuf::from("/tmp/a/main"), OpenCodeState::Idle)]);
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Idle);
        }

        #[test]
        fn apply_load_msg_dispatches_opencode_tick_to_apply_opencode_states() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            state.apply_load_msg(LoadMsg::OpenCodeTick {
                states: vec![(PathBuf::from("/tmp/a/main"), OpenCodeState::Hung)],
            });
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Hung);
        }

        #[test]
        fn apply_task_card_details_updates_matching_row_by_path() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            state.apply_task_card_details(&[(
                PathBuf::from("/tmp/a/main"),
                TaskCardDetails {
                    diff: WorktreeDiff {
                        added_lines: 12,
                        deleted_lines: 3,
                        changed_files: 2,
                    },
                    session_title: Some("Ship cards".to_string()),
                    last_activity_ms: Some(1_234),
                },
            )]);

            let details = state.task_card_details_for(&state.task_rows[0]);
            assert_eq!(details.diff.added_lines, 12);
            assert_eq!(details.diff.deleted_lines, 3);
            assert_eq!(details.session_title.as_deref(), Some("Ship cards"));
            assert_eq!(details.last_activity_ms, Some(1_234));
        }

        #[test]
        fn apply_task_card_details_preserves_filter_state_when_filter_empty() {
            let mut state = UiState::new(
                vec![
                    row("github.com/a/app", "main", "/tmp/a/main"),
                    row("github.com/b/app", "main", "/tmp/b/main"),
                ],
                vec![],
                None,
            );
            state.task_filtered_indices = vec![1];
            state.task_selected = 1;
            let path = state.task_rows[0].path.clone();

            state.apply_task_card_details(&[(
                path,
                TaskCardDetails {
                    session_title: Some("Ship cards".to_string()),
                    ..TaskCardDetails::default()
                },
            )]);

            assert_eq!(state.task_filtered_indices, vec![1]);
            assert_eq!(state.task_selected, 1);
        }

        /// `OpenCodeTick` is intentionally exempt from the generation
        /// filter: the path-keyed payload can only update rows that
        /// still exist, so applying it after a refresh cannot corrupt
        /// state but dropping it would cause a ~600ms staleness window
        /// after every refresh. This test pins that exemption.
        #[test]
        fn opencode_tick_applies_after_new_generation_started() {
            let mut state = UiState::new(
                vec![row("github.com/a/app", "main", "/tmp/a/main")],
                vec![],
                None,
            );
            // Current generation becomes 1 after begin_load.
            state.begin_load();
            // Re-seed the row because begin_load cleared task_rows.
            state.task_rows = vec![row("github.com/a/app", "main", "/tmp/a/main")];
            state.apply_task_filter();
            assert_eq!(state.load_generation, 1);

            // A tick spawned by the previous generation is still applied
            // because it carries no generation at all.
            state.apply_load_msg(LoadMsg::OpenCodeTick {
                states: vec![(PathBuf::from("/tmp/a/main"), OpenCodeState::Idle)],
            });
            assert_eq!(state.task_rows[0].opencode, OpenCodeState::Idle);
        }

        /// Non-`OpenCodeTick` messages are still gated on generation.
        #[test]
        fn stale_non_opencode_messages_are_still_dropped() {
            let mut state = UiState::new_empty_loading(None);
            state.begin_load();
            assert_eq!(state.load_generation, 1);
            // Stale RepoRow with generation 0 must be ignored.
            state.apply_load_msg(LoadMsg::RepoRow {
                generation: 0,
                row: repo_row("github.com/a/app", 1, 0),
            });
            assert!(state.repo_rows.is_empty());
        }
    }
}
