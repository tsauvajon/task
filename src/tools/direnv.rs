use std::path::Path;

use crate::{error::Result, runtime::process};

#[must_use]
pub fn is_available() -> bool {
    process::command_exists("direnv")
}

pub fn allow(path: &Path) -> Result<()> {
    process::run_status("direnv", &["allow"], Some(path))
}
