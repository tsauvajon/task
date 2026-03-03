use std::{fs, path::Path, thread, time::Duration};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, Signal, System, UpdateKind};

use super::{
    naming::user_data_dir, process_match::cmdline_matches_user_data_dir, trust::seed_trusted_roots,
};
use crate::{error::Result, runtime::process};

pub fn open_window(
    repo_key: &str,
    branch: &str,
    worktree_path: &Path,
    codium_trusted_roots: &[std::path::PathBuf],
) -> Result<()> {
    if !process::command_exists("codium") {
        return Ok(());
    }

    let user_data_dir = user_data_dir(repo_key, branch);
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

pub fn seed_task_trusted_roots(
    repo_key: &str,
    branch: &str,
    codium_trusted_roots: &[std::path::PathBuf],
) {
    let user_data_dir = user_data_dir(repo_key, branch);
    if let Err(err) = fs::create_dir_all(&user_data_dir) {
        process::warn(&format!(
            "Could not create VSCodium profile directory for {}: {err}",
            user_data_dir.display()
        ));
        return;
    }

    if let Err(err) = seed_trusted_roots(&user_data_dir, codium_trusted_roots) {
        process::warn(&format!(
            "Could not seed VSCodium trusted roots for {}: {err}",
            user_data_dir.display()
        ));
    }
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
    let user_data_dir = user_data_dir(repo_key, branch);
    let pids = codium_pids_for_user_data_dir(&user_data_dir);
    if pids.is_empty() {
        Ok(CodiumState::NotRunning)
    } else {
        Ok(CodiumState::Running)
    }
}

pub fn close_windows(repo_key: &str, branch: &str) -> Result<()> {
    let user_data_dir = user_data_dir(repo_key, branch);
    for pid in codium_pids_for_user_data_dir(&user_data_dir) {
        terminate_pid(pid)?;
    }
    Ok(())
}

pub fn cleanup(repo_key: &str, branch: &str) -> Result<()> {
    let user_data_dir = user_data_dir(repo_key, branch);
    cleanup_user_data_dir(&user_data_dir)
}

fn codium_pids_for_user_data_dir(user_data_dir: &Path) -> Vec<u32> {
    let refresh_kind = ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always);
    let mut system =
        System::new_with_specifics(RefreshKind::nothing().with_processes(refresh_kind));
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind);

    system
        .processes()
        .values()
        .filter(|process| {
            let args: Vec<String> = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().to_string())
                .collect();
            cmdline_matches_user_data_dir(&args, user_data_dir)
        })
        .map(|process| process.pid().as_u32())
        .collect()
}

fn terminate_pid(pid: u32) -> Result<()> {
    let sysinfo_pid = sysinfo::Pid::from_u32(pid);

    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );

    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sysinfo_pid]),
        false,
        ProcessRefreshKind::nothing(),
    );
    if let Some(process) = system.process(sysinfo_pid) {
        process.kill_with(Signal::Term);
    }

    for _ in 0..10 {
        if !pid_exists(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sysinfo_pid]),
        false,
        ProcessRefreshKind::nothing(),
    );
    if let Some(process) = system.process(sysinfo_pid) {
        process.kill_with(Signal::Kill);
    }

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
    let sysinfo_pid = sysinfo::Pid::from_u32(pid);
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sysinfo_pid]),
        false,
        ProcessRefreshKind::nothing(),
    );
    system.process(sysinfo_pid).is_some()
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

    mod codium_args {
        use super::*;

        #[test]
        fn uses_expected_flags() {
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
        fn has_four_arguments() {
            let args = codium_args(Path::new("/u/d/dir"), Path::new("/wt/path"));
            assert_eq!(args.len(), 4);
        }
    }

    mod codium_state {
        use super::*;

        #[test]
        fn returns_not_running_for_unknown_task() {
            let state = codium_state("no-such-repo", "no-such-branch").expect("codium_state");
            assert_eq!(state, CodiumState::NotRunning);
        }
    }

    mod cleanup_user_data_dir {
        use super::*;

        #[test]
        fn removes_existing_directory() {
            let base = std::env::temp_dir().join("task-vscodium-cleanup-existing");
            let _ = fs::remove_dir_all(&base);
            let target = base.join("task/codium/session");
            fs::create_dir_all(target.join("User")).expect("create nested dir");

            cleanup_user_data_dir(&target).expect("cleanup state");
            assert!(!target.exists());

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn ignores_missing_directory() {
            let base = std::env::temp_dir().join("task-vscodium-cleanup-missing");
            let _ = fs::remove_dir_all(&base);
            let target = base.join("task/codium/missing");

            cleanup_user_data_dir(&target).expect("cleanup missing state");

            let _ = fs::remove_dir_all(&base);
        }
    }
}
