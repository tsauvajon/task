use std::path::{Path, PathBuf};

use super::{
    naming::session_name,
    run::{capture, status},
    sessions::{has_session, has_session_in, is_available},
};
use crate::{
    error::{Error, Result},
    runtime::{
        config::EditorKind,
        process::{self, CommandPlan, ExternalTool},
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
    KillTmuxSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionStartup {
    ShellOnly,
    WithOpencode(CommandPlan),
}

/// Teardown always attempts to close Codium windows, independent of the
/// currently-configured editor. A task may have been opened under VSCodium
/// and parked/finished after the config was switched to Helix (or vice
/// versa); gating Codium cleanup on the current [`EditorKind`] would leak
/// Codium windows and processes across config changes. `close_windows` is
/// a no-op when no matching Codium processes are running, so calling it
/// unconditionally is cheap in the Helix-only case.
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

/// A reference to a tmux pane. Either a literal string (the typical case —
/// a session-qualified target like `"session:0"` that tmux resolves on the
/// fly), a placeholder that is substituted at execution time with a
/// previously-captured pane id (e.g. `"%12"`), or a templated string with
/// a captured pane id substituted into a fixed template (used when the
/// pane id has to live *inside* a single tmux argument, like the body of
/// a `set-hook` command).
///
/// Using captured pane ids for targets avoids assumptions about
/// `pane-base-index`, which can be set to non-zero in user tmux configs and
/// would otherwise break positional targets like `session:0.0`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Arg {
    Literal(String),
    PaneSlot(PaneSlot),
    /// Single string built by replacing every occurrence of `{}` in
    /// `template` with the captured pane id from `slot`. Used to
    /// embed a pane id inside a quoted tmux command argument, such
    /// as the body of a `set-hook` directive.
    Format {
        template: String,
        slot: PaneSlot,
    },
}

/// Named pane ids captured during session construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PaneSlot {
    /// The first pane of the session (opencode or shell).
    Primary,
    /// The fixed-width left pane running `task ui`. Captured so a
    /// `client-attached` hook can re-pin its width on every (re)attach,
    /// counteracting tmux's proportional pane rescaling.
    LeftUi,
}

impl Arg {
    fn literal(value: impl Into<String>) -> Self {
        Self::Literal(value.into())
    }

    /// Build a [`Arg::Format`] from a template string and a slot. The
    /// template must contain at least one `{}` substitution marker.
    fn format(template: impl Into<String>, slot: PaneSlot) -> Self {
        Self::Format {
            template: template.into(),
            slot,
        }
    }
}

/// One command in the tmux session-construction plan.
///
/// Models tmux's `[options...] [shell-command]` argv shape directly:
/// `flags` carry the subcommand and its options; `command`, when
/// present, is the trailing program to spawn in the new pane (e.g.
/// `opencode`, `task ui`, `hx .`).
///
/// When `capture_into` is set, the step runs via `tmux -P -F
/// '#{pane_id}'` and its stdout (the new pane id) is stored in that
/// slot for later steps to reference via [`Arg::PaneSlot`]. The
/// `-P -F '#{pane_id}'` flags are **not** stored in `flags` — they are
/// emitted by [`SessionStep::render_args`] automatically for capturing
/// steps. This means it is impossible by construction to build a
/// captured step that forgets to ask tmux for the pane id format.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionStep {
    /// Subcommand and pre-command flags (`new-session`, `split-window`,
    /// `-c <path>`, etc.). Format flags for capturing steps live in
    /// [`SessionStep::render_args`].
    flags: Vec<Arg>,
    /// Trailing shell command, if any (e.g. `opencode`, `task ui`,
    /// `hx .`). Optional because some steps (`select-pane`, the
    /// shell-only `new-session` branch) carry no command.
    command: Option<Vec<Arg>>,
    /// When `Some`, the executor reads stdout into the named slot and
    /// `render_args` injects `-P -F '#{pane_id}'` between flags and
    /// command.
    capture_into: Option<PaneSlot>,
}

impl SessionStep {
    /// Fire-and-forget step with no trailing command (e.g. `select-pane`).
    fn fire(flags: Vec<Arg>) -> Self {
        Self {
            flags,
            command: None,
            capture_into: None,
        }
    }

    /// Fire-and-forget step with a trailing shell command (e.g. the
    /// helix `split-window … hx .` step).
    fn fire_with_command(flags: Vec<Arg>, command: Vec<Arg>) -> Self {
        Self {
            flags,
            command: Some(command),
            capture_into: None,
        }
    }

    /// Captured step with no trailing command. Currently unused, but
    /// kept symmetrical with `fire`/`fire_with_command` so the four
    /// constructors form a clean 2x2 (capture/no-capture × command/no-command).
    fn capturing(flags: Vec<Arg>, slot: PaneSlot) -> Self {
        Self {
            flags,
            command: None,
            capture_into: Some(slot),
        }
    }

    /// Captured step that runs a trailing shell command after the new
    /// pane is created (e.g. `new-session … opencode`, `split-window
    /// … task ui`).
    fn capturing_with_command(flags: Vec<Arg>, command: Vec<Arg>, slot: PaneSlot) -> Self {
        Self {
            flags,
            command: Some(command),
            capture_into: Some(slot),
        }
    }

    /// Materialise the final argument vector for tmux. Capturing steps
    /// have `-P -F '#{pane_id}'` injected between `flags` and `command`
    /// so tmux prints the new pane id on stdout. The position matters:
    /// tmux treats arguments after the trailing shell command as part
    /// of that command's argv, so the format flags must precede it.
    fn render_args(&self) -> Vec<Arg> {
        let mut out = self.flags.clone();
        if self.capture_into.is_some() {
            out.push(Arg::literal("-P"));
            out.push(Arg::literal("-F"));
            out.push(Arg::literal("#{pane_id}"));
        }
        if let Some(command) = &self.command {
            out.extend(command.iter().cloned());
        }
        out
    }
}

/// Build the args (without `-P -F '#{pane_id}'`) and optional trailing
/// command for the `new-session` step.
///
/// Returned as a `(flags, command)` pair so the caller can plug them
/// into the appropriate [`SessionStep`] constructor. Format flags are
/// *not* added here — they're emitted automatically by
/// [`SessionStep::render_args`] for capturing steps.
///
/// `window_size`, when set, emits `-x <cols> -y <rows>` so tmux creates
/// the detached session at that size instead of the default 80×24. This
/// is load-bearing for the left-ui pane: a `split-window -l 40` against
/// an 80-cell window leaves the new pane at 40 cells, but tmux then
/// rescales every pane proportionally when the client attaches with a
/// wider terminal — turning the 40-cell pane into ~50% of the actual
/// width. Sizing the detached session to the attaching client's
/// dimensions sidesteps that rescale.
fn new_session_flags_and_command(
    session: &str,
    path: &Path,
    startup: &SessionStartup,
    window_size: Option<(u16, u16)>,
) -> (Vec<Arg>, Option<Vec<Arg>>) {
    let mut flags = vec![
        Arg::literal("new-session"),
        Arg::literal("-d"),
        Arg::literal("-s"),
        Arg::literal(session),
    ];
    if let Some((cols, rows)) = window_size {
        flags.push(Arg::literal("-x"));
        flags.push(Arg::literal(cols.to_string()));
        flags.push(Arg::literal("-y"));
        flags.push(Arg::literal(rows.to_string()));
    }
    flags.push(Arg::literal("-c"));
    flags.push(Arg::literal(path.to_string_lossy().to_string()));

    let command = match startup {
        SessionStartup::ShellOnly => None,
        SessionStartup::WithOpencode(plan) => {
            let mut command = vec![Arg::literal(plan.program())];
            for a in plan.args() {
                command.push(Arg::literal(a));
            }
            Some(command)
        }
    };

    (flags, command)
}

/// Width (in tmux cells) of the left pane that runs `task ui`.
///
/// Sized to fit the Tasks view's `Branch / Tmux / Agent` columns when
/// the Repo column auto-hides via `pick_task_column_layout`. Budget:
/// 6 cells of table chrome (block padding 2 + highlight symbol 2 +
/// 2 inter-column gaps for 3 visible columns) + 20 cells branch +
/// 8 cells Tmux + 5 cells Agent = 39 cells, with 1 cell of slack to
/// make the rightmost column read as a balanced block.
const LEFT_UI_PANE_WIDTH: u16 = 40;

