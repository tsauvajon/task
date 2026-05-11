use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    layout::{LayoutInput, SessionStartup, render_layout},
    naming::session_name,
    run::{status, status_quiet},
    sessions::{has_session, is_available},
};
use crate::{
    error::{Error, Result},
    runtime::{
        config::EditorKind,
        process::{self, ExternalTool},
    },
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
    KillSession,
}

/// Teardown always attempts to close Codium windows, independent of the
/// currently-configured editor. A task may have been opened under VSCodium
/// and parked/finished after the config was switched to Helix (or vice
/// versa); gating Codium cleanup on the current [`EditorKind`] would leak
/// Codium windows and processes across config changes. `close_windows` is
/// a no-op when no matching Codium processes are running, so calling it
/// unconditionally is cheap in the Helix-only case.
fn park_teardown_actions(has_zellij_session: bool) -> Vec<TeardownAction> {
    let mut actions = vec![TeardownAction::CloseCodium];
    if has_zellij_session {
        actions.push(TeardownAction::KillSession);
    }
    actions
}

fn finish_teardown_actions(zellij_available: bool) -> Vec<TeardownAction> {
    let mut actions = vec![TeardownAction::CloseCodium];
    if zellij_available {
        actions.push(TeardownAction::KillSession);
    }
    actions
}

/// Ensures VSCodium trusted roots are seeded and codium is running for the given task.
///
/// Trusted roots are always seeded unconditionally so that config changes take
/// effect on the next codium restart, even when codium is already running.
/// If codium is not running, a new window is opened.
fn ensure_codium_running(
    repo_key: &str,
    worktree_name: &str,
    path: &Path,
    codium_trusted_roots: &[PathBuf],
) {
    seed_task_trusted_roots(repo_key, worktree_name, codium_trusted_roots);
    match codium_state(repo_key, worktree_name) {
        Ok(CodiumState::Running) => {}
        Ok(CodiumState::NotRunning) | Err(_) => {
            if let Err(err) = open_window(repo_key, worktree_name, path, codium_trusted_roots) {
                process::warn(&format!(
                    "Failed to open VSCodium for {repo_key} {worktree_name}: {err}"
                ));
            }
        }
    }
}

/// Returns `true` when the given env-var value indicates the process is running
/// inside a Zellij session (i.e. the value is present and non-empty).
fn zellij_env_indicates_inside(zellij_var: Option<&str>) -> bool {
    zellij_var.is_some_and(|v| !v.is_empty())
}

/// Returns `true` when the process is already running inside a Zellij
/// session (i.e. the `ZELLIJ` environment variable is set and non-empty).
fn is_inside_zellij() -> bool {
    zellij_env_indicates_inside(std::env::var("ZELLIJ").ok().as_deref())
}

/// Absolute path to the currently-running binary, used to spawn `task
/// ui` in the status pane.
///
/// Going through `std::env::current_exe()` instead of hardcoding the
/// string `"task"` removes the PATH dependency entirely: the spawned
/// pane works regardless of binary rename (e.g. `custom-task`),
/// `cargo run` (where the binary lives at `target/.../task` and may
/// not be on `$PATH`), or non-PATH installs.
///
/// Returns `None` only on pathological OS states where the kernel
/// cannot report the current executable. In that case the caller
/// should skip the status pane and warn — session creation must
/// still succeed.
fn current_binary_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Width (in cells) of the controlling terminal, used to derive the
/// status pane's `size="<N>%"` percentage so it starts at exactly
/// [`crate::tools::zellij::layout::STATUS_PANE_WIDTH`] cells.
///
/// `None` on non-TTY contexts (CI, piped invocations), in which case
/// the layout falls back to a sensible default percentage.
fn current_terminal_width() -> Option<u16> {
    crossterm::terminal::size().ok().map(|(cols, _rows)| cols)
}

/// Directory where rendered task layouts are persisted before being
/// passed to Zellij. Living in the system temp dir avoids polluting
/// the user's config directory and keeps cleanup implicit — Zellij
/// reads the file once at session start, then never again.
fn layouts_dir() -> PathBuf {
    std::env::temp_dir().join("task-zellij-layouts")
}

