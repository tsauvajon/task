use ratatui::style::Style;

use crate::{tools::opencode::status::OpenCodeState, ui::theme::Theme};

/// Keep only the last `/`-separated segment; return the input unchanged
/// if it has no `/`.
pub(super) fn short_last_segment(full: &str) -> &str {
    full.rsplit_once('/').map_or(full, |(_, last)| last)
}

pub(super) fn opencode_cell_style(state: OpenCodeState, theme: &Theme) -> Style {
    // Colour language keeps the Tasks view readable at a glance:
    //   - `idle` (amber) — ready for attention.
    //   - `busy` (green) — background work.
    //   - `hung` uses the error colour (salmon) — needs a look.
    //   - `gone`/`·` stay muted (dim blue-grey).
    //
    // BOLD is intentionally not applied here: it is reserved for the
    // highlighted row (added via `row_highlight_style`) so the eye is
    // drawn to the current selection rather than every active agent.
    match state {
        OpenCodeState::None | OpenCodeState::Gone => theme.muted_style(),
        OpenCodeState::Busy => Style::default().fg(theme.success),
        OpenCodeState::Idle => Style::default().fg(theme.warning),
        OpenCodeState::Hung => Style::default().fg(theme.error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod short_last_segment_tests {
        use super::short_last_segment;

        #[test]
        fn returns_segment_after_last_slash() {
            assert_eq!(
                short_last_segment("github.com/thomas.sauvajon/goto"),
                "goto"
            );
        }

        #[test]
        fn returns_input_unchanged_when_no_slash() {
            assert_eq!(short_last_segment("goto"), "goto");
        }

        #[test]
        fn strips_multi_level_path() {
            assert_eq!(short_last_segment("feat/example/short-desc"), "short-desc");
        }

        #[test]
        fn empty_input_returns_empty() {
            assert_eq!(short_last_segment(""), "");
        }

        #[test]
        fn trailing_slash_yields_empty_segment() {
            // This is a degenerate case. We opt for fidelity over
            // heuristics: if the caller passed a trailing slash, the
            // last segment is the empty string. Pragmatically neither
            // repo keys nor branch names ever end with `/`, so this
            // branch is exercised by tests alone.
            assert_eq!(short_last_segment("foo/"), "");
        }
    }

    mod opencode_cell_style_tests {
        use ratatui::style::Modifier;

        use super::*;

        #[test]
        fn hung_uses_error_foreground_and_is_not_bold() {
            // BOLD is reserved for the highlighted row (applied by
            // `row_highlight_style`), so no Agent state is bold by
            // default — including `hung`.
            let theme = Theme::dark();
            let style = opencode_cell_style(OpenCodeState::Hung, &theme);
            assert_eq!(style.fg, Some(theme.error));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn idle_uses_warning_foreground_and_is_not_bold() {
            // Amber signals that the agent is waiting for attention.
            let theme = Theme::dark();
            let style = opencode_cell_style(OpenCodeState::Idle, &theme);
            assert_eq!(style.fg, Some(theme.warning));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn busy_uses_success_foreground_and_is_not_bold() {
            // Green signals background work.
            let theme = Theme::dark();
            let style = opencode_cell_style(OpenCodeState::Busy, &theme);
            assert_eq!(style.fg, Some(theme.success));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn no_agent_state_is_bold_by_default() {
            // Bold is applied only by the row highlight; verify the
            // contract holds for every Agent state at once so a future
            // edit can't silently reintroduce default-bold styling.
            let theme = Theme::dark();
            for state in [
                OpenCodeState::None,
                OpenCodeState::Gone,
                OpenCodeState::Busy,
                OpenCodeState::Idle,
                OpenCodeState::Hung,
            ] {
                let style = opencode_cell_style(state, &theme);
                assert!(
                    !style.add_modifier.contains(Modifier::BOLD),
                    "{state:?} must not carry BOLD by default"
                );
            }
        }

        #[test]
        fn shut_is_muted_like_none() {
            let theme = Theme::dark();
            assert_eq!(
                opencode_cell_style(OpenCodeState::Gone, &theme),
                opencode_cell_style(OpenCodeState::None, &theme),
            );
        }
    }
}
