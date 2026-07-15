use std::fmt;

use crate::{
    error::{Error, Result},
    runtime::{
        config::{EditorKind, OpenCodeCommand},
        environment::RuntimeEnvironment,
        process::{self, ExternalTool, InstallHint},
    },
    tools::opencode,
};

/// Tool availability classification for doctor output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Importance {
    Required,
    Recommended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingToolGuidance {
    Install(InstallHint),
    ConfiguredOpenCodeCommand,
}

const CUSTOM_OPENCODE_STATUS_NOTE: &str = "[info]    custom OpenCode launcher must exec stock \
    OpenCode and use the standard data directory for status/session detection";

impl fmt::Display for MissingToolGuidance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Install(hint) => write!(f, "install: {hint}"),
            Self::ConfiguredOpenCodeCommand => {
                f.write_str("configured OpenCode command is unavailable")
            }
        }
    }
}

fn missing_tool_guidance(
    tool: ExternalTool,
    opencode_command: &OpenCodeCommand,
) -> MissingToolGuidance {
    if tool == ExternalTool::Opencode && opencode_command != &OpenCodeCommand::default() {
        return MissingToolGuidance::ConfiguredOpenCodeCommand;
    }
    MissingToolGuidance::Install(tool.install_hint())
}

fn binary_for(tool: ExternalTool, opencode_command: &OpenCodeCommand) -> &str {
    if tool == ExternalTool::Opencode {
        return opencode_command.as_str();
    }
    tool.binary_name()
}

fn custom_opencode_status_note(command: &OpenCodeCommand) -> Option<&'static str> {
    (command != &OpenCodeCommand::default()).then_some(CUSTOM_OPENCODE_STATUS_NOTE)
}

