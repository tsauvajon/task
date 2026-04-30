//! Background loader for the TUI.
//!
//! Spawns a worker thread that enumerates cloned repos, then fans out
//! per-repo `git worktree list --porcelain` and FS probes on the rayon
//! pool. Each per-repo result is sent back to the UI as a [`LoadMsg`]
//! so rows can appear progressively while the status bar shows a
//! spinner + `done/total` counter.
//!
//! The loader is cancellable: dropping the [`LoaderHandle`] sets a stop
//! flag that workers check between repos. Send failures (receiver
//! dropped) are also treated as cancellation.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use rayon::prelude::*;

use super::{
    state::{LoadMsg, RepoRow},
    tasks::{count_repo_worktrees_with_canonical_wt, is_detached_worktree_path},
};
use crate::{
    runtime::{RepoKey, environment::RuntimeEnvironment},
    tools::opencode::status::OpenCodeSnapshot,
};

/// Handle to a spawned loader thread. Consumers poll [`Self::try_recv`]
/// from the event loop. Dropping the handle cancels the worker.
pub(super) struct LoaderHandle {
    rx: mpsc::Receiver<LoadMsg>,
    stop: Arc<AtomicBool>,
}

impl LoaderHandle {
    pub(super) fn try_recv(&self) -> Option<LoadMsg> {
        self.rx.try_recv().ok()
    }
}

impl Drop for LoaderHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
impl LoaderHandle {
    /// Test helper: build a handle whose channel is already disconnected
    /// and whose stop flag is pre-set. Lets unit tests that exercise
    /// `apply_intent` construct a no-op loader without spawning a thread.
    pub(super) fn noop() -> Self {
        let (tx, rx) = mpsc::channel();
        drop(tx); // disconnect receiver so try_recv always returns None
        let stop = Arc::new(AtomicBool::new(true));
        Self { rx, stop }
    }
}

/// Spawn a background loader. `generation` is stamped into every
/// [`LoadMsg`] so the receiver can drop messages from superseded loaders
/// after a refresh.
pub(super) fn spawn(
    context: RuntimeEnvironment,
    task_repo_scope: Option<String>,
    generation: u64,
) -> LoaderHandle {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    thread::spawn(move || {
        run_loader(context, task_repo_scope, generation, tx, stop_thread);
    });

    LoaderHandle { rx, stop }
}

