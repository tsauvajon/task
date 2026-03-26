use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout as UiLayout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Cell, Clear, Padding, Paragraph, Row, Table, TableState},
};

use super::{
    state::{InputMode, UiState, ViewMode},
    theme::Theme,
};
use crate::runtime::task_rows::TaskStatus;

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

fn render_body(frame: &mut Frame, area: Rect, state: &mut UiState, theme: &Theme) {
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
        InputMode::Normal => match state.view {
            ViewMode::Tasks => "List tasks",
            ViewMode::Repos => "List repos",
        },
        InputMode::Filter => "Filter",
        InputMode::CreateTask => "Create task",
        InputMode::CloneRepo => "Clone repo",
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

fn render_tasks(frame: &mut Frame, area: Rect, state: &mut UiState, theme: &Theme) {
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

fn render_repos(frame: &mut Frame, area: Rect, state: &mut UiState, theme: &Theme) {
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

// ── Actions / keybind panel ──────────────────────────────────────────────────

fn keybind_line(key: &str, desc: &str, key_color: Color, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<7}"), Style::default().fg(key_color)),
        Span::styled(format!(" {desc}"), theme.key_desc_style()),
    ])
}

fn actions_for_mode(state: &UiState, theme: &Theme) -> Vec<Line<'static>> {
    let kc = theme.mode_color(state.mode);

    match state.mode {
        InputMode::Normal => match state.view {
            ViewMode::Tasks => {
                let mut lines = vec![
                    keybind_line("enter", "open selected task", kc, theme),
                    keybind_line("tab", "switch to repos view", kc, theme),
                    keybind_line("/", "enter filter mode", kc, theme),
                    keybind_line("t", "create new task", kc, theme),
                    keybind_line("p", "park selected task", kc, theme),
                    keybind_line("f", "finish selected task", kc, theme),
                    keybind_line("r", "refresh tasks", kc, theme),
                    keybind_line("ctrl+p", "commands", kc, theme),
                    keybind_line("q", "quit", kc, theme),
                ];
                if state.task_repo_scope.is_some() {
                    lines.insert(0, keybind_line("esc", "back to repos", kc, theme));
                }
                lines
            }
            ViewMode::Repos => vec![
                keybind_line("enter", "open selected repo tasks", kc, theme),
                keybind_line("tab", "switch to tasks view", kc, theme),
                keybind_line("/", "enter filter mode", kc, theme),
                keybind_line("t", "create new task", kc, theme),
                keybind_line("c", "clone repo interactively", kc, theme),
                keybind_line("d", "toggle detached worktree", kc, theme),
                keybind_line("r", "refresh repos", kc, theme),
                keybind_line("ctrl+p", "commands", kc, theme),
                keybind_line("q", "quit", kc, theme),
            ],
        },
        InputMode::Filter => {
            let mut lines = vec![
                keybind_line("tab", "switch tasks/repos", kc, theme),
                keybind_line("ctrl-u", "clear filter", kc, theme),
                keybind_line("enter", "apply and return", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("filter ", theme.muted_style()),
                Span::styled(
                    state.filter_text.clone(),
                    Style::default().fg(kc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", theme.cursor_style(InputMode::Filter)),
            ]));
            lines
        }
        InputMode::CreateTask => {
            let mut lines = vec![
                keybind_line("ctrl-u", "clear branch name", kc, theme),
                keybind_line("enter", "create and open task", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("branch ", theme.muted_style()),
                Span::styled(
                    state.create_branch.clone(),
                    Style::default().fg(kc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", theme.cursor_style(InputMode::CreateTask)),
            ]));
            lines
        }
        InputMode::CloneRepo => {
            let mut lines = vec![
                keybind_line("ctrl-u", "clear input", kc, theme),
                keybind_line("enter", "clone repository", kc, theme),
                keybind_line("esc", "return to normal", kc, theme),
            ];
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("url ", theme.muted_style()),
                Span::styled(
                    state.clone_input.clone(),
                    Style::default().fg(kc).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", theme.cursor_style(InputMode::CloneRepo)),
            ]));
            lines
        }
    }
}

