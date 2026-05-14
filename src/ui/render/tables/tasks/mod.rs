use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Padding},
};

use super::{
    super::scrollbar::render_scrollbar,
    cells::{opencode_cell_style, short_last_segment},
};
use crate::{
    runtime::task_rows::{TaskRow, TaskStatus},
    tools::{git::worktrees::WorktreeDiff, opencode::status::OpenCodeState},
    ui::{
        state::{TaskCardDetails, UiState},
        theme::Theme,
    },
};

const CARD_HEIGHT: usize = 4;

pub(in crate::ui::render) fn render_tasks(
    frame: &mut Frame,
    area: Rect,
    state: &mut UiState,
    theme: &Theme,
) {
    let scoped = state.task_repo_scope.is_some();
    let rows: Vec<ListItem> = state
        .task_filtered_indices
        .iter()
        .enumerate()
        .filter_map(|(visible_index, row_index)| {
            let row = state.task_rows.get(*row_index)?;
            let details = state.task_card_details_for(row);
            Some(task_card(
                row,
                &details,
                scoped,
                area.width,
                visible_index == state.task_selected,
                theme,
            ))
        })
        .collect();
    let row_count = rows.len();

    let list = List::new(rows)
        .block(
            Block::default()
                .padding(Padding::new(1, 1, 0, 0))
                .style(Style::default().bg(theme.panel_main)),
        )
        .highlight_style(theme.row_highlight_style());

    let mut list_state = ListState::default();
    if !state.task_filtered_indices.is_empty() {
        list_state.select(Some(state.task_selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);

    let visible_cards = visible_cards(area.height);
    state.set_visible_rows(visible_cards);

    if row_count > visible_cards {
        render_scrollbar(
            frame,
            area,
            state.task_selected,
            row_count,
            visible_cards,
            theme,
        );
    }
}

fn task_card<'a>(
    row: &TaskRow,
    details: &TaskCardDetails,
    scoped: bool,
    width: u16,
    selected: bool,
    theme: &Theme,
) -> ListItem<'a> {
    let inactive = is_inactive(row);
    let side_style = Style::default().fg(if inactive {
        theme.muted
    } else {
        agent_color(row.opencode, theme)
    });
    let text_style = if inactive {
        theme.muted_style()
    } else {
        theme.text_style()
    };
    let branch_style = if inactive {
        theme.muted_style()
    } else {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    };
    let agent_style = if inactive {
        theme.muted_style()
    } else {
        opencode_cell_style(row.opencode, theme)
    };
    let branch = display_branch(row, width);
    let repo = display_repo(row, scoped, width);
    let title = details
        .session_title
        .as_deref()
        .unwrap_or("No session title");
    let diff_line = diff_line(side_style, details.diff, title, inactive, theme);

    ListItem::new(vec![
        Line::from(vec![
            selection_marker(false, theme),
            side_span(side_style),
            Span::raw(" "),
            Span::styled(agent_icon(row.opencode), agent_style),
            Span::raw(" "),
            Span::styled(branch, branch_style),
        ]),
        Line::from(vec![
            selection_marker(selected, theme),
            side_span(side_style),
            Span::raw(" "),
            Span::styled(repo, text_style),
        ]),
        Line::from(diff_line_with_marker(diff_line, theme)),
        Line::from(""),
    ])
}

fn selection_marker(selected: bool, theme: &Theme) -> Span<'static> {
    if selected {
        Span::styled("▶ ", theme.text_style())
    } else {
        Span::raw("  ")
    }
}

fn diff_line_with_marker<'a>(mut diff_line: Vec<Span<'a>>, theme: &Theme) -> Vec<Span<'a>> {
    diff_line.insert(0, selection_marker(false, theme));
    diff_line
}

fn diff_line<'a>(
    side_style: Style,
    diff: WorktreeDiff,
    title: &str,
    inactive: bool,
    theme: &Theme,
) -> Vec<Span<'a>> {
    let mut spans = vec![side_span(side_style), Span::raw(" ")];
    spans.extend(diff_spans(diff, inactive, theme));
    spans.push(Span::raw(" · "));
    spans.push(Span::styled(title.to_string(), theme.muted_style()));
    spans
}

