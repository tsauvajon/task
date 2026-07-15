use std::path::Path;

use crate::runtime::{
    config::EditorKind,
    process::{CommandPlan, ExternalTool},
};

/// Target width (in cells) of the status pane that runs `task ui`.
///
/// Sized to fit the Tasks view's `Branch / Session / Agent` columns when
/// the Repo column auto-hides via `pick_task_column_layout`. Budget:
/// 6 cells of table chrome + 20 cells branch + 8 cells Session + 5
/// cells Agent = 39 cells, with 1 cell of slack to make the rightmost
/// column read as a balanced block.
///
/// The pane is emitted as `size="<N>%"` rather than `size=40` because
/// Zellij locks panes with a fixed integer `size` from being resized
/// at runtime (<https://github.com/zellij-org/zellij/issues/1758>) —
/// even from Zellij's resize mode (`Ctrl+n`). The percentage is
/// derived from the actual terminal width via
/// [`status_pane_size_percent`] so the pane is exactly
/// `STATUS_PANE_WIDTH` cells at creation, and the user can resize
/// from there.
pub(super) const STATUS_PANE_WIDTH: u16 = 40;

/// Fallback percentage used when the terminal width is unknown
/// (typically a non-TTY context such as CI or a piped invocation).
///
/// Matches `STATUS_PANE_WIDTH` cells on a 160-cell terminal, which is
/// a reasonable "average widescreen" width. The same value falls out
/// of `status_pane_size_percent(160)`.
const STATUS_PANE_DEFAULT_PERCENT: u16 = 25;

/// Compute the `size="<N>%"` percentage that maps to
/// [`STATUS_PANE_WIDTH`] cells given the current terminal width.
///
/// Uses **ceiling** division so that Zellij's percentage→cells
/// computation (which truncates) never drops below the target width
/// by a cell. Result is clamped to `[5, 50]`:
///
/// - lower bound `5`: very wide terminals (>800 cells) still get a
///   visible status pane rather than a 1-cell sliver after Zellij
///   truncates.
/// - upper bound `50`: very narrow terminals (<80 cells) leave room
///   for the rest of the layout instead of being dominated by the
///   status pane.
#[must_use]
pub(super) fn status_pane_size_percent(terminal_width: u16) -> u16 {
    let target = u32::from(STATUS_PANE_WIDTH);
    let width = u32::from(terminal_width.max(1));
    let percent = target.saturating_mul(100).div_ceil(width);
    let clamped = percent.clamp(5, 50);
    // `clamp(5, 50)` keeps the value in `u16` range; the `as u16` is
    // safe by construction.
    u16::try_from(clamped).unwrap_or(STATUS_PANE_DEFAULT_PERCENT)
}

/// What goes in the primary pane when a session is freshly created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SessionStartup {
    /// `OpenCode` is not on PATH — the primary pane just spawns a shell.
    ShellOnly,
    /// `OpenCode` is available — the primary pane runs the resolved command.
    WithOpencode(CommandPlan),
}

/// Inputs to [`render_layout`]. Gathered into a struct because the
/// number of parameters would otherwise be unwieldy and call sites
/// would lose the labelled-keyword feel.
pub(super) struct LayoutInput<'a> {
    pub session: &'a str,
    pub path: &'a Path,
    pub editor: EditorKind,
    pub startup: SessionStartup,
    /// Absolute path to the running `task` binary. When `Some`, the
    /// layout includes a fixed-width left status pane running
    /// `<task_binary> ui`. When `None` (rare — only when
    /// `std::env::current_exe()` fails) the status pane is dropped so
    /// session creation can still succeed.
    pub task_binary: Option<&'a Path>,
    /// Current terminal width in cells. Used to derive the status
    /// pane's `size="<N>%"` percentage so it starts at exactly
    /// [`STATUS_PANE_WIDTH`] cells. `None` falls back to
    /// [`STATUS_PANE_DEFAULT_PERCENT`] (typically a non-TTY context
    /// like CI).
    pub terminal_width: Option<u16>,
}

