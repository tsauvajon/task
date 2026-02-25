use crate::{runtime::task_rows::TaskRow, types::RepoKey};

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
    pub(super) view: ViewMode,
    pub(super) mode: InputMode,
    pub(super) message: String,
    pub(super) show_help: bool,
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
            view: ViewMode::Tasks,
            mode: InputMode::Normal,
            message: "Ready".to_string(),
            show_help: false,
        };
        state.apply_filters();
        state
    }

    pub(super) fn apply_filters(&mut self) {
        self.apply_task_filter();
        self.apply_repo_filter();
    }

    pub(super) fn apply_task_filter(&mut self) {
        let needle = self.filter_text.to_lowercase();
        self.task_filtered_indices = self
            .task_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                if needle.is_empty() {
                    return true;
                }

                row.repo.to_lowercase().contains(&needle)
                    || row.branch.to_lowercase().contains(&needle)
                    || row.path.to_string_lossy().to_lowercase().contains(&needle)
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
                self.task_selected = (self.task_selected + 1)
                    .min(self.task_filtered_indices.len().saturating_sub(1));
            }
            ViewMode::Repos => {
                if self.repo_filtered_indices.is_empty() {
                    return;
                }
                self.repo_selected = (self.repo_selected + 1)
                    .min(self.repo_filtered_indices.len().saturating_sub(1));
            }
        }
    }

    pub(super) fn move_prev(&mut self) {
        match self.view {
            ViewMode::Tasks => {
                self.task_selected = self.task_selected.saturating_sub(1);
            }
            ViewMode::Repos => {
                self.repo_selected = self.repo_selected.saturating_sub(1);
            }
        }
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
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{RepoRow, UiState};
    use crate::runtime::task_rows::{TaskRow, TaskStatus};

    #[test]
    fn repo_filter_matches_repo_name_only() {
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
    fn repo_selection_clamps_when_filter_reduces_results() {
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
    fn repo_filter_matches_all_space_separated_tokens() {
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
    fn repo_filter_matches_host_fragment() {
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

    fn repo_row(repo: &str, open_tasks: usize, parked_tasks: usize) -> RepoRow {
        use crate::types::RepoKey;
        RepoRow {
            repo: RepoKey::new(repo),
            open_tasks,
            parked_tasks,
        }
    }

    fn sample_task_row() -> TaskRow {
        use crate::types::{BranchName, RepoKey};
        TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new("github.com/acme/app"),
            branch: BranchName::new("main"),
            path: PathBuf::from("/tmp/dev/wt/github.com/acme/app/main"),
        }
    }

    #[test]
    fn filter_text_is_shared_between_task_and_repo_views() {
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

    fn task_row_for_repo(repo: &str) -> TaskRow {
        use crate::types::{BranchName, RepoKey};
        TaskRow {
            status: TaskStatus::Open,
            repo: RepoKey::new(repo),
            branch: BranchName::new("main"),
            path: PathBuf::from(format!("/tmp/dev/wt/{repo}/main")),
        }
    }
}
