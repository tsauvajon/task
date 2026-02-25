use std::path::Path;

use crate::error::Result;

use super::direnv_runner::run_direnv_status;

pub fn is_available() -> bool {
    crate::runtime::process::command_exists("direnv")
}

pub fn allow(path: &Path) -> Result<()> {
    run_direnv_status(&["allow"], Some(path))
}
