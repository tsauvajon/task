use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Cell, Padding, Row, Table, TableState},
};

use super::scrollbar::render_scrollbar;
use crate::{
    runtime::task_rows::TaskStatus,
    ui::{state::UiState, theme::Theme},
};

// ── Tasks table ──────────────────────────────────────────────────────────────

pub(super) fn render_tasks(frame: &mut Frame, area: Rect, state: &mut UiState, theme: &Theme) {
    let scoped = state.task_repo_scope.is_some();

    let header = if scoped {
        Row::new(vec!["Status", "Branch", "Path"])
    } else {
        Row::new(vec!["Status", "Repo", "Branch", "Path"])
    }
    .style(theme.header_style())
    .bottom_margin(0);

    let rows: Vec<Row> = state
        .task_filtered_indices
        .iter()
        .filter_map(|index| {
            let row = state.task_rows.get(*index)?;
            let status_style = match row.status {
                TaskStatus::Open => Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
                TaskStatus::Parked => Style::default().fg(theme.warning),
            };
            let repo_style = theme.text_style();
            let branch_style = Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD);
            let path_style = theme.muted_style();

            if scoped {
                Some(Row::new(vec![
                    Cell::from(status_label(row.status)).style(status_style),
                    Cell::from(row.branch.to_string()).style(branch_style),
                    Cell::from(row.path.to_string_lossy().to_string()).style(path_style),
                ]))
            } else {
                Some(Row::new(vec![
                    Cell::from(status_label(row.status)).style(status_style),
                    Cell::from(row.repo.to_string()).style(repo_style),
                    Cell::from(row.branch.to_string()).style(branch_style),
                    Cell::from(row.path.to_string_lossy().to_string()).style(path_style),
                ]))
            }
        })
        .collect();

    let row_count = rows.len();

    let branch_width = state
        .task_filtered_indices
        .iter()
        .filter_map(|i| state.task_rows.get(*i))
        .map(|row| row.branch.len())
        .max()
        .unwrap_or(0)
        .max("Branch".len()) as u16;

    let widths: Vec<Constraint> = if scoped {
        vec![
            Constraint::Length(8),
            Constraint::Length(branch_width),
            Constraint::Fill(1),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Fill(1),
            Constraint::Length(branch_width),
            Constraint::Fill(2),
        ]
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .padding(Padding::new(1, 1, 0, 0))
                .style(Style::default().bg(theme.panel_main)),
        )
        .row_highlight_style(theme.row_highlight_style())
        .highlight_symbol("▶ ");

    let mut table_state = TableState::default();
    if !state.task_filtered_indices.is_empty() {
        table_state.select(Some(state.task_selected));
    }
    frame.render_stateful_widget(table, area, &mut table_state);

    // Feed the actual visible row count back to state so that PgUp/PgDn
    // can jump by a screen-full on the next key event.
    let visible_rows = area.height.saturating_sub(2) as usize;
    state.set_visible_rows(visible_rows);

    // Scrollbar — only when content overflows the visible area.
    if row_count > visible_rows {
        let sb_area = Rect {
            x: area.x,
            y: area.y + 1, // below header
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        render_scrollbar(
            frame,
            sb_area,
            state.task_selected,
            row_count,
            visible_rows,
            theme,
        );
    }
}

// ── Repos table ──────────────────────────────────────────────────────────────

pub(super) fn render_repos(frame: &mut Frame, area: Rect, state: &mut UiState, theme: &Theme) {
    let header = Row::new(vec!["Repo", "Open", "Parked", "Detached"])
        .style(theme.header_style())
        .bottom_margin(0);

    let rows: Vec<Row> = state
        .repo_filtered_indices
        .iter()
        .filter_map(|index| {
            let row = state.repo_rows.get(*index)?;
            let det_cell = if row.is_detached {
                Cell::from("✓").style(Style::default().fg(theme.info).add_modifier(Modifier::BOLD))
            } else {
                Cell::from("·").style(theme.muted_style())
            };
            Some(Row::new(vec![
                Cell::from(row.repo.to_string()).style(theme.text_style()),
                Cell::from(row.open_tasks.to_string()).style(
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(row.parked_tasks.to_string()).style(Style::default().fg(theme.warning)),
                det_cell,
            ]))
        })
        .collect();

    let row_count = rows.len();

    let table = Table::new(
        rows,
        [
            Constraint::Fill(1),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .padding(Padding::new(1, 1, 0, 0))
            .style(Style::default().bg(theme.panel_main)),
    )
    .row_highlight_style(theme.row_highlight_style())
    .highlight_symbol("▶ ");

    let mut table_state = TableState::default();
    if !state.repo_filtered_indices.is_empty() {
        table_state.select(Some(state.repo_selected));
    }
    frame.render_stateful_widget(table, area, &mut table_state);

    // Feed the actual visible row count back to state so that PgUp/PgDn
    // can jump by a screen-full on the next key event.
    let visible_rows = area.height.saturating_sub(2) as usize;
    state.set_visible_rows(visible_rows);

    // Scrollbar — only when content overflows the visible area.
    if row_count > visible_rows {
        let sb_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        render_scrollbar(
            frame,
            sb_area,
            state.repo_selected,
            row_count,
            visible_rows,
            theme,
        );
    }
}

// ── Utilities ────────────────────────────────────────────────────────────────

fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Open => "open",
        TaskStatus::Parked => "parked",
    }
}

#[cfg(test)]
mod tests {
    use super::status_label;
    use crate::runtime::task_rows::TaskStatus;

    mod scoped_tasks_view {
        use std::path::PathBuf;

        use crate::{
            runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            },
            ui::state::UiState,
        };

        fn task_row(repo: &str, branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
            }
        }

        #[test]
        fn scoped_state_has_repo_in_task_repo_scope() {
            let row = task_row("github.com/org/repo", "my-branch");
            let state = UiState::new(vec![row], vec![], Some("github.com/org/repo".to_string()));
            assert_eq!(
                state.task_repo_scope.as_deref(),
                Some("github.com/org/repo")
            );
        }

        #[test]
        fn unscoped_state_has_no_task_repo_scope() {
            let row = task_row("github.com/org/repo", "my-branch");
            let state = UiState::new(vec![row], vec![], None);
            assert!(state.task_repo_scope.is_none());
        }
    }

    mod status_label_tests {
        use super::*;

        #[test]
        fn open_shows_open() {
            assert_eq!(status_label(TaskStatus::Open), "open");
        }

        #[test]
        fn parked_shows_parked() {
            assert_eq!(status_label(TaskStatus::Parked), "parked");
        }
    }
}
