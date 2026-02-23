use crate::runtime::task_rows::TaskRow;

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
    pub(super) repo: String,
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
    pub(super) repo_selected: usize,
    pub(super) task_filter: String,
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
            repo_selected: 0,
            task_filter: String::new(),
            task_repo_scope,
            create_branch: String::new(),
            clone_input: String::new(),
            view: ViewMode::Tasks,
            mode: InputMode::Normal,
            message: "Ready".to_string(),
            show_help: false,
        };
        state.apply_task_filter();
        state
    }

    pub(super) fn apply_task_filter(&mut self) {
        let needle = self.task_filter.to_lowercase();
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
        self.repo_rows.get(self.repo_selected)
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
                if self.repo_rows.is_empty() {
                    return;
                }
                self.repo_selected =
                    (self.repo_selected + 1).min(self.repo_rows.len().saturating_sub(1));
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
        if self.repo_selected >= self.repo_rows.len() {
            self.repo_selected = self.repo_rows.len().saturating_sub(1);
        }
    }

    pub(super) fn switch_view(&mut self) {
        self.mode = InputMode::Normal;
        self.view = match self.view {
            ViewMode::Tasks => ViewMode::Repos,
            ViewMode::Repos => ViewMode::Tasks,
        };
    }

    pub(super) fn select_repo_for_tasks(&mut self, repo: String) {
        self.task_repo_scope = Some(repo);
        self.view = ViewMode::Tasks;
        self.mode = InputMode::Normal;
    }
}
