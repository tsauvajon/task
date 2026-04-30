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

    let normal_c = theme.mode_color(InputMode::Normal);
    let filter_c = theme.mode_color(InputMode::Filter);
    let create_c = theme.mode_color(InputMode::CreateTask);
    let clone_c = theme.mode_color(InputMode::CloneRepo);
    let desc_style = theme.key_desc_style();

    let section = |title: &str, color: Color| -> Line<'static> {
        Line::from(Span::styled(
            title.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
    };

    let pad_h: u16 = 4;
    let inner_w = popup.width.saturating_sub(pad_h * 2) as usize;

    let cmd_style = Style::default().fg(theme.text);
    let hk = move |key: &str, desc: &str| -> Line<'static> {
        let gap = inner_w.saturating_sub(desc.chars().count() + key.chars().count());
        Line::from(vec![
            Span::styled(desc.to_string(), cmd_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(key.to_string(), desc_style),
        ])
    };

    let mut lines = vec![
        section("All views", normal_c),
        hk("tab", "Switch tasks/repos view"),
        hk("/", "Enter filter mode"),
        hk("b", "Toggle sidebar"),
        hk("ctrl+p", "Commands"),
        hk("q/ctrl-c", "Quit"),
        Line::from(""),
        section("Tasks view", normal_c),
        hk("enter", "Open selected task"),
        hk("p", "Park selected task"),
        hk("f", "Finish selected task"),
        hk("t", "Create new task"),
        hk("r", "Refresh tasks"),
        hk("esc", "Back to repos (when scoped)"),
        Line::from(""),
        section("Repos view", normal_c),
        hk("enter", "View selected repo tasks"),
        hk("t", "Create new task"),
        hk("c", "Clone repo interactively"),
        hk("d", "Toggle detached worktree"),
        hk("r", "Refresh repos"),
        Line::from(""),
        section("Filter", filter_c),
        hk("tab", "Switch tasks/repos view"),
        hk("ctrl-u", "Clear filter"),
        hk("esc", "Return to view"),
        Line::from(""),
        section("Create task", create_c),
        hk("ctrl-u", "Clear branch name"),
        hk("esc", "Return to view"),
        Line::from(""),
        section("Clone repo", clone_c),
        hk("ctrl-u", "Clear input"),
        hk("esc", "Return to view"),
    ];

    let left = "Commands";
    let right = "esc";
    let gap = inner_w.saturating_sub(left.len() + right.len());
    let title_line = Line::from(vec![
        Span::styled(
            left.to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(right.to_string(), Style::default().fg(theme.text)),
    ]);

    lines.insert(0, title_line);
    lines.insert(1, Line::from(""));

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

/// Compute a fixed-width, top-offset popup rect for the help/commands overlay.
///
/// Mimics OpenCode's command palette layout:
/// - Fixed width of 60 columns (clamped to `terminal_width - 2`).
/// - Positioned 25% from the top of the terminal.
/// - Height capped at `floor(terminal_height / 2)`.
/// - Horizontally centered.
fn help_popup_rect(area: Rect) -> Rect {
    let max_w: u16 = 60;
    let w = max_w.min(area.width.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;

    let y = area.y + area.height / 4;
    let max_h = area.height / 2;
    let h = max_h.min(area.height.saturating_sub(y));

    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::help_popup_rect;

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
}
