use std::path::Path;

use crate::runtime::{nix_store::NixRunner, process::ManagedTool};

static TMUX: NixRunner = NixRunner::new(ManagedTool::Tmux);

pub(super) fn run_tmux_capture(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<String> {
    TMUX.capture(args, cwd)
}

pub(super) fn run_tmux_status(args: &[&str], cwd: Option<&Path>) -> crate::error::Result<()> {
    TMUX.status(args, cwd)
}