/// Render the session layout as a Zellij KDL document.
///
/// The result is intended to be written to a temporary file and passed
/// to `zellij --session ... --new-session-with-layout <file>` (or to
/// `zellij action switch-session --layout <file> <session>` from inside
/// an existing Zellij session).
///
/// ## `VSCodium` layout
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
#[must_use]
pub(super) fn render_layout(input: &LayoutInput<'_>) -> String {
    let path_str = kdl_string(&input.path.to_string_lossy());
    let session_str = kdl_string(input.session);

    let mut out = String::new();
    out.push_str("layout {\n");
    out.push_str("    cwd ");
    out.push_str(&path_str);
    out.push('\n');
    out.push_str("    tab name=");
    out.push_str(&session_str);
    out.push_str(" split_direction=\"vertical\" {\n");

    if let Some(task_binary) = input.task_binary {
        render_status_pane(&mut out, task_binary, input.terminal_width);
    }

    render_primary_column(&mut out, &input.startup);

    if matches!(input.editor, EditorKind::Helix) {
        render_helix_pane(&mut out);
    }

    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn render_status_pane(out: &mut String, task_binary: &Path, terminal_width: Option<u16>) {
    let binary = kdl_string(&task_binary.to_string_lossy());
    let percent = terminal_width.map_or(STATUS_PANE_DEFAULT_PERCENT, status_pane_size_percent);
    out.push_str("        pane size=\"");
    out.push_str(&percent.to_string());
    out.push_str("%\" {\n");
    out.push_str("            command ");
    out.push_str(&binary);
    out.push('\n');
    out.push_str("            args \"ui\"\n");
    out.push_str("        }\n");
}

fn render_primary_column(out: &mut String, startup: &SessionStartup) {
    out.push_str("        pane split_direction=\"horizontal\" {\n");
    match startup {
        SessionStartup::ShellOnly => {
            out.push_str("            pane focus=true\n");
            out.push_str("            pane\n");
        }
        SessionStartup::WithOpencode(plan) => {
            let program = kdl_string(plan.program());
            out.push_str("            pane focus=true {\n");
            out.push_str("                command ");
            out.push_str(&program);
            out.push('\n');
            if !plan.args().is_empty() {
                out.push_str("                args");
                for arg in plan.args() {
                    out.push(' ');
                    out.push_str(&kdl_string(arg));
                }
                out.push('\n');
            }
            out.push_str("            }\n");
            out.push_str("            pane\n");
        }
    }
    out.push_str("        }\n");
}

fn render_helix_pane(out: &mut String) {
    let binary = kdl_string(ExternalTool::Helix.binary_name());
    out.push_str("        pane {\n");
    out.push_str("            command ");
    out.push_str(&binary);
    out.push('\n');
    out.push_str("            args \".\"\n");
    out.push_str("        }\n");
}

/// Format a Rust `&str` as a quoted KDL string literal, escaping
/// `\\`, `"`, newlines, carriage returns, and tabs. Returns a value
/// already surrounded by double quotes, ready to drop into a KDL
/// node argument position.
fn kdl_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        LayoutInput, STATUS_PANE_DEFAULT_PERCENT, STATUS_PANE_WIDTH, SessionStartup, kdl_string,
        render_layout, status_pane_size_percent,
    };
    use crate::runtime::{
        config::EditorKind,
        process::{CommandPlan, ExternalTool},
    };

    /// Standard reference terminal width used across layout tests.
    /// `status_pane_size_percent(160) == 25` so the produced layout
    /// contains the easy-to-spot `pane size="25%"` token.
    const REFERENCE_TERMINAL_WIDTH: u16 = 160;

    fn opencode_startup() -> SessionStartup {
        SessionStartup::WithOpencode(CommandPlan::for_tool(ExternalTool::Opencode, vec![]))
    }

    fn opencode_startup_with_args() -> SessionStartup {
        SessionStartup::WithOpencode(CommandPlan::for_tool(
            ExternalTool::Opencode,
            vec!["--session".to_owned(), "ses_123".to_owned()],
        ))
    }

    fn custom_opencode_startup_with_args() -> SessionStartup {
        SessionStartup::WithOpencode(CommandPlan::for_program(
            "/opt/OpenCode Shared/opencode-shared",
            vec!["--session".to_owned(), "ses_123".to_owned()],
        ))
    }

    fn task_binary() -> PathBuf {
        PathBuf::from("/usr/local/bin/task")
    }

    mod kdl_string {
        use super::kdl_string;

        #[test]
        fn wraps_bare_string_in_quotes() {
            assert_eq!(kdl_string("hello"), "\"hello\"");
        }

        #[test]
        fn escapes_backslashes() {
            assert_eq!(kdl_string("a\\b"), "\"a\\\\b\"");
        }

        #[test]
        fn escapes_inner_quotes() {
            assert_eq!(kdl_string("say \"hi\""), "\"say \\\"hi\\\"\"");
        }

        #[test]
        fn escapes_newlines_and_tabs() {
            assert_eq!(kdl_string("a\nb\tc"), "\"a\\nb\\tc\"");
        }

        #[test]
        fn empty_string_renders_as_empty_pair_of_quotes() {
            assert_eq!(kdl_string(""), "\"\"");
        }
    }

    mod render_layout {
        use super::*;

        #[test]
        fn vscodium_layout_has_status_primary_and_no_helix() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(layout.contains("layout {"));
            assert!(layout.contains("cwd \"/wt/repo\""));
            assert!(layout.contains("tab name=\"repo-branch\""));
            // 160 cells / 40 target → 25% percentage. The pane is
            // emitted as a percentage (not a fixed integer) so Zellij
            // allows runtime resizing.
            assert!(layout.contains("pane size=\"25%\""));
            assert!(layout.contains("command \"/usr/local/bin/task\""));
            assert!(layout.contains("args \"ui\""));
            assert!(layout.contains("command \"opencode\""));
            assert!(layout.contains("focus=true"));
            assert!(
                !layout.contains("command \"hx\""),
                "vscodium layout must not spawn hx: {layout}"
            );
        }

        #[test]
        fn helix_layout_adds_third_pane_running_hx() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Helix,
                startup: opencode_startup(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                layout.contains("command \"hx\""),
                "helix layout must spawn hx: {layout}"
            );
            assert!(
                layout.contains("args \".\""),
                "hx pane must run with '.': {layout}"
            );
        }

        #[test]
        fn shell_only_startup_omits_opencode_command() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: SessionStartup::ShellOnly,
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                !layout.contains("command \"opencode\""),
                "shell-only must omit opencode command: {layout}"
            );
            // Two bare panes (focus + shell) replace opencode/shell.
            assert!(layout.contains("pane focus=true"));
        }

        #[test]
        fn missing_task_binary_drops_status_pane() {
            let path = PathBuf::from("/wt/repo");
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup(),
                task_binary: None,
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                !layout.contains("pane size=\""),
                "missing task binary must drop the status pane: {layout}"
            );
            // Primary opencode pane still renders.
            assert!(layout.contains("command \"opencode\""));
        }

        #[test]
        fn opencode_args_are_emitted_after_command() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup_with_args(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                layout.contains("args \"--session\" \"ses_123\""),
                "opencode args must follow the command: {layout}"
            );
        }

        #[test]
        fn custom_opencode_program_is_emitted_as_one_command() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: custom_opencode_startup_with_args(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                layout.contains("command \"/opt/OpenCode Shared/opencode-shared\""),
                "custom executable must be one KDL command value: {layout}"
            );
            assert!(layout.contains("args \"--session\" \"ses_123\""));
        }

        #[test]
        fn path_with_quotes_is_escaped() {
            // Pathological but possible: a path containing a quote
            // character must be escaped to keep the KDL parseable.
            let path = PathBuf::from("/wt/has\"quote");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                layout.contains("cwd \"/wt/has\\\"quote\""),
                "quoted path must be escaped: {layout}"
            );
        }

        #[test]
        fn layout_is_wrapped_in_top_level_layout_block() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(layout.trim_start().starts_with("layout {"));
            assert!(layout.trim_end().ends_with('}'));
        }

        #[test]
        fn status_pane_uses_percentage_size_not_fixed_integer() {
            // Zellij locks panes with a fixed integer `size` from runtime
            // resize. The percentage form is what makes the status pane
            // resizable, even though the percentage is calibrated to map
            // to exactly STATUS_PANE_WIDTH cells at creation time.
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup(),
                task_binary: Some(&bin),
                terminal_width: Some(REFERENCE_TERMINAL_WIDTH),
            });

            assert!(
                !layout.contains(&format!("pane size={STATUS_PANE_WIDTH}")),
                "status pane must not use a fixed integer size: {layout}"
            );
            assert!(
                layout.contains("pane size=\""),
                "status pane must use a quoted percentage size: {layout}"
            );
        }

        #[test]
        fn missing_terminal_width_falls_back_to_default_percent() {
            let path = PathBuf::from("/wt/repo");
            let bin = task_binary();
            let layout = render_layout(&LayoutInput {
                session: "repo-branch",
                path: &path,
                editor: EditorKind::Vscodium,
                startup: opencode_startup(),
                task_binary: Some(&bin),
                terminal_width: None,
            });

            assert!(
                layout.contains(&format!("pane size=\"{STATUS_PANE_DEFAULT_PERCENT}%\"")),
                "missing terminal width must use the fallback default percent: {layout}"
            );
        }
    }

    mod status_pane_size_percent_tests {
        use super::{STATUS_PANE_WIDTH, status_pane_size_percent};

        #[test]
        fn exact_match_when_target_divides_terminal_width() {
            // 160 / 40 = 4 → 25%. 25% of 160 = 40 cells. Exact.
            assert_eq!(status_pane_size_percent(160), 25);
            // 200 / 40 = 5 → 20%. 20% of 200 = 40 cells.
            assert_eq!(status_pane_size_percent(200), 20);
            // 80 / 40 = 2 → 50%. 50% of 80 = 40 cells.
            assert_eq!(status_pane_size_percent(80), 50);
        }

        #[test]
        fn ceiling_division_prevents_undershoot() {
            // 40 * 100 / 120 = 33.33…; floor would yield 33% (= 39
            // cells, short of the target). Ceiling yields 34% so the
            // pane never starts narrower than STATUS_PANE_WIDTH due
            // to Zellij's percent→cells truncation.
            assert_eq!(status_pane_size_percent(120), 34);
            // Same property at 150.
            assert_eq!(status_pane_size_percent(150), 27);
        }

        #[test]
        fn clamps_to_upper_bound_when_terminal_is_narrower_than_target_doubled() {
            // 40-cell terminal: target would be 100%, clamped to 50%
            // so the rest of the layout still has visible panes.
            assert_eq!(status_pane_size_percent(40), 50);
            // Even a 20-cell terminal stays at 50%.
            assert_eq!(status_pane_size_percent(20), 50);
        }

        #[test]
        fn clamps_to_lower_bound_on_very_wide_terminals() {
            // 1600 cells: raw = ceil(4000/1600) = 3, clamped to 5
            // (status pane at least visible).
            assert_eq!(status_pane_size_percent(1600), 5);
            // 4000 cells: raw = 1, still clamped to 5.
            assert_eq!(status_pane_size_percent(4000), 5);
        }

        #[test]
        fn handles_zero_terminal_width_without_panic() {
            // Degenerate input — should not divide by zero. Falls
            // back to the upper clamp.
            assert_eq!(status_pane_size_percent(0), 50);
        }

        #[test]
        fn target_width_in_cells_round_trips_near_exact() {
            // Cross-check the contract for representative widths:
            // applying the returned percentage back to the terminal
            // width should land at-or-just-above STATUS_PANE_WIDTH.
            for terminal_width in [80, 100, 120, 150, 160, 200, 250, 300] {
                let percent = u32::from(status_pane_size_percent(terminal_width));
                let cells = (u32::from(terminal_width) * percent) / 100;
                let target = u32::from(STATUS_PANE_WIDTH);
                assert!(
                    cells >= target,
                    "terminal_width={terminal_width} percent={percent} yielded {cells} cells; \
                     must be >= {target}"
                );
                assert!(
                    cells <= target + 5,
                    "terminal_width={terminal_width} percent={percent} yielded {cells} cells; \
                     must be within 5 of {target}"
                );
            }
        }
    }
}