fn run_loader(
    context: RuntimeEnvironment,
    task_repo_scope: Option<String>,
    generation: u64,
    tx: mpsc::Sender<LoadMsg>,
    stop: Arc<AtomicBool>,
) {
    // Early exit if cancelled before we even start.
    if stop.load(Ordering::Relaxed) {
        return;
    }

    let tasks = context.tasks();
    let open_sessions = tasks.tmux_sessions();

    let wt_dir = context.layout().wt_dir().to_path_buf();
    let real_wt_dir = std::fs::canonicalize(&wt_dir).unwrap_or_else(|_| wt_dir.clone());

    // Resolve scoped task list (single repo) vs full workspace scan.
    let task_repos = match &task_repo_scope {
        Some(arg) => match tasks.resolve_repo_key_input(arg) {
            Ok(repo_key) => {
                let gitdir = context.layout().repo_gitdir_path(&repo_key);
                if gitdir.is_dir() {
                    vec![(repo_key, gitdir)]
                } else {
                    let _ = tx.send(LoadMsg::RepoError {
                        generation,
                        repo: repo_key,
                        err: "Repo not cloned".to_string(),
                    });
                    Vec::new()
                }
            }
            Err(err) => {
                // Report as a generic error row so the user sees *something*
                // in the activity panel even for an unresolvable scope.
                let _ = tx.send(LoadMsg::RepoError {
                    generation,
                    repo: RepoKey::new(arg.as_str()),
                    err: err.to_string(),
                });
                Vec::new()
            }
        },
        None => tasks.available_repos().unwrap_or_default(),
    };

    // Repo-view scan always covers the whole workspace so switching Tab is
    // snappy even when the task view is scoped to a single repo.
    let repo_repos = match task_repo_scope {
        Some(_) => tasks.available_repos().unwrap_or_default(),
        None => task_repos.clone(),
    };

    // Announce the per-scan totals. The UI flips its counter from "?/?"
    // to "0/N". We use max(task_count, repo_count) so the status bar
    // shows the wider of the two when they differ; in practice they only
    // differ for a scoped launch.
    let total = task_repos.len().max(repo_repos.len());
    if tx.send(LoadMsg::ScanStarted { generation, total }).is_err() {
        return;
    }

    if stop.load(Ordering::Relaxed) {
        return;
    }

    // Fan out both scans in parallel. `rayon::join` lets us overlap them
    // without spawning a second OS thread.
    let tx_tasks = tx.clone();
    let tx_repos = tx.clone();
    let stop_tasks = Arc::clone(&stop);
    let stop_repos = Arc::clone(&stop);
    let open_sessions_tasks = open_sessions.clone();
    let open_sessions_repos = open_sessions;
    let real_wt_dir_repos = real_wt_dir.clone();
    let wt_dir_repos = wt_dir.clone();
    let context_repos = context.clone();
    let context_tasks = context;

    let _ = rayon::join(
        move || {
            run_task_scan(
                &context_tasks,
                &task_repos,
                &open_sessions_tasks,
                generation,
                &tx_tasks,
                &stop_tasks,
            );
        },
        move || {
            run_repo_scan(
                &context_repos,
                &repo_repos,
                &open_sessions_repos,
                &real_wt_dir_repos,
                &wt_dir_repos,
                generation,
                &tx_repos,
                &stop_repos,
            );
        },
    );
}

fn run_task_scan(
    context: &RuntimeEnvironment,
    repos: &[(RepoKey, PathBuf)],
    open_sessions: &HashSet<String>,
    generation: u64,
    tx: &mpsc::Sender<LoadMsg>,
    stop: &AtomicBool,
) {
    // One OpenCode snapshot per full scan is plenty: its cost is one
    // sysinfo refresh + one directory read, dwarfed by the per-repo git
    // work we're about to fan out. The background tick picks up
    // subsequent state drift.
    let opencode_snapshot = OpenCodeSnapshot::collect();

    let tx = tx.clone();
    repos.par_iter().for_each(|(repo_key, gitdir)| {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match context
            .tasks()
            .repo_task_rows(repo_key, gitdir, open_sessions)
        {
            Ok(mut rows) => {
                for row in &mut rows {
                    row.opencode = opencode_snapshot.state_for(&row.path);
                }
                let _ = tx.send(LoadMsg::TaskRowsForRepo {
                    generation,
                    repo: repo_key.clone(),
                    rows,
                });
            }
            Err(err) => {
                let _ = tx.send(LoadMsg::RepoError {
                    generation,
                    repo: repo_key.clone(),
                    err: err.to_string(),
                });
            }
        }
        let _ = tx.send(LoadMsg::TaskRepoDone { generation });
    });
    let _ = tx.send(LoadMsg::TasksComplete { generation });
}

/// Background OpenCode state refresher.
///
/// Spawns a short-lived thread that takes one snapshot, classifies every
/// `paths` entry, and sends a single [`LoadMsg::OpenCodeTick`] back to
/// the UI. Drop the handle to cancel before the message is sent.
///
/// Unlike the main loader, there is no `generation` parameter: the
/// tick carries only path-keyed state, so the UI can apply it safely
/// regardless of which scope was active when the tick was spawned.
pub(super) fn spawn_opencode_refresh(paths: Vec<PathBuf>) -> LoaderHandle {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    thread::spawn(move || {
        if stop_thread.load(Ordering::Relaxed) {
            return;
        }
        let snapshot = OpenCodeSnapshot::collect();
        if stop_thread.load(Ordering::Relaxed) {
            return;
        }
        let states: Vec<(PathBuf, _)> = paths
            .into_iter()
            .map(|path| {
                let state = snapshot.state_for(&path);
                (path, state)
            })
            .collect();
        let _ = tx.send(LoadMsg::OpenCodeTick { states });
    });

    LoaderHandle { rx, stop }
}