fn write_layout_file(session: &str, layout: &str) -> Result<PathBuf> {
    let dir = layouts_dir();
    fs::create_dir_all(&dir).map_err(Error::from)?;
    let path = dir.join(format!("{session}.kdl"));
    fs::write(&path, layout).map_err(Error::from)?;
    Ok(path)
}

pub fn open_session(
    repo_key: &str,
    worktree_name: &str,
    path: &Path,
    editor: EditorKind,
    codium_trusted_roots: &[PathBuf],
) -> Result<OpenResult> {
    if !is_available() {
        return Ok(OpenResult::Unavailable);
    }

    if matches!(editor, EditorKind::Vscodium) {
        // Codium window lifecycle is independent of the Zellij session, so
        // (re)opening it happens on both create and reattach paths.
        ensure_codium_running(repo_key, worktree_name, path, codium_trusted_roots);
    }

    let session = session_name(repo_key, worktree_name);

    if has_session(&session) {
        attach_existing_session(&session)?;
    } else {
        let startup = resolve_startup(path);
        let task_binary = resolve_task_binary();
        let layout = render_layout(&LayoutInput {
            session: &session,
            path,
            editor,
            startup,
            task_binary: task_binary.as_deref(),
            terminal_width: current_terminal_width(),
        });
        let layout_path = write_layout_file(&session, &layout)?;
        attach_new_session(&session, &layout_path)?;
    }

    Ok(OpenResult::Attached)
}

/// Decide what runs in the primary pane on first session creation.
///
/// When opencode is on PATH the layout spawns it directly; otherwise
/// the pane falls back to the user's default shell so the rest of the
/// session can still come up.
fn resolve_startup(path: &Path) -> SessionStartup {
    if process::command_exists(ExternalTool::Opencode.binary_name()) {
        SessionStartup::WithOpencode(opencode::launch_command(path))
    } else {
        process::warn("'opencode' is not available; the primary pane will start as a plain shell.");
        SessionStartup::ShellOnly
    }
}

fn resolve_task_binary() -> Option<PathBuf> {
    let task_binary = current_binary_path();
    if task_binary.is_none() {
        process::warn(
            "Could not resolve own binary path; the status pane will be skipped this session.",
        );
    }
    task_binary
}

/// Create the new session with the rendered layout and put the caller
/// in front of it.
///
/// Inside an existing Zellij session, the new session is created and
/// switched to in a single `switch-session --layout <file>` call.
///
/// Outside Zellij, the caller's terminal is taken over by the new
/// session via `zellij --session <name> --new-session-with-layout
/// <file>`. The process blocks until the user detaches or kills the
/// session — exactly the behavior we want for `task open`.
fn attach_new_session(session: &str, layout_path: &Path) -> Result<()> {
    let layout_str = layout_path.to_string_lossy();
    let layout_str = layout_str.as_ref();
    if is_inside_zellij() {
        status(
            &["action", "switch-session", "--layout", layout_str, session],
            None,
        )
    } else {
        status(
            &[
                "--session",
                session,
                "--new-session-with-layout",
                layout_str,
            ],
            None,
        )
    }
}

/// Re-attach to an already running Zellij session.
///
/// Inside Zellij we use `switch-session`, which migrates the current
/// client. Outside Zellij we use `attach`, which takes over the
/// terminal until detach.
fn attach_existing_session(session: &str) -> Result<()> {
    if is_inside_zellij() {
        status(&["action", "switch-session", session], None)
    } else {
        status(&["attach", session], None)
    }
}

pub fn park(repo_key: &str, worktree_name: &str, path: &Path) -> Result<ParkResult> {
    let session = session_name(repo_key, worktree_name);
    let has_zellij_session = is_available() && has_session(&session);
    let mut result = ParkResult::AlreadyParked;

    for action in park_teardown_actions(has_zellij_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_windows(repo_key, worktree_name);
            }
            TeardownAction::KillSession => {
                kill_session(&session, Some(path))?;
                result = ParkResult::Parked;
            }
        }
    }

    Ok(result)
}

