use std::collections::HashSet;

use super::runner::{run_tmux_capture, run_tmux_status};
use crate::runtime::process::ProcessRunner;

pub fn is_available(process: ProcessRunner) -> bool {
    process.command_exists("tmux")
}

pub fn list_sessions(process: ProcessRunner) -> HashSet<String> {
    if !is_available(process) {
        return HashSet::new();
    }

    let output = match run_tmux_capture(&["ls"], None) {
        Ok(output) => output,
        Err(_) => return HashSet::new(),
    };

    parse_sessions(&output)
}

pub fn has_session(_process: ProcessRunner, session: &str) -> bool {
    run_tmux_status(&["has-session", "-t", session], None).is_ok()
}

fn parse_sessions(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| line.split(':').next())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
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
