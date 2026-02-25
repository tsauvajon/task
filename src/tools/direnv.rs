use std::path::Path;

use crate::runtime::process::ProcessRunner;

use super::direnv_runner::run_direnv_status;

pub fn is_available(process: ProcessRunner) -> bool {
    process.command_exists("direnv")
}

pub fn allow(process: ProcessRunner, path: &Path) -> Result<(), String> {
    let _ = process; // availability already checked by caller
    run_direnv_status(&["allow"], Some(path))
}