pub fn finish_session(repo_key: &str, worktree_name: &str, cwd: &Path) -> Result<()> {
    let zellij_available = is_available();
    let session = session_name(repo_key, worktree_name);

    for action in finish_teardown_actions(zellij_available) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_windows(repo_key, worktree_name);
            }
            TeardownAction::KillSession => {
                // Kill (best-effort) and then delete the saved session
                // state so a finished task never reappears as a
                // resurrectable Zellij session. `status_quiet` swallows
                // stderr so "session not found" doesn't bubble up to
                // the user when the session is already gone.
                let _ = kill_session_quietly(&session, Some(cwd));
                let _ = delete_session_quietly(&session, Some(cwd));
                let _ = remove_layout_file(&session);
            }
        }
    }

    Ok(())
}

fn kill_session(session: &str, cwd: Option<&Path>) -> Result<()> {
    status(&["kill-session", session], cwd)
}

fn kill_session_quietly(session: &str, cwd: Option<&Path>) -> Result<()> {
    status_quiet(&["kill-session", session], cwd)
}

fn delete_session_quietly(session: &str, cwd: Option<&Path>) -> Result<()> {
    status_quiet(&["delete-session", session], cwd)
}

fn remove_layout_file(session: &str) -> Result<()> {
    let path = layouts_dir().join(format!("{session}.kdl"));
    if path.exists() {
        fs::remove_file(&path).map_err(Error::from)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        TeardownAction, finish_teardown_actions, is_inside_zellij, park_teardown_actions,
        zellij_env_indicates_inside,
    };

    mod park_teardown {
        use super::*;

        #[test]
        fn closes_codium_before_zellij_when_session_exists() {
            let actions = park_teardown_actions(true);
            assert_eq!(
                actions,
                vec![TeardownAction::CloseCodium, TeardownAction::KillSession]
            );
        }

        #[test]
        fn only_closes_codium_without_zellij_session() {
            let actions = park_teardown_actions(false);
            assert_eq!(actions, vec![TeardownAction::CloseCodium]);
        }

        #[test]
        fn always_attempts_close_codium_even_without_session() {
            // Guards the cross-config regression: park must attempt
            // Codium cleanup regardless of session state so a task
            // opened under VSCodium and later parked after switching
            // editors does not leak its Codium window.
            let actions = park_teardown_actions(false);
            assert!(
                actions.contains(&TeardownAction::CloseCodium),
                "park must always attempt to close Codium: {actions:?}"
            );
        }
    }

    mod finish_teardown {
        use super::*;

        #[test]
        fn always_attempts_kill_when_zellij_available() {
            let actions = finish_teardown_actions(true);
            assert_eq!(
                actions,
                vec![TeardownAction::CloseCodium, TeardownAction::KillSession]
            );
        }

        #[test]
        fn only_closes_codium_when_zellij_unavailable() {
            let actions = finish_teardown_actions(false);
            assert_eq!(actions, vec![TeardownAction::CloseCodium]);
        }

        #[test]
        fn always_attempts_close_codium_even_when_zellij_unavailable() {
            // Same cross-config invariant as park: finish must attempt
            // Codium cleanup regardless of the currently-configured editor.
            let actions = finish_teardown_actions(false);
            assert!(
                actions.contains(&TeardownAction::CloseCodium),
                "finish must always attempt to close Codium: {actions:?}"
            );
        }
    }

    mod is_inside_zellij_detection {
        use super::*;

        #[test]
        fn consistent_with_current_environment() {
            // Verify that `is_inside_zellij` agrees with the extracted pure
            // function when both inspect the same env snapshot.
            let expected = zellij_env_indicates_inside(std::env::var("ZELLIJ").ok().as_deref());
            assert_eq!(is_inside_zellij(), expected);
        }

        #[test]
        fn detects_typical_value() {
            // Zellij sets `ZELLIJ=0` inside a session. Non-empty means inside.
            assert!(zellij_env_indicates_inside(Some("0")));
        }

        #[test]
        fn rejects_empty_value() {
            assert!(!zellij_env_indicates_inside(Some("")));
        }

        #[test]
        fn rejects_absent_value() {
            assert!(!zellij_env_indicates_inside(None));
        }

        #[test]
        fn accepts_any_non_empty_string() {
            assert!(zellij_env_indicates_inside(Some("1")));
        }
    }
}