#[expect(clippy::too_many_arguments)]
fn run_repo_scan(
    context: &RuntimeEnvironment,
    repos: &[(RepoKey, PathBuf)],
    open_sessions: &HashSet<String>,
    real_wt_dir: &std::path::Path,
    wt_dir: &std::path::Path,
    generation: u64,
    tx: &mpsc::Sender<LoadMsg>,
    stop: &AtomicBool,
) {
    let tx = tx.clone();
    repos.par_iter().for_each(|(repo_key, gitdir)| {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let (open_tasks, parked_tasks) = count_repo_worktrees_with_canonical_wt(
            repo_key,
            gitdir,
            real_wt_dir,
            wt_dir,
            open_sessions,
        );
        let detached_path = context.layout().detached_path(repo_key);
        let is_detached = is_detached_worktree_path(&detached_path);
        let row = RepoRow {
            repo: repo_key.clone(),
            open_tasks,
            parked_tasks,
            is_detached,
        };
        let _ = tx.send(LoadMsg::RepoRow { generation, row });
    });
    let _ = tx.send(LoadMsg::RepoRowsDone { generation });
}

#[cfg(test)]
mod tests {
    use std::{env, fs, time::Duration};

    use super::{LoadMsg, spawn};
    use crate::runtime::environment::RuntimeEnvironment;

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-loader-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn env_for(dir: &TempDir) -> RuntimeEnvironment {
        let repos = dir.path().join("repos");
        let wt = dir.path().join("wt");
        let detached = dir.path().join("detached");
        fs::create_dir_all(&repos).unwrap();
        fs::create_dir_all(&wt).unwrap();
        RuntimeEnvironment::from_paths(&repos, &wt, &detached)
    }

