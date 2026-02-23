use crate::runtime::{RuntimeEnvironment, task_session_name};

pub fn run(context: &RuntimeEnvironment) -> Result<(), String> {
    context.ensure_layout()?;
    let (repo_key, branch, root) = context.current_task_info()?;

    if !context.command_exists("tmux") {
        return Err("tmux is not available. Run 'task list' to inspect tasks.".to_string());
    }

    let session = task_session_name(&repo_key, &branch);
    if context.tmux_has_session(&session) {
        context.run_status("tmux", &["kill-session", "-t", &session], None)?;
        context.log(&format!("Parked task: {repo_key} {branch}"));
    } else {
        context.log(&format!("Task already parked: {repo_key} {branch}"));
    }

    println!("{}", root.display());
    Ok(())
}
