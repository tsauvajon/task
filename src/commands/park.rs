use crate::{
    error::{Error, Result},
    runtime::environment::RuntimeEnvironment,
    tools::tmux::{
        sessions::is_available,
        workflow::{park_task, ParkResult},
    },
};

pub fn run(context: &RuntimeEnvironment) -> Result<()> {
    context.tasks().ensure_layout()?;
    let (repo_key, branch, root) = context.tasks().current_task_info()?;

    if !is_available(context.process()) {
        return Err(Error::failed(
            "tmux is not available. Run 'task list' to inspect tasks.",
        ));
    }

    match park_task(context.process(), &repo_key, &branch, &root)? {
        ParkResult::Parked => context
            .process()
            .log(&format!("Parked task: {repo_key} {branch}")),
        ParkResult::AlreadyParked => context
            .process()
            .log(&format!("Task already parked: {repo_key} {branch}")),
    }

    println!("{}", root.display());
    Ok(())
}
