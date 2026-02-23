use std::path::Path;

use crate::runtime::ProcessRunner;

pub fn is_available(process: ProcessRunner) -> bool {
    process.command_exists("direnv")
}

pub fn allow(process: ProcessRunner, path: &Path) -> Result<(), String> {
    process.run_status("direnv", &["allow"], Some(path))
}
