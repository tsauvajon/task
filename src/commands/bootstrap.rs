use crate::{
    error::{Error, Result},
    runtime::{
        environment::RuntimeEnvironment,
        setup::{self, SetupApproval},
    },
};

pub fn run(env: &RuntimeEnvironment) -> Result<()> {
    const GUIDANCE: &str = "'task bootstrap' needs an interactive terminal because it applies local setup changes. Re-run it in an interactive terminal, or use 'task doctor' for read-only diagnostics.";

    let applied =
        setup::apply_full_setup(env, SetupApproval::Prompt("Run bootstrap now?"), GUIDANCE)?;

    if applied {
        return Ok(());
    }

    Err(Error::failed(
        "Bootstrap cancelled. No changes were applied.",
    ))
}
