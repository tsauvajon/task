use std::path::Path;

use crate::{
    error::Result,
    runtime::{nix_store::NixRunner, process::ManagedTool},
};

static DIRENV: NixRunner = NixRunner::new(ManagedTool::Direnv);

pub fn is_available() -> bool {
    crate::runtime::process::command_exists("direnv")
}

pub fn allow(path: &Path) -> Result<()> {
    DIRENV.status(&["allow"], Some(path))
}
