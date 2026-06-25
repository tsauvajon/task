use std::collections::HashSet;

use super::run::{available, capture};

#[must_use]
pub fn is_available() -> bool {
    available()
}

/// Snapshot of currently-running Zellij sessions, by name.
///
/// Resurrectable (EXITED) sessions are filtered out so that a parked
/// task isn't mis-classified as open. Returns an empty set on any
/// error so callers degrade gracefully (e.g. when `zellij` is missing
/// from PATH).
#[must_use]
pub fn list_sessions() -> HashSet<String> {
    if !is_available() {
        return HashSet::new();
    }

    match capture(&["list-sessions", "--short", "--no-formatting"], None) {
        Ok(output) => parse_sessions(&output),
        Err(_) => HashSet::new(),
    }
}

#[must_use]
pub fn has_session(session: &str) -> bool {
    list_sessions().contains(session)
}

/// Parse the output of `zellij list-sessions --short --no-formatting`.
///
/// Each line is expected to contain a single session name. Lines marked
/// as `EXITED` (resurrectable sessions that Zellij may still print) are
/// dropped so callers only see live sessions.
fn parse_sessions(output: &str) -> HashSet<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.contains("EXITED"))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_sessions;

    mod parse_sessions {
        use super::*;

        #[test]
        fn extracts_single_session_name_per_line() {
            let text = "task_a\ndefault\n";
            let sessions = parse_sessions(text);
            assert!(sessions.contains("task_a"));
            assert!(sessions.contains("default"));
            assert_eq!(sessions.len(), 2);
        }

        #[test]
        fn returns_empty_for_empty_input() {
            assert!(parse_sessions("").is_empty());
        }

        #[test]
        fn skips_blank_lines() {
            let sessions = parse_sessions("\n\ntask_b\n\n");
            assert_eq!(sessions.len(), 1);
            assert!(sessions.contains("task_b"));
        }

        #[test]
        fn deduplicates_identical_names() {
            let sessions = parse_sessions("main\nmain\n");
            assert_eq!(sessions.len(), 1);
            assert!(sessions.contains("main"));
        }

        #[test]
        fn trims_surrounding_whitespace() {
            let sessions = parse_sessions(" padded \n");
            assert!(sessions.contains("padded"));
        }

        #[test]
        fn collects_multiple_sessions() {
            let sessions = parse_sessions("alpha\nbeta\ngamma\n");
            assert_eq!(sessions.len(), 3);
            assert!(sessions.contains("alpha"));
            assert!(sessions.contains("beta"));
            assert!(sessions.contains("gamma"));
        }

        #[test]
        fn skips_exited_resurrectable_sessions() {
            // `list-sessions --short` does not normally print EXITED markers,
            // but be defensive: a future Zellij that does include them
            // must not cause parked tasks to be classified as open.
            let sessions = parse_sessions("alpha\nbeta (EXITED - 5 minutes ago)\ngamma\n");
            assert!(sessions.contains("alpha"));
            assert!(sessions.contains("gamma"));
            assert!(!sessions.contains("beta"));
        }

        #[test]
        fn takes_first_whitespace_token_only() {
            // Defensive: with `--no-formatting --short` we expect bare
            // names, but if extra columns ever appear we still recover
            // the name from the first column.
            let sessions = parse_sessions("alpha extra data\n");
            assert!(sessions.contains("alpha"));
            assert!(!sessions.iter().any(|s| s.contains("extra")));
        }
    }
}
