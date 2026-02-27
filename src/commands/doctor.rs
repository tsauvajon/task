use crate::{
    error::{Error, Result},
    runtime::{
        config::is_interactive_terminal,
        environment::RuntimeEnvironment,
        process,
        setup::{self, SetupApproval},
    },
    tools::opencode,
};

struct DoctorReport {
    missing_required: bool,
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

    if process::nix_available() {
        println!("[ok]      nix");
        println!("[ok]      managed tools launch via nix run");
    } else {
        println!("[missing] nix");
        println!("[missing] managed tools launch via nix run");
        missing_required = true;
    }

    if layout.repos_dir().is_dir() && layout.wt_dir().is_dir() {
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
    use super::{DoctorAction, decide_action};

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
}
