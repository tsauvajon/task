use std::fmt::Display;

use super::super::cells::first_char_str;

/// Cell value that can fold to a single-character compact form when
/// the column is too narrow. The full form is whatever `Display`
/// writes (provided here by strum-derived impls on
/// [`crate::runtime::task_rows::TaskStatus`] and
/// [`crate::tools::opencode::status::OpenCodeState`]); the compact
/// form is the first Unicode character of that.
///
/// The trait has a blanket impl over `Display`, so any type with a
/// user-facing display form picks up `.label(compact)` automatically.
/// Keep it `pub(super)` so it doesn't leak past the Tasks renderer.
pub(super) trait CellLabel: Display {
    /// Cell value to render in a column at its current state. When
    /// `compact` is true, the result is the single-character compact
    /// form; otherwise the full `Display` form is returned verbatim.
    fn label(&self, compact: bool) -> String {
        let full = self.to_string();
        if compact {
            first_char_str(&full).to_string()
        } else {
            full
        }
    }
}

impl<T: Display> CellLabel for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{runtime::task_rows::TaskStatus, tools::opencode::status::OpenCodeState};

    mod session_label_tests {
        use super::*;

        #[test]
        fn open_full_is_open() {
            assert_eq!(TaskStatus::Open.label(false), "open");
        }

        #[test]
        fn open_compact_is_o() {
            assert_eq!(TaskStatus::Open.label(true), "o");
        }

        #[test]
        fn parked_full_is_parked() {
            assert_eq!(TaskStatus::Parked.label(false), "parked");
        }

        #[test]
        fn parked_compact_is_p() {
            assert_eq!(TaskStatus::Parked.label(true), "p");
        }
    }

    mod opencode_label_tests {
        use super::*;

        #[test]
        fn none_stays_dot_in_both_forms() {
            // `·` is already one character so the compact form
            // matches the full form for this state.
            assert_eq!(OpenCodeState::None.label(false), "·");
            assert_eq!(OpenCodeState::None.label(true), "·");
        }

        #[test]
        fn gone_full_and_compact() {
            assert_eq!(OpenCodeState::Gone.label(false), "gone");
            assert_eq!(OpenCodeState::Gone.label(true), "g");
        }

        #[test]
        fn busy_full_and_compact() {
            assert_eq!(OpenCodeState::Busy.label(false), "busy");
            assert_eq!(OpenCodeState::Busy.label(true), "b");
        }

        #[test]
        fn idle_full_and_compact() {
            assert_eq!(OpenCodeState::Idle.label(false), "idle");
            assert_eq!(OpenCodeState::Idle.label(true), "i");
        }

        #[test]
        fn hung_full_and_compact() {
            assert_eq!(OpenCodeState::Hung.label(false), "hung");
            assert_eq!(OpenCodeState::Hung.label(true), "h");
        }
    }

    mod compact_invariant {
        //! Guards against future variants whose `Display` form has no
        //! useful first character or returns something multi-grapheme.
        use super::*;

        #[test]
        fn every_session_variant_folds_to_one_char() {
            for status in [TaskStatus::Open, TaskStatus::Parked] {
                let compact = status.label(true);
                assert_eq!(
                    compact.chars().count(),
                    1,
                    "{status:?} compact label must be 1 char, got {compact:?}"
                );
            }
        }

        #[test]
        fn every_opencode_variant_folds_to_one_char() {
            for state in [
                OpenCodeState::None,
                OpenCodeState::Gone,
                OpenCodeState::Busy,
                OpenCodeState::Idle,
                OpenCodeState::Hung,
            ] {
                let compact = state.label(true);
                assert_eq!(
                    compact.chars().count(),
                    1,
                    "{state:?} compact label must be 1 char, got {compact:?}"
                );
            }
        }
    }
}