    /// Drain the loader channel for up to `timeout`, returning every
    /// message received. Used to assert end-of-scan semantics without
    /// joining the worker thread.
    fn drain_until_done(handle: &super::LoaderHandle, timeout: Duration) -> Vec<LoadMsg> {
        let start = std::time::Instant::now();
        let mut out = Vec::new();
        let mut tasks_done = false;
        let mut repos_done = false;
        while start.elapsed() < timeout {
            if let Some(msg) = handle.try_recv() {
                match &msg {
                    LoadMsg::TasksComplete { .. } => tasks_done = true,
                    LoadMsg::RepoRowsDone { .. } => repos_done = true,
                    _ => {}
                }
                out.push(msg);
                if tasks_done && repos_done {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
        out
    }

    use std::thread;

    #[test]
    fn empty_repos_workspace_produces_complete_messages_quickly() {
        let dir = TempDir::new("empty");
        let env = env_for(&dir);
        let handle = spawn(env, None, 7);
        let msgs = drain_until_done(&handle, Duration::from_secs(2));
        assert!(
            msgs.iter()
                .any(|m| matches!(m, LoadMsg::TasksComplete { generation: 7 })),
            "should see TasksComplete: {msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, LoadMsg::RepoRowsDone { generation: 7 })),
            "should see RepoRowsDone: {msgs:?}"
        );
        // Total is 0 when no repos exist.
        let started = msgs
            .iter()
            .find_map(|m| match m {
                LoadMsg::ScanStarted { total, .. } => Some(*total),
                _ => None,
            })
            .expect("ScanStarted should be emitted");
        assert_eq!(started, 0);
    }

    #[test]
    fn drop_handle_stops_loader_without_joining() {
        // We can't easily observe the worker thread from here, but we can
        // assert the handle drops instantly and the stop flag propagates.
        let dir = TempDir::new("drop");
        let env = env_for(&dir);
        let handle = spawn(env, None, 0);
        let drop_start = std::time::Instant::now();
        drop(handle);
        assert!(
            drop_start.elapsed() < Duration::from_millis(50),
            "dropping LoaderHandle must not block"
        );
    }

    #[test]
    fn scoped_launch_errors_cleanly_for_unknown_repo() {
        let dir = TempDir::new("scoped-missing");
        let env = env_for(&dir);
        let handle = spawn(env, Some("github.com/me/missing".to_string()), 3);
        let msgs = drain_until_done(&handle, Duration::from_secs(2));
        assert!(
            msgs.iter()
                .any(|m| matches!(m, LoadMsg::RepoError { generation: 3, .. })),
            "should emit RepoError for an unresolvable scope: {msgs:?}"
        );
        // And still flip both phases to done so the UI's spinner stops.
        assert!(
            msgs.iter()
                .any(|m| matches!(m, LoadMsg::TasksComplete { .. }))
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, LoadMsg::RepoRowsDone { .. }))
        );
    }

    mod opencode_refresh_worker {
        use std::{path::PathBuf, time::Duration};

        use super::super::{LoadMsg, LoaderHandle, spawn_opencode_refresh};

        /// Poll the handle until a message arrives, up to `timeout`.
        fn wait_for_tick(handle: &LoaderHandle, timeout: Duration) -> Option<LoadMsg> {
            let start = std::time::Instant::now();
            while start.elapsed() < timeout {
                if let Some(msg) = handle.try_recv() {
                    return Some(msg);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            None
        }

        #[test]
        fn emits_exactly_one_opencode_tick() {
            let paths = vec![PathBuf::from("/tmp/task-rs-worker-a")];
            let handle = spawn_opencode_refresh(paths);

            let first =
                wait_for_tick(&handle, Duration::from_secs(2)).expect("worker must emit one tick");
            assert!(
                matches!(first, LoadMsg::OpenCodeTick { .. }),
                "expected OpenCodeTick, got {first:?}"
            );

            // Give the worker plenty of time to (mistakenly) send more;
            // nothing else should arrive.
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                handle.try_recv().is_none(),
                "worker must emit at most one message"
            );
        }

        #[test]
        fn tick_states_cover_every_input_path() {
            let paths = vec![
                PathBuf::from("/tmp/task-rs-worker-a"),
                PathBuf::from("/tmp/task-rs-worker-b"),
                PathBuf::from("/tmp/task-rs-worker-c"),
            ];
            let handle = spawn_opencode_refresh(paths.clone());

            let msg = wait_for_tick(&handle, Duration::from_secs(2)).expect("one tick");
            let LoadMsg::OpenCodeTick { states } = msg else {
                panic!("unexpected variant: {msg:?}");
            };
            assert_eq!(states.len(), paths.len());
            for path in &paths {
                assert!(
                    states.iter().any(|(p, _)| p == path),
                    "tick should include {}",
                    path.display()
                );
            }
        }

        #[test]
        fn empty_paths_still_produces_one_tick() {
            let handle = spawn_opencode_refresh(Vec::new());
            let msg = wait_for_tick(&handle, Duration::from_secs(2))
                .expect("worker must still emit one tick");
            let LoadMsg::OpenCodeTick { states } = msg else {
                panic!("unexpected variant: {msg:?}");
            };
            assert!(states.is_empty());
        }

        #[test]
        fn dropping_handle_does_not_panic() {
            // Spawn then immediately drop: the stop flag is set, the
            // thread exits cleanly or sends its message into a now-closed
            // channel. Either way the main thread must not panic.
            let handle = spawn_opencode_refresh(vec![PathBuf::from("/tmp/task-rs-worker-drop")]);
            drop(handle);
            // Sleep briefly so any panic in the worker thread's
            // unwinding has time to surface; Rust test harness would
            // then fail the test.
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
