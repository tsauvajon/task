use ratatui::style::Style;

use crate::{
    runtime::task_rows::TaskStatus, tools::opencode::status::OpenCodeState, ui::theme::Theme,
};

/// Keep only the last `/`-separated segment; return the input unchanged
/// if it has no `/`.
pub(super) fn short_last_segment(full: &str) -> &str {
    full.rsplit_once('/').map_or(full, |(_, last)| last)
}

/// First Unicode character of `s` returned as a `&str` slice (so
/// callers can use it as a header label or cell value). Preserves
/// the input lifetime: `&'static str` in, `&'static str` out — which
/// matters for column header folding. Returns `""` for empty input.
pub(super) fn first_char_str(s: &str) -> &str {
    match s.chars().next() {
        Some(c) => &s[..c.len_utf8()],
        None => "",
    }
}

/// Style for a `Session` column cell. BOLD is intentionally absent: it
/// is reserved for the highlighted row (added via
/// `row_highlight_style`) so the eye is drawn to the current
/// selection rather than every active session.
pub(super) fn session_cell_style(status: TaskStatus, theme: &Theme) -> Style {
    match status {
        TaskStatus::Open => Style::default().fg(theme.success),
        TaskStatus::Parked => Style::default().fg(theme.warning),
    }
}

pub(super) fn opencode_cell_style(state: OpenCodeState, theme: &Theme) -> Style {
    // Colour language mirrors the Session column so the Tasks view reads
    // consistently:
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

    mod first_char_str_tests {
        use super::first_char_str;

        #[test]
        fn empty_string_returns_empty() {
            assert_eq!(first_char_str(""), "");
        }

        #[test]
        fn single_ascii_char_returns_self() {
            assert_eq!(first_char_str("o"), "o");
        }

        #[test]
        fn ascii_word_returns_first_letter() {
            assert_eq!(first_char_str("open"), "o");
            assert_eq!(first_char_str("parked"), "p");
            assert_eq!(first_char_str("idle"), "i");
            assert_eq!(first_char_str("Session"), "S");
            assert_eq!(first_char_str("Agent"), "A");
        }

        #[test]
        fn multibyte_single_char_returns_self() {
            // `·` is U+00B7, two bytes in example. The slice must
            // include both bytes so the result is still valid example.
            assert_eq!(first_char_str("·"), "·");
        }

        #[test]
        fn multibyte_word_returns_first_grapheme() {
            // Mixed-byte input: the first character is `ü` (2 bytes).
            assert_eq!(first_char_str("über"), "ü");
        }

        #[test]
        fn preserves_static_lifetime() {
            // The slice must inherit the input's lifetime so it can
            // be used as a `&'static str` header label.
            const FULL: &str = "Session";
            let short: &'static str = first_char_str(FULL);
            assert_eq!(short, "S");
        }
    }

    mod session_cell_style_tests {
        use ratatui::style::Modifier;

        use super::*;

        #[test]
        fn open_uses_success_foreground_and_is_not_bold() {
            // BOLD is reserved for the highlighted row, so `open`
            // must carry colour but no bold by default.
            let theme = Theme::dark();
            let style = session_cell_style(TaskStatus::Open, &theme);
            assert_eq!(style.fg, Some(theme.success));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn parked_uses_warning_foreground_and_is_not_bold() {
            let theme = Theme::dark();
            let style = session_cell_style(TaskStatus::Parked, &theme);
            assert_eq!(style.fg, Some(theme.warning));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn no_session_status_is_bold_by_default() {
            // Bold is applied only by the row highlight; verify the
            // contract holds for every `TaskStatus` so a future edit
            // can't silently reintroduce default-bold styling.
            let theme = Theme::dark();
            for status in [TaskStatus::Open, TaskStatus::Parked] {
                let style = session_cell_style(status, &theme);
                assert!(
                    !style.add_modifier.contains(Modifier::BOLD),
                    "{status:?} must not carry BOLD by default"
                );
            }
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
        fn idle_matches_session_parked_colour_and_is_not_bold() {
            // Idle on the OpenCode column reads like `parked` in the
            // Session column: amber, not bold — the agent is waiting for
            // attention. Bold is reserved for the highlighted row.
            let theme = Theme::dark();
            let style = opencode_cell_style(OpenCodeState::Idle, &theme);
            assert_eq!(style.fg, Some(theme.warning));
            assert!(!style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn busy_matches_session_open_colour_and_is_not_bold() {
            // Busy mirrors `open` in the Session column: green, not
            // bold — the agent is actively working in the background.
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
