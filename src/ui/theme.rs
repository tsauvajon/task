use ratatui::style::{Color, Modifier, Style};

/// Semantic color palette for the TUI.
///
/// Every color used in rendering is sourced from this struct so that swapping
/// themes is a single-point change.
pub(super) struct Theme {
    /// Primary accent (selection highlights, active borders, key hints).
    pub accent: Color,
    /// Secondary accent (status indicators, counters).
    pub secondary: Color,
    /// Default foreground text.
    pub text: Color,
    /// Dimmed / muted text (paths, descriptions, inactive items).
    pub muted: Color,
    /// Panel / overlay background (slightly off-black).
    pub surface: Color,
    /// Row highlight background.
    pub highlight_bg: Color,
    /// Default border color.
    pub border: Color,
    /// Border color for the active / focused panel.
    pub border_active: Color,
    /// "Open" / success state.
    pub success: Color,
    /// "Parked" / warning state.
    pub warning: Color,
    /// Error / destructive state (e.g. future confirmation dialogs).
    #[expect(dead_code)]
    pub error: Color,
    /// Informational / cyan accent (detach indicator, create mode).
    pub info: Color,
    /// Help overlay background.
    pub overlay_bg: Color,
}

impl Theme {
    /// Tokyo-Night-inspired dark palette — the default.
    pub fn dark() -> Self {
        Self {
            accent: Color::Rgb(122, 162, 247),        // soft blue
            secondary: Color::Rgb(187, 154, 247),     // purple
            text: Color::Rgb(192, 202, 227),          // light grey-blue
            muted: Color::Rgb(100, 112, 140),         // dim blue-grey
            surface: Color::Rgb(22, 22, 35),          // near-black
            highlight_bg: Color::Rgb(32, 35, 55),     // subtle navy
            border: Color::Rgb(55, 60, 80),           // muted border
            border_active: Color::Rgb(122, 162, 247), // matches accent
            success: Color::Rgb(115, 218, 157),       // green
            warning: Color::Rgb(224, 175, 104),       // amber
            error: Color::Rgb(247, 118, 142),         // salmon-red
            info: Color::Rgb(125, 207, 255),          // sky blue
            overlay_bg: Color::Rgb(18, 18, 28),       // darker than surface
        }
    }

    // ── Convenience style constructors ────────────────────────────────────

    /// Plain text in the default foreground.
    pub fn text_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    /// Dimmed / secondary text.
    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    /// Style for a keybind label in the actions / help panels.
    pub fn key_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for the description next to a keybind.
    pub fn key_desc_style(&self) -> Style {
        self.muted_style()
    }

    /// Table header row.
    pub fn header_style(&self) -> Style {
        Style::default().fg(self.muted).add_modifier(Modifier::BOLD)
    }

    /// Highlighted (selected) table row.
    pub fn row_highlight_style(&self) -> Style {
        Style::default()
            .bg(self.highlight_bg)
            .add_modifier(Modifier::BOLD)
    }

    /// Block border (inactive).
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Block border (active / focused).
    pub fn border_active_style(&self) -> Style {
        Style::default().fg(self.border_active)
    }

    /// Title on a block (non-focused).
    pub fn title_style(&self) -> Style {
        Style::default().fg(self.text).add_modifier(Modifier::BOLD)
    }

    /// Counter / secondary info in titles.
    pub fn title_counter_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    /// Mode indicator badge.
    pub fn mode_style(&self, mode: super::state::InputMode) -> Style {
        let color = match mode {
            super::state::InputMode::Normal => self.success,
            super::state::InputMode::Filter => self.warning,
            super::state::InputMode::CreateTask => self.info,
            super::state::InputMode::CloneRepo => self.secondary,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
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
    fn mode_style_returns_bold_for_all_modes() {
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
}
