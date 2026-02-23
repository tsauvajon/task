use crate::runtime::RuntimeEnvironment;
use crate::tmux::{self, ParkResult};

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    let (repo_key, branch, root) = context.current_task_info()?;

    if !tmux::is_available(context.process()) {
        return Err("tmux is not available. Run 'task list' to inspect tasks.".to_string());
    }

    match tmux::park_task(context.process(), &repo_key, &branch)? {
        ParkResult::Parked => context.log(&format!("Parked task: {repo_key} {branch}")),
        ParkResult::AlreadyParked => {
            context.log(&format!("Task already parked: {repo_key} {branch}"))
        }
    }

    println!("{}", root.display());
    Ok(())
}
