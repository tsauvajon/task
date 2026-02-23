use std::io;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout as UiLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState};
use ratatui::{Frame, Terminal};

use crate::git::parsing::TaskRow;
use crate::runtime::session_name::task_session_name;
use crate::workspace_paths::WorkspacePaths;

type AppTerminal = Terminal<CrosstermBackend<io::Stdout>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Filter,
    Create,
}

#[derive(Debug, Clone)]
enum UiAction {
    Quit,
    Open(TaskRow),
    Create { repo: String, branch: String },
}

#[derive(Debug, Clone)]
struct UiState {
    rows: Vec<TaskRow>,
    filtered_indices: Vec<usize>,
    selected: usize,
    filter: String,
    create_branch: String,
    mode: InputMode,
    message: String,
    show_help: bool,
}

impl UiState {
    fn new(rows: Vec<TaskRow>) -> Self {
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

    fn apply_filter(&mut self) {
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

    fn selected_row(&self) -> Option<&TaskRow> {
        let index = *self.filtered_indices.get(self.selected)?;
        self.rows.get(index)
    }

    fn move_next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.filtered_indices.len().saturating_sub(1));
    }

    fn move_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn set_rows(&mut self, rows: Vec<TaskRow>) {
        self.rows = rows;
        self.apply_filter();
    }
}

pub fn run(layout: &WorkspacePaths, repo_arg: Option<&str>) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let rows = load_rows(layout, repo_arg)?;
    let mut state = UiState::new(rows);

    if state.rows.is_empty() {
        state.message = "No tasks found. Press q to quit.".to_string();
    }

    let mut terminal = init_terminal()?;
    let ui_result = run_event_loop(layout, repo_arg, &mut terminal, &mut state);
    let restore_result = restore_terminal(&mut terminal);
    restore_result?;

    match ui_result? {
        UiAction::Quit => Ok(()),
        UiAction::Open(row) => super::launch_workspace(&row.repo, &row.branch, &row.path),
        UiAction::Create { repo, branch } => super::start::run(layout, &repo, &branch, None),
    }
}

fn load_rows(layout: &WorkspacePaths, repo_arg: Option<&str>) -> Result<Vec<TaskRow>, String> {
    let open_sessions = super::tmux_sessions();
    let mut rows = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(super::current_repo_key);

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
        let gitdir = layout.repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }
        rows.extend(super::repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
    } else {
        for repo_key in super::available_repo_keys(layout)? {
            let gitdir = layout.repo_gitdir_path(&repo_key);
            rows.extend(super::repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
        }
    }

    rows.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then(left.repo.cmp(&right.repo))
            .then(left.branch.cmp(&right.branch))
    });

    Ok(rows)
}

fn status_rank(status: &str) -> u8 {
    match status {
        "open" => 0,
        "parked" => 1,
        _ => 2,
    }
}

fn init_terminal() -> Result<AppTerminal, String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(|e| e.to_string())?;

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).map_err(|e| e.to_string())
}

fn restore_terminal(terminal: &mut AppTerminal) -> Result<(), String> {
    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;
    Ok(())
}

fn run_event_loop(
    layout: &WorkspacePaths,
    repo_arg: Option<&str>,
    terminal: &mut AppTerminal,
    state: &mut UiState,
) -> Result<UiAction, String> {
    loop {
        terminal
            .draw(|frame| render(frame, state))
            .map_err(|e| e.to_string())?;

        let event = event::read().map_err(|e| e.to_string())?;
        if let Event::Key(key) = event {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(UiAction::Quit);
            }

            match state.mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(UiAction::Quit),
                    KeyCode::Down | KeyCode::Char('j') => state.move_next(),
                    KeyCode::Up | KeyCode::Char('k') => state.move_prev(),
                    KeyCode::Char('/') => {
                        state.mode = InputMode::Filter;
                        state.message = "Filter mode: type to refine tasks".to_string();
                    }
                    KeyCode::Char('?') => {
                        state.show_help = !state.show_help;
                    }
                    KeyCode::Char('c') => {
                        state.mode = InputMode::Create;
                        state.create_branch.clear();
                        state.message = "Create mode: type branch name".to_string();
                    }
                    KeyCode::Char('f') => {
                        finish_selected(layout, state)?;
                        let rows = load_rows(layout, repo_arg)?;
                        state.set_rows(rows);
                    }
                    KeyCode::Char('r') => {
                        let rows = load_rows(layout, repo_arg)?;
                        state.set_rows(rows);
                        state.message = "Refreshed task list".to_string();
                    }
                    KeyCode::Char('p') => {
                        park_selected(state)?;
                        let rows = load_rows(layout, repo_arg)?;
                        state.set_rows(rows);
                    }
                    KeyCode::Enter => {
                        if let Some(row) = state.selected_row() {
                            return Ok(UiAction::Open(row.clone()));
                        }
                    }
                    _ => {}
                },
                InputMode::Filter => match key.code {
                    KeyCode::Esc => {
                        state.mode = InputMode::Normal;
                        state.message = "Returned to normal mode".to_string();
                    }
                    KeyCode::Enter => {
                        state.mode = InputMode::Normal;
                        state.message =
                            format!("Filter applied: {} matches", state.filtered_indices.len());
                    }
                    KeyCode::Backspace => {
                        state.filter.pop();
                        state.apply_filter();
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.filter.clear();
                        state.apply_filter();
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.filter.push(ch);
                        state.apply_filter();
                    }
                    _ => {}
                },
                InputMode::Create => match key.code {
                    KeyCode::Esc => {
                        state.mode = InputMode::Normal;
                        state.message = "Create cancelled".to_string();
                    }
                    KeyCode::Enter => {
                        let branch = state.create_branch.trim();
                        if branch.is_empty() {
                            state.message = "Branch name cannot be empty".to_string();
                            continue;
                        }

                        let repo = if let Some(row) = state.selected_row() {
                            row.repo.clone()
                        } else if let Some(repo_arg) = repo_arg {
                            super::resolve_repo_input(Some(repo_arg))?
                        } else {
                            super::resolve_repo_input(None)?
                        };

                        return Ok(UiAction::Create {
                            repo,
                            branch: branch.to_string(),
                        });
                    }
                    KeyCode::Backspace => {
                        state.create_branch.pop();
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.create_branch.push(ch);
                    }
                    _ => {}
                },
            }
        }
    }
}

fn park_selected(state: &mut UiState) -> Result<(), String> {
    let Some(row) = state.selected_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    if !super::command_exists("tmux") {
        return Err("tmux is not available. Run 'task list' to inspect tasks.".to_string());
    }

    let session = task_session_name(&row.repo, &row.branch);
    if super::tmux_has_session(&session) {
        super::run_status("tmux", &["kill-session", "-t", &session], None)?;
        state.message = format!("Parked task: {} {}", row.repo, row.branch);
    } else {
        state.message = format!("Task already parked: {} {}", row.repo, row.branch);
    }

    Ok(())
}

fn finish_selected(layout: &WorkspacePaths, state: &mut UiState) -> Result<(), String> {
    let Some(row) = state.selected_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    let session = task_session_name(&row.repo, &row.branch);
    if super::tmux_has_session(&session) {
        super::run_status("tmux", &["kill-session", "-t", &session], None)?;
    }

    super::finish::run(layout, Some(&row.repo), Some(&row.branch), false)?;
    state.message = format!("Finished task: {} {}", row.repo, row.branch);
    Ok(())
}

fn render(frame: &mut Frame, state: &UiState) {
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
                Span::raw(task_session_name(&row.repo, &row.branch)),
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