/// Build the ordered plan of tmux steps required to create the session
/// layout for a given editor.
///
/// Pure function — does not invoke tmux. Enables unit testing the layout
/// without an available tmux binary.
///
/// The first step creates the session and captures the id of its primary
/// pane (`%N`), which subsequent splits target directly. This avoids
/// targeting `session:0.0`, which breaks for users with a non-zero
/// `pane-base-index` in their tmux config.
///
/// When `task_binary` is `Some`, every layout starts by splitting a
/// fixed-width pane on the **left** that runs `<task_binary> ui`.
/// Passing the absolute binary path (typically from
/// [`current_binary_path`]) sidesteps the need for `task` to be on
/// `$PATH` inside the spawned pane. When `None`, the left pane is
/// omitted entirely — used as a graceful fallback when
/// [`std::env::current_exe`] cannot resolve the running binary.
///
/// The id of the primary pane (and therefore every subsequent split
/// that targets it) is unaffected by whether the left pane is
/// present, so the editor-specific layout below is identical in
/// either case.
///
/// ## Vscodium layout
/// ```text
/// +---------+----------+
/// |         | opencode |
/// | task ui +----------+
/// |         |  shell   |
/// +---------+----------+
/// ```
///
/// ## Helix layout
/// ```text
/// +---------+----------+-------+
/// |         | opencode |       |
/// | task ui +----------+ helix |
/// |         |  shell   |       |
/// +---------+----------+-------+
/// ```
fn build_session_plan(
    session: &str,
    path: &Path,
    editor: EditorKind,
    startup: &SessionStartup,
    window_size: Option<(u16, u16)>,
    task_binary: Option<&Path>,
) -> Vec<SessionStep> {
    let path_arg = Arg::literal(path.to_string_lossy().to_string());

    // Step 0: new-session, captures the primary pane id.
    let (new_session_flags, new_session_command) =
        new_session_flags_and_command(session, path, startup, window_size);
    let mut plan = vec![match new_session_command {
        Some(command) => {
            SessionStep::capturing_with_command(new_session_flags, command, PaneSlot::Primary)
        }
        None => SessionStep::capturing(new_session_flags, PaneSlot::Primary),
    }];

    // Step 1 (optional): a fixed-width left pane running `<task_binary>
    // ui`, inserted before the editor-specific splits. `-h -b` splits
    // horizontally and places the new pane to the left of the target;
    // `-l 40` pins it at 40 cells wide. The new pane runs `task ui`
    // with no scope so the user sees every active worktree at a
    // glance.
    //
    // The pane id is captured into [`PaneSlot::LeftUi`] so the
    // immediately-following `set-hook` step can reference it: tmux
    // proportionally rescales every pane on attach when the client
    // size differs from the session's last size, which silently
    // erodes the `-l 40` we asked for. The `client-attached` hook
    // re-pins the pane back to 40 cells on every (re)attach.
    if let Some(task_binary) = task_binary {
        plan.push(SessionStep::capturing_with_command(
            vec![
                Arg::literal("split-window"),
                Arg::literal("-h"),
                Arg::literal("-b"),
                Arg::literal("-t"),
                Arg::PaneSlot(PaneSlot::Primary),
                Arg::literal("-l"),
                Arg::literal(LEFT_UI_PANE_WIDTH.to_string()),
                Arg::literal("-c"),
                path_arg.clone(),
            ],
            vec![
                Arg::literal(task_binary.to_string_lossy().to_string()),
                Arg::literal("ui"),
            ],
            PaneSlot::LeftUi,
        ));

        // Step 2..N: install hooks that re-pin the left-ui pane width
        // to LEFT_UI_PANE_WIDTH on every event that can rescale panes,
        // counteracting tmux's proportional pane rescaling.
        //
        // The pane was created at exactly LEFT_UI_PANE_WIDTH cells in
        // a session sized to the current terminal, but tmux rescales
        // proportionally whenever the window dimensions change. The
        // three hooks below cover the three triggers users actually
        // see:
        //
        // * `client-attached` — initial attach via `attach-session`,
        //   typical when running `task open` from outside tmux. Also
        //   fires on every reattach.
        // * `client-session-changed` — `switch-client`, used by
        //   `open_session` when the user is already inside tmux.
        //   Without this hook, opening (or reopening) a task from
        //   inside an outer tmux pane drags the pane to whatever
        //   fraction it was at creation.
        // * `client-resized` — the user resizes their terminal /
        //   client window after attach. Without this hook, a resize
        //   any time after first attach drifts the pane back to a
        //   proportional fraction.
        //
        // Each hook body must be a single tmux argument with the
        // captured pane id baked in, hence [`Arg::Format`]. `-a`
        // appends rather than replaces, so any user-defined global
        // hooks for the same event still fire.
        let hook_body = Arg::format(
            format!("resize-pane -t {{}} -x {LEFT_UI_PANE_WIDTH}"),
            PaneSlot::LeftUi,
        );
        for hook_name in [
            "client-attached",
            "client-session-changed",
            "client-resized",
        ] {
            plan.push(SessionStep::fire(vec![
                Arg::literal("set-hook"),
                Arg::literal("-a"),
                Arg::literal("-t"),
                Arg::literal(session.to_string()),
                Arg::literal(hook_name),
                hook_body.clone(),
            ]));
        }
    }

    match editor {
        EditorKind::Vscodium => {
            // Split the primary pane vertically to create a shell pane
            // underneath.
            plan.push(SessionStep::fire(vec![
                Arg::literal("split-window"),
                Arg::literal("-v"),
                Arg::literal("-t"),
                Arg::PaneSlot(PaneSlot::Primary),
                Arg::literal("-c"),
                path_arg.clone(),
            ]));
            plan.push(SessionStep::fire(vec![
                Arg::literal("select-pane"),
                Arg::literal("-t"),
                Arg::PaneSlot(PaneSlot::Primary),
            ]));
        }
        EditorKind::Helix => {
            // Split horizontally first → new pane on the right runs `hx .`.
            plan.push(SessionStep::fire_with_command(
                vec![
                    Arg::literal("split-window"),
                    Arg::literal("-h"),
                    Arg::literal("-t"),
                    Arg::PaneSlot(PaneSlot::Primary),
                    Arg::literal("-c"),
                    path_arg.clone(),
                ],
                vec![
                    Arg::literal(ExternalTool::Helix.binary_name()),
                    Arg::literal("."),
                ],
            ));
            // Then split the primary pane vertically → new shell pane beneath.
            plan.push(SessionStep::fire(vec![
                Arg::literal("split-window"),
                Arg::literal("-v"),
                Arg::literal("-t"),
                Arg::PaneSlot(PaneSlot::Primary),
                Arg::literal("-c"),
                path_arg,
            ]));
            // Focus the opencode pane at startup.
            plan.push(SessionStep::fire(vec![
                Arg::literal("select-pane"),
                Arg::literal("-t"),
                Arg::PaneSlot(PaneSlot::Primary),
            ]));
        }
    }

    plan
}

/// Resolve [`Arg::PaneSlot`] and [`Arg::Format`] placeholders into
/// concrete strings, given the pane ids captured so far. Returns an
/// error if a referenced slot has not been populated yet.
fn resolve_args(
    args: &[Arg],
    panes: &std::collections::HashMap<PaneSlot, String>,
) -> Result<Vec<String>> {
    args.iter()
        .map(|arg| match arg {
            Arg::Literal(s) => Ok(s.clone()),
            Arg::PaneSlot(slot) => lookup_pane(panes, *slot).map(str::to_string),
            Arg::Format { template, slot } => {
                let pane_id = lookup_pane(panes, *slot)?;
                Ok(template.replace("{}", pane_id))
            }
        })
        .collect()
}

/// Pane-id lookup with a uniform error for missing slots.
fn lookup_pane(
    panes: &std::collections::HashMap<PaneSlot, String>,
    slot: PaneSlot,
) -> Result<&str> {
    panes.get(&slot).map(String::as_str).ok_or_else(|| {
        Error::failed(format!(
            "tmux session plan references pane slot {slot:?} before it was captured"
        ))
    })
}

/// Abstracts the two tmux operations that [`execute_session_plan`] needs.
///
/// Exists solely so the plan executor can be unit-tested without spawning
/// tmux; production code always uses [`RealTmux`]. Kept private to this
/// module — no caller outside `workflow.rs` should implement this trait.
trait TmuxExecutor {
    fn capture(&self, args: &[&str]) -> Result<String>;
    fn status(&self, args: &[&str]) -> Result<()>;
}

/// Zero-sized production [`TmuxExecutor`] that shells out via the module's
/// existing `run::{capture, status}` helpers. `execute_session_plan` never
/// uses a cwd, so the trait does not expose one.
struct RealTmux;

impl TmuxExecutor for RealTmux {
    fn capture(&self, args: &[&str]) -> Result<String> {
        capture(args, None)
    }

    fn status(&self, args: &[&str]) -> Result<()> {
        status(args, None)
    }
}

/// Execute a session-construction plan against the tmux CLI, capturing pane
/// ids between steps so subsequent steps can target them directly.
fn execute_session_plan(plan: &[SessionStep]) -> Result<()> {
    execute_session_plan_with(&RealTmux, plan)
}

