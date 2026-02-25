use crate::error::Result;

use super::runner::{
    corepack_binary_available, node_binary_available, pnpm_binary_available, run_corepack_status,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Pnpm,
    Corepack,
}

pub fn node_available() -> bool {
    node_binary_available()
}

pub fn corepack_available() -> bool {
    corepack_binary_available()
}

pub fn enable_corepack() -> Result<()> {
    run_corepack_status(&["enable"], None)
}

pub fn resolve_runner() -> Option<Runner> {
    if pnpm_binary_available() {
        return Some(Runner::Pnpm);
    }
    if corepack_binary_available() {
        return Some(Runner::Corepack);
    }
    None
}
