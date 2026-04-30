use crate::{
    error::{Error, Result},
    runtime::{
        config::{EditorKind, is_interactive_terminal},
        environment::RuntimeEnvironment,
        process::{self, ExternalTool},
        setup::{self, SetupApproval},
    },
    tools::opencode,
};

struct DoctorReport {
    missing_required: bool,
}

/// Tool availability classification for doctor output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Importance {
    Required,
    Recommended,
}

fn importance_for(tool: ExternalTool, editor: EditorKind) -> Importance {
    match tool {
        // `git` is the only unconditional hard requirement — almost every
        // code path shells out to it.
        ExternalTool::Git => Importance::Required,
        // `hx` is a hard requirement when the configured editor is Helix:
        // `task start`/`task open` refuses to create a new tmux session if
        // `hx` is not on PATH, so reporting it as only a warning would be
        // misleading. `codium` remains recommended because the VSCodium
        // workflow degrades gracefully when `codium` is missing.
        ExternalTool::Helix if matches!(editor, EditorKind::Helix) => Importance::Required,
        ExternalTool::Tmux
        | ExternalTool::Codium
        | ExternalTool::Helix
        | ExternalTool::Opencode
        | ExternalTool::Direnv
        | ExternalTool::Asdf
        | ExternalTool::Pnpm
        | ExternalTool::Corepack
        | ExternalTool::Node
        | ExternalTool::Cargo
        | ExternalTool::Nix => Importance::Recommended,
    }
}

/// Returns the subset of [`ExternalTool::all`] that doctor should report on
/// for the currently configured editor.
///
/// The two editor-specific tools (`codium`, `hx`) are only relevant for
/// their respective `EditorKind`; reporting both unconditionally produces
/// false-positive `[warn]` lines (e.g. warning about missing `hx` on a
/// default VSCodium setup).
fn expected_tools(editor: EditorKind) -> Vec<ExternalTool> {
    ExternalTool::all()
        .iter()
        .copied()
        .filter(|tool| match tool {
            ExternalTool::Codium => matches!(editor, EditorKind::Vscodium),
            ExternalTool::Helix => matches!(editor, EditorKind::Helix),
            ExternalTool::Git
            | ExternalTool::Tmux
            | ExternalTool::Opencode
            | ExternalTool::Direnv
            | ExternalTool::Asdf
            | ExternalTool::Pnpm
            | ExternalTool::Corepack
            | ExternalTool::Node
            | ExternalTool::Cargo
            | ExternalTool::Nix => true,
        })
        .collect()
}

