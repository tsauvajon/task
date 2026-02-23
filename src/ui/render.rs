use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout as UiLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState};

use crate::tools::tmux;

use super::state::{InputMode, UiState};

pub(super) fn render(frame: &mut Frame, state: &UiState) {
    let outer = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(19),
        ])
        .split(frame.area());

    render_header(frame, outer[0], state);
    render_body(frame, outer[1], state);
    render_footer(frame, outer[2], state);

    if state.show_help {
        render_help(frame);
    }
}

fn render_header(frame: &mut Frame, area: Rect, state: &UiState) {
    let open_count = state.rows.iter().filter(|row| row.status == "open").count();
    let total_count = state.rows.len();
    let mode = match state.mode {
        InputMode::Normal => "NORMAL",
        InputMode::Filter => "FILTER",
        InputMode::Create => "CREATE",
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                " task ui ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {} open / {} total", open_count, total_count)),
            Span::raw("  •  "),
            Span::styled(
                mode,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(Color::Gray)),
            Span::raw(if state.mode == InputMode::Create {
                if state.create_branch.is_empty() {
                    "(none)".to_string()
                } else {
                    state.create_branch.clone()
                }
            } else if state.filter.is_empty() {
                "(none)".to_string()
            } else {
                state.filter.clone()
            }),
            Span::raw("   "),
            Span::styled(&state.message, Style::default().fg(Color::Yellow)),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));
    frame.render_widget(paragraph, area);
}

fn render_body(frame: &mut Frame, area: Rect, state: &UiState) {
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
        let status_style = match row.status.as_str() {
            "open" => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            "parked" => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::Gray),
        };
        Some(Row::new(vec![
            Cell::from(row.status.clone()).style(status_style),
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
    .block(Block::default().title("Tasks").borders(Borders::ALL))
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

    let details = if let Some(row) = state.selected_row() {
        vec![
            Line::from(vec![
                Span::styled("Repo: ", Style::default().fg(Color::Gray)),
                Span::raw(row.repo.clone()),
            ]),
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(Color::Gray)),
                Span::raw(row.branch.clone()),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::raw(row.status.clone()),
            ]),
            Line::from(vec![
                Span::styled("Session: ", Style::default().fg(Color::Gray)),
                Span::raw(tmux::session_name(&row.repo, &row.branch)),
            ]),
            Line::from(vec![
                Span::styled("Path: ", Style::default().fg(Color::Gray)),
                Span::raw(row.path.to_string_lossy().to_string()),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Actions",
                Style::default().fg(Color::Cyan),
            )]),
            Line::from("Enter  open selected task"),
            Line::from("p      park selected task"),
            Line::from("f      finish selected task"),
            Line::from("c      create new task"),
            Line::from("r      refresh task list"),
            Line::from("/      enter filter mode"),
            Line::from("?      toggle help"),
            Line::from("q      quit"),
        ]
    } else {
        vec![
            Line::from("No matching tasks"),
            Line::from("Try clearing the filter or refreshing."),
        ]
    };

    let details_panel =
        Paragraph::new(details).block(Block::default().title("Details").borders(Borders::ALL));
    frame.render_widget(details_panel, chunks[1]);
}

fn render_footer(frame: &mut Frame, area: Rect, state: &UiState) {
    let mode = match state.mode {
        InputMode::Normal => "NORMAL",
        InputMode::Filter => "FILTER",
        InputMode::Create => "CREATE",
    };

    let keys = vec![
        Line::from(vec![
            Span::styled("Mode: ", Style::default().fg(Color::Gray)),
            Span::styled(
                mode,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
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

    let footer = Paragraph::new(keys)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
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