// ── Help overlay ─────────────────────────────────────────────────────────────

fn render_help(frame: &mut Frame, state: &mut UiState, theme: &Theme) {
    let popup = help_popup_rect(frame.area());
    state.help_area = Some(popup);

    let normal_c = theme.mode_color(InputMode::Normal);
    let filter_c = theme.mode_color(InputMode::Filter);
    let create_c = theme.mode_color(InputMode::CreateTask);
    let clone_c = theme.mode_color(InputMode::CloneRepo);
    let desc_style = theme.key_desc_style();

    let section = |title: &str, color: Color| -> Line<'static> {
        Line::from(Span::styled(
            title.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
    };

    let pad_h: u16 = 4;
    let inner_w = popup.width.saturating_sub(pad_h * 2) as usize;

    let cmd_style = Style::default().fg(theme.text);
    let hk = move |key: &str, desc: &str| -> Line<'static> {
        let gap = inner_w.saturating_sub(desc.chars().count() + key.chars().count());
        Line::from(vec![
            Span::styled(desc.to_string(), cmd_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(key.to_string(), desc_style),
        ])
    };

    let mut lines = vec![
        section("All views", normal_c),
        hk("tab", "Switch tasks/repos view"),
        hk("/", "Enter filter mode"),
        hk("ctrl+p", "Commands"),
        hk("q/ctrl-c", "Quit"),
        Line::from(""),
        section("Tasks view", normal_c),
        hk("enter", "Open selected task"),
        hk("esc", "Back to repos (when scoped)"),
        hk("t", "Create new task"),
        hk("p", "Park selected task"),
        hk("f", "Finish selected task"),
        hk("r", "Refresh tasks"),
        Line::from(""),
        section("Repos view", normal_c),
        hk("enter", "Open selected repo tasks"),
        hk("t", "Create new task"),
        hk("c", "Clone repo interactively"),
        hk("d", "Toggle detached worktree"),
        hk("r", "Refresh repos"),
        Line::from(""),
        section("Filter", filter_c),
        hk("tab", "Switch tasks/repos view"),
        hk("ctrl-u", "Clear filter"),
        hk("esc", "Return to view"),
        Line::from(""),
        section("Create task", create_c),
        hk("ctrl-u", "Clear branch name"),
        hk("esc", "Return to view"),
        Line::from(""),
        section("Clone repo", clone_c),
        hk("ctrl-u", "Clear input"),
        hk("esc", "Return to view"),
    ];

    let left = "Commands";
    let right = "esc";
    let gap = inner_w.saturating_sub(left.len() + right.len());
    let title_line = Line::from(vec![
        Span::styled(
            left.to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(right.to_string(), Style::default().fg(theme.text)),
    ]);

    lines.insert(0, title_line);
    lines.insert(1, Line::from(""));

    let help = Paragraph::new(lines)
        .block(
            Block::default()
                .padding(Padding::new(pad_h, pad_h, 1, 1))
                .style(Style::default().bg(theme.overlay_bg)),
        )
        .style(Style::default().fg(theme.text));

    frame.render_widget(Clear, popup);
    frame.render_widget(help, popup);
}

// ── Scrollbar ────────────────────────────────────────────────────────────────

/// Compute thumb start position and length using integer-only arithmetic.
///
/// Returns `(thumb_start, thumb_len)` where both are in track-row units.
/// The thumb length is constant for a given `(item_count, visible_rows, track_len)`
/// regardless of `selected`, which avoids the ±1 jitter that ratatui's
/// float-based `Scrollbar` widget produces.
fn scrollbar_geometry(
    selected: usize,
    item_count: usize,
    visible_rows: usize,
    track_len: usize,
) -> (usize, usize) {
    if track_len == 0 || item_count == 0 {
        return (0, 0);
    }
    let thumb_len = (visible_rows * track_len)
        .div_ceil(item_count)
        .max(1)
        .min(track_len);
    let max_offset = track_len.saturating_sub(thumb_len);
    let thumb_start = if item_count <= 1 {
        0
    } else {
        selected * max_offset / (item_count - 1)
    };
    (thumb_start, thumb_len)
}

/// Paint a vertical scrollbar into the rightmost column of `sb_area`.
fn render_scrollbar(
    frame: &mut Frame,
    sb_area: Rect,
    selected: usize,
    item_count: usize,
    visible_rows: usize,
    theme: &Theme,
) {
    let track_len = sb_area.height as usize;
    let (thumb_start, thumb_len) =
        scrollbar_geometry(selected, item_count, visible_rows, track_len);
    if thumb_len == 0 {
        return;
    }
    let col = sb_area.x + sb_area.width - 1;
    let buf = frame.buffer_mut();
    for row in 0..track_len {
        let in_thumb = row >= thumb_start && row < thumb_start + thumb_len;
        let (sym, color) = if in_thumb {
            ("┃", theme.scrollbar_thumb)
        } else {
            ("│", theme.scrollbar_track)
        };
        buf.set_string(col, sb_area.y + row as u16, sym, Style::default().fg(color));
    }
}

// ── Utilities ────────────────────────────────────────────────────────────────

/// Compute a fixed-width, top-offset popup rect for the help/commands overlay.
///
/// Mimics OpenCode's command palette layout:
/// - Fixed width of 60 columns (clamped to `terminal_width - 2`).
/// - Positioned 25% from the top of the terminal.
/// - Height capped at `floor(terminal_height / 2)`.
/// - Horizontally centered.
fn help_popup_rect(area: Rect) -> Rect {
    let max_w: u16 = 60;
    let w = max_w.min(area.width.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;

    let y = area.y + area.height / 4;
    let max_h = area.height / 2;
    let h = max_h.min(area.height.saturating_sub(y));

    Rect::new(x, y, w, h)
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

    use super::{help_popup_rect, scrollbar_geometry, status_label};
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

    mod help_popup_rect_tests {
        use super::*;

        #[test]
        fn fits_within_terminal_bounds() {
            let area = Rect::new(0, 0, 120, 40);
            let popup = help_popup_rect(area);
            assert!(popup.x >= area.x);
            assert!(popup.y >= area.y);
            assert!(popup.x + popup.width <= area.x + area.width);
            assert!(popup.y + popup.height <= area.y + area.height);
        }

        #[test]
        fn width_capped_at_60() {
            let area = Rect::new(0, 0, 200, 60);
            let popup = help_popup_rect(area);
            assert_eq!(popup.width, 60);
        }

        #[test]
        fn width_clamped_to_narrow_terminal() {
            let area = Rect::new(0, 0, 40, 30);
            let popup = help_popup_rect(area);
            assert_eq!(popup.width, 38); // 40 - 2
        }

        #[test]
        fn positioned_25_percent_from_top() {
            let area = Rect::new(0, 0, 80, 40);
            let popup = help_popup_rect(area);
            assert_eq!(popup.y, 10); // 40 / 4
        }

        #[test]
        fn height_capped_at_half_terminal() {
            let area = Rect::new(0, 0, 80, 40);
            let popup = help_popup_rect(area);
            assert!(popup.height <= 20); // 40 / 2
        }

        #[test]
        fn horizontally_centered() {
            let area = Rect::new(0, 0, 100, 40);
            let popup = help_popup_rect(area);
            let left_margin = popup.x;
            let right_margin = area.width - popup.x - popup.width;
            assert_eq!(left_margin, right_margin);
        }

        #[test]
        fn zero_area_returns_zero_rect() {
            let area = Rect::new(0, 0, 0, 0);
            let popup = help_popup_rect(area);
            assert_eq!(popup.width, 0);
            assert_eq!(popup.height, 0);
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
            assert!(text.contains("clear filter"));
            assert!(text.contains("esc"));
        }

        #[test]
        fn create_task_mode_lists_create_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("clear branch name"));
            assert!(text.contains("create and open"));
        }

        #[test]
        fn clone_repo_mode_lists_clone_actions() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("clear input"));
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

        #[test]
        fn normal_repos_includes_create_new_task_action() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("create new task"),
                "repos view should include create new task action: {text}"
            );
        }

        #[test]
        fn normal_repos_includes_detach_action() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("toggle detached worktree"),
                "repos view should include detach action: {text}"
            );
        }

        #[test]
        fn create_task_mode_shows_branch_label_when_empty() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
            assert!(state.create_branch.is_empty());
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("branch"),
                "branch label should appear even when branch is empty: {text}"
            );
        }

        #[test]
        fn create_task_mode_shows_typed_branch_text() {
            let theme = Theme::dark();
            let mut state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
            state.create_branch = "feat/my-feature".to_string();
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("feat/my-feature"),
                "typed branch name should appear in actions: {text}"
            );
        }

        #[test]
        fn create_task_mode_shows_ctrl_u_action() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("ctrl-u") && text.contains("clear branch name"),
                "create task mode should include ctrl-u clear action: {text}"
            );
        }

        #[test]
        fn scoped_tasks_view_includes_esc_action() {
            let theme = Theme::dark();
            let mut state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
            state.task_repo_scope = Some("github.com/acme/app".to_string());
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("back to repos"),
                "scoped tasks view should include esc/back to repos: {text}"
            );
        }

        #[test]
        fn unscoped_tasks_view_excludes_esc_action() {
            let theme = Theme::dark();
            let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
            assert!(state.task_repo_scope.is_none());
            let lines = actions_for_mode(&state, &theme);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                !text.contains("back to repos"),
                "unscoped tasks view should not include back to repos: {text}"
            );
        }
    }

    mod scrollbar {
        use super::scrollbar_geometry;

        #[test]
        fn thumb_len_is_stable_across_positions() {
            let (_, baseline_len) = scrollbar_geometry(0, 50, 30, 31);
            for pos in 1..50 {
                let (_, len) = scrollbar_geometry(pos, 50, 30, 31);
                assert_eq!(len, baseline_len, "thumb_len changed at position {pos}");
            }
        }

        #[test]
        fn thumb_shrinks_as_item_count_grows() {
            let (_, len_few) = scrollbar_geometry(0, 20, 15, 30);
            let (_, len_many) = scrollbar_geometry(0, 200, 15, 30);
            assert!(
                len_few >= len_many,
                "fewer items should have equal or larger thumb: {len_few} vs {len_many}"
            );
        }

        #[test]
        fn thumb_fills_track_when_all_visible() {
            let (start, len) = scrollbar_geometry(0, 10, 30, 31);
            assert_eq!(start, 0);
            assert_eq!(len, 31);
        }

        #[test]
        fn thumb_minimum_one_cell() {
            let (_, len) = scrollbar_geometry(0, 1000, 5, 10);
            assert!(len >= 1, "thumb must be at least 1 cell: {len}");
        }

        #[test]
        fn thumb_start_zero_at_first_item() {
            let (start, _) = scrollbar_geometry(0, 50, 30, 31);
            assert_eq!(start, 0);
        }

        #[test]
        fn thumb_reaches_bottom_for_last_item() {
            let track = 31;
            let (start, len) = scrollbar_geometry(49, 50, 30, track);
            assert_eq!(
                start + len,
                track,
                "thumb should reach bottom: start={start} len={len} track={track}"
            );
        }

        #[test]
        fn thumb_start_monotonically_increases() {
            let mut prev = 0;
            for pos in 0..100 {
                let (start, _) = scrollbar_geometry(pos, 100, 30, 40);
                assert!(
                    start >= prev,
                    "thumb_start decreased at pos {pos}: {start} < {prev}"
                );
                prev = start;
            }
        }

        #[test]
        fn empty_list_returns_zero() {
            let (start, len) = scrollbar_geometry(0, 0, 30, 31);
            assert_eq!(start, 0);
            assert_eq!(len, 0);
        }

        #[test]
        fn single_item_thumb_fills_track() {
            let (start, len) = scrollbar_geometry(0, 1, 30, 31);
            assert_eq!(start, 0);
            assert_eq!(len, 31);
        }
    }
}
