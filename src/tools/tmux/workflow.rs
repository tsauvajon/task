use std::path::{Path, PathBuf};

use super::{
    naming::session_name,
    runner::run_tmux_status,
    sessions::{has_session, is_available},
};
use crate::{
    error::Result,
    runtime::process::{CommandPlan, ProcessRunner},
    tools::{
        opencode,
        vscodium::workflow::{close_task_windows, codium_state, open_task_window, CodiumState},
    },
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionStartup {
    ShellOnly,
    WithOpencode(CommandPlan),
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

fn new_session_args(session: &str, path: &Path, startup: SessionStartup) -> Vec<String> {
    let mut args = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        session.to_string(),
        "-c".to_string(),
        path.to_string_lossy().to_string(),
    ];

    if let SessionStartup::WithOpencode(command) = startup {
        args.push(command.program().to_string());
        args.extend(command.args().iter().cloned());
    }

    args
}

/// Reopens VSCodium for the given task if it is not already running.
/// On failure to detect state, warns and attempts to reopen (best effort).
fn ensure_codium_running(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
    path: &Path,
    codium_trusted_roots: &[PathBuf],
) {
    match codium_state(repo_key, branch) {
        Ok(CodiumState::Running) => {}
        Ok(CodiumState::NotRunning) | Err(_) => {
            if let Err(err) =
                open_task_window(process, repo_key, branch, path, codium_trusted_roots)
            {
                process.warn(&format!(
                    "Failed to open VSCodium for {repo_key} {branch}: {err}"
                ));
            }
        }
    }
}

pub fn open_task_session(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
    path: &Path,
    codium_trusted_roots: &[PathBuf],
) -> Result<OpenResult> {
    if !is_available(process) {
        return Ok(OpenResult::Unavailable);
    }

    ensure_codium_running(process, repo_key, branch, path, codium_trusted_roots);

    let session = session_name(repo_key, branch);
    if !has_session(process, &session) {
        let startup = if process.command_exists("opencode") {
            SessionStartup::WithOpencode(opencode::launch_command(path))
        } else {
            process.warn("'opencode' is not available; opening tmux with shell panes only.");
            SessionStartup::ShellOnly
        };

        let args = new_session_args(&session, path, startup);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_tmux_status(&arg_refs, None)?;

        let path_str = path.to_string_lossy();
        run_tmux_status(
            &[
                "split-window",
                "-v",
                "-t",
                &format!("{session}:0"),
                "-c",
                path_str.as_ref(),
            ],
            None,
        )?;
        run_tmux_status(&["select-pane", "-t", &format!("{session}:0.0")], None)?;
    }

    if std::env::var("TMUX")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
    {
        run_tmux_status(&["switch-client", "-t", &session], None)?;
    } else {
        run_tmux_status(&["attach-session", "-t", &session], None)?;
    }

    Ok(OpenResult::Attached)
}

pub fn park_task(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
    path: &Path,
) -> Result<ParkResult> {
    let session = session_name(repo_key, branch);
    let has_tmux_session = has_session(process, &session);
    let mut result = ParkResult::AlreadyParked;
    let title = format!("{repo_key} {branch}");

    if let Err(err) = opencode::rename_latest_session_title(path, &title) {
        process.warn(&format!(
            "Failed to update opencode session title for {repo_key} {branch}: {err}"
        ));
    }

    for action in park_teardown_actions(has_tmux_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_task_windows(process, repo_key, branch);
            }
            TeardownAction::KillTmuxSession => {
                run_tmux_status(&["kill-session", "-t", &session], None)?;
                result = ParkResult::Parked;
            }
        }
    }

    Ok(result)
}

pub fn finish_task_session(process: ProcessRunner, repo_key: &str, branch: &str) -> Result<()> {
    let tmux_available = is_available(process);
    let session = session_name(repo_key, branch);
    let has_tmux_session = tmux_available && has_session(process, &session);

    for action in finish_teardown_actions(tmux_available, has_tmux_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_task_windows(process, repo_key, branch);
            }
            TeardownAction::KillTmuxSession => {
                run_tmux_status(&["kill-session", "-t", &session], None)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        finish_teardown_actions, new_session_args, park_teardown_actions, SessionStartup,
        TeardownAction,
    };
    use crate::runtime::process::{CommandPlan, ManagedTool};

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

    #[test]
    fn new_session_args_shell_only_does_not_include_opencode_command() {
        let args = new_session_args(
            "repo-branch",
            Path::new("/tmp/wt/repo"),
            SessionStartup::ShellOnly,
        );
        assert_eq!(
            args,
            vec![
                "new-session",
                "-d",
                "-s",
                "repo-branch",
                "-c",
                "/tmp/wt/repo"
            ]
        );
    }

    #[test]
    fn new_session_args_with_opencode_uses_nix_wrapped_command() {
        let opencode_command = CommandPlan::for_managed_tool(
            ManagedTool::Opencode,
            vec!["--session".to_string(), "ses_123".to_string()],
        );

        let args = new_session_args(
            "repo-branch",
            Path::new("/tmp/wt/repo"),
            SessionStartup::WithOpencode(opencode_command),
        );

        assert_eq!(
            args,
            vec![
                "new-session",
                "-d",
                "-s",
                "repo-branch",
                "-c",
                "/tmp/wt/repo",
                "nix",
                "run",
                "nixpkgs#opencode",
                "--",
                "--session",
                "ses_123",
            ]
        );
    }
}
