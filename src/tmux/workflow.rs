use std::path::Path;

use crate::runtime::ProcessRunner;

use super::{has_session, is_available, session_name};

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

pub fn open_task_session(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
    path: &Path,
) -> Result<OpenResult, String> {
    if !is_available(process) {
        return Ok(OpenResult::Unavailable);
    }

    let session = session_name(repo_key, branch);
    if !has_session(process, &session) {
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
    if has_session(process, &session) {
        process.run_status("tmux", &["kill-session", "-t", &session], None)?;
        return Ok(ParkResult::Parked);
    }

    Ok(ParkResult::AlreadyParked)
}

pub fn finish_task_session(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
) -> Result<(), String> {
    if !is_available(process) {
        return Ok(());
    }

    let session = session_name(repo_key, branch);
    if has_session(process, &session) {
        process.run_status("tmux", &["kill-session", "-t", &session], None)?;
    }
    Ok(())
}
