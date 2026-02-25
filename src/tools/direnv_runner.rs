use std::path::Path;

use crate::runtime::{nix_store::NixRunner, process::ManagedTool};

static DIRENV: NixRunner = NixRunner::new(ManagedTool::Direnv);

pub fn run_direnv_status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    DIRENV.status(args, cwd)
}
