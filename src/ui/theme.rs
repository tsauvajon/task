use ratatui::style::{Color, Modifier, Style};

/// Semantic color palette for the TUI.
///
/// Every color used in rendering is sourced from this struct so that swapping
/// themes is a single-point change.
pub(super) struct Theme {
    /// Primary accent (selection highlights, key hints).
    pub accent: Color,
    /// Secondary accent (status indicators, counters).
    pub secondary: Color,
    /// Default foreground text.
    pub text: Color,
    /// Dimmed / muted text (paths, descriptions, inactive items).
    pub muted: Color,
    /// Row highlight background.
    pub highlight_bg: Color,
    /// "Open" / success state.
    pub success: Color,
    /// "Parked" / warning state.
    pub warning: Color,
    /// Error / destructive state (stuck `OpenCode` sessions, confirmation dialogs).
    pub error: Color,
    /// Informational / cyan accent (detach indicator, create mode).
    pub info: Color,

    /// Main content panel background (slightly lighter).
    pub panel_main: Color,
    /// Side panel background (actions, activity — slightly darker).
    pub panel_side: Color,
    /// Status bar background (darkest).
    pub panel_bar: Color,
    /// Help overlay background.
    pub overlay_bg: Color,
    /// Scrollbar track color.
    pub scrollbar_track: Color,
    /// Scrollbar thumb color.
    pub scrollbar_thumb: Color,
}

impl Theme {
    /// Tokyo-Night-inspired dark palette — the default.
    pub(super) const fn dark() -> Self {
        Self {
            accent: Color::Rgb(122, 162, 247),
            secondary: Color::Rgb(187, 154, 247),
            text: Color::Rgb(192, 202, 227),
            muted: Color::Rgb(100, 112, 140),
            highlight_bg: Color::Rgb(32, 35, 55),
            success: Color::Rgb(115, 218, 157),
            warning: Color::Rgb(224, 175, 104),
            error: Color::Rgb(247, 118, 142),
            info: Color::Rgb(125, 207, 255),

            panel_main: Color::Rgb(24, 25, 38),
            panel_side: Color::Rgb(18, 19, 30),
            panel_bar: Color::Rgb(14, 14, 22),
            overlay_bg: Color::Rgb(20, 20, 32),
            scrollbar_track: Color::Rgb(30, 32, 48),
            scrollbar_thumb: Color::Rgb(60, 65, 90),
        }
    }

    /// Plain text in the default foreground.
    pub(super) fn text_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    /// Dimmed / secondary text.
    pub(super) fn muted_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    /// Style for a keybind label in the actions / help panels.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "kept for help/action key styling consistency")
    )]
    pub(super) fn key_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Style for the description next to a keybind.
    pub(super) fn key_desc_style(&self) -> Style {
        self.muted_style()
    }

    /// Table header row.
    pub(super) fn header_style(&self) -> Style {
        Style::default().fg(self.muted).add_modifier(Modifier::BOLD)
    }

    /// Highlighted (selected) table row.
    pub(super) fn row_highlight_style(&self) -> Style {
        Style::default()
            .bg(self.highlight_bg)
            .add_modifier(Modifier::BOLD)
    }

    /// Title on a panel (rendered as a line, not a block title).
    pub(super) fn title_style(&self) -> Style {
        Style::default().fg(self.text).add_modifier(Modifier::BOLD)
    }

    /// Counter / secondary info in titles.
    pub(super) fn title_counter_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    /// The semantic color for a given input mode.
    pub(super) const fn mode_color(&self, mode: super::state::InputMode) -> Color {
        match mode {
            super::state::InputMode::Normal => self.accent,
            super::state::InputMode::Filter => self.secondary,
            super::state::InputMode::CreateTask | super::state::InputMode::CloneRepo => self.info,
        }
    }

    /// Mode indicator badge.
    pub(super) fn mode_style(&self, mode: super::state::InputMode) -> Style {
        Style::default()
            .fg(self.mode_color(mode))
            .add_modifier(Modifier::BOLD)
    }

    /// Block cursor style for text input fields.
    ///
    /// Renders a solid block in the mode's accent color, mimicking
    /// `OpenCode`'s "big square cursor" look.
    pub(super) fn cursor_style(&self, mode: super::state::InputMode) -> Style {
        Style::default()
            .bg(self.mode_color(mode))
            .fg(self.panel_bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::InputMode;

    #[test]
    fn dark_theme_constructs_without_panic() {
        let _theme = Theme::dark();
    }

    #[test]
    fn mode_style_is_bold() {
        let theme = Theme::dark();
        for mode in [
            InputMode::Normal,
            InputMode::Filter,
            InputMode::CreateTask,
            InputMode::CloneRepo,
        ] {
            let style = theme.mode_style(mode);
            assert!(
                style.add_modifier.contains(Modifier::BOLD),
                "mode {mode:?} should be bold"
            );
        }
    }

    #[test]
    fn key_style_is_not_bold() {
        let theme = Theme::dark();
        let style = theme.key_style();
        assert!(
            !style.add_modifier.contains(Modifier::BOLD),
            "key_style should not be bold"
        );
    }
}
