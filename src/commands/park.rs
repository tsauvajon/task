use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::tmux::{
        sessions::is_available,
        workflow::{ParkResult, park_task},
    },
};

pub fn run(context: &RuntimeEnvironment) -> Result<()> {
    context.tasks().ensure_layout()?;
    let (repo_key, branch, root) = context.tasks().current_task_info()?;

    if !is_available() {
        return Err(Error::failed(
            "tmux is not available. Run 'task list' to inspect tasks.",
        ));
    }

    match park_task(&repo_key, &branch, &root)? {
        ParkResult::Parked => process::log(&format!("Parked task: {repo_key} {branch}")),
        ParkResult::AlreadyParked => {
            process::log(&format!("Task already parked: {repo_key} {branch}"))
        }
    }

    println!("{}", root.display());
    Ok(())
}
