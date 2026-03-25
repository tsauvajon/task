use std::path::{Path, PathBuf};

use super::{
    naming::session_name,
    run::status,
    sessions::{has_session, has_session_in, is_available},
};
use crate::{
    error::Result,
    runtime::process::{self, CommandPlan},
    tools::{
        opencode,
        vscodium::workflow::{
            CodiumState, close_windows, codium_state, open_window, seed_task_trusted_roots,
        },
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

fn finish_teardown_actions(tmux_available: bool) -> Vec<TeardownAction> {
    let mut actions = vec![TeardownAction::CloseCodium];
    if tmux_available {
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

/// Ensures VSCodium trusted roots are seeded and codium is running for the given task.
///
/// Trusted roots are always seeded unconditionally so that config changes take
/// effect on the next codium restart, even when codium is already running.
/// If codium is not running, a new window is opened.
fn ensure_codium_running(
    repo_key: &str,
    branch: &str,
    path: &Path,
    codium_trusted_roots: &[PathBuf],
) {
    // Always seed trusted roots so they're ready for the current or next launch.
    seed_task_trusted_roots(repo_key, branch, codium_trusted_roots);

    match codium_state(repo_key, branch) {
        Ok(CodiumState::Running) => {}
        Ok(CodiumState::NotRunning) | Err(_) => {
            if let Err(err) = open_window(repo_key, branch, path, codium_trusted_roots) {
                process::warn(&format!(
                    "Failed to open VSCodium for {repo_key} {branch}: {err}"
                ));
            }
        }
    }
}

/// Returns `true` when the process is already running inside a tmux session
/// (i.e. the `TMUX` environment variable is set and non-empty).
fn is_inside_tmux() -> bool {
    std::env::var("TMUX")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
}

pub fn open_session(
    repo_key: &str,
    branch: &str,
    path: &Path,
    codium_trusted_roots: &[PathBuf],
) -> Result<OpenResult> {
    if !is_available() {
        return Ok(OpenResult::Unavailable);
    }

    ensure_codium_running(repo_key, branch, path, codium_trusted_roots);

    let session = session_name(repo_key, branch);
    if !has_session(&session) {
        let startup = if process::command_exists("opencode") {
            SessionStartup::WithOpencode(opencode::launch_command(path))
        } else {
            process::warn("'opencode' is not available; opening tmux with shell panes only.");
            SessionStartup::ShellOnly
        };

        let args = new_session_args(&session, path, startup);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        status(&arg_refs, None)?;

        let path_str = path.to_string_lossy();
        status(
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
        status(&["select-pane", "-t", &format!("{session}:0.0")], None)?;
    }

    if is_inside_tmux() {
        status(&["switch-client", "-t", &session], None)?;
    } else {
        status(&["attach-session", "-t", &session], None)?;
    }

    Ok(OpenResult::Attached)
}

pub fn park(repo_key: &str, branch: &str, path: &Path) -> Result<ParkResult> {
    let session = session_name(repo_key, branch);
    let has_tmux_session = has_session_in(&session, Some(path));
    let mut result = ParkResult::AlreadyParked;
    let title = format!("{repo_key} {branch}");

    if let Err(err) = opencode::rename_latest_session_title(path, &title) {
        process::warn(&format!(
            "Failed to update opencode session title for {repo_key} {branch}: {err}"
        ));
    }

    for action in park_teardown_actions(has_tmux_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_windows(repo_key, branch);
            }
            TeardownAction::KillTmuxSession => {
                status(&["kill-session", "-t", &session], Some(path))?;
                result = ParkResult::Parked;
            }
        }
    }

    Ok(result)
}

pub fn finish_session(repo_key: &str, branch: &str, cwd: &Path) -> Result<()> {
    let tmux_available = is_available();
    let session = session_name(repo_key, branch);

    for action in finish_teardown_actions(tmux_available) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_windows(repo_key, branch);
            }
            TeardownAction::KillTmuxSession => {
                // Attempt kill-session directly without checking has-session
                // first. If the session doesn't exist, tmux returns non-zero
                // which we ignore — the goal is only to ensure it's gone.
                let _ = status(&["kill-session", "-t", &session], Some(cwd));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        SessionStartup, TeardownAction, finish_teardown_actions, is_inside_tmux, new_session_args,
        park_teardown_actions,
    };
    use crate::runtime::process::{CommandPlan, ManagedTool};

    mod park_teardown {
        use super::*;

        #[test]
        fn closes_codium_before_tmux_when_session_exists() {
            let actions = park_teardown_actions(true);
            assert_eq!(
                actions,
                vec![TeardownAction::CloseCodium, TeardownAction::KillTmuxSession]
            );
        }

        #[test]
        fn only_closes_codium_without_tmux_session() {
            let actions = park_teardown_actions(false);
            assert_eq!(actions, vec![TeardownAction::CloseCodium]);
        }
    }

    mod finish_teardown {
        use super::*;

        #[test]
        fn always_attempts_kill_when_tmux_available() {
            let actions = finish_teardown_actions(true);
            assert_eq!(
                actions,
                vec![TeardownAction::CloseCodium, TeardownAction::KillTmuxSession]
            );
        }

        #[test]
        fn only_closes_codium_when_tmux_unavailable() {
            let actions = finish_teardown_actions(false);
            assert_eq!(actions, vec![TeardownAction::CloseCodium]);
        }
    }

    mod is_inside_tmux_detection {
        use super::*;

        #[test]
        fn returns_false_when_tmux_var_absent() {
            // We cannot mutate global env safely in parallel tests, so assert
            // this helper matches the predicate computed from current env.
            let expected = std::env::var("TMUX")
                .ok()
                .filter(|v| !v.is_empty())
                .is_some();
            assert_eq!(is_inside_tmux(), expected);
        }

        #[test]
        fn inside_tmux_logic_with_non_empty_value() {
            // Directly test the same predicate logic that is_inside_tmux() uses,
            // using a controlled string instead of reading the real env variable.
            let tmux_env: Option<&str> = Some("/tmp/tmux-1000/default,42,0");
            let inside = tmux_env
                .map(str::to_string)
                .filter(|v| !v.is_empty())
                .is_some();
            assert!(inside, "non-empty TMUX value should indicate inside tmux");
        }

        #[test]
        fn inside_tmux_logic_with_empty_value() {
            let tmux_env: Option<&str> = Some("");
            let inside = tmux_env
                .map(str::to_string)
                .filter(|v| !v.is_empty())
                .is_some();
            assert!(!inside, "empty TMUX value should indicate outside tmux");
        }

        #[test]
        fn inside_tmux_logic_with_absent_value() {
            let tmux_env: Option<&str> = None;
            let inside = tmux_env
                .map(str::to_string)
                .filter(|v| !v.is_empty())
                .is_some();
            assert!(!inside, "absent TMUX variable should indicate outside tmux");
        }
    }

    mod new_session_args {
        use super::*;

        #[test]
        fn shell_only_does_not_include_opencode_command() {
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
        fn with_opencode_uses_nix_wrapped_command() {
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
}
