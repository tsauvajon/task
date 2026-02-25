use dialoguer::{theme::ColorfulTheme, Confirm};

use crate::{
    error::{Error, Result},
    runtime::{config::is_interactive_terminal, environment::RuntimeEnvironment, state},
    tools::{
        asdf,
        nodejs::runtime::{corepack_available, enable_corepack, node_available},
    },
};

pub enum SetupApproval<'a> {
    Prompt(&'a str),
    AssumeYes,
}

pub fn ensure_first_run_setup(env: &RuntimeEnvironment) -> Result<()> {
    if state::onboarding_complete()? {
        return Ok(());
    }

    let guidance = "First-run setup is required and needs an interactive terminal. Re-run in an interactive terminal, or run 'task doctor' for diagnostics.";
    let applied = apply_full_setup(
        env,
        SetupApproval::Prompt("Run first-run setup now?"),
        guidance,
    )?;
    if applied {
        env.process().log("First-run setup complete");
        return Ok(());
    }

    Err(Error::failed(
        "First-run setup cancelled. Run 'task doctor --fix' or 'task bootstrap' when ready.",
    ))
}

pub fn apply_full_setup(
    env: &RuntimeEnvironment,
    approval: SetupApproval<'_>,
    non_interactive_guidance: &str,
) -> Result<bool> {
    if !is_interactive_terminal() {
        return Err(Error::failed(non_interactive_guidance));
    }

    let approved = match approval {
        SetupApproval::Prompt(prompt) => Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(true)
            .interact()?,
        SetupApproval::AssumeYes => true,
    };

    if !approved {
        return Ok(false);
    }

    run_full_setup(env)?;
    Ok(true)
}

pub fn run_full_setup(env: &RuntimeEnvironment) -> Result<()> {
    let process = env.process();
    let layout = env.layout();

    env.tasks().ensure_layout()?;
    process.log(&format!("Repos dir: {}", layout.repos_dir().display()));
    process.log(&format!("Worktrees dir: {}", layout.wt_dir().display()));

    if !process.command_exists("nix") {
        return Err(Error::failed(
            "nix is required for setup. Install nix and retry 'task bootstrap'.",
        ));
    }

    if !asdf::is_available(process) {
        process.warn("asdf could not be launched via nix. Skipping asdf-managed runtime setup.");
    } else {
        if !asdf::has_nodejs_plugin(process) {
            process.log("Installing asdf nodejs plugin");
            asdf::install_nodejs_plugin(process)?;
        }

        if let Err(err) = asdf::import_nodejs_release_keyring(process) {
            process.warn(&format!("Could not import nodejs release keyring: {err}"));
        }

        if asdf::install_from_user_tool_versions(process)? {
            process.log("Installing runtimes from ~/.tool-versions");
        }
    }

    if node_available(process) && corepack_available(process) {
        let _ = enable_corepack(process);
        process.log("Enabled corepack");
    }

    state::mark_onboarding_complete()?;
    process.log("Bootstrap complete");
    Ok(())
}
