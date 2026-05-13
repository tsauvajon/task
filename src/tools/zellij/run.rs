use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

use crate::{
    error::{Error, Result},
    runtime::process::{self, ExternalTool},
};

pub(super) fn available() -> bool {
    process::command_exists("zellij")
}

pub(super) fn capture(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = build_zellij_command(args, cwd);
    let output = cmd.output().map_err(spawn_error_from_io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("command failed with status {}", output.status)
        } else {
            stderr
        };
        return Err(Error::failed(msg));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(super) fn status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = build_zellij_command(args, cwd);
    let status = cmd.status().map_err(spawn_error_from_io)?;
    if status.success() {
        return Ok(());
    }
    Err(Error::failed(format!(
        "command failed with status {status}"
    )))
}

/// Like [`status`], but swallows stdout/stderr so failures stay quiet.
/// Used for best-effort teardown calls (e.g. `delete-session`) where
/// "session not found" is an expected non-error outcome.
pub(super) fn status_quiet(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = build_zellij_command(args, cwd);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let status = cmd.status().map_err(spawn_error_from_io)?;
    if status.success() {
        return Ok(());
    }
    Err(Error::failed(format!(
        "command failed with status {status}"
    )))
}

/// Build a `zellij` `Command` with [`ZELLIJ_SOCKET_DIR`](resolve_socket_dir)
/// preset, so macOS's 104-byte `sun_path` limit doesn't reject our
/// session names just because `$TMPDIR` sits in a long
/// `/var/folders/...` path.
fn build_zellij_command(args: &[&str], cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("zellij");
    cmd.args(args);
    cmd.env("ZELLIJ_SOCKET_DIR", zellij_socket_dir());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

/// Resolve the socket directory Zellij should use.
///
/// macOS sockets live under `$TMPDIR/zellij-<user>/contract_version_1/`
/// by default; on most macOS installs `$TMPDIR` is a 50-character
/// `/var/folders/.../T/` path, which combined with the 104-byte
/// `sun_path` limit leaves as little as 10–15 characters for the
/// session name — sometimes negative, which Zellij surfaces as the
/// cryptic error `session name must be less than 0 characters`.
///
/// Forcing the socket dir to `/tmp` (5 characters) keeps a usable
/// headroom of ~58 cells for the session name. Respects an existing
/// `$ZELLIJ_SOCKET_DIR` so users who have already configured one
/// (e.g. via shell rc) keep their choice.
fn zellij_socket_dir() -> OsString {
    resolve_socket_dir(std::env::var_os("ZELLIJ_SOCKET_DIR"))
}

/// Pure helper extracted for testing — see [`zellij_socket_dir`].
fn resolve_socket_dir(env_value: Option<OsString>) -> OsString {
    env_value
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from("/tmp"))
}

/// Translate a spawn-time `io::Error` into a user-facing message,
/// upgrading `NotFound` into the standard "install with: …" hint when
/// the missing binary is `zellij` itself.
fn spawn_error_from_io(err: std::io::Error) -> Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        return Error::tool_missing(ExternalTool::Zellij);
    }
    Error::from(err)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::resolve_socket_dir;

    #[test]
    fn falls_back_to_tmp_when_env_value_is_absent() {
        assert_eq!(resolve_socket_dir(None), OsString::from("/tmp"));
    }

    #[test]
    fn falls_back_to_tmp_when_env_value_is_empty() {
        // An empty `$ZELLIJ_SOCKET_DIR` is treated as "not set" so we
        // don't pass an unusable empty path to Zellij.
        assert_eq!(
            resolve_socket_dir(Some(OsString::from(""))),
            OsString::from("/tmp")
        );
    }

    #[test]
    fn preserves_explicit_env_value() {
        assert_eq!(
            resolve_socket_dir(Some(OsString::from("/var/run/zellij"))),
            OsString::from("/var/run/zellij")
        );
    }

    #[test]
    fn preserves_explicit_tmp_value() {
        // Identity case: an existing `/tmp` value passes through unchanged.
        assert_eq!(
            resolve_socket_dir(Some(OsString::from("/tmp"))),
            OsString::from("/tmp")
        );
    }
}
