use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout as UiLayout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};

use super::state::{InputMode, UiState, ViewMode};
use crate::runtime::task_rows::TaskStatus;

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
    let chunks = UiLayout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    match state.view {
        ViewMode::Tasks => render_tasks(frame, chunks[0], state),
        ViewMode::Repos => render_repos(frame, chunks[0], state),
    }

    let mode = match state.mode {
        InputMode::Normal => "NORMAL",
        InputMode::Filter => "FILTER",
        InputMode::CreateTask => "CREATE TASK",
        InputMode::CloneRepo => "CLONE REPO",
    };
    let mode_style = match state.mode {
        InputMode::Normal => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        InputMode::Filter => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        InputMode::CreateTask => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        InputMode::CloneRepo => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    };

    let actions = actions_for_mode(state);

    let details_lines = actions;

    let details_panel = Paragraph::new(details_lines).block(
        Block::default()
            .title(Line::from(Span::styled(mode, mode_style)))
            .borders(Borders::ALL),
    );
    frame.render_widget(details_panel, chunks[1]);
}

fn render_tasks(frame: &mut Frame, area: Rect, state: &UiState) {
    let open_count = state
        .task_rows
        .iter()
        .filter(|row| row.status == TaskStatus::Open)
        .count();
    let total_count = state.task_rows.len();
    let task_title = task_view_title(state.task_repo_scope.as_deref(), &state.filter_text);

    let scoped = state.task_repo_scope.is_some();

    let header = if scoped {
        Row::new(vec!["STATUS", "BRANCH", "PATH"])
    } else {
        Row::new(vec!["STATUS", "REPO", "BRANCH", "PATH"])
    }
    .style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows = state.task_filtered_indices.iter().filter_map(|index| {
        let row = state.task_rows.get(*index)?;
        let status_style = match row.status {
            TaskStatus::Open => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            TaskStatus::Parked => Style::default().fg(Color::Yellow),
        };
        if scoped {
            Some(Row::new(vec![
                Cell::from(status_label(row.status)).style(status_style),
                Cell::from(row.branch.to_string()),
                Cell::from(row.path.to_string_lossy().to_string()),
            ]))
        } else {
            Some(Row::new(vec![
                Cell::from(status_label(row.status)).style(status_style),
                Cell::from(row.repo.to_string()),
                Cell::from(row.branch.to_string()),
                Cell::from(row.path.to_string_lossy().to_string()),
            ]))
        }
    });

    let widths: &[Constraint] = if scoped {
        &[
            Constraint::Length(8),
            Constraint::Length(24),
            Constraint::Min(10),
        ]
    } else {
        &[
            Constraint::Length(8),
            Constraint::Length(28),
            Constraint::Length(24),
            Constraint::Min(10),
        ]
    };

    let table = Table::new(rows, widths.to_vec())
        .header(header)
        .block(
            Block::default()
                .title(task_title)
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
    if !state.task_filtered_indices.is_empty() {
        table_state.select(Some(state.task_selected));
    }
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn render_repos(frame: &mut Frame, area: Rect, state: &UiState) {
    let filtered_count = state.repo_filtered_indices.len();
    let repos_count = state.repo_rows.len();
    let repo_title = view_title("Repos", &state.filter_text);

    let header = Row::new(vec!["REPO", "OPEN", "PARKED", "DET"]).style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows = state.repo_filtered_indices.iter().filter_map(|index| {
        let row = state.repo_rows.get(*index)?;
        let det_cell = if row.is_detached {
            Cell::from("✓").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Cell::from("·").style(Style::default().fg(Color::DarkGray))
        };
        Some(Row::new(vec![
            Cell::from(row.repo.to_string()),
            Cell::from(row.open_tasks.to_string()).style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Cell::from(row.parked_tasks.to_string()).style(Style::default().fg(Color::Yellow)),
            det_cell,
        ]))
    });

    let table = Table::new(
        rows,
        [
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(repo_title)
            .title(
                Line::from(Span::styled(
                    format!("{} shown / {} total", filtered_count, repos_count),
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
    if !state.repo_filtered_indices.is_empty() {
        table_state.select(Some(state.repo_selected));
    }
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn actions_for_mode(state: &UiState) -> Vec<Line<'static>> {
    match state.mode {
        InputMode::Normal => match state.view {
            ViewMode::Tasks => vec![
                Line::from("Tab     switch to repos view"),
                Line::from("Enter  open selected task"),
                Line::from("c      create new task"),
                Line::from("p      park selected task"),
                Line::from("f      finish selected task"),
                Line::from("/      enter filter mode"),
                Line::from("r      refresh tasks"),
                Line::from("?      toggle help"),
                Line::from("q      quit"),
            ],
            ViewMode::Repos => vec![
                Line::from("Tab     switch to tasks view"),
                Line::from("Enter  open selected repo tasks"),
                Line::from("c      clone repo interactively"),
                Line::from("d      toggle detached worktree"),
                Line::from("/      enter filter mode"),
                Line::from("r      refresh repos"),
                Line::from("?      toggle help"),
                Line::from("q      quit"),
            ],
        },
        InputMode::Filter => vec![
            Line::from("Tab    switch Tasks/Repos"),
            Line::from("Type   append filter text"),
            Line::from("Backsp delete character"),
            Line::from("Ctrl-U clear filter"),
            Line::from("Enter  apply and return"),
            Line::from("Esc    return to normal"),
        ],
        InputMode::CreateTask => vec![
            Line::from("Type   set new branch name"),
            Line::from("Backsp delete character"),
            Line::from("Enter  create and open task"),
            Line::from("Esc    return to normal"),
        ],
        InputMode::CloneRepo => vec![
            Line::from("Type   <repo-url> [repo-key]"),
            Line::from("Backsp delete character"),
            Line::from("Ctrl-U clear input"),
            Line::from("Enter  clone repository"),
            Line::from("Esc    return to normal"),
            Line::from(format!("Input: {}", state.clone_input)),
        ],
    }
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
        Line::from("Normal mode (all views):"),
        Line::from("↑/k       move up"),
        Line::from("↓/j       move down"),
        Line::from("Tab       switch Tasks/Repos view"),
        Line::from("?         toggle help"),
        Line::from("q/Ctrl-C  quit"),
        Line::from(""),
        Line::from("Tasks view:"),
        Line::from("Enter     open selected task"),
        Line::from("p         park selected task"),
        Line::from("f         finish selected task"),
        Line::from("c         create new task"),
        Line::from("r         refresh tasks"),
        Line::from("/         enter filter mode"),
        Line::from(""),
        Line::from("Repos view:"),
        Line::from("Enter     open selected repo tasks"),
        Line::from("c         clone repo interactively"),
        Line::from("d         toggle detached worktree (add/remove)"),
        Line::from("/         enter filter mode"),
        Line::from("r         refresh repos"),
        Line::from(""),
        Line::from("Filter mode:"),
        Line::from("Tab       switch Tasks/Repos view"),
        Line::from("Type      append filter text"),
        Line::from("Backspace delete character"),
        Line::from("Ctrl-U    clear filter"),
        Line::from("Enter     apply and return to normal"),
        Line::from("Esc       return to normal"),
        Line::from(""),
        Line::from("Create task mode:"),
        Line::from("Type      set new branch name"),
        Line::from("Backspace delete character"),
        Line::from("Enter     create and open new task"),
        Line::from("Esc       return to normal"),
        Line::from(""),
        Line::from("Clone repo mode:"),
        Line::from("Type      <repo-url> [repo-key]"),
        Line::from("Backspace delete character"),
        Line::from("Ctrl-U    clear input"),
        Line::from("Enter     clone repository"),
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

fn view_title(base: &str, filter: &str) -> String {
    let trimmed = filter.trim();
    if trimmed.is_empty() {
        base.to_string()
    } else {
        format!("{base} - {trimmed}")
    }
}

/// Title for the Tasks panel, incorporating the scoped repo (if any) and the
/// active filter text.
///
/// Examples:
/// - unscoped, no filter  → "Tasks"
/// - unscoped, filter     → "Tasks - feat"
/// - scoped, no filter    → "Tasks (kakarot.chorse.space/funding/hyperion)"
/// - scoped + filter      → "Tasks (kakarot.chorse.space/funding/hyperion) - feat"
fn task_view_title(repo_scope: Option<&str>, filter: &str) -> String {
    let base = match repo_scope {
        None => "Tasks".to_string(),
        Some(repo) => format!("Tasks ({repo})"),
    };
    view_title(&base, filter)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{actions_for_mode, centered_rect, status_label, task_view_title, view_title};
    use crate::{
        runtime::task_rows::TaskStatus,
        ui::state::{InputMode, UiState, ViewMode},
    };

    mod view_title_tests {
        use super::*;

        #[test]
        fn returns_base_when_filter_empty() {
            assert_eq!(view_title("Tasks", ""), "Tasks");
        }

        #[test]
        fn returns_base_when_filter_whitespace_only() {
            assert_eq!(view_title("Tasks", "   "), "Tasks");
        }

        #[test]
        fn appends_trimmed_filter() {
            assert_eq!(view_title("Tasks", " foo "), "Tasks - foo");
        }

        #[test]
        fn handles_repos_base() {
            assert_eq!(view_title("Repos", "bar"), "Repos - bar");
        }
    }

    mod task_view_title_tests {
        use super::*;

        #[test]
        fn unscoped_no_filter() {
            assert_eq!(task_view_title(None, ""), "Tasks");
        }

        #[test]
        fn unscoped_with_filter() {
            assert_eq!(task_view_title(None, "feat"), "Tasks - feat");
        }

        #[test]
        fn scoped_no_filter() {
            assert_eq!(
                task_view_title(Some("github.com/org/repo"), ""),
                "Tasks (github.com/org/repo)"
            );
        }

        #[test]
        fn scoped_with_filter() {
            assert_eq!(
                task_view_title(Some("github.com/org/repo"), "fix"),
                "Tasks (github.com/org/repo) - fix"
            );
        }

        #[test]
        fn scoped_filter_whitespace_only() {
            assert_eq!(
                task_view_title(Some("github.com/org/repo"), "   "),
                "Tasks (github.com/org/repo)"
            );
        }
    }

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
            // Inner rect should be contained within outer
            assert!(inner.x >= outer.x);
            assert!(inner.y >= outer.y);
            assert!(inner.x + inner.width <= outer.x + outer.width);
            assert!(inner.y + inner.height <= outer.y + outer.height);
        }

        #[test]
        fn full_percent_covers_most_of_area() {
            let outer = Rect::new(0, 0, 100, 100);
            let inner = centered_rect(100, 100, outer);
            // With 100% should cover nearly all the area
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
        use super::*;

        fn state_with_mode(mode: InputMode, view: ViewMode) -> UiState {
            let mut state = UiState::new(Vec::new(), Vec::new(), None);
            state.view = view;
            state.mode = mode;
            state
        }

        #[test]
        fn normal_tasks_lists_task_actions() {
            let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("open selected task"));
            assert!(text.contains("park"));
            assert!(text.contains("finish"));
            assert!(text.contains("create new task"));
        }

        #[test]
        fn normal_repos_lists_repo_actions() {
            let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("clone repo"));
            assert!(text.contains("switch to tasks view"));
        }

        #[test]
        fn filter_mode_lists_filter_actions() {
            let state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("filter text"));
            assert!(text.contains("Esc"));
        }

        #[test]
        fn create_task_mode_lists_create_actions() {
            let state = state_with_mode(InputMode::CreateTask, ViewMode::Tasks);
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("branch name"));
            assert!(text.contains("create and open"));
        }

        #[test]
        fn clone_repo_mode_lists_clone_actions() {
            let state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(text.contains("repo-url"));
            assert!(text.contains("clone repository"));
        }

        #[test]
        fn clone_repo_mode_interpolates_input_text() {
            let mut state = state_with_mode(InputMode::CloneRepo, ViewMode::Repos);
            state.clone_input = "git@github.com:me/app.git".to_string();
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.contains("git@github.com:me/app.git"),
                "clone_input should appear in actions: {text}"
            );
        }

        #[test]
        fn filter_mode_same_actions_regardless_of_view() {
            // Filter mode actions must not vary by view — they always show the
            // same six lines regardless of whether Tasks or Repos is active.
            let tasks_state = state_with_mode(InputMode::Filter, ViewMode::Tasks);
            let repos_state = state_with_mode(InputMode::Filter, ViewMode::Repos);
            let tasks_lines: Vec<String> = actions_for_mode(&tasks_state)
                .iter()
                .map(|l| l.to_string())
                .collect();
            let repos_lines: Vec<String> = actions_for_mode(&repos_state)
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
            let state = state_with_mode(InputMode::Normal, ViewMode::Tasks);
            let lines = actions_for_mode(&state);
            let text: String = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                !text.contains("clone repo"),
                "tasks view should not include clone action: {text}"
            );
        }

        #[test]
        fn normal_repos_does_not_include_task_specific_actions() {
            let state = state_with_mode(InputMode::Normal, ViewMode::Repos);
            let lines = actions_for_mode(&state);
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
