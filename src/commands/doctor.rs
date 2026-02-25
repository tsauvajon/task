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

    if fix {
        apply_fixes(env, SetupApproval::AssumeYes)?;
        println!("\nRe-running checks after fixes...\n");
        report = check(env);
    } else if report.missing_required && is_interactive_terminal() {
        let applied = apply_fixes(
            env,
            SetupApproval::Prompt("Issues found. Apply automatic fixes now?"),
        )?;
        if applied {
            println!("\nRe-running checks after fixes...\n");
            report = check(env);
        }
    }

    if report.missing_required {
        return Err(Error::failed("Doctor check found missing dependencies"));
    }

    Ok(())
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
