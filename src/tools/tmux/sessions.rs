use std::collections::HashSet;

use super::run::{capture, status};

pub fn is_available() -> bool {
    crate::runtime::process::command_exists("tmux")
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

    #[test]
    fn parse_sessions_extracts_session_names() {
        let text = "task_a: 1 windows\ndefault: 2 windows\n";
        let sessions = parse_sessions(text);
        assert!(sessions.contains("task_a"));
        assert!(sessions.contains("default"));
    }
}
