use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout as UiLayout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Cell, Clear, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
};

use super::{
    state::{InputMode, UiState, ViewMode},
    theme::Theme,
};
use crate::runtime::task_rows::TaskStatus;

pub(super) fn render(frame: &mut Frame, state: &UiState) {
    let theme = Theme::dark();

    let outer = UiLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(frame.area());

    render_body(frame, outer[0], state, &theme);
    render_status_bar(frame, outer[1], state, &theme);

    if state.show_help {
        render_help(frame, &theme);
    }
}

fn render_body(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
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
        InputMode::Normal => "NORMAL",
        InputMode::Filter => "FILTER",
        InputMode::CreateTask => "CREATE TASK",
        InputMode::CloneRepo => "CLONE REPO",
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

// ── Status bar ───────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
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
    match state.mode {
        InputMode::Filter if !state.filter_text.is_empty() => {
            spans.push(Span::styled("  filter ", theme.text_style()));
            spans.push(Span::styled(&state.filter_text, theme.muted_style()));
        }
        InputMode::CreateTask => {
            spans.push(Span::styled("  branch: ", theme.text_style()));
            spans.push(Span::styled(&state.create_branch, theme.muted_style()));
            spans.push(Span::styled("▎", theme.key_style()));
        }
        InputMode::CloneRepo => {
            spans.push(Span::styled("  url: ", theme.text_style()));
            spans.push(Span::styled(&state.clone_input, theme.muted_style()));
            spans.push(Span::styled("▎", theme.key_style()));
        }
        _ => {}
    }

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

// ── Activity panel ───────────────────────────────────────────────────────────

fn render_activity(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
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

// ── Tasks table ──────────────────────────────────────────────────────────────

fn render_tasks(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let scoped = state.task_repo_scope.is_some();

    let header = if scoped {
        Row::new(vec!["STATUS", "BRANCH", "PATH"])
    } else {
        Row::new(vec!["STATUS", "REPO", "BRANCH", "PATH"])
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
        .max("BRANCH".len()) as u16;

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

    // Scrollbar — only when content overflows the visible area.
    // Account for padding (1 top) + header row (1) = 2 rows overhead.
    let visible_rows = area.height.saturating_sub(2) as usize;
    if row_count > visible_rows {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .begin_symbol(None)
            .end_symbol(None)
            .track_style(Style::default().fg(theme.scrollbar_track))
            .thumb_style(Style::default().fg(theme.scrollbar_thumb));
        let content_len = row_count.max(visible_rows * 10);
        let scaled_pos = (state.task_selected * content_len.saturating_sub(1))
            .checked_div(row_count.saturating_sub(1))
            .unwrap_or(0);
        let mut sb_state = ScrollbarState::new(content_len).position(scaled_pos);
        let sb_area = Rect {
            x: area.x,
            y: area.y + 1, // below header
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        frame.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
    }
}

// ── Repos table ──────────────────────────────────────────────────────────────

fn render_repos(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let header = Row::new(vec!["REPO", "OPEN", "PARKED", "DET"])
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
            Constraint::Length(5),
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

    // Scrollbar — only when content overflows the visible area.
    let visible_rows = area.height.saturating_sub(2) as usize;
    if row_count > visible_rows {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .begin_symbol(None)
            .end_symbol(None)
            .track_style(Style::default().fg(theme.scrollbar_track))
            .thumb_style(Style::default().fg(theme.scrollbar_thumb));
        let content_len = row_count.max(visible_rows * 10);
        let scaled_pos = (state.repo_selected * content_len.saturating_sub(1))
            .checked_div(row_count.saturating_sub(1))
            .unwrap_or(0);
        let mut sb_state = ScrollbarState::new(content_len).position(scaled_pos);
        let sb_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        frame.render_stateful_widget(scrollbar, sb_area, &mut sb_state);
    }
}

// ── Actions / keybind panel ──────────────────────────────────────────────────

fn keybind_line(key: &str, desc: &str, key_color: Color, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{key:<7}"),
            Style::default().fg(key_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {desc}"), theme.key_desc_style()),
    ])
}

fn actions_for_mode(state: &UiState, theme: &Theme) -> Vec<Line<'static>> {
    let kc = theme.mode_color(state.mode);

    match state.mode {
        InputMode::Normal => match state.view {
            ViewMode::Tasks => {
                let mut lines = vec![
                    keybind_line("tab", "switch to repos view", kc, theme),
                    keybind_line("enter", "open selected task", kc, theme),
                    keybind_line("c", "create new task", kc, theme),
                    keybind_line("p", "park selected task", kc, theme),
                    keybind_line("f", "finish selected task", kc, theme),
                    keybind_line("/", "enter filter mode", kc, theme),
                    keybind_line("r", "refresh tasks", kc, theme),
                    keybind_line("?", "toggle help", kc, theme),
                    keybind_line("q", "quit", kc, theme),
                ];
                if state.task_repo_scope.is_some() {
                    lines.insert(0, keybind_line("esc", "show all tasks", kc, theme));
                }
                lines
            }
            ViewMode::Repos => vec![
                keybind_line("tab", "switch to tasks view", kc, theme),
                keybind_line("enter", "open selected repo tasks", kc, theme),
                keybind_line("c", "clone repo interactively", kc, theme),
                keybind_line("d", "toggle detached worktree", kc, theme),
                keybind_line("/", "enter filter mode", kc, theme),
                keybind_line("r", "refresh repos", kc, theme),
                keybind_line("?", "toggle help", kc, theme),
                keybind_line("q", "quit", kc, theme),
            ],
        },
        InputMode::Filter => vec![
            keybind_line("tab", "switch tasks/repos", kc, theme),
            keybind_line("type", "append filter text", kc, theme),
            keybind_line("backsp", "delete character", kc, theme),
            keybind_line("ctrl-u", "clear filter", kc, theme),
            keybind_line("enter", "apply and return", kc, theme),
            keybind_line("esc", "return to normal", kc, theme),
        ],
        InputMode::CreateTask => {
            let mut lines = vec![
                keybind_line("type", "set new branch name", kc, theme),
                keybind_line("backsp", "delete character", kc, theme),
                keybind_line("enter", "create and open task", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            if !state.create_branch.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Branch: ", theme.muted_style()),
                    Span::styled(
                        state.create_branch.clone(),
                        Style::default().fg(kc).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            lines
        }
        InputMode::CloneRepo => {
            let mut lines = vec![
                keybind_line("type", "<repo-url> [repo-key]", kc, theme),
                keybind_line("backsp", "delete character", kc, theme),
                keybind_line("ctrl-u", "clear input", kc, theme),
                keybind_line("enter", "clone repository", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            if !state.clone_input.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Input: ", theme.muted_style()),
                    Span::styled(
                        state.clone_input.clone(),
                        Style::default().fg(kc).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            lines
        }
    }
}

// ── Help overlay ─────────────────────────────────────────────────────────────

fn render_help(frame: &mut Frame, theme: &Theme) {
    let popup = centered_rect(80, 80, frame.area());

    let section = |title: &str| -> Line<'static> {
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
    };

    let hk = |key: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {key:<11}"), theme.key_style()),
            Span::styled(desc.to_string(), theme.key_desc_style()),
        ])
    };

    let lines = vec![
        section("normal mode (all views)"),
        hk("↑/k", "move up"),
        hk("↓/j", "move down"),
        hk("tab", "switch tasks/repos view"),
        hk("?", "toggle help"),
        hk("q/ctrl-c", "quit"),
        Line::from(""),
        section("tasks view"),
        hk("enter", "open selected task"),
        hk("esc", "exit repo scope (when scoped)"),
        hk("p", "park selected task"),
        hk("f", "finish selected task"),
        hk("c", "create new task"),
        hk("r", "refresh tasks"),
        hk("/", "enter filter mode"),
        Line::from(""),
        section("repos view"),
        hk("enter", "open selected repo tasks"),
        hk("c", "clone repo interactively"),
        hk("d", "toggle detached worktree"),
        hk("/", "enter filter mode"),
        hk("r", "refresh repos"),
        Line::from(""),
        section("filter mode"),
        hk("tab", "switch tasks/repos view"),
        hk("type", "append filter text"),
        hk("backspace", "delete character"),
        hk("ctrl-u", "clear filter"),
        hk("enter", "apply and return to normal"),
        hk("esc", "return to normal"),
        Line::from(""),
        section("create task mode"),
        hk("type", "set new branch name"),
        hk("backspace", "delete character"),
        hk("enter", "create and open new task"),
        hk("esc", "return to normal"),
        Line::from(""),
        section("clone repo mode"),
        hk("type", "<repo-url> [repo-key]"),
        hk("backspace", "delete character"),
        hk("ctrl-u", "clear input"),
        hk("enter", "clone repository"),
        hk("esc", "return to normal"),
    ];

    let help = Paragraph::new(lines)
        .block(
            Block::default()
                .padding(Padding::new(2, 2, 1, 1))
                .style(Style::default().bg(theme.overlay_bg)),
        )
        .style(Style::default().fg(theme.text));

    frame.render_widget(Clear, popup);
    frame.render_widget(help, popup);
}

// ── Utilities ────────────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{centered_rect, status_label};
    use crate::{
        runtime::task_rows::TaskStatus,
        ui::{
            state::{InputMode, UiState, ViewMode},
            theme::Theme,
        },
    };

    mod scoped_tasks_view {
        use std::path::PathBuf;

        use super::*;
        use crate::runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
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

    mod centered_rect_tests {
        use super::*;

        #[test]
        fn returns_inner_rect_within_bounds() {
            let outer = Rect::new(0, 0, 100, 100);
            let inner = centered_rect(50, 50, outer);
            assert!(inner.x >= outer.x);
            assert!(inner.y >= outer.y);
            assert!(inner.x + inner.width <= outer.x + outer.width);
            assert!(inner.y + inner.height <= outer.y + outer.height);
        }

        #[test]
        fn full_percent_covers_most_of_area() {
            let outer = Rect::new(0, 0, 100, 100);
            let inner = centered_rect(100, 100, outer);
            assert!(inner.width >= outer.width / 2);
            assert!(inner.height >= outer.height / 2);
        }

        #[test]
        fn zero_rect_returns_zero_area() {
            let outer = Rect::new(0, 0, 0, 0);
            let inner = centered_rect(50, 50, outer);
            assert_eq!(inner.width, 0);
            assert_eq!(inner.height, 0);
        }
    }

    mod actions_for_mode_tests {
        use super::{super::actions_for_mode, *};

        fn state_with_mode(mode: InputMode, view: ViewMode) -> UiState {
            let mut state = UiState::new(Vec::new(), Vec::new(), None);
            state.view = view;
            state.mode = mode;
            state
        }

        #[test]
        fn normal_tasks_lists_task_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("open selected task"));
            assert!(text.contains("park"));
            assert!(text.contains("finish"));
            assert!(text.contains("create new task"));
        }

        #[test]
        fn normal_repos_lists_repo_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("clone repo"));
            assert!(text.contains("switch to tasks view"));
        }

        #[test]
        fn filter_mode_lists_filter_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("filter text"));
            assert!(text.contains("esc"));
        }

        #[test]
        fn create_task_mode_lists_create_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("branch name"));
            assert!(text.contains("create and open"));
        }

        #[test]
        fn clone_repo_mode_lists_clone_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("repo-url"));
            assert!(text.contains("clone repository"));
        }

        #[test]
        fn clone_repo_mode_interpolates_input_text() {
            let theme = Theme::dark();
            let mut state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
            state.clone_input = "git@github.com:me/app.git".to_string();
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("git@github.com:me/app.git"),
                "clone_input should appear in actions: {text}"
            );
        }

        #[test]
        fn filter_mode_same_actions_regardless_of_view() {
            let theme = Theme::dark();
            let tasks_state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
            let repos_state = state_with_mode(InputMode::Filter, ViewMode::Repos);
            let tasks_lines: Vec<String> = actions_for_mode(&tasks_state, &theme)
                .iter()
                .map(|l| l.to_string())
                .collect();
            let repos_lines: Vec<String> = actions_for_mode(&repos_state, &theme)
                .iter()
                .map(|l| l.to_string())
                .collect();
            assert_eq!(
                tasks_lines, repos_lines,
                "Filter mode actions should be identical regardless of view"
            );
        }

        #[test]
        fn normal_tasks_does_not_include_repo_specific_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                !text.contains("clone repo"),
                "tasks view should not include clone action: {text}"
            );
        }

        #[test]
        fn normal_repos_does_not_include_task_specific_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                !text.contains("park"),
                "repos view should not include park action: {text}"
            );
            assert!(
                !text.contains("finish"),
                "repos view should not include finish action: {text}"
            );
        }
    }
}
