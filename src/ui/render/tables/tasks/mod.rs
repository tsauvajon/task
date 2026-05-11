use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Cell, Padding, Row, Table, TableState},
};

use self::compact::CellLabel;
use super::{
    super::scrollbar::render_scrollbar,
    cells::{opencode_cell_style, session_cell_style, short_last_segment},
    column_layout::{AGENT_COLUMN, SESSION_COLUMN, pick_task_column_layout, table_chrome_overhead},
};
use crate::ui::{state::UiState, theme::Theme};

mod compact;

pub(in crate::ui::render) fn render_tasks(
    frame: &mut Frame,
    area: Rect,
    state: &mut UiState,
    theme: &Theme,
) {
    let scoped = state.task_repo_scope.is_some();

    // Precompute max widths for the flex columns so we can pick the
    // densest layout that fits. `.max(header.len())` ensures each
    // column can at least display its header.
    let visible_rows_iter = || {
        state
            .task_filtered_indices
            .iter()
            .filter_map(|i| state.task_rows.get(*i))
    };
    let max_repo_full = visible_rows_iter()
        .map(|row| row.repo.len())
        .max()
        .unwrap_or(0)
        .max("Repo".len()) as u16;
    let max_repo_short = visible_rows_iter()
        .map(|row| short_last_segment(row.repo.as_str()).len())
        .max()
        .unwrap_or(0)
        .max("Repo".len()) as u16;
    let max_branch_full = visible_rows_iter()
        .map(|row| row.branch.len())
        .max()
        .unwrap_or(0)
        .max("Branch".len()) as u16;
    let max_branch_short = visible_rows_iter()
        .map(|row| short_last_segment(row.branch.as_str()).len())
        .max()
        .unwrap_or(0)
        .max("Branch".len()) as u16;

    // Pick the layout using the widest possible column count so we
    // pick the densest layout that fits. `table_chrome_overhead`
    // already accounts for *every* inter-column gap, so subtracting
    // only the fixed-width column contents (Session + Agent) leaves the
    // raw content budget for Repo + Branch. The result is signed so
    // sub-baseline widths preserve their deficit; saturating to zero
    // here would let `pick_task_column_layout` overestimate cells
    // reclaimed by compact columns and still truncate the branch.
    let max_column_count: u16 = if scoped { 3 } else { 4 };
    let content_width = i32::from(area.width)
        - i32::from(table_chrome_overhead(max_column_count))
        - i32::from(SESSION_COLUMN.full_width + AGENT_COLUMN.full_width);

    let layout = pick_task_column_layout(
        scoped,
        content_width,
        max_repo_full,
        max_repo_short,
        max_branch_full,
        max_branch_short,
    );

    // The actual rendered column count depends on which columns the
    // picked layout shows. Branch and Agent are always rendered;
    // Repo and Session are conditional. Used below to size the header /
    // cell vectors.
    let mut column_count: u16 = 2; // Branch + Agent
    if layout.shows_repo() {
        column_count += 1;
    }
    if layout.shows_session() {
        column_count += 1;
    }

    let mut header_labels: Vec<&str> = Vec::with_capacity(column_count as usize);
    if layout.shows_repo() {
        header_labels.push("Repo");
    }
    header_labels.push("Branch");
    if layout.shows_session() {
        header_labels.push(SESSION_COLUMN.header(layout.compact_session()));
    }
    header_labels.push(AGENT_COLUMN.header(layout.compact_agent()));
    let header = Row::new(header_labels)
        .style(theme.header_style())
        .bottom_margin(0);

    let rows: Vec<Row> = state
        .task_filtered_indices
        .iter()
        .filter_map(|index| {
            let row = state.task_rows.get(*index)?;
            // Session and Agent cells deliberately don't carry BOLD by
            // default. Bold is reserved for the highlighted row (added
            // via `row_highlight_style`) so the eye is drawn to the
            // current selection rather than every active session.
            let repo_style = theme.text_style();
            let branch_style = Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD);
            let opencode_style = opencode_cell_style(row.opencode, theme);
            let opencode_cell =
                Cell::from(row.opencode.label(layout.compact_agent())).style(opencode_style);

            let branch_text = if layout.shortens_branch() {
                short_last_segment(row.branch.as_str()).to_string()
            } else {
                row.branch.to_string()
            };
            let branch_cell = Cell::from(branch_text).style(branch_style);

            let mut cells: Vec<Cell> = Vec::with_capacity(column_count as usize);
            if layout.shows_repo() {
                let repo_text = if layout.shortens_repo() {
                    short_last_segment(row.repo.as_str()).to_string()
                } else {
                    row.repo.to_string()
                };
                cells.push(Cell::from(repo_text).style(repo_style));
            }
            cells.push(branch_cell);
            if layout.shows_session() {
                let session_style = session_cell_style(row.status, theme);
                let session_cell =
                    Cell::from(row.status.label(layout.compact_session())).style(session_style);
                cells.push(session_cell);
            }
            cells.push(opencode_cell);
            Some(Row::new(cells))
        })
        .collect();

    let row_count = rows.len();

    // Column widths track `layout`. The Branch column is `Fill(1)` when
    // the Repo column is hidden so it absorbs the freed-up space;
    // otherwise Repo is `Fill(1)` (flex) and Branch is a tight
    // `Length` of the longest currently-rendered branch string.
    let branch_display_width = if layout.shortens_branch() {
        max_branch_short
    } else {
        max_branch_full
    };
    let mut widths: Vec<Constraint> = Vec::with_capacity(column_count as usize);
    if layout.shows_repo() {
        widths.push(Constraint::Fill(1));
        widths.push(Constraint::Length(branch_display_width));
    } else {
        // Branch absorbs the freed Repo column.
        widths.push(Constraint::Fill(1));
    }
    if layout.shows_session() {
        widths.push(Constraint::Length(
            SESSION_COLUMN.width(layout.compact_session()),
        ));
    }
    widths.push(Constraint::Length(
        AGENT_COLUMN.width(layout.compact_agent()),
    ));

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

