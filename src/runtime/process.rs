use std::{
    cell::RefCell,
    ffi::OsStr,
    fmt,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    thread,
};

use owo_colors::OwoColorize;

use crate::error::{Error, Result};

#[derive(Default)]
struct LogCapture {
    enabled: bool,
    lines: Vec<String>,
}

static LOG_CAPTURE: OnceLock<Mutex<LogCapture>> = OnceLock::new();

fn log_capture() -> &'static Mutex<LogCapture> {
    LOG_CAPTURE.get_or_init(|| Mutex::new(LogCapture::default()))
}

/// A single buffered line, tagged with the stream it should be flushed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapturedLine {
    Stdout(String),
    Stderr(String),
}

impl CapturedLine {
    pub fn flush(&self) -> Result<()> {
        match self {
            Self::Stdout(line) => write_stdout_line(line),
            Self::Stderr(line) => write_stderr_line(line),
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Stdout(s) | Self::Stderr(s) => s,
        }
    }
}

/// A thread-local output sink used by [`OutputScope`] to capture everything
/// a parallel worker would otherwise print — log lines, warnings, and
/// subprocess stdout/stderr — so it can be flushed to the terminal as one
/// grouped block after the worker finishes.
///
/// The sink stores lines in order of arrival. Nothing here is global: each
/// worker thread installs (and drops) its own `OutputScope`.
type Sink = Arc<Mutex<Vec<CapturedLine>>>;

