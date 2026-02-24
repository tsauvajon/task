use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use super::{
    naming::task_user_data_dir,
    process_match::{cmdline_matches_user_data_dir, parse_cmdline_bytes},
    trust::seed_trusted_roots,
};
use crate::runtime::process::ProcessRunner;

pub fn open_task_window(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
    worktree_path: &Path,
    codium_trusted_roots: &[std::path::PathBuf],
) -> Result<(), String> {
    if !process.command_exists("codium") {
        return Ok(());
    }

    let user_data_dir = task_user_data_dir(repo_key, branch);
    fs::create_dir_all(&user_data_dir).map_err(|error| error.to_string())?;
    if let Err(error) = seed_trusted_roots(&user_data_dir, codium_trusted_roots) {
        process.warn(&format!(
            "Could not seed VSCodium trusted roots for {}: {error}",
            user_data_dir.display()
        ));
    }

    Command::new("codium")
        .arg("--new-window")
        .arg("--user-data-dir")
        .arg(&user_data_dir)
        .arg(worktree_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn close_task_windows(
    process: ProcessRunner,
    repo_key: &str,
    branch: &str,
) -> Result<(), String> {
    let user_data_dir = task_user_data_dir(repo_key, branch);
    let pids = codium_pids_for_user_data_dir(&user_data_dir)?;

    for pid in pids {
        terminate_pid(process, pid)?;
    }

    Ok(())
}

pub fn cleanup_task_state(repo_key: &str, branch: &str) -> Result<(), String> {
    let user_data_dir = task_user_data_dir(repo_key, branch);
    cleanup_user_data_dir(&user_data_dir)
}

fn codium_pids_for_user_data_dir(user_data_dir: &Path) -> Result<Vec<u32>, String> {
    let mut matches = Vec::new();

    for entry in fs::read_dir("/proc").map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(pid_text) = name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_text.parse::<u32>() else {
            continue;
        };

        let cmdline_path = entry.path().join("cmdline");
        let Ok(cmdline_bytes) = fs::read(cmdline_path) else {
            continue;
        };

        let args = parse_cmdline_bytes(&cmdline_bytes);
        if cmdline_matches_user_data_dir(&args, user_data_dir) {
            matches.push(pid);
        }
    }

    Ok(matches)
}

fn terminate_pid(process: ProcessRunner, pid: u32) -> Result<(), String> {
    let pid_text = pid.to_string();
    let _ = process.run_status("kill", &["-TERM", &pid_text], None);

    for _ in 0..10 {
        if !pid_exists(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    let _ = process.run_status("kill", &["-KILL", &pid_text], None);

    for _ in 0..10 {
        if !pid_exists(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err(format!("Failed to stop codium process {pid}"))
}

fn pid_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn cleanup_user_data_dir(user_data_dir: &Path) -> Result<(), String> {
    if !user_data_dir.exists() {
        return Ok(());
    }

    fs::remove_dir_all(user_data_dir).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::cleanup_user_data_dir;

    #[test]
    fn cleanup_task_state_removes_existing_directory() {
        let base = std::env::temp_dir().join("task-vscodium-cleanup-existing");
        let _ = fs::remove_dir_all(&base);
        let target = base.join("task").join("codium").join("session");
        fs::create_dir_all(target.join("User")).expect("create nested dir");

        cleanup_user_data_dir(&target).expect("cleanup state");
        assert!(!target.exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn cleanup_task_state_ignores_missing_directory() {
        let base = std::env::temp_dir().join("task-vscodium-cleanup-missing");
        let _ = fs::remove_dir_all(&base);
        let target = base.join("task").join("codium").join("missing");

        cleanup_user_data_dir(&target).expect("cleanup missing state");

        let _ = fs::remove_dir_all(&base);
    }
}