pub fn run(env: &RuntimeEnvironment, fix: bool) -> Result<()> {
    let mut report = check(env);

    let action = decide_action(fix, report.missing_required, is_interactive_terminal());
    match action {
        DoctorAction::FixAndRecheck => {
            apply_fixes(env, SetupApproval::AssumeYes)?;
            println!("\nRe-running checks after fixes...\n");
            report = check(env);
        }
        DoctorAction::PromptAndMaybeRecheck => {
            let applied = apply_fixes(
                env,
                SetupApproval::Prompt("Issues found. Apply automatic fixes now?"),
            )?;
            if applied {
                println!("\nRe-running checks after fixes...\n");
                report = check(env);
            }
        }
        DoctorAction::ReportOnly => {}
    }

    if report.missing_required {
        return Err(Error::failed("Doctor check found missing dependencies"));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorAction {
    /// `--fix` was passed: apply fixes unconditionally and recheck.
    FixAndRecheck,
    /// Issues found in an interactive terminal: prompt the user.
    PromptAndMaybeRecheck,
    /// No issues, or non-interactive: just report.
    ReportOnly,
}

fn decide_action(fix: bool, missing_required: bool, interactive: bool) -> DoctorAction {
    if fix {
        return DoctorAction::FixAndRecheck;
    }
    if missing_required && interactive {
        return DoctorAction::PromptAndMaybeRecheck;
    }
    DoctorAction::ReportOnly
}

fn check(env: &RuntimeEnvironment) -> DoctorReport {
    let layout = env.layout();
    let mut missing_required = false;

    println!("repos_dir: {}", layout.repos_dir().display());
    println!("wt_dir: {}", layout.wt_dir().display());
    println!("detached_dir: {}", layout.detached_dir().display());

    let editor = env.tasks().editor();
    for tool in expected_tools(editor) {
        let binary = tool.binary_name();
        let importance = importance_for(tool, editor);
        let present = process::command_exists(binary);

        match (present, importance) {
            (true, _) => println!("[ok]      {binary}"),
            (false, Importance::Required) => {
                println!(
                    "[missing] {binary:<9} install: {hint}",
                    hint = tool.install_hint()
                );
                missing_required = true;
            }
            (false, Importance::Recommended) => {
                println!(
                    "[warn]    {binary:<9} install: {hint}",
                    hint = tool.install_hint()
                );
            }
        }
    }

    if layout.repos_dir().is_dir() && layout.wt_dir().is_dir() && layout.detached_dir().is_dir() {
        println!("[ok]      configured layout exists");
    } else {
        println!("[missing] configured layout does not exist");
        missing_required = true;
    }

    if process::command_exists("opencode") && opencode::auth_storage_reachable() {
        println!("[ok]      opencode auth storage reachable");
    } else if process::command_exists("opencode") {
        println!("[warn]    opencode auth storage not initialized yet");
    }

    DoctorReport { missing_required }
}

fn apply_fixes(env: &RuntimeEnvironment, approval: SetupApproval<'_>) -> Result<bool> {
    const GUIDANCE: &str = "'task doctor --fix' needs an interactive terminal because it applies local setup changes. Re-run in an interactive terminal, or run 'task doctor' for read-only diagnostics.";
    setup::apply_full_setup(env, approval, GUIDANCE)
}

#[cfg(test)]
mod tests {
    use super::{DoctorAction, Importance, decide_action, expected_tools, importance_for};
    use crate::runtime::{config::EditorKind, process::ExternalTool};

    mod decide_action {
        use super::*;

        #[test]
        fn fix_flag_always_triggers_fix_and_recheck() {
            assert_eq!(
                decide_action(true, false, false),
                DoctorAction::FixAndRecheck
            );
            assert_eq!(
                decide_action(true, true, false),
                DoctorAction::FixAndRecheck
            );
            assert_eq!(
                decide_action(true, false, true),
                DoctorAction::FixAndRecheck
            );
            assert_eq!(decide_action(true, true, true), DoctorAction::FixAndRecheck);
        }

        #[test]
        fn prompts_when_missing_and_interactive() {
            assert_eq!(
                decide_action(false, true, true),
                DoctorAction::PromptAndMaybeRecheck
            );
        }

        #[test]
        fn reports_only_when_nothing_missing() {
            assert_eq!(decide_action(false, false, true), DoctorAction::ReportOnly);
            assert_eq!(decide_action(false, false, false), DoctorAction::ReportOnly);
        }

        #[test]
        fn reports_only_when_missing_but_not_interactive() {
            assert_eq!(decide_action(false, true, false), DoctorAction::ReportOnly);
        }
    }

    mod importance {
        use super::*;

        #[test]
        fn git_is_required_regardless_of_editor() {
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                assert_eq!(
                    importance_for(ExternalTool::Git, editor),
                    Importance::Required,
                    "git should be required for {editor:?}"
                );
            }
        }

        #[test]
        fn helix_is_required_only_when_editor_is_helix() {
            // `task start`/`task open` hard-fails without `hx` when the
            // configured editor is Helix; doctor must reflect that.
            assert_eq!(
                importance_for(ExternalTool::Helix, EditorKind::Helix),
                Importance::Required
            );
            // When the editor is Vscodium, `hx` is filtered out entirely by
            // `expected_tools`, but the classifier must still treat it as
            // merely recommended so it can never be upgraded to required
            // by accident.
            assert_eq!(
                importance_for(ExternalTool::Helix, EditorKind::Vscodium),
                Importance::Recommended
            );
        }

        #[test]
        fn codium_is_never_required() {
            // The VSCodium workflow degrades gracefully when `codium` is
            // missing (open_window early-returns), so doctor should never
            // upgrade it to required.
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                assert_eq!(
                    importance_for(ExternalTool::Codium, editor),
                    Importance::Recommended,
                    "codium should stay recommended for {editor:?}"
                );
            }
        }

        #[test]
        fn other_tools_are_recommended() {
            for &tool in ExternalTool::all() {
                if tool == ExternalTool::Git {
                    continue;
                }
                // `Helix` is only conditionally required (see dedicated
                // test above); here we assert the editor-independent
                // baseline for all other tools.
                if tool == ExternalTool::Helix {
                    continue;
                }
                for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                    assert_eq!(
                        importance_for(tool, editor),
                        Importance::Recommended,
                        "{tool} should be recommended for {editor:?}, not required"
                    );
                }
            }
        }
    }

    mod expected_tools {
        use super::*;

        #[test]
        fn vscodium_includes_codium_and_excludes_helix() {
            let tools = expected_tools(EditorKind::Vscodium);
            assert!(
                tools.contains(&ExternalTool::Codium),
                "vscodium setup should still report codium"
            );
            assert!(
                !tools.contains(&ExternalTool::Helix),
                "vscodium setup should not warn about hx"
            );
        }

        #[test]
        fn helix_includes_helix_and_excludes_codium() {
            let tools = expected_tools(EditorKind::Helix);
            assert!(
                tools.contains(&ExternalTool::Helix),
                "helix setup should report hx"
            );
            assert!(
                !tools.contains(&ExternalTool::Codium),
                "helix setup should not warn about codium"
            );
        }

        #[test]
        fn editor_independent_tools_are_always_reported() {
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let tools = expected_tools(editor);
                for required in [
                    ExternalTool::Git,
                    ExternalTool::Tmux,
                    ExternalTool::Opencode,
                    ExternalTool::Nix,
                ] {
                    assert!(
                        tools.contains(&required),
                        "{required} should be reported for {editor:?}"
                    );
                }
            }
        }

        #[test]
        fn preserves_stable_ordering_from_external_tool_all() {
            // Doctor output order must stay predictable; filtering should
            // keep the original relative order defined by ExternalTool::all.
            let all: Vec<ExternalTool> = ExternalTool::all().to_vec();
            for editor in [EditorKind::Vscodium, EditorKind::Helix] {
                let filtered = expected_tools(editor);
                let reference: Vec<ExternalTool> = all
                    .iter()
                    .copied()
                    .filter(|tool| filtered.contains(tool))
                    .collect();
                assert_eq!(filtered, reference, "ordering changed for {editor:?}");
            }
        }
    }
}
