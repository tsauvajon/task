use ratatui::{Frame, layout::Rect, style::Style};

use crate::ui::theme::Theme;

/// Compute thumb start position and length using integer-only arithmetic.
///
/// Returns `(thumb_start, thumb_len)` where both are in track-row units.
/// The thumb length is constant for a given `(item_count, visible_rows, track_len)`
/// regardless of `selected`, which avoids the ±1 jitter that ratatui's
/// float-based `Scrollbar` widget produces.
pub(super) fn scrollbar_geometry(
    selected: usize,
    item_count: usize,
    visible_rows: usize,
    track_len: usize,
) -> (usize, usize) {
    if track_len == 0 || item_count == 0 {
        return (0, 0);
    }
    let thumb_len = visible_rows
        .saturating_mul(track_len)
        .div_ceil(item_count)
        .max(1)
        .min(track_len);
    let max_offset = track_len.saturating_sub(thumb_len);
    let thumb_start = if item_count <= 1 {
        0
    } else {
        selected
            .saturating_mul(max_offset)
            .checked_div(item_count.saturating_sub(1))
            .unwrap_or(0)
    };
    (thumb_start, thumb_len)
}

/// Paint a vertical scrollbar into the rightmost column of `sb_area`.
pub(super) fn render_scrollbar(
    frame: &mut Frame,
    sb_area: Rect,
    selected: usize,
    item_count: usize,
    visible_rows: usize,
    theme: &Theme,
) {
    let track_len = usize::from(sb_area.height);
    let (thumb_start, thumb_len) =
        scrollbar_geometry(selected, item_count, visible_rows, track_len);
    if thumb_len == 0 || sb_area.width == 0 {
        return;
    }
    let col = sb_area.x.saturating_add(sb_area.width.saturating_sub(1));
    let buf = frame.buffer_mut();
    for row_offset in 0..sb_area.height {
        let row = usize::from(row_offset);
        let in_thumb = row >= thumb_start && row < thumb_start.saturating_add(thumb_len);
        let (sym, color) = if in_thumb {
            ("┃", theme.scrollbar_thumb)
        } else {
            ("│", theme.scrollbar_track)
        };
        buf.set_string(
            col,
            sb_area.y.saturating_add(row_offset),
            sym,
            Style::default().fg(color),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::scrollbar_geometry;

    #[test]
    fn thumb_len_is_stable_across_positions() {
        let (_, baseline_len) = scrollbar_geometry(0, 50, 30, 31);
        for pos in 1..50 {
            let (_, len) = scrollbar_geometry(pos, 50, 30, 31);
            assert_eq!(len, baseline_len, "thumb_len changed at position {pos}");
        }
    }

    #[test]
    fn thumb_shrinks_as_item_count_grows() {
        let (_, len_few) = scrollbar_geometry(0, 20, 15, 30);
        let (_, len_many) = scrollbar_geometry(0, 200, 15, 30);
        assert!(
            len_few >= len_many,
            "fewer items should have equal or larger thumb: {len_few} vs {len_many}"
        );
    }

    #[test]
    fn thumb_fills_track_when_all_visible() {
        let (start, len) = scrollbar_geometry(0, 10, 30, 31);
        assert_eq!(start, 0);
        assert_eq!(len, 31);
    }

    #[test]
    fn thumb_minimum_one_cell() {
        let (_, len) = scrollbar_geometry(0, 1000, 5, 10);
        assert!(len >= 1, "thumb must be at least 1 cell: {len}");
    }

    #[test]
    fn thumb_start_zero_at_first_item() {
        let (start, _) = scrollbar_geometry(0, 50, 30, 31);
        assert_eq!(start, 0);
    }

    #[test]
    fn thumb_reaches_bottom_for_last_item() {
        let track = 31;
        let (start, len) = scrollbar_geometry(49, 50, 30, track);
        assert_eq!(
            start + len,
            track,
            "thumb should reach bottom: start={start} len={len} track={track}"
        );
    }

    #[test]
    fn thumb_start_monotonically_increases() {
        let mut prev = 0;
        for pos in 0..100 {
            let (start, _) = scrollbar_geometry(pos, 100, 30, 40);
            assert!(
                start >= prev,
                "thumb_start decreased at pos {pos}: {start} < {prev}"
            );
            prev = start;
        }
    }

    #[test]
    fn empty_list_returns_zero() {
        let (start, len) = scrollbar_geometry(0, 0, 30, 31);
        assert_eq!(start, 0);
        assert_eq!(len, 0);
    }

    #[test]
    fn single_item_thumb_fills_track() {
        let (start, len) = scrollbar_geometry(0, 1, 30, 31);
        assert_eq!(start, 0);
        assert_eq!(len, 31);
    }
}
