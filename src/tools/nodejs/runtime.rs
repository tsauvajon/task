use crate::runtime::ProcessRunner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Pnpm,
    Corepack,
}

pub fn node_available(process: ProcessRunner) -> bool {
    process.command_exists("node")
}

pub fn corepack_available(process: ProcessRunner) -> bool {
    process.command_exists("corepack")
}

pub fn enable_corepack(process: ProcessRunner) -> Result<(), String> {
    process.run_status("corepack", &["enable"], None)
}

pub fn resolve_runner(process: ProcessRunner) -> Option<Runner> {
    if process.command_exists("pnpm") {
        return Some(Runner::Pnpm);
    }
    if corepack_available(process) {
        return Some(Runner::Corepack);
    }
    None
}