const fn importance_for(tool: ExternalTool, editor: EditorKind) -> Importance {
    match tool {
        // `git` is the only unconditional hard requirement — almost every
        // code path shells out to it.
        ExternalTool::Git => Importance::Required,
        // `hx` is a hard requirement when the configured editor is Helix:
        // `task start`/`task open` refuses to create a new Zellij session if
        // `hx` is not on PATH, so reporting it as only a warning would be
        // misleading. `codium` remains recommended because the VSCodium
        // workflow degrades gracefully when `codium` is missing.
        ExternalTool::Helix if matches!(editor, EditorKind::Helix) => Importance::Required,
        ExternalTool::Zellij
        | ExternalTool::Codium
        | ExternalTool::Helix
        | ExternalTool::Opencode
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
/// default `VSCodium` setup).
fn expected_tools(editor: EditorKind) -> Vec<ExternalTool> {
    ExternalTool::all()
        .iter()
        .copied()
        .filter(|tool| match tool {
            ExternalTool::Codium => matches!(editor, EditorKind::Vscodium),
            ExternalTool::Helix => matches!(editor, EditorKind::Helix),
            ExternalTool::Git
            | ExternalTool::Zellij
            | ExternalTool::Opencode
            | ExternalTool::Cargo
            | ExternalTool::Nix => true,
        })
        .collect()
}

pub fn run(env: &RuntimeEnvironment) -> Result<()> {
    if check(env)? {
        return Err(Error::failed("Doctor check found missing dependencies"));
    }

    Ok(())
}

fn check(env: &RuntimeEnvironment) -> Result<bool> {
    let layout = env.layout();
    let mut missing_required = false;

    process::write_stdout_line(format_args!("repos_dir: {}", layout.repos_dir().display()))?;
    process::write_stdout_line(format_args!("wt_dir: {}", layout.wt_dir().display()))?;
    process::write_stdout_line(format_args!(
        "detached_dir: {}",
        layout.detached_dir().display()
    ))?;

    let editor = env.tasks().editor();
    let opencode_command = env.tasks().opencode_command();
    for tool in expected_tools(editor) {
        let binary = binary_for(tool, opencode_command);
        let importance = importance_for(tool, editor);
        let present = process::command_exists(binary);
        let guidance = missing_tool_guidance(tool, opencode_command);

        match (present, importance) {
            (true, _) => process::write_stdout_line(format_args!("[ok]      {binary}"))?,
            (false, Importance::Required) => {
                process::write_stdout_line(format_args!("[missing] {binary:<9} {guidance}"))?;
                missing_required = true;
            }
            (false, Importance::Recommended) => {
                process::write_stdout_line(format_args!("[warn]    {binary:<9} {guidance}"))?;
            }
        }
    }

    if process::command_exists(opencode_command.as_str()) {
        if let Some(note) = custom_opencode_status_note(opencode_command) {
            process::write_stdout_line(note)?;
        }
        if opencode::auth_storage_reachable(opencode_command) {
            process::write_stdout_line(format_args!(
                "[ok]      {opencode_command} auth storage reachable"
            ))?;
        } else {
            process::write_stdout_line(format_args!(
                "[warn]    {opencode_command} auth storage not initialized yet"
            ))?;
        }
    }

    Ok(missing_required)
}

#[cfg(test)]
mod tests {
    use super::{
        Importance, MissingToolGuidance, binary_for, custom_opencode_status_note, expected_tools,
        importance_for, missing_tool_guidance,
    };
    use crate::runtime::{
        config::{EditorKind, OpenCodeCommand},
        process::{ExternalTool, InstallHint},
    };

    mod missing_guidance {
        use super::*;

        #[test]
        fn stock_opencode_uses_stock_install_hint() {
            assert_eq!(
                missing_tool_guidance(ExternalTool::Opencode, &OpenCodeCommand::default()),
                MissingToolGuidance::Install(InstallHint::NixPackage("nixpkgs#opencode"))
            );
        }

        #[test]
        fn custom_opencode_does_not_use_stock_install_hint() {
            let command = OpenCodeCommand::try_new("opencode-shared").expect("valid command");
            let guidance = missing_tool_guidance(ExternalTool::Opencode, &command);

            assert_eq!(guidance, MissingToolGuidance::ConfiguredOpenCodeCommand);
            assert!(!guidance.to_string().contains("nixpkgs#opencode"));
        }

        #[test]
        fn other_tools_keep_their_install_hint() {
            assert_eq!(
                missing_tool_guidance(ExternalTool::Git, &OpenCodeCommand::default()),
                MissingToolGuidance::Install(InstallHint::NixPackage("nixpkgs#git"))
            );
        }
    }

    mod binary_selection {
        use super::*;

        #[test]
        fn default_opencode_uses_stock_binary_name() {
            assert_eq!(
                binary_for(ExternalTool::Opencode, &OpenCodeCommand::default()),
                "opencode"
            );
        }

        #[test]
        fn custom_opencode_uses_configured_binary_name() {
            let command = OpenCodeCommand::try_new("opencode-shared").expect("valid command");

            assert_eq!(
                binary_for(ExternalTool::Opencode, &command),
                "opencode-shared"
            );
        }

        #[test]
        fn other_tools_use_their_stock_binary_name() {
            let command = OpenCodeCommand::try_new("opencode-shared").expect("valid command");

            assert_eq!(binary_for(ExternalTool::Git, &command), "git");
        }
    }

    mod custom_status_note {
        use super::*;

        #[test]
        fn omitted_for_default_command() {
            assert_eq!(
                custom_opencode_status_note(&OpenCodeCommand::default()),
                None
            );
        }

        #[test]
        fn explains_custom_launcher_requirements() {
            let command = OpenCodeCommand::try_new("opencode-shared").expect("valid command");
            let note = custom_opencode_status_note(&command).expect("custom command note");

            assert!(note.contains("exec stock OpenCode"));
            assert!(note.contains("standard data directory"));
            assert!(note.contains("status/session detection"));
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
                    ExternalTool::Zellij,
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
