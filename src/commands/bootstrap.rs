use crate::{
    error::{Error, Result},
    runtime::{
        environment::RuntimeEnvironment,
        setup::{self, SetupApproval},
    },
};

pub fn run(env: &RuntimeEnvironment, yes: bool) -> Result<()> {
    const GUIDANCE: &str = "'task bootstrap' needs an interactive terminal because it applies local setup changes. Re-run it in an interactive terminal, use --yes to skip confirmation, or use 'task doctor' for read-only diagnostics.";

    let approval = if yes {
        SetupApproval::AssumeYes
    } else {
        SetupApproval::Prompt("Run bootstrap now?")
    };

    let applied = setup::apply_full_setup(env, approval, GUIDANCE)?;
    ensure_bootstrap_applied(applied)
}

fn ensure_bootstrap_applied(applied: bool) -> Result<()> {
    if applied {
        return Ok(());
    }

    Err(Error::failed(
        "Bootstrap cancelled. No changes were applied.",
    ))
}

#[cfg(test)]
mod tests {
    use super::ensure_bootstrap_applied;

    #[test]
    fn ok_when_bootstrap_applied() {
        ensure_bootstrap_applied(true).expect("bootstrap should succeed when applied");
    }

    #[test]
    fn error_when_bootstrap_cancelled() {
        let err = ensure_bootstrap_applied(false).expect_err("expected cancellation error");
        assert!(err.to_string().contains("Bootstrap cancelled"));
    }
}
