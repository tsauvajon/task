use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process},
    tools::tmux::{
        sessions::is_available,
        workflow::{ParkResult, park},
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

    let park_result = park(&repo_key, &branch, &root)?;
    process::log(&park_log_message(&park_result, &repo_key, &branch));

    println!("{}", root.display());
    Ok(())
}

fn park_log_message(result: &ParkResult, repo_key: &str, branch: &str) -> String {
    match result {
        ParkResult::Parked => format!("Parked task: {repo_key} {branch}"),
        ParkResult::AlreadyParked => format!("Task already parked: {repo_key} {branch}"),
    }
}

#[cfg(test)]
mod tests {
    use super::park_log_message;
    use crate::tools::tmux::workflow::ParkResult;

    #[test]
    fn parked_message_includes_repo_and_branch() {
        let message = park_log_message(&ParkResult::Parked, "org/repo", "feat/one");
        assert_eq!(message, "Parked task: org/repo feat/one");
    }

    #[test]
    fn already_parked_message_includes_repo_and_branch() {
        let message = park_log_message(&ParkResult::AlreadyParked, "org/repo", "feat/one");
        assert_eq!(message, "Task already parked: org/repo feat/one");
    }
}
