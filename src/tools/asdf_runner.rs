use std::path::Path;

use crate::runtime::{nix_store::NixRunner, process::ManagedTool};

static ASDF: NixRunner = NixRunner::new(ManagedTool::Asdf);

pub fn run_asdf_capture(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<String> {
    ASDF.capture(args, cwd)
}

pub fn run_asdf_status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    ASDF.status(args, cwd)
}
