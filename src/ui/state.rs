use crate::runtime::TaskRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputMode {
    Normal,
    Filter,
    Create,
}

#[derive(Debug, Clone)]
pub(super) enum UiAction {
    Quit,
    Open(TaskRow),
    Create { repo: String, branch: String },
}

#[derive(Debug, Clone)]
pub(super) struct UiState {
    pub(super) rows: Vec<TaskRow>,
    pub(super) filtered_indices: Vec<usize>,
    pub(super) selected: usize,
    pub(super) filter: String,
    pub(super) create_branch: String,
    pub(super) mode: InputMode,
    pub(super) message: String,
    pub(super) show_help: bool,
}

impl UiState {
    pub(super) fn new(rows: Vec<TaskRow>) -> Self {
        let mut state = Self {
            rows,
            filtered_indices: Vec::new(),
            selected: 0,
            filter: String::new(),
            create_branch: String::new(),
            mode: InputMode::Normal,
            message: "Ready".to_string(),
            show_help: false,
        };
        state.apply_filter();
        state
    }

    pub(super) fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.filtered_indices = self
            .rows
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

        if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len().saturating_sub(1);
        }
    }

    pub(super) fn selected_row(&self) -> Option<&TaskRow> {
        let index = *self.filtered_indices.get(self.selected)?;
        self.rows.get(index)
    }

    pub(super) fn move_next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.filtered_indices.len().saturating_sub(1));
    }

    pub(super) fn move_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(super) fn set_rows(&mut self, rows: Vec<TaskRow>) {
        self.rows = rows;
        self.apply_filter();
    }
}
