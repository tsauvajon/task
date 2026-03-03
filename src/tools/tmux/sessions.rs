use std::collections::HashSet;

use super::run::{available, capture, status};

pub fn is_available() -> bool {
    available()
}

pub fn list_sessions() -> HashSet<String> {
    if !is_available() {
        return HashSet::new();
    }

    match capture(&["ls"], None) {
        Ok(output) => parse_sessions(&output),
        Err(_) => HashSet::new(),
    }
}

pub fn has_session(session: &str) -> bool {
    status(&["has-session", "-t", session], None).is_ok()
}

fn parse_sessions(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let name = line.split(':').next()?.trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_sessions;

    mod parse_sessions {
        use super::*;

        #[test]
        fn extracts_session_names() {
            let text = "task_a: 1 windows\ndefault: 2 windows\n";
            let sessions = parse_sessions(text);
            assert!(sessions.contains("task_a"));
            assert!(sessions.contains("default"));
        }

        #[test]
        fn returns_empty_for_empty_input() {
            let sessions = parse_sessions("");
            assert!(sessions.is_empty());
        }

        #[test]
        fn skips_blank_lines() {
            let text = "\n\ntask_b: 1 windows\n\n";
            let sessions = parse_sessions(text);
            assert_eq!(sessions.len(), 1);
            assert!(sessions.contains("task_b"));
        }

        #[test]
        fn handles_session_with_no_colon() {
            // A line with no colon → split(':').next() returns the whole line.
            let text = "orphaned-session\n";
            let sessions = parse_sessions(text);
            assert!(sessions.contains("orphaned-session"));
        }

        #[test]
        fn deduplicates_identical_names() {
            let text = "main: 1 windows\nmain: 2 windows\n";
            let sessions = parse_sessions(text);
            assert_eq!(sessions.len(), 1);
            assert!(sessions.contains("main"));
        }

        #[test]
        fn trims_leading_space_from_name() {
            let text = " padded-name: 1 windows\n";
            let sessions = parse_sessions(text);
            assert!(sessions.contains("padded-name"));
        }

        #[test]
        fn collects_multiple_sessions() {
            let text = "alpha: 1 windows\nbeta: 2 windows\ngamma: 1 windows\n";
            let sessions = parse_sessions(text);
            assert_eq!(sessions.len(), 3);
            assert!(sessions.contains("alpha"));
            assert!(sessions.contains("beta"));
            assert!(sessions.contains("gamma"));
        }
    }
}
