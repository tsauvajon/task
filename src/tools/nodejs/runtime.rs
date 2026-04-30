use std::path::Path;

use crate::{error::Result, runtime::process};

pub fn pnpm_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    process::run_status("pnpm", args, cwd)
}

pub fn corepack_status(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    process::run_status("corepack", args, cwd)
}

/// Returns `true` if the `node` binary is on PATH.
#[must_use]
pub fn node_binary_available() -> bool {
    process::command_exists("node")
}

/// Returns `true` if the `pnpm` binary is on PATH.
#[must_use]
pub fn pnpm_binary_available() -> bool {
    process::command_exists("pnpm")
}

/// Returns `true` if the `corepack` binary is on PATH.
#[must_use]
pub fn corepack_binary_available() -> bool {
    process::command_exists("corepack")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Pnpm,
    Corepack,
}

#[must_use]
pub fn node_available() -> bool {
    node_binary_available()
}

#[must_use]
pub fn corepack_available() -> bool {
    corepack_binary_available()
}

pub fn enable_corepack() -> Result<()> {
    // `corepack enable` writes shims into the directory that contains the
    // running `corepack` binary, so it relies on `node` being installed in
    // the same bin directory on PATH (the normal case for `nix profile install
    // nixpkgs#nodejs`, asdf, brew, etc.).
    corepack_status(&["enable"], None)
}

#[must_use]
pub fn resolve_runner() -> Option<Runner> {
    if pnpm_binary_available() {
        return Some(Runner::Pnpm);
    }
    if corepack_binary_available() {
        return Some(Runner::Corepack);
    }
    None
}

/// Resolve runner from explicit availability flags (for testing without nix store).
#[cfg(test)]
fn resolve_runner_from(pnpm_available: bool, corepack_available: bool) -> Option<Runner> {
    if pnpm_available {
        return Some(Runner::Pnpm);
    }
    if corepack_available {
        return Some(Runner::Corepack);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Runner, resolve_runner_from};

    mod resolve_runner {
        use super::*;

        #[test]
        fn prefers_pnpm_over_corepack_when_both_available() {
            let runner = resolve_runner_from(true, true);
            assert_eq!(runner, Some(Runner::Pnpm));
        }

        #[test]
        fn uses_corepack_when_pnpm_unavailable() {
            let runner = resolve_runner_from(false, true);
            assert_eq!(runner, Some(Runner::Corepack));
        }

        #[test]
        fn returns_none_when_neither_available() {
            let runner = resolve_runner_from(false, false);
            assert_eq!(runner, None);
        }

        #[test]
        fn uses_pnpm_when_only_pnpm_available() {
            let runner = resolve_runner_from(true, false);
            assert_eq!(runner, Some(Runner::Pnpm));
        }

        #[test]
        fn variants_are_distinct() {
            assert_ne!(Runner::Pnpm, Runner::Corepack);
        }
    }
}
