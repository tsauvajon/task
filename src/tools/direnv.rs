use std::path::Path;

use crate::{error::Result, runtime::process};

pub fn is_available() -> bool {
    process::command_exists("direnv")
}

pub fn allow(path: &Path) -> Result<()> {
    process::run_status("direnv", &["allow"], Some(path))
}