thread_local! {
    static OUTPUT_SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// Install a per-thread output sink. When active:
///
/// - [`log`] and [`warn`] append into the sink instead of writing to the
///   terminal.
/// - Subprocess helpers ([`run_status`], [`run_status_quiet`]) stream their
///   child's stdout/stderr line-by-line into the sink.
///
/// On drop, the previous sink (if any) is restored and the captured lines
/// are available via [`OutputScope::into_lines`].
///
/// Sequential code that never constructs an `OutputScope` is completely
/// unaffected.
#[derive(Debug)]
pub struct OutputScope {
    sink: Sink,
    previous: Option<Sink>,
}

impl OutputScope {
    #[must_use]
    pub fn new() -> Self {
        let sink: Sink = Arc::new(Mutex::new(Vec::new()));
        let previous = OUTPUT_SINK.with(|slot| slot.replace(Some(Arc::clone(&sink))));
        Self { sink, previous }
    }

    /// Consume the scope and return the captured lines in arrival order.
    #[must_use]
    pub fn into_lines(self) -> Vec<CapturedLine> {
        let scope = std::mem::ManuallyDrop::new(self);
        let lines = std::mem::take(
            &mut *scope
                .sink
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        OUTPUT_SINK.with(|slot| (*slot.borrow_mut()).clone_from(&scope.previous));
        lines
    }
}

impl Default for OutputScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OutputScope {
    fn drop(&mut self) {
        OUTPUT_SINK.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

fn with_active_sink<R>(f: impl FnOnce(&Sink) -> R) -> Option<R> {
    OUTPUT_SINK.with(|slot| slot.borrow().as_ref().map(f))
}

fn push_to_sink(line: CapturedLine) -> bool {
    with_active_sink(|sink| {
        if let Ok(mut guard) = sink.lock() {
            guard.push(line);
        }
    })
    .is_some()
}

/// Flush previously-captured lines to stdout/stderr in arrival order.
pub fn flush_captured_lines(lines: Vec<CapturedLine>) {
    for line in lines {
        match line.flush() {
            Ok(()) => {}
            Err(_) => return,
        }
    }
}

pub fn try_flush_captured_lines(lines: Vec<CapturedLine>) -> Result<()> {
    for line in lines {
        line.flush()?;
    }
    Ok(())
}

pub fn write_stdout_line(message: impl fmt::Display) -> Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{message}").map_err(Error::from)
}

pub fn write_stderr_line(message: impl fmt::Display) -> Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{message}").map_err(Error::from)
}

fn ignore_output_result(result: Result<()>) {
    match result {
        Ok(()) => {}
        Err(_err) => {}
    }
}

/// External binaries the CLI shells out to. The enum captures just enough
/// metadata to generate install hints when the binary is missing from PATH.
///
/// The `Display`, `IntoStaticStr`, and `EnumString` impls are derived
/// from a single source of truth — each variant's `serialize` value is
/// its on-disk binary name. [`Self::binary_name`] returns that string
/// without allocating; [`Self::from_binary`] parses it back. Adding a
/// new tool means adding one variant + its `serialize` attribute.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::IntoStaticStr, strum::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum ExternalTool {
    Git,
    Zellij,
    Codium,
    #[strum(serialize = "hx")]
    Helix,
    Opencode,
    Cargo,
    Nix,
}

/// Install guidance attached to an [`ExternalTool`]. Most tools ship in
/// `nixpkgs`; a few (like `nix` itself) need a different channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum InstallHint {
    /// `nix profile install <package>` (e.g. `nixpkgs#git`).
    #[strum(to_string = "nix profile install {0}")]
    NixPackage(&'static str),
    /// Free-form hint, used when the tool can't be installed via
    /// `nix profile install` (or has a strongly-preferred channel).
    #[strum(to_string = "{0}")]
    Custom(&'static str),
}

impl ExternalTool {
    /// Parse a binary name (e.g. `"git"`, `"hx"`) into the matching
    /// tool, if any. Thin wrapper around the strum-derived
    /// [`std::str::FromStr`] impl that returns `Option` instead of
    /// `Result` to keep call sites concise.
    #[must_use]
    pub fn from_binary(binary: &str) -> Option<Self> {
        binary.parse().ok()
    }

    #[must_use]
    pub const fn install_hint(self) -> InstallHint {
        match self {
            Self::Git => InstallHint::NixPackage("nixpkgs#git"),
            Self::Zellij => InstallHint::NixPackage("nixpkgs#zellij"),
            Self::Codium => InstallHint::NixPackage("nixpkgs#vscodium"),
            Self::Helix => InstallHint::NixPackage("nixpkgs#helix"),
            Self::Opencode => InstallHint::NixPackage("nixpkgs#opencode"),
            Self::Cargo => InstallHint::Custom(
                "install via rustup (https://rustup.rs) or nix profile install nixpkgs#cargo",
            ),
            Self::Nix => InstallHint::Custom("see https://nixos.org/download"),
        }
    }

    /// The binary name used to look the tool up on PATH. Returns the
    /// strum-derived static string (same value `Display` writes), so
    /// there's no allocation and no second source of truth.
    #[must_use]
    pub fn binary_name(self) -> &'static str {
        <&'static str>::from(self)
    }

    /// All external tools, in a stable order suitable for doctor output.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Nix,
            Self::Git,
            Self::Zellij,
            Self::Opencode,
            Self::Codium,
            Self::Helix,
            Self::Cargo,
        ]
    }
}

/// A prepared program + arguments pair. Used when the invocation needs to be
/// serialized (e.g. embedded as a `command`/`args` pair in a Zellij KDL
/// layout), as well as inside the generic `run_*` helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    program: String,
    args: Vec<String>,
}

impl CommandPlan {
    #[must_use]
    pub fn from_program(program: &str, args: &[&str]) -> Self {
        Self::for_program(program, args.iter().map(|&arg| arg.to_owned()).collect())
    }

    #[must_use]
    pub fn for_program(program: &str, args: Vec<String>) -> Self {
        Self {
            program: program.to_owned(),
            args,
        }
    }

    /// Build a plan invoking a known external tool directly from PATH.
    #[must_use]
    pub fn for_tool(tool: ExternalTool, tool_args: Vec<String>) -> Self {
        Self::for_program(tool.binary_name(), tool_args)
    }

    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    #[must_use]
    pub fn args_refs(&self) -> Vec<&str> {
        self.args.iter().map(String::as_str).collect()
    }
}

