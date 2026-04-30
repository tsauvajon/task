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
    state::{SIDEBAR_WIDTH, UiState, ViewMode},
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

    let [body_area, status_area] = outer.as_ref() else {
        return;
    };

    render_body(frame, *body_area, state, &theme);
    render_status_bar(frame, *status_area, state, &theme);

    if state.show_help {
        render_help(frame, state, &theme);
    } else {
        state.help_area = None;
    }
}

fn render_body(frame: &mut Frame, area: ratatui::layout::Rect, state: &mut UiState, theme: &Theme) {
    // Cache the body width so the `ToggleSidebar` intent handler can
    // pick the correct direction without re-querying the terminal.
    state.last_frame_width = area.width;

    if !state.sidebar_visible(area.width) {
        match state.view {
            ViewMode::Tasks => render_tasks(frame, area, state, theme),
            ViewMode::Repos => render_repos(frame, area, state, theme),
        }
        return;
    }

    // Fixed-width sidebar: content flexes, sidebar stays at
    // `SIDEBAR_WIDTH` cells. `Min` + `Length` in ratatui gives the
    // `Length` segment exactly that many cells and hands the remainder
    // to the `Min` segment (which shrinks no further than its floor of
    // 0). Matches OpenCode's fixed 42-col sidebar model.
    let chunks = UiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(SIDEBAR_WIDTH)])
        .split(area);
    let [main_area, side_area] = chunks.as_ref() else {
        return;
    };

    match state.view {
        ViewMode::Tasks => render_tasks(frame, *main_area, state, theme),
        ViewMode::Repos => render_repos(frame, *main_area, state, theme),
    }

    let actions = actions_for_mode(state, theme);

    let details_chunks = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(*side_area);
    let [actions_area, activity_area] = details_chunks.as_ref() else {
        return;
    };

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
    frame.render_widget(details_panel, *actions_area);
    render_activity(frame, *activity_area, state, theme);
}
