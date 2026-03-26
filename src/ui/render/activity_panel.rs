use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use crate::ui::{state::UiState, theme::Theme};

pub(super) fn render_activity(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let mut lines = vec![Line::from(Span::styled("Activity", theme.title_style()))];
    lines.push(Line::from(""));

    if state.activity_lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No recent activity",
            theme.muted_style(),
        )));
    } else {
        lines.extend(
            state
                .activity_lines
                .iter()
                .map(|line| Line::from(Span::styled(line.clone(), theme.muted_style()))),
        );
    }

    let panel = Paragraph::new(lines).block(
        Block::default()
            .padding(Padding::new(2, 1, 1, 0))
            .style(Style::default().bg(theme.panel_side)),
    );
    frame.render_widget(panel, area);
}
