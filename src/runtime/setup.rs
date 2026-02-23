use dialoguer::{Confirm, theme::ColorfulTheme};

use crate::runtime::config::is_interactive_terminal;
use crate::runtime::environment::RuntimeEnvironment;
use crate::runtime::state;
use crate::tools::asdf;
use crate::tools::nodejs::runtime::{corepack_available, enable_corepack, node_available};

pub enum SetupApproval<'a> {
    Prompt(&'a str),
    AssumeYes,
}

pub fn ensure_first_run_setup(context: &RuntimeEnvironment) -> Result<(), String> {
    if state::onboarding_complete()? {
        return Ok(());
    }

    let guidance = "First-run setup is required and needs an interactive terminal. Re-run in an interactive terminal, or run 'task doctor' for diagnostics.";
    let applied = apply_full_setup(
        context,
        SetupApproval::Prompt("Run first-run setup now?"),
        guidance,
    )?;
    if applied {
        context.log("First-run setup complete");
        return Ok(());
    }

    Err(
        "First-run setup cancelled. Run 'task doctor --fix' or 'task bootstrap' when ready."
            .to_string(),
    )
}

pub fn apply_full_setup(
    context: &RuntimeEnvironment,
    approval: SetupApproval<'_>,
    non_interactive_guidance: &str,
) -> Result<bool, String> {
    if !is_interactive_terminal() {
        return Err(non_interactive_guidance.to_string());
    }

    let approved = match approval {
        SetupApproval::Prompt(prompt) => Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(true)
            .interact()
            .map_err(|error| error.to_string())?,
        SetupApproval::AssumeYes => true,
    };

    if !approved {
        return Ok(false);
    }

    run_full_setup(context)?;
    Ok(true)
}

pub fn run_full_setup(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    context.log(&format!("Repos dir: {}", context.repos_dir().display()));
    context.log(&format!("Worktrees dir: {}", context.wt_dir().display()));

    if !asdf::is_available(context.process()) {
        context.warn(
            "asdf not found. Install toolchain first (nix profile install path:~/flakes#toolchain).",
        );
    } else {
        if !asdf::has_nodejs_plugin(context.process()) {
            context.log("Installing asdf nodejs plugin");
            asdf::install_nodejs_plugin(context.process())?;
        }

        if let Err(error) = asdf::import_nodejs_release_keyring(context.process()) {
            context.warn(&format!("Could not import nodejs release keyring: {error}"));
        }

        if asdf::install_from_user_tool_versions(context.process())? {
            context.log("Installing runtimes from ~/.tool-versions");
        }
    }

    if node_available(context.process()) && corepack_available(context.process()) {
        let _ = enable_corepack(context.process());
        context.log("Enabled corepack");
    }

    state::mark_onboarding_complete()?;
    context.log("Bootstrap complete");
    Ok(())
}
