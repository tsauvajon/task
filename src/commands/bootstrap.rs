use crate::runtime::{
    environment::RuntimeEnvironment,
    setup::{self, SetupApproval},
};

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    let guidance = "'task bootstrap' needs an interactive terminal because it applies local setup changes. Re-run it in an interactive terminal, or use 'task doctor' for read-only diagnostics.";
    let applied = setup::apply_full_setup(
        context,
        SetupApproval::Prompt("Run bootstrap now?"),
        guidance,
    )?;

    if applied {
        return Ok(());
    }

    Err("Bootstrap cancelled. No changes were applied.".to_string())
}
