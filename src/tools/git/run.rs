use std::path::Path;

use crate::runtime::{nix_store::NixRunner, process::ManagedTool};

static GIT: NixRunner = NixRunner::new(ManagedTool::Git);

pub(super) fn capture(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<String> {
    GIT.capture(args, cwd)
}

pub(super) fn status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    GIT.status(args, cwd)
}
