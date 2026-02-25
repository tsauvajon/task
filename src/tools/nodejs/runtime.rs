use super::runner::{
    corepack_binary_available, node_binary_available, pnpm_binary_available, run_corepack_status,
};
use crate::runtime::process::ProcessRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Pnpm,
    Corepack,
}

pub fn node_available(process: ProcessRunner) -> bool {
    let _ = process;
    node_binary_available()
}

pub fn corepack_available(process: ProcessRunner) -> bool {
    let _ = process;
    corepack_binary_available()
}

pub fn enable_corepack(process: ProcessRunner) -> Result<(), String> {
    let _ = process;
    run_corepack_status(&["enable"], None)
}

pub fn resolve_runner(process: ProcessRunner) -> Option<Runner> {
    let _ = process;
    if pnpm_binary_available() {
        return Some(Runner::Pnpm);
    }
    if corepack_binary_available() {
        return Some(Runner::Corepack);
    }
    None
}
