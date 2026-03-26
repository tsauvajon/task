use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout as UiLayout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use self::{
    actions_panel::actions_for_mode,
    activity_panel::render_activity,
    help_overlay::render_help,
    status_bar::render_status_bar,
    tables::{render_repos, render_tasks},
};
use crate::ui::{
    state::{UiState, ViewMode},
    theme::Theme,
};

mod actions_panel;
mod activity_panel;
mod help_overlay;
mod scrollbar;
mod status_bar;
mod tables;

pub(super) fn render(frame: &mut Frame, state: &mut UiState) {
    let theme = Theme::dark();

    let outer = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());

    render_body(frame, outer[0], state, &theme);
    render_status_bar(frame, outer[1], state, &theme);

    if state.show_help {
        render_help(frame, state, &theme);
    } else {
        state.help_area = None;
    }
}

fn render_body(frame: &mut Frame, area: ratatui::layout::Rect, state: &mut UiState, theme: &Theme) {
    let chunks = UiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    match state.view {
        ViewMode::Tasks => render_tasks(frame, chunks[0], state, theme),
        ViewMode::Repos => render_repos(frame, chunks[0], state, theme),
    }

    let actions = actions_for_mode(state, theme);

    let details_chunks = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(chunks[1]);

    let mode_label = match state.mode {
        crate::ui::state::InputMode::Normal => match state.view {
            ViewMode::Tasks => "List tasks",
            ViewMode::Repos => "List repos",
        },
        crate::ui::state::InputMode::Filter => "Filter",
        crate::ui::state::InputMode::CreateTask => "Create task",
        crate::ui::state::InputMode::CloneRepo => "Clone repo",
    };

    // Actions panel — side panel shade with padding.
    let mut action_lines = vec![Line::from(Span::styled(
        mode_label,
        theme.mode_style(state.mode),
    ))];
    action_lines.push(Line::from(""));
    action_lines.extend(actions);

    let details_panel = Paragraph::new(action_lines).block(
        Block::default()
            .padding(Padding::new(2, 1, 1, 0))
            .style(Style::default().bg(theme.panel_side)),
    );
    frame.render_widget(details_panel, details_chunks[0]);
    render_activity(frame, details_chunks[1], state, theme);
}
