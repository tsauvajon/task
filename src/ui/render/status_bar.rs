use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    runtime::task_rows::TaskStatus,
    ui::{
        state::{UiState, ViewMode},
        theme::Theme,
    },
};

pub(super) fn render_status_bar(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let mut spans: Vec<Span> = Vec::new();

    // View context: view label, optional scope, counts.
    spans.push(Span::raw(" "));

    match state.view {
        ViewMode::Tasks => {
            let open_count = state
                .task_rows
                .iter()
                .filter(|row| row.status == TaskStatus::Open)
                .count();
            let total_count = state.task_rows.len();

            spans.push(Span::styled("tasks", theme.text_style()));
            if let Some(scope) = &state.task_repo_scope {
                spans.push(Span::styled(format!(" ({scope})"), theme.muted_style()));
            }
            spans.push(Span::styled(
                format!("  {open_count} open / {total_count} total"),
                theme.title_counter_style(),
            ));
        }
        ViewMode::Repos => {
            let shown = state.repo_filtered_indices.len();
            let total = state.repo_rows.len();

            spans.push(Span::styled("repos", theme.text_style()));
            spans.push(Span::styled(
                format!("  {shown} shown / {total} total"),
                theme.title_counter_style(),
            ));
        }
    }

    // Input text for interactive modes — shown after view context.
    // Filter, CreateTask, and CloneRepo inputs are rendered in the right panel;
    // the status bar stays clean in all input modes.

    // Right-aligned message.
    let msg = &state.message;
    if !msg.is_empty() {
        let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
        let pad = (area.width as usize).saturating_sub(left_len + msg.len() + 1);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        spans.push(Span::styled(msg, theme.muted_style()));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.panel_bar));
    frame.render_widget(bar, area);
}
