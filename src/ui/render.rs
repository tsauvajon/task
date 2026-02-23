use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout as UiLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState};

use crate::runtime::TaskStatus;

use super::state::{InputMode, UiState};

pub(super) fn render(frame: &mut Frame, state: &UiState) {
    let outer = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10)])
        .split(frame.area());

    render_body(frame, outer[0], state);

    if state.show_help {
        render_help(frame);
    }
}

fn render_body(frame: &mut Frame, area: Rect, state: &UiState) {
    let open_count = state
        .rows
        .iter()
        .filter(|row| row.status == TaskStatus::Open)
        .count();
    let total_count = state.rows.len();

    let chunks = UiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let header = Row::new(vec!["STATUS", "REPO", "BRANCH", "PATH"]).style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows = state.filtered_indices.iter().filter_map(|index| {
        let row = state.rows.get(*index)?;
        let status_style = match row.status {
            TaskStatus::Open => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            TaskStatus::Parked => Style::default().fg(Color::Yellow),
        };
        Some(Row::new(vec![
            Cell::from(status_label(row.status)).style(status_style),
            Cell::from(row.repo.clone()),
            Cell::from(row.branch.clone()),
            Cell::from(row.path.to_string_lossy().to_string()),
        ]))
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(28),
            Constraint::Length(24),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title("Tasks")
            .title(
                Line::from(Span::styled(
                    format!("{} open / {} total", open_count, total_count),
                    Style::default().fg(Color::Gray),
                ))
                .right_aligned(),
            )
            .borders(Borders::ALL),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::Rgb(25, 25, 40))
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ");

    let mut table_state = TableState::default();
    if !state.filtered_indices.is_empty() {
        table_state.select(Some(state.selected));
    }
    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    let mode = match state.mode {
        InputMode::Normal => "NORMAL",
        InputMode::Filter => "FILTER",
        InputMode::Create => "CREATE",
    };
    let mode_style = match state.mode {
        InputMode::Normal => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        InputMode::Filter => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        InputMode::Create => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    };

    let actions = vec![
        Line::from(match state.mode {
            InputMode::Normal => "Enter  open selected task",
            InputMode::Filter => "Type   append filter text",
            InputMode::Create => "Type   set new branch name",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "p      park selected task",
            InputMode::Filter | InputMode::Create => "Backsp delete character",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "f      finish selected task",
            InputMode::Filter => "Ctrl-U clear filter",
            InputMode::Create => "Enter  create and open task",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "c      create new task",
            InputMode::Filter => "Enter  apply and return",
            InputMode::Create => "Esc    return to normal",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "r      refresh task list",
            InputMode::Filter => "Esc    return to normal",
            InputMode::Create => "?      toggle help",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "/      enter filter mode",
            InputMode::Filter => "?      toggle help",
            InputMode::Create => "q      quit",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "?      toggle help",
            InputMode::Filter | InputMode::Create => "",
        }),
        Line::from(match state.mode {
            InputMode::Normal => "q      quit",
            InputMode::Filter | InputMode::Create => "",
        }),
    ];

    let details_panel = Paragraph::new(actions).block(
        Block::default()
            .title(Line::from(Span::styled(mode, mode_style)))
            .borders(Borders::ALL),
    );
    frame.render_widget(details_panel, chunks[1]);
}

fn render_help(frame: &mut Frame) {
    let popup = centered_rect(85, 85, frame.area());
    let lines = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Normal mode:"),
        Line::from("↑/k       move up"),
        Line::from("↓/j       move down"),
        Line::from("Enter     open selected task"),
        Line::from("p         park selected task"),
        Line::from("f         finish selected task"),
        Line::from("c         create new task"),
        Line::from("r         refresh tasks"),
        Line::from("/         enter filter mode"),
        Line::from("?         toggle help"),
        Line::from("q/Ctrl-C  quit"),
        Line::from(""),
        Line::from("Filter mode:"),
        Line::from("Type      append filter text"),
        Line::from("Backspace delete character"),
        Line::from("Ctrl-U    clear filter"),
        Line::from("Enter     apply and return to normal"),
        Line::from("Esc       return to normal"),
        Line::from(""),
        Line::from("Create mode:"),
        Line::from("Type      set new branch name"),
        Line::from("Backspace delete character"),
        Line::from("Enter     create and open new task"),
        Line::from("Esc       return to normal"),
    ];

    let help = Paragraph::new(lines)
        .block(Block::default().title("Help").borders(Borders::ALL))
        .style(Style::default().fg(Color::White).bg(Color::Black));

    frame.render_widget(Clear, popup);
    frame.render_widget(help, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, rect: Rect) -> Rect {
    let vertical = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(rect);

    UiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Open => "open",
        TaskStatus::Parked => "parked",
    }
}
