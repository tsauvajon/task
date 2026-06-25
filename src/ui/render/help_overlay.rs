use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};

use crate::ui::{
    state::{InputMode, UiState},
    theme::Theme,
};

pub(super) fn render_help(frame: &mut Frame, state: &mut UiState, theme: &Theme) {
    let popup = help_popup_rect(frame.area());
    state.help_area = Some(popup);

    let pad_h: u16 = 4;
    let inner_w = usize::from(popup.width.saturating_sub(pad_h.saturating_mul(2)));
    let lines = help_lines(theme, inner_w);

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

fn help_lines(theme: &Theme, inner_w: usize) -> Vec<Line<'static>> {
    let normal_c = theme.mode_color(InputMode::Normal);
    let filter_c = theme.mode_color(InputMode::Filter);
    let create_c = theme.mode_color(InputMode::CreateTask);
    let clone_c = theme.mode_color(InputMode::CloneRepo);
    let desc_style = theme.key_desc_style();

    let section = |title: &str, color: Color| -> Line<'static> {
        Line::from(Span::styled(
            title.to_owned(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
    };

    let cmd_style = Style::default().fg(theme.text);
    let hk = move |key: &str, desc: &str| -> Line<'static> {
        let gap = inner_w.saturating_sub(desc.chars().count().saturating_add(key.chars().count()));
        Line::from(vec![
            Span::styled(desc.to_owned(), cmd_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(key.to_owned(), desc_style),
        ])
    };

    let mut lines = vec![
        section("All views", normal_c),
        hk("tab", "Switch tasks/repos view"),
        hk("/", "Enter filter mode"),
        hk("ctrl-u/d", "Half page up/down"),
        hk("click", "Select row"),
        hk("b", "Toggle sidebar"),
        hk("ctrl+p", "Commands"),
        hk("q/ctrl-c", "Quit"),
        Line::from(""),
        section("Tasks view", normal_c),
        hk("enter", "Open selected task"),
        hk("click selected", "Open task"),
        hk("p", "Park selected task"),
        hk("f", "Finish task (press again to force if dirty)"),
        hk("t", "Create new task"),
        hk("r", "Refresh tasks"),
        hk("esc", "Back to repos (when scoped)"),
        Line::from(""),
        section("Repos view", normal_c),
        hk("enter", "View selected repo tasks"),
        hk("click selected", "Create task"),
        hk("t", "Create new task"),
        hk("c", "Clone repo interactively"),
        hk("d", "Toggle detached worktree"),
        hk("r", "Refresh repos"),
        Line::from(""),
        section("Filter", filter_c),
        hk("tab", "Switch tasks/repos view"),
        hk("ctrl-a/e", "Cursor start/end"),
        hk("ctrl-u", "Clear filter"),
        hk("esc", "Return to view"),
        Line::from(""),
        section("Create task", create_c),
        hk("ctrl-a/e", "Cursor start/end"),
        hk("ctrl-u", "Clear branch name"),
        hk("esc", "Return to view"),
        Line::from(""),
        section("Clone repo", clone_c),
        hk("ctrl-a/e", "Cursor start/end"),
        hk("ctrl-u", "Clear input"),
        hk("esc", "Return to view"),
    ];

    let left = "Commands";
    let right = "esc";
    let gap = inner_w.saturating_sub(left.len().saturating_add(right.len()));
    let title_line = Line::from(vec![
        Span::styled(
            left.to_owned(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(right.to_owned(), Style::default().fg(theme.text)),
    ]);

    lines.insert(0, title_line);
    lines.insert(1, Line::from(""));
    lines
}

/// Compute a fixed-width, top-offset popup rect for the help/commands overlay.
///
/// Mimics `OpenCode`'s command palette layout:
/// - Fixed width of 60 columns (clamped to `terminal_width - 2`).
/// - Positioned 25% from the top of the terminal.
/// - Height capped at `floor(terminal_height / 2)`.
/// - Horizontally centered.
fn help_popup_rect(area: Rect) -> Rect {
    let max_w: u16 = 60;
    let w = max_w.min(area.width.saturating_sub(2));
    let x = area.x.saturating_add(area.width.saturating_sub(w) / 2);

    let y = area.y.saturating_add(area.height / 4);
    let max_h = area.height / 2;
    let h = max_h.min(area.height.saturating_sub(y));

    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{help_lines, help_popup_rect};
    use crate::ui::theme::Theme;

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

    #[test]
    fn help_lists_half_page_and_input_cursor_keys() {
        let theme = Theme::dark();
        let lines = help_lines(&theme, 52);
        let text: String = lines.iter().map(std::string::ToString::to_string).collect();

        assert!(text.contains("Half page up/down"));
        assert!(text.contains("Cursor start/end"));
        assert!(text.contains("press again to force if dirty"));
    }
}
