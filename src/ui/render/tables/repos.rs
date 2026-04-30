use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Cell, Padding, Row, Table, TableState},
};

use super::super::scrollbar::render_scrollbar;
use crate::ui::{state::UiState, theme::Theme};

pub(in crate::ui::render) fn render_repos(
    frame: &mut Frame,
    area: Rect,
    state: &mut UiState,
    theme: &Theme,
) {
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