fn diff_spans<'a>(diff: WorktreeDiff, inactive: bool, theme: &Theme) -> Vec<Span<'a>> {
    let muted = theme.muted_style();
    let icon_style = if inactive {
        muted
    } else {
        Style::default().fg(theme.secondary)
    };
    if diff.is_clean() {
        return vec![Span::styled("✎", icon_style), Span::styled(" clean", muted)];
    }
    if diff.added_lines == 0 && diff.deleted_lines == 0 {
        return vec![
            Span::styled("✎", icon_style),
            Span::raw(" "),
            Span::styled(diff.label(), muted),
        ];
    }

    let addition_style = if inactive {
        muted
    } else {
        Style::default().fg(theme.success)
    };
    let deletion_style = if inactive {
        muted
    } else {
        Style::default().fg(theme.error)
    };
    let mut spans = vec![Span::styled("✎", icon_style)];
    if diff.added_lines > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("+{}", diff.added_lines),
            addition_style,
        ));
    }
    if diff.deleted_lines > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("-{}", diff.deleted_lines),
            deletion_style,
        ));
    }
    spans
}

fn visible_cards(height: u16) -> usize {
    (usize::from(height) / CARD_HEIGHT).max(1)
}

fn display_branch(row: &TaskRow, width: u16) -> String {
    if width < 44 {
        short_last_segment(row.branch.as_str()).to_string()
    } else {
        row.branch.to_string()
    }
}

fn display_repo(row: &TaskRow, scoped: bool, width: u16) -> String {
    if scoped {
        return row.path.display().to_string();
    }
    if width < 64 {
        short_last_segment(row.repo.as_str()).to_string()
    } else {
        row.repo.to_string()
    }
}

fn is_inactive(row: &TaskRow) -> bool {
    row.status == TaskStatus::Parked
        && matches!(row.opencode, OpenCodeState::None | OpenCodeState::Gone)
}

fn agent_icon(state: OpenCodeState) -> &'static str {
    match state {
        OpenCodeState::None => "○",
        OpenCodeState::Gone => "◌",
        OpenCodeState::Idle => "◆",
        OpenCodeState::Busy => "●",
        OpenCodeState::Hung => "⚠",
    }
}

fn agent_color(state: OpenCodeState, theme: &Theme) -> ratatui::style::Color {
    match state {
        OpenCodeState::None | OpenCodeState::Gone => theme.muted,
        OpenCodeState::Idle => theme.warning,
        OpenCodeState::Busy => theme.success,
        OpenCodeState::Hung => theme.error,
    }
}

