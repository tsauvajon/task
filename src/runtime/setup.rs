use dialoguer::{Confirm, theme::ColorfulTheme};

use crate::{
    error::{Error, Result},
    runtime::{config::is_interactive_terminal, environment::RuntimeEnvironment, process, state},
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
        process::log("First-run setup complete");
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
    ensure_interactive_terminal(is_interactive_terminal(), non_interactive_guidance)?;
    let approved = resolve_setup_approval(approval)?;

    if !approved {
        return Ok(false);
    }

    run_full_setup(env)?;
    Ok(true)
}

pub fn run_full_setup(env: &RuntimeEnvironment) -> Result<()> {
    let layout = env.layout();

    env.tasks().ensure_layout()?;
    process::log(&format!("Repos dir: {}", layout.repos_dir().display()));
    process::log(&format!("Worktrees dir: {}", layout.wt_dir().display()));
    process::log(&format!(
        "Detached dir: {}",
        layout.detached_dir().display()
    ));

    if !process::nix_available() {
        return Err(Error::failed(
            "nix is required for setup. Install nix and retry 'task bootstrap'.",
        ));
    }

    if !asdf::is_available() {
        process::warn("asdf could not be launched via nix. Skipping asdf-managed runtime setup.");
    } else {
        if !asdf::has_nodejs_plugin() {
            process::log("Installing asdf nodejs plugin");
            asdf::install_nodejs_plugin()?;
        }

        if let Err(err) = asdf::import_nodejs_release_keyring() {
            process::warn(&format!("Could not import nodejs release keyring: {err}"));
        }

        if asdf::install_from_user_tool_versions()? {
            process::log("Installing runtimes from ~/.tool-versions");
        }
    }

    if should_enable_corepack(node_available(), corepack_available()) {
        let _ = enable_corepack();
        process::log("Enabled corepack");
    }

    state::mark_onboarding_complete()?;
    process::log("Bootstrap complete");
    Ok(())
}

fn ensure_interactive_terminal(is_interactive: bool, guidance: &str) -> Result<()> {
    if is_interactive {
        return Ok(());
    }
    Err(Error::failed(guidance))
}

fn resolve_setup_approval(approval: SetupApproval<'_>) -> Result<bool> {
    resolve_setup_approval_with(approval, |prompt| {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(true)
            .interact()
            .map_err(Error::from)
    })
}

fn resolve_setup_approval_with<F>(
    approval: SetupApproval<'_>,
    mut confirm_prompt: F,
) -> Result<bool>
where
    F: FnMut(&str) -> Result<bool>,
{
    match approval {
        SetupApproval::Prompt(prompt) => confirm_prompt(prompt),
        SetupApproval::AssumeYes => Ok(true),
    }
}

fn should_enable_corepack(node_installed: bool, corepack_installed: bool) -> bool {
    node_installed && corepack_installed
}

#[cfg(test)]
mod tests {
    use super::{
        SetupApproval, ensure_interactive_terminal, resolve_setup_approval_with,
        should_enable_corepack,
    };

    mod ensure_interactive_terminal {
        use super::*;

        #[test]
        fn allows_interactive_terminal() {
            ensure_interactive_terminal(true, "guidance").expect("interactive terminal is allowed");
        }

        #[test]
        fn rejects_non_interactive_terminal_with_guidance() {
            let err =
                ensure_interactive_terminal(false, "use a terminal").expect_err("expected error");
            assert!(err.to_string().contains("use a terminal"));
        }
    }

    mod resolve_setup_approval_with {
        use super::*;

        #[test]
        fn approve_without_prompt_when_assume_yes() {
            let approved = resolve_setup_approval_with(SetupApproval::AssumeYes, |_| {
                panic!("prompt should not be invoked")
            })
            .expect("approval should succeed");
            assert!(approved);
        }

        #[test]
        fn returns_prompt_decision() {
            let approved =
                resolve_setup_approval_with(SetupApproval::Prompt("Run setup?"), |prompt| {
                    assert_eq!(prompt, "Run setup?");
                    Ok(false)
                })
                .expect("approval should succeed");
            assert!(!approved);
        }

        #[test]
        fn propagates_prompt_errors() {
            let err = resolve_setup_approval_with(SetupApproval::Prompt("Run setup?"), |_| {
                Err(crate::error::Error::failed("prompt failed"))
            })
            .expect_err("expected prompt error");
            assert!(err.to_string().contains("prompt failed"));
        }
    }

    mod should_enable_corepack {
        use super::*;

        #[test]
        fn true_when_node_and_corepack_are_available() {
            assert!(should_enable_corepack(true, true));
        }

        #[test]
        fn false_when_node_is_missing() {
            assert!(!should_enable_corepack(false, true));
        }

        #[test]
        fn false_when_corepack_is_missing() {
            assert!(!should_enable_corepack(true, false));
        }
    }
}