#[cfg(test)]
mod tests {
    mod scoped_tasks_view {
        use std::path::PathBuf;

        use crate::{
            runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            },
            tools::opencode::status::OpenCodeState,
            ui::state::UiState,
        };

        fn task_row(repo: &str, branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                worktree_name: branch.to_string(),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
                opencode: OpenCodeState::None,
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

    mod render_tasks_smoke {
        //! Smoke tests for `render_tasks` — draws the widget into a
        //! `TestBackend` and asserts the header reflects the new
        //! Agent column. These tests don't care about exact cell
        //! widths or styling (that's covered by `opencode_cell_style`);
        //! they only pin the high-level header structure so the
        //! column rename (`Path` → `Agent`) can't silently regress.
        use std::path::PathBuf;

        use ratatui::{Terminal, backend::TestBackend, layout::Rect};

        use super::super::render_tasks;
        use crate::{
            runtime::{
                BranchName, RepoKey,
                task_rows::{TaskRow, TaskStatus},
            },
            tools::opencode::status::OpenCodeState,
            ui::{state::UiState, theme::Theme},
        };

        fn task_row(repo: &str, branch: &str, opencode: OpenCodeState) -> TaskRow {
            TaskRow {
                status: TaskStatus::Open,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                worktree_name: branch.to_string(),
                path: PathBuf::from(format!("/tmp/{repo}/{branch}")),
                opencode,
            }
        }

        /// Render into a `TestBackend` and return the flattened line
        /// text, one row per terminal line.
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
        fn unscoped_header_has_session_just_left_of_agent() {
            let row = task_row("github.com/acme/app", "main", OpenCodeState::Idle);
            let mut state = UiState::new(vec![row], vec![], None);

            let lines = render_to_lines(&mut state, 120, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Session"))
                .expect("header line must be rendered");

            assert!(
                header.contains("Repo"),
                "unscoped header has Repo: {header}"
            );
            assert!(
                header.contains("Branch"),
                "unscoped header has Branch: {header}"
            );
            assert!(
                header.contains("Session"),
                "unscoped header has Session: {header}"
            );
            // "Agent" is 5 chars and the column is also 5 cells wide,
            // so the header fits exactly — no truncation.
            assert!(
                header.contains("Agent"),
                "unscoped header has Agent: {header}"
            );
            assert!(
                !header.contains("Status"),
                "unscoped header must not have legacy Status column: {header}"
            );
            assert!(
                !header.contains("Path"),
                "unscoped header must not have Path column: {header}"
            );

            // Column order: Repo, Branch, Session, Agent. Session must sit
            // immediately to the left of Agent (the feature request).
            let session_pos = header.find("Session").expect("Session in header");
            let agent_pos = header.find("Agent").expect("Agent in header");
            let repo_pos = header.find("Repo").expect("Repo in header");
            let branch_pos = header.find("Branch").expect("Branch in header");
            assert!(
                repo_pos < branch_pos,
                "Repo must come before Branch: {header}"
            );
            assert!(
                branch_pos < session_pos,
                "Branch must come before Session: {header}"
            );
            assert!(
                session_pos < agent_pos,
                "Session must come before Agent: {header}"
            );
        }

        #[test]
        fn scoped_header_has_session_just_left_of_agent() {
            let row = task_row("github.com/acme/app", "main", OpenCodeState::Idle);
            let mut state =
                UiState::new(vec![row], vec![], Some("github.com/acme/app".to_string()));

            let lines = render_to_lines(&mut state, 80, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Session"))
                .expect("header line must be rendered");

            assert!(
                header.contains("Branch"),
                "scoped header has Branch: {header}"
            );
            assert!(
                header.contains("Session"),
                "scoped header has Session: {header}"
            );
            assert!(
                header.contains("Agent"),
                "scoped header has Agent: {header}"
            );
            assert!(
                !header.contains("Status"),
                "scoped header must not have legacy Status column: {header}"
            );
            assert!(
                !header.contains("Path"),
                "scoped header must not have Path column: {header}"
            );
            // Repo column is dropped in scoped mode because every row
            // belongs to the same repo.
            assert!(
                !header.contains("Repo"),
                "scoped header must not have Repo column: {header}"
            );

            // Column order: Branch, Session, Agent. Session must sit
            // immediately to the left of Agent.
            let branch_pos = header.find("Branch").expect("Branch in header");
            let session_pos = header.find("Session").expect("Session in header");
            let agent_pos = header.find("Agent").expect("Agent in header");
            assert!(
                branch_pos < session_pos,
                "Branch must come before Session: {header}"
            );
            assert!(
                session_pos < agent_pos,
                "Session must come before Agent: {header}"
            );
        }

        #[test]
        fn hung_row_renders_hung_label() {
            let row = task_row("github.com/acme/app", "main", OpenCodeState::Hung);
            let mut state = UiState::new(vec![row], vec![], None);

            let lines = render_to_lines(&mut state, 120, 8);
            // The `hung` label is 4 chars and is rendered in the
            // rightmost Agent column (5 cells). Find it on any
            // non-header line.
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains("hung") && !l.contains("Session")),
                "expected a row to render 'hung': {lines:?}"
            );
        }

        #[test]
        fn wide_terminal_shows_full_repo_path() {
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "main",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            // 120 cells — plenty of room for the full repo path.
            let lines = render_to_lines(&mut state, 120, 8);
            let data = lines
                .iter()
                .find(|l| l.contains("goto") && !l.contains("Repo"))
                .expect("data line rendered");
            assert!(
                data.contains("github.com/thomas.sauvajon/goto"),
                "wide terminal must show full repo path: {data}"
            );
        }

        #[test]
        fn narrow_terminal_shortens_repo_to_last_segment() {
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "main",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            // 35 cells — too narrow for the 30-char repo path next to
            // Branch + Session + Agent, but wide enough for "goto" alone.
            let lines = render_to_lines(&mut state, 35, 8);
            let data = lines
                .iter()
                .find(|l| l.contains("goto") && !l.contains("Repo"))
                .expect("data line rendered");
            assert!(
                !data.contains("github.com"),
                "narrow terminal must shorten repo path: {data}"
            );
            assert!(
                data.contains("goto"),
                "narrow terminal must still show the repo name: {data}"
            );
        }

        #[test]
        fn very_narrow_terminal_shortens_branch_to_last_segment() {
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "feat/example/short-desc",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            // 30 cells — too narrow to keep the Repo column at all,
            // and too narrow for the full 24-char branch next to Session
            // + Agent + chrome. Expect the branch to collapse to its
            // last `/`-segment, but Session must still be visible at
            // this width.
            let lines = render_to_lines(&mut state, 30, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                header.contains("Session"),
                "Session column must still be visible at width 30: {header}"
            );
            let data = lines
                .iter()
                .find(|l| l.contains("short-desc"))
                .expect("data line rendered");
            assert!(
                !data.contains("example"),
                "very narrow terminal must shorten branch name: {data}"
            );
            assert!(
                data.contains("short-desc"),
                "very narrow terminal must still show the branch leaf: {data}"
            );
        }

        #[test]
        fn extremely_narrow_terminal_folds_session_to_compact() {
            // Once the terminal is too narrow to fit the branch leaf
            // alongside the full Session column, the column folds to a
            // single cell — `T` header and `o` (open) / `p` (parked)
            // labels — instead of disappearing immediately. This
            // keeps a glanceable session indicator next to the
            // branch leaf for as long as possible.
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "feat/example/short-desc",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            // 25 cells — too narrow for the 10-char branch leaf
            // alongside the full 8-cell Session column, but wide enough
            // for the compact 1-cell variant.
            let lines = render_to_lines(&mut state, 25, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "full Session header must fold to compact at width 25: {header}"
            );
            assert!(
                header.contains("Branch"),
                "Branch column must remain at width 25: {header}"
            );
            assert!(
                header.contains("Agent"),
                "Agent column must remain at width 25: {header}"
            );

            let data = lines
                .iter()
                .find(|l| l.contains("short-desc"))
                .expect("branch leaf must still render at width 25");
            assert!(
                !data.contains("open"),
                "full 'open' label must not render in compact mode: {data}"
            );
            assert!(
                !data.contains("parked"),
                "full 'parked' label must not render in compact mode: {data}"
            );
        }

        #[test]
        fn even_narrower_terminal_folds_agent_to_compact() {
            // Once compact Session alone can't keep the branch leaf in
            // frame, the Agent column folds to a single cell as
            // well. Session stays compact rather than disappearing —
            // the next degradation step is Session dropping while
            // Agent stays compact.
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "feat/example/short-desc",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            // 21 cells — content budget is 1, compact-Session budget is
            // 8, all-compact budget is 12 — the 10-char leaf needs
            // both columns compact to fit.
            let lines = render_to_lines(&mut state, 21, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "full Session header must fold to compact at width 21: {header}"
            );
            assert!(
                !header.contains("Agent"),
                "full Agent header must fold to compact at width 21: {header}"
            );
            assert!(
                header.contains("Branch"),
                "Branch column must remain at width 21: {header}"
            );

            let data = lines
                .iter()
                .find(|l| l.contains("short-desc"))
                .expect("branch leaf must still render at width 21");
            assert!(
                !data.contains("open"),
                "full 'open' label must not render in compact mode: {data}"
            );
            assert!(
                !data.contains("idle"),
                "full 'idle' Agent label must not render in compact mode: {data}"
            );
        }

        #[test]
        fn sub_baseline_width_drops_session_instead_of_picking_all_compact() {
            // Regression: at width 17 the table chrome (7 cells) +
            // full Session (8) + full Agent (5) already overshoot the
            // available cells by 3, leaving a -3 content budget.
            // Earlier code saturated that to 0 and then mistakenly
            // added compact-column savings on top, picking
            // `NoRepoBranchShortAllCompact` even though the rendered
            // table still couldn't fit the 10-char branch leaf.
            // With the deficit preserved through signed arithmetic,
            // Session must drop entirely and the branch leaf must
            // render in full.
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "feat/example/short-desc",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            let lines = render_to_lines(&mut state, 17, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "Session column must drop at sub-baseline width 17: {header}"
            );
            assert!(
                !header.contains("Agent"),
                "Agent column must stay compact (never un-folds): {header}"
            );
            assert!(
                header.contains("Branch"),
                "Branch column must remain at width 17: {header}"
            );

            let data = lines
                .iter()
                .find(|l| l.contains("short-desc"))
                .expect("branch leaf must render at sub-baseline width 17");
            assert!(
                data.contains("short-desc"),
                "full 10-char branch leaf must render: {data}"
            );
        }

        #[test]
        fn extremely_narrow_terminal_drops_session_with_agent_already_compact() {
            // Once even all-compact (Session + Agent both folded) can't
            // keep the branch leaf in frame, Session disappears
            // entirely. Agent stays compact — once a column is
            // folded it never un-folds.
            let row = task_row(
                "github.com/thomas.sauvajon/goto",
                "feat/example/this-is-a-long-leaf",
                OpenCodeState::Idle,
            );
            let mut state = UiState::new(vec![row], vec![], None);

            // 25 cells with a 19-char branch leaf — content budget
            // is 5, all-compact budget is 17, still 2 short of the
            // 19-char leaf. Session must drop.
            let lines = render_to_lines(&mut state, 25, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "Session column must be gone at this width: {header}"
            );
            assert!(
                !header.contains("Agent"),
                "Agent column must stay compact (never un-folds): {header}"
            );
            assert!(
                header.contains("Branch"),
                "Branch column must remain: {header}"
            );

            let data = lines
                .iter()
                .find(|l| l.contains("this-is-a-long-leaf"))
                .expect("branch leaf must still render");
            assert!(
                !data.contains("open"),
                "full 'open' label must not render once Session is dropped: {data}"
            );
            assert!(
                !data.contains("parked"),
                "full 'parked' label must not render once Session is dropped: {data}"
            );
            assert!(
                !data.contains("idle"),
                "full 'idle' Agent label must not render once Agent is compact: {data}"
            );
        }

        #[test]
        fn scoped_narrow_folds_session_to_compact() {
            // Scoped mode follows the same fold-before-drop policy.
            let row = task_row(
                "github.com/acme/app",
                "feat/example/short-desc",
                OpenCodeState::Idle,
            );
            let mut state =
                UiState::new(vec![row], vec![], Some("github.com/acme/app".to_string()));

            // 24 cells in scoped mode — content budget is 5 (full
            // Session can't fit the 10-char leaf) but compact budget is
            // 12 (leaf fits).
            let lines = render_to_lines(&mut state, 24, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "full Session header must fold to compact in scoped mode: {header}"
            );
            assert!(
                header.contains("Agent"),
                "Agent column must remain in scoped mode: {header}"
            );
        }

        #[test]
        fn scoped_extremely_narrow_folds_agent_to_compact() {
            // Scoped mode follows the same fold-Agent-before-drop
            // policy as unscoped mode. Once compact Session alone can't
            // fit the branch leaf, the Agent column folds too.
            let row = task_row(
                "github.com/acme/app",
                "feat/example/short-desc",
                OpenCodeState::Idle,
            );
            let mut state =
                UiState::new(vec![row], vec![], Some("github.com/acme/app".to_string()));

            // 18 cells in scoped mode — content budget is 0, compact
            // Session budget is 7, all-compact budget is 11. The 10-char
            // leaf needs both columns compact.
            let lines = render_to_lines(&mut state, 18, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "full Session header must fold to compact in scoped mode: {header}"
            );
            assert!(
                !header.contains("Agent"),
                "full Agent header must fold to compact in scoped mode: {header}"
            );
            assert!(
                header.contains("Branch"),
                "Branch column must remain in scoped mode: {header}"
            );
        }

        #[test]
        fn scoped_drops_session_when_even_all_compact_does_not_fit() {
            // Scoped mode drops Session entirely once even all-compact
            // (Session + Agent both folded) can't fit the branch leaf.
            let row = task_row(
                "github.com/acme/app",
                "feat/example/much-longer-branch",
                OpenCodeState::Idle,
            );
            let mut state =
                UiState::new(vec![row], vec![], Some("github.com/acme/app".to_string()));

            // 18 cells in scoped mode with an 18-char leaf
            // ("much-longer-branch") — content budget is 0,
            // all-compact budget is 11, both short of 18. Session drops.
            let lines = render_to_lines(&mut state, 18, 8);
            let header = lines
                .iter()
                .find(|l| l.contains("Branch"))
                .expect("header line must be rendered");
            assert!(
                !header.contains("Session"),
                "Session column must be gone in scoped mode: {header}"
            );
            assert!(
                !header.contains("Agent"),
                "Agent column must stay compact (never un-folds): {header}"
            );
            assert!(
                header.contains("Branch"),
                "Branch column must remain in scoped mode: {header}"
            );
        }
    }
}