/// Generic-over-executor inner loop. Split out so tests can drive the
/// capture/status ordering and error paths with a fake executor.
fn execute_session_plan_with<E: TmuxExecutor>(executor: &E, plan: &[SessionStep]) -> Result<()> {
    let mut panes: std::collections::HashMap<PaneSlot, String> = std::collections::HashMap::new();

    for step in plan {
        let rendered = step.render_args();
        let resolved = resolve_args(&rendered, &panes)?;
        let arg_refs: Vec<&str> = resolved.iter().map(String::as_str).collect();

        if let Some(slot) = step.capture_into {
            let output = executor.capture(&arg_refs)?;
            let pane_id = output.trim().to_string();
            if pane_id.is_empty() {
                return Err(Error::failed(
                    "tmux returned an empty pane id for a captured step".to_string(),
                ));
            }
            panes.insert(slot, pane_id);
        } else {
            executor.status(&arg_refs)?;
        }
    }

    Ok(())
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
    // Always seed trusted roots so they're ready for the current or next launch.
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
/// inside a tmux session (i.e. the value is present and non-empty).
fn tmux_env_indicates_inside(tmux_var: Option<&str>) -> bool {
    tmux_var.is_some_and(|v| !v.is_empty())
}

/// Returns `true` when the process is already running inside a tmux session
/// (i.e. the `TMUX` environment variable is set and non-empty).
fn is_inside_tmux() -> bool {
    tmux_env_indicates_inside(std::env::var("TMUX").ok().as_deref())
}

/// Best-effort lookup of the controlling terminal's size in cells.
///
/// On Unix, crossterm probes stdout's fd via `ioctl(TIOCGWINSZ)`, so
/// this returns `None` when stdout is not connected to a TTY (e.g. CI,
/// piped invocations). On those callsites the caller should fall back
/// to tmux's default detached-session size.
fn current_terminal_size() -> Option<(u16, u16)> {
    crossterm::terminal::size().ok()
}

/// Absolute path to the currently-running binary, used to spawn `task
/// ui` in the left-ui pane.
///
/// Going through `std::env::current_exe()` instead of hardcoding the
/// string `"task"` removes the PATH dependency entirely: the spawned
/// pane works regardless of binary rename (e.g. `custom-task`),
/// `cargo run` (where the binary lives at `target/.../task` and may
/// not be on `$PATH`), or non-PATH installs.
///
/// Returns `None` only on pathological OS states where the kernel
/// cannot report the current executable. In that case the caller
/// should skip the left-ui pane and warn — session creation must
/// still succeed.
fn current_binary_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
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
        // Codium window lifecycle is independent of the tmux session, so
        // (re)opening it happens on both create and reattach paths.
        ensure_codium_running(repo_key, worktree_name, path, codium_trusted_roots);
    }

    let session = session_name(repo_key, worktree_name);
    if !has_session(&session) {
        // If the editor binary is missing (e.g. `hx` for Helix), tmux will
        // fail mid-plan on the `split-window … hx .` step; `spawn_error`
        // translates the resulting ENOENT into `Error::tool_missing` with
        // the correct install hint. Reattaching never spawns the editor.
        let startup = if process::command_exists(ExternalTool::Opencode.binary_name()) {
            SessionStartup::WithOpencode(opencode::launch_command(path))
        } else {
            // When opencode is missing, we still open the session — the
            // primary pane just starts as a plain shell. For Helix, that
            // means opencode's pane falls back to a shell while the `hx`
            // pane is still spawned.
            process::warn(
                "'opencode' is not available; the primary pane will start as a plain shell.",
            );
            SessionStartup::ShellOnly
        };

        // Size the detached session to the attaching client's terminal
        // so the left-ui pane's `-l 40` split holds its absolute width
        // on attach. Falls back to tmux defaults when no TTY is
        // available (e.g. CI), at which point the pane scales
        // proportionally on attach — acceptable for non-interactive runs.
        let window_size = current_terminal_size();

        // Resolve our own binary path so the left-ui pane spawns the
        // running binary regardless of how it's named or whether it
        // lives on $PATH (e.g. `cargo run`, renamed installs). On the
        // rare OS error, skip the pane and warn — the rest of the
        // session must still come up.
        let task_binary = current_binary_path();
        if task_binary.is_none() {
            process::warn(
                "Could not resolve own binary path; the left-ui status pane will be skipped this session.",
            );
        }
        let plan = build_session_plan(
            &session,
            path,
            editor,
            &startup,
            window_size,
            task_binary.as_deref(),
        );
        execute_session_plan(&plan)?;
    }

    if is_inside_tmux() {
        status(&["switch-client", "-t", &session], None)?;
    } else {
        status(&["attach-session", "-t", &session], None)?;
    }

    Ok(OpenResult::Attached)
}

pub fn park(repo_key: &str, worktree_name: &str, path: &Path) -> Result<ParkResult> {
    // The previous implementation took a `title` used to override the
    // OpenCode session title on park; OpenCode >= 1.4.3 auto-derives
    // titles from the conversation and our override was strictly
    // worse, so the parameter was dropped.
    let session = session_name(repo_key, worktree_name);
    let has_tmux_session = has_session_in(&session, Some(path));
    let mut result = ParkResult::AlreadyParked;

    for action in park_teardown_actions(has_tmux_session) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_windows(repo_key, worktree_name);
            }
            TeardownAction::KillTmuxSession => {
                status(&["kill-session", "-t", &session], Some(path))?;
                result = ParkResult::Parked;
            }
        }
    }

    Ok(result)
}

