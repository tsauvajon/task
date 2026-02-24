use crate::{
    runtime::{
        environment::RuntimeEnvironment,
        setup::{self, SetupApproval},
    },
    tools::opencode,
};

struct DoctorReport {
    missing_required: bool,
}

pub fn run(context: &RuntimeEnvironment, fix: bool) -> Result<(), String> {
    let mut report = check(context);

    if fix {
        apply_fixes(context, SetupApproval::AssumeYes)?;
        println!("\nRe-running checks after fixes...\n");
        report = check(context);
    } else if report.missing_required && crate::runtime::config::is_interactive_terminal() {
        let applied = apply_fixes(
            context,
            SetupApproval::Prompt("Issues found. Apply automatic fixes now?"),
        )?;
        if applied {
            println!("\nRe-running checks after fixes...\n");
            report = check(context);
        }
    }

    if report.missing_required {
        return Err("Doctor check found missing dependencies".to_string());
    }

    Ok(())
}

fn check(context: &RuntimeEnvironment) -> DoctorReport {
    let mut missing_required = false;

    println!("repos_dir: {}", context.repos_dir().display());
    println!("wt_dir: {}", context.wt_dir().display());
    for cmd in [
        "git", "tmux", "vim", "codium", "opencode", "nix", "direnv", "asdf",
    ] {
        if context.command_exists(cmd) {
            println!("[ok]      {cmd}");
        } else {
            println!("[missing] {cmd}");
            missing_required = true;
        }
    }

    if context.repos_dir().is_dir() && context.wt_dir().is_dir() {
        println!("[ok]      configured layout exists");
    } else {
        println!("[missing] configured layout does not exist");
        missing_required = true;
    }

    if context.command_exists("opencode") {
        if opencode::auth_storage_reachable(context.process()) {
            println!("[ok]      opencode auth storage reachable");
        } else {
            println!("[warn]    opencode auth storage not initialized yet");
        }
    }

    DoctorReport { missing_required }
}

fn apply_fixes(context: &RuntimeEnvironment, approval: SetupApproval<'_>) -> Result<bool, String> {
    let guidance = "'task doctor --fix' needs an interactive terminal because it applies local setup changes. Re-run in an interactive terminal, or run 'task doctor' for read-only diagnostics.";
    setup::apply_full_setup(context, approval, guidance)
}
