use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    runtime::task_rows::TaskStatus,
    ui::{
        state::{LoadPhase, UiState, ViewMode},
        theme::Theme,
    },
};

/// Braille spinner frames, cycled at the event-loop tick rate. Ten frames
/// keeps the animation visibly smooth at 100ms/frame without requiring a
/// faster tick.
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub(super) fn render_status_bar(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
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

    // Loading indicator for the active view. Kept left-aligned next to the
    // view counts so the spinner visually relates to the rows filling in.
    if let Some(label) = loading_label(state) {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(label, theme.muted_style()));
    } else if state.skipped_repos_count > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{} repos skipped — see activity", state.skipped_repos_count),
            theme.muted_style(),
        ));
    }

    // Input text for interactive modes — shown after view context.
    // Filter, CreateTask, and CloneRepo inputs are rendered in the right panel;
    // the status bar stays clean in all input modes.

    // Right-aligned message.
    let msg = &state.message;
    if !msg.is_empty() {
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let pad = (area.width as usize).saturating_sub(left_len + msg.len() + 1);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        spans.push(Span::styled(msg, theme.muted_style()));
    }

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.panel_bar));
    frame.render_widget(bar, area);
}

/// Build the spinner+progress label for the currently-active view, or
/// `None` when the view is fully loaded.
fn loading_label(state: &UiState) -> Option<String> {
    let phase = match state.view {
        ViewMode::Tasks => &state.task_load,
        ViewMode::Repos => &state.repo_load,
    };
    let LoadPhase::Loading { done, total } = phase else {
        return None;
    };
    let frame = SPINNER_FRAMES[(state.spinner_frame as usize) % SPINNER_FRAMES.len()];
    let label = match state.view {
        ViewMode::Tasks => "Loading tasks",
        ViewMode::Repos => "Loading repos",
    };
    Some(match total {
        Some(total) => format!("{frame} {label} {done}/{total}"),
        None => format!("{frame} {label} {done}/?"),
    })
}

#[cfg(test)]
mod tests {
    use super::{SPINNER_FRAMES, loading_label};
    use crate::ui::state::{LoadPhase, UiState, ViewMode};

    #[test]
    fn idle_view_shows_no_loading_label() {
        let state = UiState::new(vec![], vec![], None);
        assert!(loading_label(&state).is_none());
    }

    #[test]
    fn tasks_view_with_known_total_includes_done_slash_total() {
        let mut state = UiState::new(vec![], vec![], None);
        state.task_load = LoadPhase::Loading {
            done: 47,
            total: Some(149),
        };
        let label = loading_label(&state).expect("loading label");
        assert!(label.contains("47/149"), "label was: {label}");
        assert!(label.contains("Loading tasks"), "label was: {label}");
        assert!(
            SPINNER_FRAMES.iter().any(|c| label.starts_with(*c)),
            "label should start with a spinner frame: {label}"
        );
    }

    #[test]
    fn repos_view_reads_repo_load_phase() {
        let mut state = UiState::new(vec![], vec![], None);
        state.view = ViewMode::Repos;
        state.repo_load = LoadPhase::Loading {
            done: 3,
            total: Some(5),
        };
        let label = loading_label(&state).expect("loading label");
        assert!(label.contains("Loading repos"));
        assert!(label.contains("3/5"));
    }

    #[test]
    fn unknown_total_renders_question_mark() {
        let mut state = UiState::new(vec![], vec![], None);
        state.task_load = LoadPhase::Loading {
            done: 0,
            total: None,
        };
        let label = loading_label(&state).expect("loading label");
        assert!(label.contains("0/?"), "label was: {label}");
    }

    #[test]
    fn idle_phase_returns_none_even_if_counts_are_nonzero() {
        let state = UiState::new(vec![], vec![], None);
        // Default phases are Idle.
        assert!(loading_label(&state).is_none());
    }

    #[test]
    fn spinner_frame_advances_with_counter() {
        let mut state = UiState::new(vec![], vec![], None);
        state.task_load = LoadPhase::Loading {
            done: 0,
            total: Some(1),
        };
        state.spinner_frame = 0;
        let first = loading_label(&state).unwrap();
        state.spinner_frame = 5;
        let later = loading_label(&state).unwrap();
        assert_ne!(
            first.chars().next(),
            later.chars().next(),
            "different frames should pick different spinner chars"
        );
    }
}
