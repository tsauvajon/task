use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Padding},
};

use self::compact::CellLabel;
use super::{
    super::scrollbar::render_scrollbar,
    cells::{opencode_cell_style, session_cell_style, short_last_segment},
};
use crate::{
    runtime::task_rows::{TaskRow, TaskStatus},
    tools::opencode::status::OpenCodeState,
    ui::{
        state::{TaskCardDetails, UiState},
        theme::Theme,
    },
};

mod compact;

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
        .filter_map(|index| {
            let row = state.task_rows.get(*index)?;
            let details = state.task_card_details_for(row);
            Some(task_card(row, &details, scoped, area.width, theme))
        })
        .collect();
    let row_count = rows.len();

    let list = List::new(rows)
        .block(
            Block::default()
                .padding(Padding::new(1, 1, 0, 0))
                .style(Style::default().bg(theme.panel_main)),
        )
        .highlight_style(theme.row_highlight_style())
        .highlight_symbol("▶ ");

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
    let session_style = if inactive {
        theme.muted_style()
    } else {
        session_cell_style(row.status, theme)
    };
    let agent_style = if inactive {
        theme.muted_style()
    } else {
        opencode_cell_style(row.opencode, theme)
    };
    let diff_style = if details.diff.is_clean() {
        theme.muted_style()
    } else {
        Style::default().fg(theme.secondary)
    };

    let branch = display_branch(row, width);
    let repo = display_repo(row, scoped, width);
    let title = details
        .session_title
        .as_deref()
        .unwrap_or("No session title");

    ListItem::new(vec![
        Line::from(vec![
            side_span(side_style),
            Span::raw(" "),
            Span::styled(agent_icon(row.opencode), agent_style),
            Span::raw(" "),
            Span::styled(branch, branch_style),
            Span::raw("  "),
            Span::styled(row.status.label(false), session_style),
            Span::raw(" · "),
            Span::styled(row.opencode.label(false), agent_style),
        ]),
        Line::from(vec![
            side_span(side_style),
            Span::raw(" "),
            Span::styled(repo, text_style),
        ]),
        Line::from(vec![
            side_span(side_style),
            Span::raw(" Δ "),
            Span::styled(details.diff.label(), diff_style),
            Span::raw(" · "),
            Span::styled(title.to_string(), theme.muted_style()),
        ]),
        Line::from(""),
    ])
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
    fn card_renders_branch_repo_agent_and_session_state() {
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
        assert!(lines.iter().any(|line| line.contains("open · busy")));
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
                    added: 1,
                    modified: 2,
                    untracked: 1,
                    ..WorktreeDiff::default()
                },
                session_title: Some("Improve compact TUI".to_string()),
            },
        )]);

        let lines = render_to_lines(&mut state, 100, 8);

        assert!(lines.iter().any(|line| line.contains("+1 ~2 ?1")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Improve compact TUI"))
        );
    }

    #[test]
    fn parked_task_without_agent_renders_muted_status_text() {
        let row = task_row(
            "github.com/acme/app",
            "feat/parked",
            TaskStatus::Parked,
            OpenCodeState::None,
        );
        let mut state = UiState::new(vec![row], vec![], None);

        let lines = render_to_lines(&mut state, 80, 8);

        assert!(lines.iter().any(|line| line.contains("parked · ·")));
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
