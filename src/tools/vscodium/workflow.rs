use std::{fs, path::Path, thread, time::Duration};

use super::{
    naming::task_user_data_dir,
    process_match::{cmdline_matches_user_data_dir, parse_cmdline_bytes},
    trust::seed_trusted_roots,
};
use crate::{error::Result, runtime::process};

pub fn open_task_window(
    repo_key: &str,
    branch: &str,
    worktree_path: &Path,
    codium_trusted_roots: &[std::path::PathBuf],
) -> Result<()> {
    if !process::command_exists("codium") {
        return Ok(());
    }

    let user_data_dir = task_user_data_dir(repo_key, branch);
    fs::create_dir_all(&user_data_dir)?;
    if let Err(err) = seed_trusted_roots(&user_data_dir, codium_trusted_roots) {
        process::warn(&format!(
            "Could not seed VSCodium trusted roots for {}: {err}",
            user_data_dir.display()
        ));
    }

    let args = codium_args(&user_data_dir, worktree_path);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    process::spawn_detached("codium", &arg_refs, None)
}

fn codium_args(user_data_dir: &Path, worktree_path: &Path) -> Vec<String> {
    vec![
        "--new-window".to_string(),
        "--user-data-dir".to_string(),
        user_data_dir.to_string_lossy().into_owned(),
        worktree_path.to_string_lossy().into_owned(),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodiumState {
    Running,
    NotRunning,
}

pub fn codium_state(repo_key: &str, branch: &str) -> Result<CodiumState> {
    let user_data_dir = task_user_data_dir(repo_key, branch);
    let pids = codium_pids_for_user_data_dir(&user_data_dir)?;
    if pids.is_empty() {
        Ok(CodiumState::NotRunning)
    } else {
        Ok(CodiumState::Running)
    }
}

pub fn close_task_windows(repo_key: &str, branch: &str) -> Result<()> {
    let user_data_dir = task_user_data_dir(repo_key, branch);
    for pid in codium_pids_for_user_data_dir(&user_data_dir)? {
        terminate_pid(pid)?;
    }
    Ok(())
}

pub fn cleanup_task_state(repo_key: &str, branch: &str) -> Result<()> {
    let user_data_dir = task_user_data_dir(repo_key, branch);
    cleanup_user_data_dir(&user_data_dir)
}

fn codium_pids_for_user_data_dir(user_data_dir: &Path) -> Result<Vec<u32>> {
    let mut matches = Vec::new();

    let proc_entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(matches),
        Err(err) => return Err(err.into()),
    };

    for entry in proc_entries {
        let entry = entry?;
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

fn terminate_pid(pid: u32) -> Result<()> {
    let pid_str = pid.to_string();
    let _ = process::run_status("kill", &["-TERM", &pid_str], None);

    for _ in 0..10 {
        if !pid_exists(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    let _ = process::run_status("kill", &["-KILL", &pid_str], None);

    for _ in 0..10 {
        if !pid_exists(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    Err(crate::error::Error::failed(format!(
        "Failed to stop codium process {pid}"
    )))
}

fn pid_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn cleanup_user_data_dir(user_data_dir: &Path) -> Result<()> {
    if !user_data_dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(user_data_dir).map_err(crate::error::Error::from)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::{CodiumState, cleanup_user_data_dir, codium_args, codium_state};

    #[test]
    fn codium_args_use_expected_flags() {
        let args = codium_args(Path::new("/tmp/task/codium/a"), Path::new("/tmp/wt/repo"));
        assert_eq!(
            args,
            vec![
                "--new-window",
                "--user-data-dir",
                "/tmp/task/codium/a",
                "/tmp/wt/repo",
            ]
        );
    }

    #[test]
    fn codium_state_returns_not_running_for_unknown_task() {
        let state = codium_state("no-such-repo", "no-such-branch").expect("codium_state");
        assert_eq!(state, CodiumState::NotRunning);
    }

    #[test]
    fn cleanup_task_state_removes_existing_directory() {
        let base = std::env::temp_dir().join("task-vscodium-cleanup-existing");
        let _ = fs::remove_dir_all(&base);
        let target = base.join("task/codium/session");
        fs::create_dir_all(target.join("User")).expect("create nested dir");

        cleanup_user_data_dir(&target).expect("cleanup state");
        assert!(!target.exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn cleanup_task_state_ignores_missing_directory() {
        let base = std::env::temp_dir().join("task-vscodium-cleanup-missing");
        let _ = fs::remove_dir_all(&base);
        let target = base.join("task/codium/missing");

        cleanup_user_data_dir(&target).expect("cleanup missing state");

        let _ = fs::remove_dir_all(&base);
    }
}