/// Returns true if the named binary is reachable via PATH (or is an executable
/// filesystem path).
#[must_use]
pub fn command_exists(name: &str) -> bool {
    if name.contains('/') {
        return is_executable_file(Path::new(name));
    }

    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var).any(|dir| is_executable_file(&dir.join(name)))
}

fn is_executable_file(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Maps an `io::Error` from spawning a process into a user-friendlier error.
///
/// When the OS reports "No such file or directory" (`ErrorKind::NotFound`) for
/// the program itself and the program name corresponds to a known
/// [`ExternalTool`], return [`Error::tool_missing`] so the user sees the
/// install hint instead of a bare syscall message. For unknown programs, the
/// message still says "not found on PATH" (instead of raw `io::Error`).
fn spawn_error(program: &OsStr, err: std::io::Error) -> Error {
    if err.kind() != std::io::ErrorKind::NotFound {
        return Error::from(err);
    }
    let Some(name) = program.to_str() else {
        return Error::from(err);
    };
    if let Some(tool) = ExternalTool::from_binary(name) {
        return Error::tool_missing(tool);
    }
    Error::failed(format!("Program `{name}` not found on PATH"))
}

pub fn run_capture(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().map_err(|e| spawn_error(program, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let msg = if stderr.is_empty() {
            format!("command failed with status {}", output.status)
        } else {
            stderr
        };
        return Err(Error::failed(msg));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn run_status(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let program = program.as_ref();

    // If an OutputScope is active, stream the child's stdout/stderr into
    // the sink line-by-line so the caller's buffered block stays coherent.
    if let Some(sink) = OUTPUT_SINK.with(|slot| slot.borrow().clone()) {
        return run_status_into_sink(program, args, cwd, &sink);
    }

    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let status = cmd.status().map_err(|e| spawn_error(program, e))?;
    if status.success() {
        return Ok(());
    }
    Err(Error::failed(format!(
        "command failed with status {status}"
    )))
}

pub fn run_status_quiet(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<()> {
    let program = program.as_ref();

    // When a sink is active, forward stdout/stderr into it even on success
    // so the worker's output block is complete.
    if let Some(sink) = OUTPUT_SINK.with(|slot| slot.borrow().clone()) {
        return run_status_into_sink(program, args, cwd, &sink);
    }

    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().map_err(|e| spawn_error(program, e))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let msg = if stderr.is_empty() {
        format!("command failed with status {}", output.status)
    } else {
        stderr
    };
    Err(Error::failed(msg))
}

/// Spawn a child and drain its stdout+stderr into the active output sink,
/// line by line. Used by both `run_status` and `run_status_quiet` when an
/// [`OutputScope`] is installed on the current thread.
///
/// stderr lines are prefixed with `warning:` so [`flush_captured_lines`]
/// routes them to stderr on flush, matching how [`warn`] behaves live.
fn run_status_into_sink(
    program: &OsStr,
    args: &[&str],
    cwd: Option<&Path>,
    sink: &Sink,
) -> Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd.spawn().map_err(|e| spawn_error(program, e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_handle = stdout.map(|pipe| {
        let sink = Arc::clone(sink);
        thread::spawn(move || drain_into_sink(pipe, &sink, /* stderr = */ false))
    });
    let stderr_handle = stderr.map(|pipe| {
        let sink = Arc::clone(sink);
        thread::spawn(move || drain_into_sink(pipe, &sink, /* stderr = */ true))
    });

    let status = child.wait().map_err(Error::from)?;

    // Drainers must finish before we return so the caller sees the full
    // output block in-order.
    if let Some(h) = stdout_handle {
        drop(h.join());
    }
    if let Some(h) = stderr_handle {
        drop(h.join());
    }

    if status.success() {
        return Ok(());
    }
    Err(Error::failed(format!(
        "command failed with status {status}"
    )))
}

fn drain_into_sink<R: Read>(reader: R, sink: &Sink, is_stderr: bool) {
    let buf = BufReader::new(reader);
    for line in buf.lines().map_while(std::result::Result::ok) {
        let captured = if is_stderr {
            CapturedLine::Stderr(line)
        } else {
            CapturedLine::Stdout(line)
        };
        if let Ok(mut guard) = sink.lock() {
            guard.push(captured);
        }
    }
}

pub fn spawn_detached(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.spawn().map(|_| ()).map_err(|e| spawn_error(program, e))
}

pub fn log(message: &str) {
    // Per-thread OutputScope takes precedence: it preserves the styled
    // prefix so the flushed block looks identical to live output.
    let styled = format!("{} {}", "==>".bright_blue().bold(), message);
    if push_to_sink(CapturedLine::Stdout(styled.clone())) {
        return;
    }
    if capture_log_line(&format!("==> {message}")) {
        return;
    }
    ignore_output_result(write_stdout_line(&styled));
}

pub fn warn(message: &str) {
    let styled = format!("{} {}", "warning:".yellow().bold(), message);
    if push_to_sink(CapturedLine::Stderr(styled.clone())) {
        return;
    }
    if capture_log_line(&format!("warning: {message}")) {
        return;
    }
    ignore_output_result(write_stderr_line(&styled));
}

pub fn enable_log_capture() {
    if let Ok(mut capture) = log_capture().lock() {
        capture.enabled = true;
        capture.lines.clear();
    }
}

pub fn disable_log_capture() {
    if let Ok(mut capture) = log_capture().lock() {
        capture.enabled = false;
        capture.lines.clear();
    }
}

#[must_use]
pub fn take_captured_logs() -> Vec<String> {
    if let Ok(mut capture) = log_capture().lock() {
        return std::mem::take(&mut capture.lines);
    }
    Vec::new()
}

fn capture_log_line(line: &str) -> bool {
    if let Ok(mut capture) = log_capture().lock()
        && capture.enabled
    {
        capture.lines.push(line.to_owned());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, fs, os::unix::fs::PermissionsExt, path::PathBuf};

    use super::{CommandPlan, ExternalTool, InstallHint, command_exists, spawn_error};
    use crate::error::Error;

    mod external_tool {
        use super::*;

        #[test]
        fn from_binary_maps_known_tools() {
            assert_eq!(ExternalTool::from_binary("git"), Some(ExternalTool::Git));
            assert_eq!(
                ExternalTool::from_binary("zellij"),
                Some(ExternalTool::Zellij)
            );
            assert_eq!(
                ExternalTool::from_binary("codium"),
                Some(ExternalTool::Codium)
            );
            assert_eq!(ExternalTool::from_binary("hx"), Some(ExternalTool::Helix));
            assert_eq!(
                ExternalTool::from_binary("opencode"),
                Some(ExternalTool::Opencode)
            );
            assert_eq!(
                ExternalTool::from_binary("cargo"),
                Some(ExternalTool::Cargo)
            );
            assert_eq!(ExternalTool::from_binary("nix"), Some(ExternalTool::Nix));
        }

        #[test]
        fn from_binary_returns_none_for_unmapped_tools() {
            assert_eq!(ExternalTool::from_binary("kill"), None);
            assert_eq!(ExternalTool::from_binary("rustfmt"), None);
            assert_eq!(ExternalTool::from_binary("unknown-tool"), None);
            assert_eq!(ExternalTool::from_binary("custom-editor"), None);
            assert_eq!(ExternalTool::from_binary("made-up-binary"), None);
        }

        #[test]
        fn helix_metadata_uses_hx_binary_and_nixpkgs_helix() {
            // Guards the Helix variant: if any of binary_name,
            // install_hint, from_binary, or membership in `all()` drift
            // apart, `task start` with `editor = "helix"` silently breaks.
            assert_eq!(ExternalTool::Helix.binary_name(), "hx");
            assert_eq!(
                ExternalTool::Helix.install_hint(),
                InstallHint::NixPackage("nixpkgs#helix")
            );
            assert_eq!(ExternalTool::Helix.to_string(), "hx");
            assert!(ExternalTool::all().contains(&ExternalTool::Helix));
        }
    }

    mod command_plan {
        use super::*;

        #[test]
        fn from_program_keeps_program_and_args_direct() {
            let plan = CommandPlan::from_program("git", &["status"]);
            assert_eq!(plan.program(), "git");
            assert_eq!(plan.args(), vec!["status"]);
        }

        #[test]
        fn from_program_keeps_unknown_tools_direct_too() {
            let plan = CommandPlan::from_program("kill", &["-TERM", "123"]);
            assert_eq!(plan.program(), "kill");
            assert_eq!(plan.args(), vec!["-TERM", "123"]);
        }

        #[test]
        fn args_refs_returns_str_slices() {
            let plan = CommandPlan::from_program("kill", &["-TERM", "456"]);
            assert_eq!(plan.args_refs(), vec!["-TERM", "456"]);
        }

        #[test]
        fn for_tool_resolves_to_binary_name_on_path() {
            let plan = CommandPlan::for_tool(
                ExternalTool::Git,
                vec!["status".to_owned(), "--short".to_owned()],
            );
            assert_eq!(plan.program(), "git");
            assert_eq!(plan.args(), vec!["status", "--short"]);
        }

        #[test]
        fn for_tool_with_no_extra_args_has_empty_args() {
            let plan = CommandPlan::for_tool(ExternalTool::Zellij, Vec::new());
            assert_eq!(plan.program(), "zellij");
            assert!(plan.args().is_empty());
        }
    }

    mod external_tool_metadata {
        use super::*;

        #[test]
        fn display_returns_binary_name() {
            assert_eq!(ExternalTool::Git.to_string(), "git");
            assert_eq!(ExternalTool::Zellij.to_string(), "zellij");
            assert_eq!(ExternalTool::Opencode.to_string(), "opencode");
            assert_eq!(ExternalTool::Cargo.to_string(), "cargo");
            assert_eq!(ExternalTool::Nix.to_string(), "nix");
        }

        #[test]
        fn install_hint_uses_nix_package_for_nixpkgs_tools() {
            assert_eq!(
                ExternalTool::Git.install_hint(),
                InstallHint::NixPackage("nixpkgs#git")
            );
            assert_eq!(
                ExternalTool::Zellij.install_hint(),
                InstallHint::NixPackage("nixpkgs#zellij")
            );
            assert_eq!(
                ExternalTool::Opencode.install_hint(),
                InstallHint::NixPackage("nixpkgs#opencode")
            );
        }

        #[test]
        fn install_hint_for_cargo_mentions_rustup_and_nix() {
            let hint = ExternalTool::Cargo.install_hint();
            assert!(matches!(hint, InstallHint::Custom(_)));
            let msg = hint.to_string();
            assert!(
                msg.contains("rustup"),
                "cargo hint should mention rustup: {msg}"
            );
            assert!(msg.contains("nix"), "cargo hint should mention nix: {msg}");
        }

        #[test]
        fn install_hint_for_nix_points_at_nixos_download() {
            let hint = ExternalTool::Nix.install_hint();
            assert!(matches!(hint, InstallHint::Custom(_)));
            assert!(hint.to_string().contains("nixos.org/download"));
        }

        #[test]
        fn nix_package_hint_renders_as_nix_profile_command() {
            let rendered = InstallHint::NixPackage("nixpkgs#git").to_string();
            assert_eq!(rendered, "nix profile install nixpkgs#git");
        }

        #[test]
        fn binary_name_matches_tool_name() {
            assert_eq!(ExternalTool::Git.binary_name(), "git");
            assert_eq!(ExternalTool::Opencode.binary_name(), "opencode");
            assert_eq!(ExternalTool::Zellij.binary_name(), "zellij");
            assert_eq!(ExternalTool::Cargo.binary_name(), "cargo");
            assert_eq!(ExternalTool::Nix.binary_name(), "nix");
        }

        #[test]
        fn all_tools_have_non_empty_metadata() {
            for tool in ExternalTool::all() {
                assert!(
                    !tool.install_hint().to_string().is_empty(),
                    "{tool} has empty install hint"
                );
                assert!(
                    !tool.binary_name().is_empty(),
                    "{tool} has empty binary_name"
                );
            }
        }

        #[test]
        fn external_tool_is_copy_and_eq() {
            let a = ExternalTool::Git;
            let b = a; // Copy
            assert_eq!(a, b);
        }

        #[test]
        fn all_contains_every_variant() {
            let all = ExternalTool::all();
            assert!(all.contains(&ExternalTool::Git));
            assert!(all.contains(&ExternalTool::Zellij));
            assert!(all.contains(&ExternalTool::Codium));
            assert!(all.contains(&ExternalTool::Helix));
            assert!(all.contains(&ExternalTool::Opencode));
            assert!(all.contains(&ExternalTool::Cargo));
            assert!(all.contains(&ExternalTool::Nix));
        }
    }

    mod spawn_error_mapping {
        use super::*;

        fn not_found_io_error() -> std::io::Error {
            std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory")
        }

        #[test]
        fn maps_not_found_for_known_tool_to_tool_missing_hint() {
            let err = spawn_error(OsStr::new("git"), not_found_io_error());
            let msg = err.to_string();
            assert!(matches!(err, Error::Failed(_)));
            assert!(
                msg.contains("`git`") && msg.contains("nix profile install nixpkgs#git"),
                "expected tool_missing hint, got: {msg}"
            );
        }

        #[test]
        fn maps_not_found_for_cargo_to_cargo_specific_hint() {
            let err = spawn_error(OsStr::new("cargo"), not_found_io_error());
            let msg = err.to_string();
            assert!(matches!(err, Error::Failed(_)));
            assert!(msg.contains("`cargo`"), "should quote cargo: {msg}");
            assert!(
                msg.contains("rustup"),
                "cargo hint should mention rustup: {msg}"
            );
        }

        #[test]
        fn maps_not_found_for_unknown_binary_to_generic_message() {
            let err = spawn_error(OsStr::new("not-a-known-tool"), not_found_io_error());
            let msg = err.to_string();
            assert!(matches!(err, Error::Failed(_)));
            assert!(
                msg.contains("`not-a-known-tool`") && msg.contains("not found on PATH"),
                "expected generic not-found message, got: {msg}"
            );
        }

        #[test]
        fn passes_through_non_not_found_errors_for_known_tools() {
            let io_err = std::io::Error::other("something else");
            let err = spawn_error(OsStr::new("git"), io_err);
            assert!(matches!(err, Error::Io(_)));
        }
    }

    mod command_exists {
        use super::*;

        fn temp_path(name: &str) -> PathBuf {
            std::env::temp_dir().join(format!("task-command-exists-{}-{name}", std::process::id()))
        }

        fn remove_temp_file(path: &std::path::Path) {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("failed to remove temp file {}: {error}", path.display()),
            }
        }

        #[test]
        fn returns_true_for_known_system_binary() {
            // `true` is universally available on POSIX systems
            assert!(command_exists("true"));
        }

        #[test]
        fn returns_false_for_unknown_binary() {
            assert!(!command_exists("this-binary-should-not-exist-xyz-12345"));
        }

        #[test]
        fn returns_true_for_existing_absolute_path() {
            // /usr/bin/env is available on macOS and Linux
            assert!(command_exists("/usr/bin/env"));
        }

        #[test]
        fn returns_false_for_nonexistent_absolute_path() {
            assert!(!command_exists("/this/path/does/not/exist/xyz"));
        }

        #[test]
        fn returns_false_for_existing_absolute_directory() {
            assert!(!command_exists("/tmp"));
        }

        #[test]
        fn returns_false_for_existing_non_executable_file() {
            let path = temp_path("non-executable");
            remove_temp_file(&path);
            fs::write(&path, "#!/bin/sh\n").expect("write temp file");
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(&path, permissions).expect("set permissions");

            assert!(!command_exists(&path.to_string_lossy()));

            remove_temp_file(&path);
        }

        #[test]
        fn returns_true_for_existing_executable_file() {
            let path = temp_path("executable");
            remove_temp_file(&path);
            fs::write(&path, "#!/bin/sh\n").expect("write temp file");
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).expect("set permissions");

            assert!(command_exists(&path.to_string_lossy()));

            remove_temp_file(&path);
        }

        #[test]
        fn slash_in_name_triggers_filesystem_check() {
            // A relative path that contains a slash but doesn't exist
            assert!(!command_exists("relative/path/to/nothing"));
        }
    }

    mod output_scope {
        use super::super::{
            CapturedLine, OUTPUT_SINK, OutputScope, log, run_status, run_status_quiet, warn,
        };

        /// Strip owo-colors ANSI escapes so assertions are readable.
        fn strip_ansi(s: &str) -> String {
            let mut out = String::with_capacity(s.len());
            let mut chars = s.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\u{1b}' {
                    // Skip until the terminating letter of the CSI sequence.
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }

        #[test]
        fn captures_log_and_warn_in_order() {
            let scope = OutputScope::new();
            log("first");
            warn("second");
            log("third");
            let lines = scope.into_lines();

            assert_eq!(lines.len(), 3);
            assert!(matches!(lines[0], CapturedLine::Stdout(_)));
            assert!(matches!(lines[1], CapturedLine::Stderr(_)));
            assert!(matches!(lines[2], CapturedLine::Stdout(_)));

            assert_eq!(strip_ansi(lines[0].text()), "==> first");
            assert_eq!(strip_ansi(lines[1].text()), "warning: second");
            assert_eq!(strip_ansi(lines[2].text()), "==> third");
        }

        #[test]
        fn without_scope_no_capture_happens() {
            // Nothing installed → sink is None.
            let present = OUTPUT_SINK.with(|slot| slot.borrow().as_ref().is_some());
            assert!(!present, "no sink should be installed outside of a scope");
        }

        #[test]
        fn drop_restores_previous_sink() {
            let outer = OutputScope::new();
            log("outer-before");
            {
                let inner = OutputScope::new();
                log("inner-only");
                let inner_lines = inner.into_lines();
                assert_eq!(inner_lines.len(), 1);
                assert!(strip_ansi(inner_lines[0].text()).contains("inner-only"));
            }
            log("outer-after");
            let outer_lines = outer.into_lines();
            assert_eq!(outer_lines.len(), 2);
            assert!(strip_ansi(outer_lines[0].text()).contains("outer-before"));
            assert!(strip_ansi(outer_lines[1].text()).contains("outer-after"));
        }

        #[test]
        fn run_status_streams_subprocess_stdout_into_sink() {
            // `true` produces no output; use `printf` to exercise stdout
            // streaming (portable on macOS and Linux).
            let scope = OutputScope::new();
            run_status("printf", &["hello-from-child\\n"], None).expect("printf should succeed");
            let lines = scope.into_lines();
            assert!(
                lines
                    .iter()
                    .any(|l| matches!(l, CapturedLine::Stdout(s) if s == "hello-from-child")),
                "expected child stdout line, got {lines:?}"
            );
        }

        #[test]
        fn run_status_quiet_streams_stderr_as_stderr_variant() {
            let scope = OutputScope::new();
            // Write to stderr via `sh -c`.
            run_status_quiet("sh", &["-c", "printf 'err-msg\\n' 1>&2"], None)
                .expect("sh should succeed");
            let lines = scope.into_lines();
            assert!(
                lines
                    .iter()
                    .any(|l| matches!(l, CapturedLine::Stderr(s) if s == "err-msg")),
                "expected child stderr line, got {lines:?}"
            );
        }

        #[test]
        fn captured_child_output_does_not_leak_to_terminal_on_success() {
            // Implicit: no assertion on terminal, but verify the sink got
            // the content so a sequential printer can flush later.
            let scope = OutputScope::new();
            run_status("printf", &["captured-only\\n"], None).unwrap();
            let lines = scope.into_lines();
            assert!(
                lines.iter().any(|l| l.text() == "captured-only"),
                "line should be in sink, not on live stdout"
            );
        }
    }
}
