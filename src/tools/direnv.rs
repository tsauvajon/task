use std::path::Path;

use crate::{error::Result, runtime::process::ProcessRunner};

use super::direnv_runner::run_direnv_status;

pub fn is_available(process: ProcessRunner) -> bool {
    process.command_exists("direnv")
}

pub fn allow(_process: ProcessRunner, path: &Path) -> Result<()> {
    run_direnv_status(&["allow"], Some(path))
}
