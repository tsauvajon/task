use crate::layout::Layout;
use crate::session::session_name_for;

pub fn run(layout: &Layout) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let (repo_key, branch, root) = super::current_task_info()?;

    if !super::command_exists("tmux") {
        return Err("tmux is not available. Run 'task list' to inspect tasks.".to_string());
    }

    let session = session_name_for(&repo_key, &branch);
    if super::tmux_has_session(&session) {
        super::run_status("tmux", &["kill-session", "-t", &session], None)?;
        super::log(&format!("Parked task: {repo_key} {branch}"));
    } else {
        super::log(&format!("Task already parked: {repo_key} {branch}"));
    }

    println!("{}", root.display());
    Ok(())
}
