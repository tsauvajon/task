//! Shared spinner frame set used by both the TUI status bar and the
//! CLI progress reporter.
//!
//! Ten braille frames keep the animation visibly smooth at roughly
//! 100 ms per frame without requiring a faster tick loop.

pub const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Frame set as `&[&str]`, which is the shape indicatif's
/// `ProgressStyle::tick_strings` expects.
pub const FRAMES_STR: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[cfg(test)]
mod tests {
    use super::{FRAMES, FRAMES_STR};

    #[test]
    fn char_and_str_frame_sets_agree() {
        assert_eq!(FRAMES.len(), FRAMES_STR.len());
        for (c, s) in FRAMES.iter().zip(FRAMES_STR.iter()) {
            assert_eq!(c.to_string(), *s, "char and str forms of frame must match");
        }
    }

    #[test]
    fn frames_are_nonempty_and_single_glyph() {
        assert!(!FRAMES.is_empty());
        for s in FRAMES_STR {
            assert!(!s.is_empty());
        }
    }
}
