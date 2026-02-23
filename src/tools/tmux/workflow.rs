use std::path::{Path, PathBuf};

use crate::runtime::process::ProcessRunner;
use crate::tools::vscodium::workflow::{close_task_windows, open_task_window};

use super::naming::session_name;
use super::sessions::{has_session, is_available};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkResult {
    Parked,
    AlreadyParked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenResult {
    Attached,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeardownAction {
    CloseCodium,
    KillTmuxSession,
}

fn park_teardown_actions(has_tmux_session: bool) -> Vec<TeardownAction> {
    let mut actions = vec![TeardownAction::CloseCodium];
    if has_tmux_session {
        actions.push(TeardownAction::KillTmuxSession);
    }
    actions
}

fn finish_teardown_actions(tmux_available: bool, has_tmux_session: bool) -> Vec<TeardownAction> {
    let mut actions = vec![TeardownAction::CloseCodium];
    if tmux_available && has_tmux_session {
        actions.push(TeardownAction::KillTmuxSession);
    }
    actions
}

pub fn open_task_session(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
    path: &Path,
    codium_trusted_roots: &[PathBuf],
) -> Result<OpenResult, String> {
    if !is_available(process) {
        return Ok(OpenResult::Unavailable);
    }

    let session = session_name(repo_key, branch);
    if !has_session(process, &session) {
        if let Err(error) = open_task_window(process, repo_key, branch, path, codium_trusted_roots)
        {
            process.warn(&format!(
                "Failed to open VSCodium for {repo_key} {branch}: {error}"
            ));
        }

        if process.command_exists("opencode") {
            process.run_status(
                "tmux",
                &[
                    "new-session",
                    "-d",
                    "-s",
                    &session,
                    "-c",
                    path.to_string_lossy().as_ref(),
                    "opencode",
                ],
                None,
            )?;
        } else {
            process.warn("'opencode' is not available; opening tmux with shell panes only.");
            process.run_status(
                "tmux",
                &[
                    "new-session",
                    "-d",
                    "-s",
                    &session,
                    "-c",
                    path.to_string_lossy().as_ref(),
                ],
                None,
            )?;
        }

        process.run_status(
            "tmux",
            &[
                "split-window",
                "-v",
                "-t",
                &format!("{session}:0"),
                "-c",
                path.to_string_lossy().as_ref(),
            ],
            None,
        )?;
        process.run_status(
            "tmux",
            &["select-pane", "-t", &format!("{session}:0.0")],
            None,
        )?;
    }

    if std::env::var("TMUX")
        .ok()
        .filter(|value| !value.is_empty())
        .is_some()
    {
        process.run_status("tmux", &["switch-client", "-t", &session], None)?;
    } else {
        process.run_status("tmux", &["attach-session", "-t", &session], None)?;
    }

    Ok(OpenResult::Attached)
}

pub fn park_task(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
) -> Result<ParkResult, String> {
    let session = session_name(repo_key, branch);
    let has_tmux_session = has_session(process, &session);
    let mut result = ParkResult::AlreadyParked;

    for action in park_teardown_actions(has_tmux_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_task_windows(process, repo_key, branch);
            }
            TeardownAction::KillTmuxSession => {
                process.run_status("tmux", &["kill-session", "-t", &session], None)?;
                result = ParkResult::Parked;
            }
        }
    }

    Ok(result)
}

pub fn finish_task_session(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
) -> Result<(), String> {
    let tmux_available = is_available(process);
    let session = session_name(repo_key, branch);
    let has_tmux_session = tmux_available && has_session(process, &session);

    for action in finish_teardown_actions(tmux_available, has_tmux_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_task_windows(process, repo_key, branch);
            }
            TeardownAction::KillTmuxSession => {
                process.run_status("tmux", &["kill-session", "-t", &session], None)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TeardownAction;
    use super::{finish_teardown_actions, park_teardown_actions};

    #[test]
    fn park_teardown_closes_codium_before_tmux_when_session_exists() {
        let actions = park_teardown_actions(true);
        assert_eq!(
            actions,
            vec![TeardownAction::CloseCodium, TeardownAction::KillTmuxSession]
        );
    }

    #[test]
    fn park_teardown_only_closes_codium_without_tmux_session() {
        let actions = park_teardown_actions(false);
        assert_eq!(actions, vec![TeardownAction::CloseCodium]);
    }

    #[test]
    fn finish_teardown_closes_codium_before_tmux_when_available_and_open() {
        let actions = finish_teardown_actions(true, true);
        assert_eq!(
            actions,
            vec![TeardownAction::CloseCodium, TeardownAction::KillTmuxSession]
        );
    }

    #[test]
    fn finish_teardown_only_closes_codium_when_tmux_unavailable() {
        let actions = finish_teardown_actions(false, false);
        assert_eq!(actions, vec![TeardownAction::CloseCodium]);
    }

    #[test]
    fn finish_teardown_only_closes_codium_when_session_missing() {
        let actions = finish_teardown_actions(true, false);
        assert_eq!(actions, vec![TeardownAction::CloseCodium]);
    }
}
