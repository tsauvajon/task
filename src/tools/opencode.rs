use crate::runtime::process::ProcessRunner;

pub fn auth_storage_reachable(process: ProcessRunner) -> bool {
    process
        .run_status("opencode", &["auth", "list"], None)
        .is_ok()
}