fn side_span(style: Style) -> Span<'static> {
    Span::styled("▌", style)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    use super::{super::render_tasks, visible_cards};
    use crate::{
        runtime::{
            BranchName, RepoKey,
            task_rows::{TaskRow, TaskStatus},
        },
        tools::{git::worktrees::WorktreeDiff, opencode::status::OpenCodeState},
        ui::{
            state::{TaskCardDetails, UiState},
            theme::Theme,
        },
    };

    fn task_row(repo: &str, branch: &str, status: TaskStatus, opencode: OpenCodeState) -> TaskRow {
        TaskRow {
            status,
            repo: RepoKey::new(repo),
            branch: BranchName::new(branch),
            worktree_name: branch.to_string(),
            path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
            opencode,
        }
    }

    fn render_to_lines(state: &mut UiState, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, width, height);
                render_tasks(frame, area, state, &theme);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                let mut line = String::new();
                for x in 0..width {
                    line.push_str(buffer[(x, y)].symbol());
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    #[test]
    fn card_renders_branch_repo_and_agent_icon() {
        let row = task_row(
            "github.com/acme/app",
            "feat/card-view",
            TaskStatus::Open,
            OpenCodeState::Busy,
        );
        let mut state = UiState::new(vec![row], vec![], None);

        let lines = render_to_lines(&mut state, 100, 8);

        assert!(lines.iter().any(|line| line.contains("feat/card-view")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("github.com/acme/app"))
        );
        assert!(lines.iter().any(|line| line.contains("●")));
        assert!(!lines.iter().any(|line| line.contains("open · busy")));
    }

    #[test]
    fn selected_card_indicator_is_centered_on_middle_content_line() {
        let row = task_row(
            "github.com/acme/app",
            "feat/card-view",
            TaskStatus::Open,
            OpenCodeState::Busy,
        );
        let mut state = UiState::new(vec![row], vec![], None);

        let lines = render_to_lines(&mut state, 100, 8);
        let branch_line = lines
            .iter()
            .find(|line| line.contains("feat/card-view"))
            .expect("branch line");
        let repo_line = lines
            .iter()
            .find(|line| line.contains("github.com/acme/app"))
            .expect("repo line");
        let diff_line = lines
            .iter()
            .find(|line| line.contains("clean"))
            .expect("diff line");

        assert!(!branch_line.contains("▶"));
        assert!(repo_line.contains("▶"));
        assert!(!diff_line.contains("▶"));
    }

    #[test]
    fn card_renders_diff_and_session_title() {
        let row = task_row(
            "github.com/acme/app",
            "feat/card-view",
            TaskStatus::Open,
            OpenCodeState::Idle,
        );
        let path = row.path.clone();
        let mut state = UiState::new(vec![row], vec![], None);
        state.apply_task_card_details(&[(
            path,
            TaskCardDetails {
                diff: WorktreeDiff {
                    added_lines: 127,
                    deleted_lines: 23,
                    changed_files: 3,
                },
                session_title: Some("Improve compact TUI".to_string()),
            },
        )]);

        let lines = render_to_lines(&mut state, 100, 8);

        assert!(lines.iter().any(|line| line.contains("✎ +127 -23")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Improve compact TUI"))
        );
    }

    #[test]
    fn parked_task_without_agent_omits_status_pair() {
        let row = task_row(
            "github.com/acme/app",
            "feat/parked",
            TaskStatus::Parked,
            OpenCodeState::None,
        );
        let mut state = UiState::new(vec![row], vec![], None);

        let lines = render_to_lines(&mut state, 80, 8);

        assert!(lines.iter().any(|line| line.contains("feat/parked")));
        assert!(!lines.iter().any(|line| line.contains("parked · ·")));
    }

    #[test]
    fn card_does_not_repeat_status_pairs() {
        let rows = vec![
            task_row(
                "github.com/acme/app",
                "feat/hung",
                TaskStatus::Open,
                OpenCodeState::Hung,
            ),
            task_row(
                "github.com/acme/app",
                "feat/idle",
                TaskStatus::Parked,
                OpenCodeState::Idle,
            ),
        ];
        let mut state = UiState::new(rows, vec![], None);

        let lines = render_to_lines(&mut state, 100, 12);

        assert!(!lines.iter().any(|line| line.contains("open · hung")));
        assert!(!lines.iter().any(|line| line.contains("parked · idle")));
    }

    #[test]
    fn narrow_card_shortens_repo_and_branch_to_leaf_segments() {
        let row = task_row(
            "github.com/thomas.sauvajon/goto",
            "feat/example/short-desc",
            TaskStatus::Open,
            OpenCodeState::Idle,
        );
        let mut state = UiState::new(vec![row], vec![], None);

        let lines = render_to_lines(&mut state, 35, 8);

        assert!(lines.iter().any(|line| line.contains("short-desc")));
        assert!(lines.iter().any(|line| line.contains("goto")));
        assert!(!lines.iter().any(|line| line.contains("github.com")));
    }

    #[test]
    fn visible_cards_accounts_for_multiline_card_height() {
        assert_eq!(visible_cards(2), 1);
        assert_eq!(visible_cards(4), 1);
        assert_eq!(visible_cards(10), 2);
    }
}
