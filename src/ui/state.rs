use ratatui::layout::Rect;

use crate::runtime::{RepoKey, task_rows::TaskRow};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepoRow {
    pub(super) repo: RepoKey,
    pub(super) open_tasks: usize,
    pub(super) parked_tasks: usize,
    pub(super) is_detached: bool,
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
}

impl UiState {
    pub(super) fn new(
        task_rows: Vec<TaskRow>,
        repo_rows: Vec<RepoRow>,
        task_repo_scope: Option<String>,
    ) -> Self {
        let mut state = Self {
            task_rows,
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
        };
        state.apply_filters();
        state
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

                let haystack = format!(
                    "{} {} {}",
                    row.repo.to_lowercase(),
                    row.branch.to_lowercase(),
                    row.path.to_string_lossy().to_lowercase(),
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

    pub(super) fn set_task_rows(&mut self, rows: Vec<TaskRow>) {
        self.task_rows = rows;
        self.apply_task_filter();
    }

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

    use super::{RepoRow, UiState};
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
        use crate::runtime::{BranchName, RepoKey};
        TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new("github.com/acme/app"),
            branch: BranchName::new("main"),
            path: PathBuf::from("/tmp/dev/wt/github.com/acme/app/main"),
        }
    }

    fn task_row_for_repo(repo: &str) -> TaskRow {
        use crate::runtime::{BranchName, RepoKey};
        TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new(repo),
            branch: BranchName::new("main"),
            path: PathBuf::from(format!("/tmp/dev/wt/{repo}/main")),
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
        use crate::runtime::{BranchName, RepoKey, task_rows::TaskStatus};

        fn task_row(repo: &str, branch: &str) -> crate::runtime::task_rows::TaskRow {
            crate::runtime::task_rows::TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
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

            use crate::runtime::{BranchName, RepoKey, task_rows::TaskStatus};

            let mut state = UiState::new(
                vec![
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("feature-x"),
                        path: PathBuf::from("/tmp/a"),
                    },
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("main"),
                        path: PathBuf::from("/tmp/b"),
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

            use crate::runtime::{BranchName, RepoKey, task_rows::TaskStatus};

            let mut state = UiState::new(
                vec![
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("main"),
                        path: PathBuf::from("/projects/special/path"),
                    },
                    crate::runtime::task_rows::TaskRow {
                        status: TaskStatus::Open,
                        repo: RepoKey::new("github.com/acme/app"),
                        branch: BranchName::new("main"),
                        path: PathBuf::from("/other/path"),
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
}