pub fn finish_session(repo_key: &str, worktree_name: &str, cwd: &Path) -> Result<()> {
    let tmux_available = is_available();
    let session = session_name(repo_key, worktree_name);

    for action in finish_teardown_actions(tmux_available) {
        match action {
            TeardownAction::CloseCodium => {
                let _ = close_windows(repo_key, worktree_name);
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
        Arg, PaneSlot, SessionStartup, SessionStep, TeardownAction, build_session_plan,
        finish_teardown_actions, is_inside_tmux, new_session_flags_and_command,
        park_teardown_actions, resolve_args, tmux_env_indicates_inside,
    };
    use crate::runtime::{
        config::EditorKind,
        process::{CommandPlan, ExternalTool},
    };

    /// Sentinel binary path used in tests to stand in for the real
    /// path returned by [`super::current_binary_path`]. Hard-coded so
    /// tests can assert against an exact value while remaining
    /// independent of the host machine.
    const TEST_TASK_BINARY: &str = "/usr/local/bin/task";

    /// Convenience: reproducible `Some(&Path)` used for the
    /// `task_binary` argument across plan tests.
    fn test_task_binary() -> &'static Path {
        Path::new(TEST_TASK_BINARY)
    }

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

        #[test]
        fn always_attempts_close_codium_even_without_session() {
            // Guards the cross-config regression fix: park must attempt
            // Codium cleanup regardless of tmux state so a task opened
            // under VSCodium and later parked after switching editors
            // does not leak its Codium window.
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

        #[test]
        fn always_attempts_close_codium_even_when_tmux_unavailable() {
            // Same cross-config invariant as park: finish must attempt
            // Codium cleanup regardless of the currently-configured editor.
            let actions = finish_teardown_actions(false);
            assert!(
                actions.contains(&TeardownAction::CloseCodium),
                "finish must always attempt to close Codium: {actions:?}"
            );
        }
    }

    mod is_inside_tmux_detection {
        use super::*;

        #[test]
        fn consistent_with_current_environment() {
            // Verify that `is_inside_tmux` agrees with the extracted pure
            // function when both inspect the same env snapshot.
            let expected = tmux_env_indicates_inside(std::env::var("TMUX").ok().as_deref());
            assert_eq!(is_inside_tmux(), expected);
        }

        #[test]
        fn detects_typical_socket_path() {
            assert!(tmux_env_indicates_inside(Some(
                "/tmp/tmux-1000/default,42,0"
            )));
        }

        #[test]
        fn rejects_empty_value() {
            assert!(!tmux_env_indicates_inside(Some("")));
        }

        #[test]
        fn rejects_absent_value() {
            assert!(!tmux_env_indicates_inside(None));
        }

        #[test]
        fn accepts_any_non_empty_string() {
            assert!(tmux_env_indicates_inside(Some("1")));
        }
    }

    mod new_session_flags_and_command {
        //! Tests for the `(flags, command)` builder used by `new-session`.
        //!
        //! Format flags (`-P -F '#{pane_id}'`) are *not* the builder's
        //! responsibility — they are emitted automatically by
        //! [`SessionStep::render_args`] for capturing steps. These tests
        //! focus only on the flags/command separation.
        use std::collections::HashMap;

        use super::*;

        fn render(args: &[Arg]) -> Vec<String> {
            let panes: HashMap<PaneSlot, String> = HashMap::new();
            resolve_args(args, &panes).expect("no pane slots in new-session args")
        }

        #[test]
        fn shell_only_returns_no_command() {
            let (flags, command) = new_session_flags_and_command(
                "repo-branch",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::ShellOnly,
                None,
            );
            assert_eq!(
                render(&flags),
                vec![
                    "new-session",
                    "-d",
                    "-s",
                    "repo-branch",
                    "-c",
                    "/tmp/wt/repo",
                ]
            );
            assert!(
                command.is_none(),
                "shell-only startup must not produce a trailing command: {command:?}"
            );
        }

        #[test]
        fn with_opencode_returns_command_separately() {
            let opencode_command = CommandPlan::for_tool(
                ExternalTool::Opencode,
                vec!["--session".to_string(), "ses_123".to_string()],
            );

            let (flags, command) = new_session_flags_and_command(
                "repo-branch",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::WithOpencode(opencode_command),
                None,
            );

            // Flags do not include the trailing command — that's what
            // SessionStep::capturing_with_command will glue together
            // (with `-P -F '#{pane_id}'` injected between them).
            assert_eq!(
                render(&flags),
                vec![
                    "new-session",
                    "-d",
                    "-s",
                    "repo-branch",
                    "-c",
                    "/tmp/wt/repo",
                ]
            );
            let command = command.expect("with_opencode must produce a command");
            assert_eq!(render(&command), vec!["opencode", "--session", "ses_123"]);
        }

        #[test]
        fn with_opencode_no_extra_args_returns_bare_program() {
            let opencode_command = CommandPlan::for_tool(ExternalTool::Opencode, vec![]);

            let (_flags, command) = new_session_flags_and_command(
                "repo-branch",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::WithOpencode(opencode_command),
                None,
            );

            let command = command.expect("with_opencode must produce a command");
            assert_eq!(render(&command), vec!["opencode"]);
        }

        #[test]
        fn preserves_path_with_spaces() {
            let (flags, _command) = new_session_flags_and_command(
                "my-session",
                Path::new("/home/user/my projects/repo"),
                &SessionStartup::ShellOnly,
                None,
            );

            assert_eq!(
                render(&flags),
                vec![
                    "new-session",
                    "-d",
                    "-s",
                    "my-session",
                    "-c",
                    "/home/user/my projects/repo",
                ]
            );
        }

        #[test]
        fn empty_session_name_still_produces_valid_structure() {
            let (flags, _command) = new_session_flags_and_command(
                "",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::ShellOnly,
                None,
            );

            // An empty session name is passed through unchanged — tmux itself
            // will reject it, but the builder should not panic or corrupt
            // surrounding arguments.
            assert_eq!(
                render(&flags),
                vec!["new-session", "-d", "-s", "", "-c", "/tmp/wt/repo"]
            );
        }

        #[test]
        fn flags_do_not_contain_format_flags() {
            // Format flags are the responsibility of
            // SessionStep::render_args. This test pins that the builder
            // does NOT inline them into `flags` (so the format-flag
            // injection logic stays single-sourced).
            let (flags, _) = new_session_flags_and_command(
                "repo-branch",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::ShellOnly,
                None,
            );
            let rendered = render(&flags);
            assert!(
                !rendered.iter().any(|a| a == "-P"),
                "flags must not include -P (it's emitted by render_args): {rendered:?}"
            );
            assert!(
                !rendered.iter().any(|a| a == "#{pane_id}"),
                "flags must not include #{{pane_id}} (it's emitted by render_args): {rendered:?}"
            );
        }

        #[test]
        fn with_explicit_window_size_emits_dash_x_dash_y_after_session_name() {
            // The detached session size is critical for the left-ui pane
            // to keep its absolute width on attach. Pin both presence
            // and ordering: `-x <cols>` and `-y <rows>` must appear
            // *after* `-s <session>` and *before* `-c <path>`.
            let (flags, _) = new_session_flags_and_command(
                "repo-branch",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::ShellOnly,
                Some((200, 50)),
            );
            assert_eq!(
                render(&flags),
                vec![
                    "new-session",
                    "-d",
                    "-s",
                    "repo-branch",
                    "-x",
                    "200",
                    "-y",
                    "50",
                    "-c",
                    "/tmp/wt/repo",
                ]
            );
        }

        #[test]
        fn without_explicit_window_size_omits_size_flags() {
            // `None` defers to tmux's default detached-session size
            // (typically 80×24). The fallback path must not emit
            // `-x` / `-y` at all.
            let (flags, _) = new_session_flags_and_command(
                "repo-branch",
                Path::new("/tmp/wt/repo"),
                &SessionStartup::ShellOnly,
                None,
            );
            let rendered = render(&flags);
            assert!(
                !rendered.iter().any(|a| a == "-x"),
                "no window_size means no -x flag: {rendered:?}"
            );
            assert!(
                !rendered.iter().any(|a| a == "-y"),
                "no window_size means no -y flag: {rendered:?}"
            );
        }
    }

    mod step_render {
        //! Tests for [`SessionStep::render_args`].
        //!
        //! These tests pin the load-bearing invariants:
        //! - capturing steps inject `-P -F '#{pane_id}'`,
        //! - injection happens *after* flags and *before* the optional
        //!   trailing command (which is what tmux expects),
        //! - non-capturing steps don't inject anything.
        use std::collections::HashMap;

        use super::*;

        fn render(step: &SessionStep) -> Vec<String> {
            let panes: HashMap<PaneSlot, String> = HashMap::new();
            resolve_args(&step.render_args(), &panes).expect("no pane slots in test step")
        }

        fn flags() -> Vec<Arg> {
            vec![
                Arg::literal("split-window"),
                Arg::literal("-c"),
                Arg::literal("/tmp/wt/repo"),
            ]
        }

        fn command() -> Vec<Arg> {
            vec![Arg::literal("task"), Arg::literal("ui")]
        }

        #[test]
        fn fire_step_emits_flags_only() {
            let step = SessionStep::fire(flags());
            assert_eq!(
                render(&step),
                vec!["split-window", "-c", "/tmp/wt/repo"],
                "fire step must emit flags only"
            );
        }

        #[test]
        fn fire_step_does_not_inject_format_flags() {
            // Symmetric guard against accidentally injecting `-P` into
            // non-capturing steps: that would confuse tmux (capture is
            // not requested) and silently swap in unintended args.
            let step = SessionStep::fire(flags());
            let rendered = render(&step);
            assert!(
                !rendered.iter().any(|a| a == "-P"),
                "fire step must not include -P: {rendered:?}"
            );
        }

        #[test]
        fn fire_with_command_appends_command_after_flags() {
            let step = SessionStep::fire_with_command(flags(), command());
            assert_eq!(
                render(&step),
                vec!["split-window", "-c", "/tmp/wt/repo", "task", "ui"]
            );
        }

        #[test]
        fn capturing_step_injects_format_flags_at_end() {
            // No trailing command → format flags land at the end.
            let step = SessionStep::capturing(flags(), PaneSlot::Primary);
            assert_eq!(
                render(&step),
                vec![
                    "split-window",
                    "-c",
                    "/tmp/wt/repo",
                    "-P",
                    "-F",
                    "#{pane_id}",
                ]
            );
        }

        #[test]
        fn capturing_with_command_places_format_flags_between_flags_and_command() {
            // The load-bearing invariant: tmux treats arguments after
            // the trailing shell command as part of that command's argv.
            // `-P -F '#{pane_id}'` MUST sit before `task ui`, not after,
            // or tmux would invoke `task ui -P -F '#{pane_id}'` and
            // print no pane id.
            let step = SessionStep::capturing_with_command(flags(), command(), PaneSlot::Primary);
            assert_eq!(
                render(&step),
                vec![
                    "split-window",
                    "-c",
                    "/tmp/wt/repo",
                    "-P",
                    "-F",
                    "#{pane_id}",
                    "task",
                    "ui",
                ]
            );
        }

        #[test]
        fn format_flag_order_is_dash_p_dash_f_pane_id() {
            // tmux requires this exact triple to print the pane id; pin
            // the order so it can't silently flip.
            let step = SessionStep::capturing_with_command(flags(), command(), PaneSlot::Primary);
            let rendered = render(&step);
            let p_idx = rendered
                .iter()
                .position(|a| a == "-P")
                .expect("capturing render must include -P");
            assert_eq!(rendered[p_idx + 1], "-F");
            assert_eq!(rendered[p_idx + 2], "#{pane_id}");
        }

        #[test]
        fn capture_into_distinguishes_capturing_from_fire() {
            // The four constructors form a 2x2 (capture/no-capture ×
            // command/no-command). Pin that the capture flag follows
            // the constructor used.
            let fire = SessionStep::fire(flags());
            let fire_cmd = SessionStep::fire_with_command(flags(), command());
            let cap = SessionStep::capturing(flags(), PaneSlot::Primary);
            let cap_cmd =
                SessionStep::capturing_with_command(flags(), command(), PaneSlot::Primary);

            assert!(fire.capture_into.is_none());
            assert!(fire_cmd.capture_into.is_none());
            assert_eq!(cap.capture_into, Some(PaneSlot::Primary));
            assert_eq!(cap_cmd.capture_into, Some(PaneSlot::Primary));
        }
    }

    mod build_session_plan {
        use std::collections::HashMap;

        use super::*;

        fn opencode_startup() -> SessionStartup {
            SessionStartup::WithOpencode(CommandPlan::for_tool(ExternalTool::Opencode, vec![]))
        }

        /// Resolve a step's rendered args using fake pane ids for the
        /// primary and left-ui slots, so positional pane-id targets
        /// render as recognisable tokens that tests can assert against.
        ///
        /// Both slots are pre-populated because the set-hook steps
        /// reference [`PaneSlot::LeftUi`] via [`Arg::Format`]; rendering
        /// any step in the plan must therefore have a left-ui id
        /// available even if the individual step doesn't use it.
        ///
        /// Goes through [`SessionStep::render_args`] (not the raw `flags`
        /// field) so capturing steps' format flags are exercised here too.
        fn render_step(step: &SessionStep, primary: &str) -> Vec<String> {
            render_step_with_panes(step, primary, "%left-ui")
        }

        /// Variant of [`render_step`] that lets the caller pin both the
        /// primary and left-ui pane ids. Useful for assertions that
        /// inspect the rendered hook body and need a specific left-ui
        /// pane id baked in.
        fn render_step_with_panes(step: &SessionStep, primary: &str, left_ui: &str) -> Vec<String> {
            let mut panes = HashMap::new();
            panes.insert(PaneSlot::Primary, primary.to_string());
            panes.insert(PaneSlot::LeftUi, left_ui.to_string());
            resolve_args(&step.render_args(), &panes).expect("all pane slots resolvable")
        }

        #[test]
        fn vscodium_plan_has_seven_steps() {
            // Step 0: new-session.
            // Step 1: left-ui split (capturing).
            // Steps 2-4: client-attached / client-session-changed /
            //   client-resized hooks that re-pin the left-ui pane.
            // Step 5: vertical split for the shell pane.
            // Step 6: focus primary.
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Vscodium,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            assert_eq!(plan.len(), 7);
            let s0 = render_step(&plan[0], "%1");
            assert_eq!(s0[0], "new-session");
            // Step 1: the left UI pane.
            let s1 = render_step(&plan[1], "%1");
            assert_eq!(s1[0], "split-window");
            assert_eq!(s1[1], "-h");
            assert_eq!(s1[2], "-b");
            assert!(s1.iter().any(|a| a == TEST_TASK_BINARY));
            assert!(s1.iter().any(|a| a == "ui"));
            // Steps 2..=4: set-hook entries that protect the left-ui
            // pane width across (re)attaches and resizes.
            for (idx, step) in plan.iter().enumerate().take(5).skip(2) {
                let rendered = render_step(step, "%1");
                assert_eq!(
                    rendered[0], "set-hook",
                    "step {idx} should be a set-hook step: {rendered:?}"
                );
            }
            // Step 5: the editor-specific vertical split.
            let s5 = render_step(&plan[5], "%1");
            assert_eq!(s5[0], "split-window");
            assert_eq!(s5[1], "-v");
            // Step 6: focus the primary (opencode) pane.
            let s6 = render_step(&plan[6], "%1");
            assert_eq!(s6[0], "select-pane");
        }

        #[test]
        fn vscodium_plan_does_not_spawn_hx() {
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Vscodium,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            let flat: Vec<String> = plan
                .iter()
                .flat_map(|s| render_step(s, "%1").into_iter())
                .collect();
            assert!(
                !flat.iter().any(|a| a == "hx"),
                "vscodium plan must not spawn hx: {flat:?}"
            );
        }

        #[test]
        fn helix_plan_has_eight_steps_in_order() {
            // Step 0: new-session.
            // Step 1: left-ui split (capturing).
            // Steps 2-4: hooks protecting the left-ui pane width.
            // Step 5: helix split (right).
            // Step 6: shell split (under primary).
            // Step 7: focus primary.
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Helix,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            assert_eq!(plan.len(), 8);
            let s0 = render_step(&plan[0], "%1");
            assert_eq!(s0[0], "new-session");
            // Step 1: the left UI pane.
            let s1 = render_step(&plan[1], "%1");
            assert_eq!(s1[0], "split-window");
            assert_eq!(s1[1], "-h");
            assert_eq!(s1[2], "-b");
            assert!(s1.iter().any(|a| a == TEST_TASK_BINARY));
            assert!(s1.iter().any(|a| a == "ui"));
            // Steps 2..=4: set-hook entries protecting the left-ui
            // pane width across (re)attaches and resizes.
            for (idx, step) in plan.iter().enumerate().take(5).skip(2) {
                let rendered = render_step(step, "%1");
                assert_eq!(
                    rendered[0], "set-hook",
                    "step {idx} should be a set-hook step: {rendered:?}"
                );
            }
            // Step 5: horizontal split spawning hx on the right.
            let s5 = render_step(&plan[5], "%1");
            assert_eq!(s5[0], "split-window");
            assert_eq!(s5[1], "-h");
            assert!(s5.iter().any(|a| a == "hx"));
            assert!(s5.iter().any(|a| a == "."));
            // Step 6: vertical split, under the primary pane.
            let s6 = render_step(&plan[6], "%1");
            assert_eq!(s6[0], "split-window");
            assert_eq!(s6[1], "-v");
            // Step 7: focus on primary pane.
            let s7 = render_step(&plan[7], "%1");
            assert_eq!(s7[0], "select-pane");
        }

        #[test]
        fn helix_plan_targets_right_pane_with_hx() {
            // The hx pane is created by step 5 (after new-session,
            // the left-ui split, and the three left-ui-protecting
            // hooks).
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Helix,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            let hx_step = render_step(&plan[5], "%1");
            let hx_idx = hx_step
                .iter()
                .position(|a| a == "hx")
                .expect("hx argument present");
            assert_eq!(hx_step[hx_idx + 1], ".");
        }

        #[test]
        fn helix_plan_preserves_worktree_cwd_on_every_split() {
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/custom-path"),
                EditorKind::Helix,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            for (idx, step) in plan.iter().enumerate() {
                let rendered = render_step(step, "%1");
                if rendered[0] == "new-session" || rendered[0] == "split-window" {
                    let cwd_idx = rendered
                        .iter()
                        .position(|a| a == "-c")
                        .unwrap_or_else(|| panic!("step {idx} missing -c: {rendered:?}"));
                    assert_eq!(rendered[cwd_idx + 1], "/wt/custom-path");
                }
            }
        }

        #[test]
        fn helix_plan_without_opencode_still_spawns_hx() {
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Helix,
                &SessionStartup::ShellOnly,
                None,
                Some(test_task_binary()),
            );
            // new-session in shell-only mode has no trailing command but
            // still requests pane-id capture via `-P -F '#{pane_id}'`,
            // appended after the regular flags by SessionStep::render_args.
            let s0 = render_step(&plan[0], "%1");
            assert_eq!(
                s0,
                vec![
                    "new-session",
                    "-d",
                    "-s",
                    "repo-branch",
                    "-c",
                    "/wt/repo",
                    "-P",
                    "-F",
                    "#{pane_id}",
                ]
            );
            // Helix still gets its own pane. With the left-ui split
            // and its three protective hooks landing as steps 1..=4,
            // the hx split is now step 5.
            let hx_step = render_step(&plan[5], "%1");
            assert!(hx_step.iter().any(|a| a == "hx"));
        }

        #[test]
        fn first_step_captures_primary_pane_id() {
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Vscodium,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            assert_eq!(plan[0].capture_into, Some(PaneSlot::Primary));
        }

        #[test]
        fn splits_target_captured_pane_id_not_positional_index() {
            // With a non-zero pane-base-index in a user's tmux config,
            // `session:0.0` would target a non-existent pane. Every
            // step that references a pane must do so via a captured
            // pane id (`%N`), never via the positional `session:0` /
            // `session:0.0` form.
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Helix,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            // No step at any index may reference a positional pane
            // target, regardless of which pane (primary or left-ui)
            // it operates on.
            for (idx, step) in plan.iter().enumerate() {
                let rendered = render_step_with_panes(step, "%7", "%9");
                assert!(
                    !rendered
                        .iter()
                        .any(|a| a == "repo-branch:0" || a == "repo-branch:0.0"),
                    "step {idx} must not target positional index: {rendered:?}"
                );
            }
            // Every step after the new-session step references at least
            // one captured pane id — the primary (%7) for splits and
            // select-pane, the left-ui id (%9) inside the set-hook
            // bodies.
            for (idx, step) in plan.iter().enumerate().skip(1) {
                let rendered = render_step_with_panes(step, "%7", "%9");
                let mentions_pane = rendered
                    .iter()
                    .any(|a| a.contains("%7") || a.contains("%9"));
                assert!(
                    mentions_pane,
                    "step {idx} should reference at least one captured pane id: {rendered:?}"
                );
            }
        }

        #[test]
        fn resolve_args_errors_when_slot_missing() {
            let empty: HashMap<PaneSlot, String> = HashMap::new();
            let err = resolve_args(
                &[Arg::literal("-t"), Arg::PaneSlot(PaneSlot::Primary)],
                &empty,
            )
            .expect_err("resolve should fail without captured slot");
            assert!(err.to_string().contains("pane slot"));
        }

        #[test]
        fn resolve_args_format_substitutes_pane_id_into_template() {
            // The set-hook body relies on this substitution: a single
            // string argument with the captured pane id baked in via
            // `{}`. Pin the substitution so the hook body can never
            // silently render with the literal `{}` placeholder.
            let mut panes = HashMap::new();
            panes.insert(PaneSlot::LeftUi, "%42".to_string());
            let resolved = resolve_args(
                &[Arg::format(
                    "resize-pane -t {} -x 40".to_string(),
                    PaneSlot::LeftUi,
                )],
                &panes,
            )
            .expect("format with populated slot should resolve");
            assert_eq!(resolved, vec!["resize-pane -t %42 -x 40"]);
        }

        #[test]
        fn resolve_args_format_errors_when_slot_missing() {
            // Same failure mode as Arg::PaneSlot: referencing an
            // unpopulated slot must fail loudly rather than render an
            // empty string into the hook body.
            let empty: HashMap<PaneSlot, String> = HashMap::new();
            let err = resolve_args(
                &[Arg::format(
                    "resize-pane -t {} -x 40".to_string(),
                    PaneSlot::LeftUi,
                )],
                &empty,
            )
            .expect_err("format with missing slot must fail");
            assert!(err.to_string().contains("pane slot"));
        }

        // ── left-ui pane ─────────────────────────────────────────────

        /// Locate the step that creates the left-ui pane by inspecting
        /// arg shape. Returns its index and rendered args. Centralises
        /// the lookup so the assertions stay independent of the exact
        /// position the pane lands in for each editor layout.
        ///
        /// `-b` (split before) and a trailing `ui` together uniquely
        /// identify the left-ui split, since no other step in the plan
        /// uses the `-b` flag.
        fn left_ui_step(plan: &[SessionStep]) -> (usize, Vec<String>) {
            plan.iter()
                .enumerate()
                .map(|(idx, step)| (idx, render_step(step, "%1")))
                .find(|(_, args)| {
                    args.first().map(String::as_str) == Some("split-window")
                        && args.iter().any(|a| a == "-b")
                        && args.iter().any(|a| a == "ui")
                })
                .expect("plan should contain a left-ui split step")
        }

        #[test]
        fn left_ui_pane_inserted_immediately_after_new_session() {
            // The left-ui split must run before any editor-specific
            // splits so the editor area sits to the right of it.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (idx, _) = left_ui_step(&plan);
                assert_eq!(
                    idx, 1,
                    "left-ui split must be step 1 (right after new-session) for {editor:?}"
                );
            }
        }

        #[test]
        fn left_ui_pane_uses_fixed_40_cell_width() {
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (_, args) = left_ui_step(&plan);
                let l_idx = args
                    .iter()
                    .position(|a| a == "-l")
                    .expect("left-ui split should pin width with -l");
                assert_eq!(
                    args[l_idx + 1],
                    "40",
                    "left-ui pane must be 40 cells wide for {editor:?}: {args:?}"
                );
            }
        }

        #[test]
        fn left_ui_pane_runs_supplied_binary_with_ui_subcommand_unscoped() {
            // The pane must spawn the *exact* binary supplied by the
            // caller (typically the running executable's absolute
            // path), with `ui` and no further arguments — so every
            // active worktree is visible at a glance regardless of
            // how the binary was installed.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (_, args) = left_ui_step(&plan);
                let bin_idx = args
                    .iter()
                    .position(|a| a == TEST_TASK_BINARY)
                    .unwrap_or_else(|| {
                        panic!(
                            "left-ui split should run the supplied binary path for {editor:?}: {args:?}"
                        )
                    });
                assert_eq!(
                    args[bin_idx + 1],
                    "ui",
                    "left-ui pane must run `<binary> ui` for {editor:?}: {args:?}"
                );
                assert!(
                    args.get(bin_idx + 2).is_none(),
                    "left-ui pane must run `<binary> ui` unscoped for {editor:?}: {args:?}"
                );
            }
        }

        #[test]
        fn plan_without_task_binary_omits_left_ui_pane_and_hooks() {
            // When the caller cannot resolve a binary path (e.g. the
            // pathological case where `current_exe` errors), the plan
            // must drop both the left-ui split AND its protective
            // hooks entirely, rather than spawn a broken pane or
            // register hooks that reference an unpopulated pane slot.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    None,
                );
                assert!(
                    !plan.iter().any(|step| {
                        let rendered = render_step(step, "%1");
                        rendered.iter().any(|a| a == "-b")
                    }),
                    "no `-b` (left-ui) split must be emitted when task_binary is None for {editor:?}"
                );
                assert!(
                    !plan.iter().any(|step| {
                        let rendered = render_step(step, "%1");
                        rendered.first().map(String::as_str) == Some("set-hook")
                    }),
                    "no set-hook step must be emitted when task_binary is None for {editor:?} \
                     (the hooks reference the left-ui pane id, which is never captured in this case)"
                );
                // Editor-specific layout must still be intact: helix
                // gets its hx pane, both editors finish with focus on
                // the primary pane.
                if matches!(editor, EditorKind::Helix) {
                    assert!(
                        plan.iter().any(|step| {
                            render_step(step, "%1")
                                .iter()
                                .any(|a| a == ExternalTool::Helix.binary_name())
                        }),
                        "helix layout must still spawn hx without a left-ui pane"
                    );
                }
                let last = plan.last().expect("plan is never empty");
                let rendered = render_step(last, "%1");
                assert_eq!(
                    rendered[0], "select-pane",
                    "plan must still finish with select-pane for {editor:?}: {rendered:?}"
                );
            }
        }

        // ── left-ui resize hooks ─────────────────────────────────────

        /// All set-hook steps in `plan`, rendered with the supplied
        /// pane ids substituted in.
        fn set_hook_steps(plan: &[SessionStep], left_ui: &str) -> Vec<Vec<String>> {
            plan.iter()
                .map(|step| render_step_with_panes(step, "%primary", left_ui))
                .filter(|args| args.first().map(String::as_str) == Some("set-hook"))
                .collect()
        }

        #[test]
        fn left_ui_protected_by_three_hooks_covering_attach_switch_and_resize() {
            // Each event that can rescale the left-ui pane has a
            // corresponding hook that re-pins it. Pin the exact set
            // of hook names so we don't silently lose coverage of one.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let hooks = set_hook_steps(&plan, "%9");
                let hook_names: Vec<String> = hooks
                    .iter()
                    .map(|args| {
                        // Layout: ["set-hook", "-a", "-t", <session>, <name>, <body>]
                        args.get(4).cloned().unwrap_or_default()
                    })
                    .collect();
                assert_eq!(
                    hook_names,
                    vec![
                        "client-attached".to_string(),
                        "client-session-changed".to_string(),
                        "client-resized".to_string(),
                    ],
                    "left-ui pane must be protected by exactly the three rescale-triggering hooks for {editor:?}"
                );
            }
        }

        #[test]
        fn left_ui_hook_body_resizes_captured_pane_to_pinned_width() {
            // The set-hook body must embed the captured left-ui pane
            // id (not a literal placeholder, not the primary id) and
            // pin the width to 40 cells. Render with a recognisable
            // sentinel for the left-ui id so we can assert the exact
            // body string.
            let plan = build_session_plan(
                "repo-branch",
                Path::new("/wt/repo"),
                EditorKind::Vscodium,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            let hooks = set_hook_steps(&plan, "%left-ui-id");
            assert!(!hooks.is_empty(), "expected at least one set-hook step");
            for hook in &hooks {
                // Layout: ["set-hook", "-a", "-t", <session>, <name>, <body>]
                assert_eq!(hook[0], "set-hook");
                assert_eq!(
                    hook[1], "-a",
                    "hook must use -a (append) to coexist with global hooks"
                );
                assert_eq!(hook[2], "-t");
                assert_eq!(
                    hook[3], "repo-branch",
                    "hook must be scoped to this session"
                );
                let body = hook
                    .get(5)
                    .expect("set-hook step must include a body argument");
                assert_eq!(
                    body, "resize-pane -t %left-ui-id -x 40",
                    "hook body must resize the captured left-ui pane to 40 cells"
                );
            }
        }

        #[test]
        fn left_ui_hooks_appear_immediately_after_left_ui_split() {
            // The hooks must be installed *after* the left-ui pane is
            // captured (otherwise the body's left-ui slot is empty)
            // and *before* the editor-specific splits (so they're in
            // place by the time the user attaches).
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (left_ui_idx, _) = left_ui_step(&plan);
                for offset in 1..=3 {
                    let step = &plan[left_ui_idx + offset];
                    let rendered = render_step(step, "%1");
                    assert_eq!(
                        rendered.first().map(String::as_str),
                        Some("set-hook"),
                        "step {} (offset {} from left-ui split) should be a set-hook step for {editor:?}: {rendered:?}",
                        left_ui_idx + offset,
                        offset
                    );
                }
            }
        }

        #[test]
        fn left_ui_pane_targets_primary_with_before_flag() {
            // `-h -b -t %primary` makes tmux place the new pane on the
            // *left* of the primary pane (otherwise it would land on
            // the right). Capture this invariant explicitly so the
            // direction can't silently flip.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (_, args) = left_ui_step(&plan);
                assert!(
                    args.iter().any(|a| a == "-h"),
                    "left-ui split must be horizontal for {editor:?}: {args:?}"
                );
                assert!(
                    args.iter().any(|a| a == "-b"),
                    "left-ui split must use -b (before) for {editor:?}: {args:?}"
                );
                let t_idx = args
                    .iter()
                    .position(|a| a == "-t")
                    .expect("left-ui split must target a pane with -t");
                assert_eq!(
                    args[t_idx + 1],
                    "%1",
                    "left-ui split must target the captured primary pane id for {editor:?}: {args:?}"
                );
            }
        }

        #[test]
        fn left_ui_pane_uses_worktree_cwd() {
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/custom-path"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (_, args) = left_ui_step(&plan);
                let cwd_idx = args
                    .iter()
                    .position(|a| a == "-c")
                    .expect("left-ui split must pass -c <path>");
                assert_eq!(
                    args[cwd_idx + 1],
                    "/wt/custom-path",
                    "left-ui pane must inherit the worktree cwd for {editor:?}: {args:?}"
                );
            }
        }

        #[test]
        fn left_ui_pane_captures_pane_id_for_hook_targets() {
            // The left-ui split must capture its pane id into
            // PaneSlot::LeftUi so the immediately-following set-hook
            // steps can re-pin its width on every (re)attach.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let (idx, _) = left_ui_step(&plan);
                assert_eq!(
                    plan[idx].capture_into,
                    Some(PaneSlot::LeftUi),
                    "left-ui split must capture into PaneSlot::LeftUi for {editor:?}"
                );
            }
        }

        #[test]
        fn final_select_pane_targets_primary_not_left_ui() {
            // Focus must land on the editor/opencode side, not the
            // status-only `task ui` pane the user just spun up.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let last = plan.last().expect("plan is never empty");
                let rendered = render_step(last, "%1");
                assert_eq!(rendered[0], "select-pane");
                let t_idx = rendered
                    .iter()
                    .position(|a| a == "-t")
                    .expect("select-pane must use -t");
                assert_eq!(
                    rendered[t_idx + 1],
                    "%1",
                    "final select-pane must target primary (%1), not the left-ui pane: {rendered:?}"
                );
            }
        }

        // ── window size ──────────────────────────────────────────────

        #[test]
        fn plan_with_explicit_size_passes_size_to_new_session() {
            // The detached session must inherit the caller-supplied size
            // so the left-ui split holds its absolute width on attach.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    Some((150, 40)),
                    Some(test_task_binary()),
                );
                let s0 = render_step(&plan[0], "%1");
                let x_idx = s0
                    .iter()
                    .position(|a| a == "-x")
                    .unwrap_or_else(|| panic!("step 0 missing -x for {editor:?}: {s0:?}"));
                assert_eq!(s0[x_idx + 1], "150");
                let y_idx = s0
                    .iter()
                    .position(|a| a == "-y")
                    .unwrap_or_else(|| panic!("step 0 missing -y for {editor:?}: {s0:?}"));
                assert_eq!(s0[y_idx + 1], "40");
            }
        }

        #[test]
        fn plan_without_explicit_size_omits_size_flags_from_new_session() {
            // `None` defers to tmux's default detached-session size; the
            // builder must not synthesise its own.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let plan = build_session_plan(
                    "repo-branch",
                    Path::new("/wt/repo"),
                    editor,
                    &opencode_startup(),
                    None,
                    Some(test_task_binary()),
                );
                let s0 = render_step(&plan[0], "%1");
                assert!(
                    !s0.iter().any(|a| a == "-x"),
                    "no window_size means no -x for {editor:?}: {s0:?}"
                );
                assert!(
                    !s0.iter().any(|a| a == "-y"),
                    "no window_size means no -y for {editor:?}: {s0:?}"
                );
            }
        }
    }

    mod execute_session_plan {
        use std::{
            cell::RefCell,
            collections::VecDeque,
            path::{Path, PathBuf},
        };

        use super::{
            super::{
                Arg, PaneSlot, SessionStartup, SessionStep, TmuxExecutor, build_session_plan,
                execute_session_plan_with,
            },
            test_task_binary,
        };
        use crate::{
            error::{Error, Result},
            runtime::{
                config::EditorKind,
                process::{CommandPlan, ExternalTool},
            },
        };

        /// Recorded tmux invocation made by [`FakeTmux`].
        #[derive(Debug, Clone, PartialEq, Eq)]
        enum FakeCall {
            Capture(Vec<String>),
            Status(Vec<String>),
        }

        /// Test double for [`TmuxExecutor`]. Records every call in order and
        /// returns scripted responses for `capture`; `status` is always
        /// successful unless overridden.
        struct FakeTmux {
            capture_responses: RefCell<VecDeque<Result<String>>>,
            status_responses: RefCell<VecDeque<Result<()>>>,
            calls: RefCell<Vec<FakeCall>>,
        }

        impl FakeTmux {
            fn with_capture_responses<I>(responses: I) -> Self
            where
                I: IntoIterator<Item = Result<String>>,
            {
                Self {
                    capture_responses: RefCell::new(responses.into_iter().collect()),
                    status_responses: RefCell::new(VecDeque::new()),
                    calls: RefCell::new(Vec::new()),
                }
            }

            /// Override scripted `status` results, consumed in call order.
            /// Omitted entries default to `Ok(())`.
            fn with_status_responses<I>(mut self, responses: I) -> Self
            where
                I: IntoIterator<Item = Result<()>>,
            {
                self.status_responses = RefCell::new(responses.into_iter().collect());
                self
            }

            fn calls(&self) -> Vec<FakeCall> {
                self.calls.borrow().clone()
            }
        }

        impl TmuxExecutor for FakeTmux {
            fn capture(&self, args: &[&str]) -> Result<String> {
                self.calls.borrow_mut().push(FakeCall::Capture(owned(args)));
                self.capture_responses
                    .borrow_mut()
                    .pop_front()
                    .unwrap_or_else(|| Ok(String::new()))
            }

            fn status(&self, args: &[&str]) -> Result<()> {
                self.calls.borrow_mut().push(FakeCall::Status(owned(args)));
                self.status_responses
                    .borrow_mut()
                    .pop_front()
                    .unwrap_or(Ok(()))
            }
        }

        fn owned(args: &[&str]) -> Vec<String> {
            args.iter().map(|s| (*s).to_string()).collect()
        }

        fn capturing_step() -> SessionStep {
            SessionStep::capturing(
                vec![
                    Arg::literal("new-session"),
                    Arg::literal("-s"),
                    Arg::literal("demo"),
                ],
                PaneSlot::Primary,
            )
        }

        fn status_step_referencing_primary() -> SessionStep {
            SessionStep::fire(vec![
                Arg::literal("split-window"),
                Arg::literal("-t"),
                Arg::PaneSlot(PaneSlot::Primary),
            ])
        }

        #[test]
        fn single_capture_step_populates_pane_slot() {
            let fake = FakeTmux::with_capture_responses([Ok("%42\n".to_string())]);
            execute_session_plan_with(&fake, &[capturing_step()])
                .expect("single capturing step should succeed");

            let calls = fake.calls();
            assert_eq!(calls.len(), 1, "expected exactly one call: {calls:?}");
            // SessionStep::render_args injects `-P -F #{pane_id}` for
            // capturing steps, so the executor sees them appended to
            // the original flags.
            assert!(
                matches!(
                    &calls[0],
                    FakeCall::Capture(args)
                        if args == &["new-session", "-s", "demo", "-P", "-F", "#{pane_id}"]
                ),
                "expected a capture call with format flags appended, got {calls:?}"
            );
        }

        #[test]
        fn capture_then_status_resolves_slot_after_capture() {
            // This is the load-bearing ordering test: the pane id returned
            // by step 0's capture must be visible to step 1's resolve_args
            // before the executor dispatches the status call.
            let fake = FakeTmux::with_capture_responses([Ok("%99".to_string())]);
            execute_session_plan_with(
                &fake,
                &[capturing_step(), status_step_referencing_primary()],
            )
            .expect("two-step plan should succeed");

            let calls = fake.calls();
            assert_eq!(calls.len(), 2, "expected two calls in order: {calls:?}");
            assert!(matches!(&calls[0], FakeCall::Capture(_)));
            match &calls[1] {
                FakeCall::Status(args) => {
                    assert_eq!(
                        args,
                        &["split-window", "-t", "%99"],
                        "status args should have primary slot resolved to captured pane id"
                    );
                }
                other => panic!("expected status after capture, got {other:?}"),
            }
        }

        #[test]
        fn empty_pane_id_response_produces_actionable_error() {
            let fake = FakeTmux::with_capture_responses([Ok(String::new())]);
            let err = execute_session_plan_with(&fake, &[capturing_step()])
                .expect_err("empty pane id must abort the plan");
            assert!(
                err.to_string().contains("empty pane id"),
                "error should mention empty pane id, got: {err}"
            );
        }

        #[test]
        fn whitespace_only_pane_id_is_treated_as_empty() {
            let fake = FakeTmux::with_capture_responses([Ok("   \n\t".to_string())]);
            let err = execute_session_plan_with(&fake, &[capturing_step()])
                .expect_err("whitespace-only pane id must abort the plan");
            assert!(
                err.to_string().contains("empty pane id"),
                "error should mention empty pane id, got: {err}"
            );
        }

        #[test]
        fn capture_error_propagates_and_stops_execution() {
            let fake = FakeTmux::with_capture_responses([Err(Error::failed("boom"))]);
            let err = execute_session_plan_with(
                &fake,
                &[capturing_step(), status_step_referencing_primary()],
            )
            .expect_err("capture failure must abort the plan");
            assert!(
                err.to_string().contains("boom"),
                "error should propagate: {err}"
            );

            let calls = fake.calls();
            assert_eq!(
                calls.len(),
                1,
                "no further steps should run after a capture failure: {calls:?}"
            );
            assert!(matches!(calls[0], FakeCall::Capture(_)));
        }

        #[test]
        fn status_error_propagates_and_stops_execution() {
            let plan = vec![
                SessionStep::fire(vec![
                    Arg::literal("select-window"),
                    Arg::literal("-t"),
                    Arg::literal("demo"),
                ]),
                SessionStep::fire(vec![Arg::literal("kill-pane")]),
            ];
            let fake = FakeTmux::with_capture_responses([])
                .with_status_responses([Err(Error::failed("status boom"))]);
            let err = execute_session_plan_with(&fake, &plan)
                .expect_err("status failure must abort the plan");
            assert!(
                err.to_string().contains("status boom"),
                "error should propagate: {err}"
            );

            let calls = fake.calls();
            assert_eq!(
                calls.len(),
                1,
                "no further steps should run after a status failure: {calls:?}"
            );
            assert!(matches!(calls[0], FakeCall::Status(_)));
        }

        #[test]
        fn missing_slot_reference_errors_before_executor_called() {
            // A plan whose first step references an unpopulated PaneSlot
            // must fail resolution before any tmux call is issued, so that
            // malformed plans have no observable side effects.
            let fake = FakeTmux::with_capture_responses([]);
            let plan = vec![status_step_referencing_primary()];

            let err = execute_session_plan_with(&fake, &plan).expect_err("missing slot must error");
            assert!(
                err.to_string().contains("pane slot"),
                "error should mention pane slot, got: {err}"
            );
            assert!(
                fake.calls().is_empty(),
                "no tmux calls should be issued for an unresolvable plan"
            );
        }

        fn sample_path() -> PathBuf {
            PathBuf::from("/tmp/repo/branch")
        }

        fn opencode_startup() -> SessionStartup {
            SessionStartup::WithOpencode(CommandPlan::from_program("opencode", &[]))
        }

        #[test]
        fn vscodium_plan_round_trips_through_executor() {
            let plan = build_session_plan(
                "repo-branch",
                Path::new(sample_path().as_path()),
                EditorKind::Vscodium,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            // Two captures: step 0 (new-session → primary) and step 1
            // (left-ui split → left-ui, used as the target inside the
            // following set-hook bodies).
            let fake =
                FakeTmux::with_capture_responses([Ok("%3".to_string()), Ok("%9".to_string())]);

            execute_session_plan_with(&fake, &plan)
                .expect("vscodium plan should execute end-to-end");

            let calls = fake.calls();
            assert_eq!(calls.len(), 7, "vscodium plan has 7 steps: {calls:?}");
            assert!(matches!(calls[0], FakeCall::Capture(_)));
            assert!(matches!(calls[1], FakeCall::Capture(_)));
            for call in &calls[2..] {
                assert!(
                    matches!(call, FakeCall::Status(_)),
                    "steps after the captures must be status calls: {calls:?}"
                );
            }
            // The set-hook bodies (3..=5) embed the captured left-ui
            // pane id; the editor split and the final select-pane
            // (6..=7) reference the primary pane id.
            for call in &calls[2..=4] {
                if let FakeCall::Status(args) = call {
                    assert_eq!(args[0], "set-hook");
                    assert!(
                        args.iter().any(|a| a.contains("%9")),
                        "set-hook body must embed captured left-ui pane id %9: {args:?}"
                    );
                }
            }
            for call in &calls[5..] {
                if let FakeCall::Status(args) = call {
                    assert!(
                        args.contains(&"%3".to_string()),
                        "post-hook step must reference captured primary pane id %3: {args:?}"
                    );
                }
            }
        }

        #[test]
        fn helix_plan_round_trips_through_executor() {
            let plan = build_session_plan(
                "repo-branch",
                Path::new(sample_path().as_path()),
                EditorKind::Helix,
                &opencode_startup(),
                None,
                Some(test_task_binary()),
            );
            // Two captures: primary, left-ui.
            let fake =
                FakeTmux::with_capture_responses([Ok("%7".to_string()), Ok("%11".to_string())]);

            execute_session_plan_with(&fake, &plan).expect("helix plan should execute end-to-end");

            let calls = fake.calls();
            assert_eq!(calls.len(), 8, "helix plan has 8 steps: {calls:?}");
            assert!(matches!(calls[0], FakeCall::Capture(_)));
            assert!(matches!(calls[1], FakeCall::Capture(_)));
            for call in &calls[2..] {
                assert!(
                    matches!(call, FakeCall::Status(_)),
                    "steps after the captures must be status calls: {calls:?}"
                );
            }
            // The right-split step must spawn the hx binary.
            let has_hx = calls.iter().any(|call| match call {
                FakeCall::Status(args) => {
                    args.iter().any(|a| a == ExternalTool::Helix.binary_name())
                }
                FakeCall::Capture(_) => false,
            });
            assert!(has_hx, "helix plan should invoke hx: {calls:?}");
        }
    }

    mod park_does_not_touch_opencode {
        //! Regression guard: `park` must not read or write any OpenCode
        //! SQLite file. A previous implementation renamed the latest
        //! session's title through the DB; that behavior was dropped so
        //! park is now a pure tmux/codium teardown.
        //!
        //! We verify this end-to-end by pointing `XDG_DATA_HOME` at a
        //! tempdir with an `opencode-stable.db` file, recording its
        //! mtime before and after `park`, and asserting nothing
        //! changed.
        use std::{fs, io::Write, path::PathBuf, sync::Mutex, time::SystemTime};

        use super::super::park;

        /// Serialize tests that mutate process-wide env so they don't
        /// race with each other (or with any other test that reads
        /// `XDG_DATA_HOME`).
        static ENV_MUTEX: Mutex<()> = Mutex::new(());

        fn temp_xdg_with_opencode_db(tag: &str) -> PathBuf {
            let base = std::env::temp_dir().join(format!("task-rs-park-xdg-{tag}"));
            let _ = fs::remove_dir_all(&base);
            let opencode_dir = base.join("opencode");
            fs::create_dir_all(&opencode_dir).unwrap();
            let db_path = opencode_dir.join("opencode-stable.db");
            // Seed with a real-looking SQLite header so file size is
            // non-zero and any naive "did anything change" heuristic
            // has something to compare against.
            let mut f = fs::File::create(&db_path).unwrap();
            f.write_all(b"SQLite format 3\0").unwrap();
            base
        }

        #[test]
        fn park_on_unknown_session_does_not_touch_opencode_db() {
            let _guard = ENV_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
            let xdg = temp_xdg_with_opencode_db("untouched");
            let db_path = xdg.join("opencode").join("opencode-stable.db");

            let prev_xdg = std::env::var_os("XDG_DATA_HOME");
            // SAFETY: we hold ENV_MUTEX for the duration of the test,
            // which excludes any other park/opencode test from running
            // concurrently on the same process. Restoring the prior
            // value on exit keeps other (non-locked) tests unaffected.
            unsafe { std::env::set_var("XDG_DATA_HOME", &xdg) };

            let db_mtime_before = fs::metadata(&db_path).unwrap().modified().unwrap();
            let len_before = fs::metadata(&db_path).unwrap().len();

            // Pick a repo_key/worktree that cannot match any real tmux
            // session. `park` must then skip the KillTmuxSession action
            // and return `AlreadyParked`.
            let result = park(
                "github.com/nonexistent/park-test",
                "branch-that-is-not-real",
                &xdg, // any path; park only uses it for -C in status()
            );

            // Restore env *before* asserting so any panic unwinds
            // cleanly for later tests.
            match prev_xdg {
                Some(prev) => unsafe { std::env::set_var("XDG_DATA_HOME", prev) },
                None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
            }

            assert!(result.is_ok(), "park must succeed: {result:?}");

            let db_mtime_after = fs::metadata(&db_path).unwrap().modified().unwrap();
            let len_after = fs::metadata(&db_path).unwrap().len();

            assert_eq!(
                mtime_duration(db_mtime_before),
                mtime_duration(db_mtime_after),
                "park must not touch opencode-stable.db"
            );
            assert_eq!(
                len_before, len_after,
                "park must not change opencode-stable.db size"
            );

            let _ = fs::remove_dir_all(&xdg);
        }

        fn mtime_duration(t: SystemTime) -> std::time::Duration {
            t.duration_since(SystemTime::UNIX_EPOCH).unwrap()
        }
    }
}
