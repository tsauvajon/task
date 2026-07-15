//! `OpenCode` integration.
//!
//! - [`db`]: locate `OpenCode` `SQLite` databases on disk and resolve the most
//!   recent session for a worktree across all channels.
//! - [`status`]: classify the live state of `OpenCode` sessions for the TUI.
//! - [`process`]: discover running `opencode` processes and their cwds.

use std::path::Path;

use crate::runtime::{
    config::OpenCodeCommand,
    process::{CommandPlan, run_status},
};

pub mod db;
pub mod process;
pub mod status;

#[must_use]
pub fn auth_storage_reachable(command: &OpenCodeCommand) -> bool {
    run_status(command.as_str(), &["auth", "list"], None).is_ok()
}

/// Returns the full command plan for launching opencode for a worktree.
///
/// If a previous opencode session exists for that exact directory in any
/// installed `OpenCode` database (the `opencode*.db` files under the data
/// dir), the command includes `--session <id>` so the TUI resumes it.
#[must_use]
pub fn launch_command(command: &OpenCodeCommand, directory: &Path) -> CommandPlan {
    let args = db::latest_session_for(directory)
        .map(|session| vec!["--session".to_owned(), session.id])
        .unwrap_or_default();
    CommandPlan::for_program(command.as_str(), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod launch_command {
        use super::*;

        #[test]
        fn uses_default_opencode_binary_when_no_db() {
            let plan = launch_command(
                &OpenCodeCommand::default(),
                Path::new("/nonexistent/worktree"),
            );
            assert_eq!(plan.program(), "opencode");
            assert!(plan.args().is_empty());
        }

        #[test]
        fn uses_custom_program_as_one_executable() {
            let command = OpenCodeCommand::try_new("/opt/OpenCode Shared/opencode-shared")
                .expect("valid command");
            let plan = launch_command(&command, Path::new("/nonexistent/wt/repo"));

            assert_eq!(plan.program(), "/opt/OpenCode Shared/opencode-shared");
            assert!(plan.args().is_empty());
        }
    }
}
