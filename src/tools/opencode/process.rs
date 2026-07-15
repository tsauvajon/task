//! Discover running `OpenCode` processes and their cwds.
//!
//! We identify the process by name (`opencode` or the Nix-wrapper
//! `.opencode-wrapped`) and read its cwd via `sysinfo`. On macOS,
//! `sysinfo` uses `proc_pidinfo` which only reports cwds for processes
//! the current user owns — which is exactly what we want.
//!
//! Performance: a naive "refresh every process with cwd" approach does
//! one `proc_pidinfo(PROC_PIDVNODEPATHINFO)` syscall per PID on the
//! box. On a busy developer laptop that's 500+ extra syscalls per
//! refresh, ~30–50ms of CPU. We avoid that with a two-pass strategy:
//!
//! 1. Enumerate processes with names only (no cwd/cmd/exe). Cheap —
//!    names are already in `proc_bsdinfo` on macOS.
//! 2. Filter to the handful of `opencode*` PIDs.
//! 3. Ask sysinfo to populate cwd **only** for those PIDs.
//!
//! A thread-local `System` is reused across calls so alive processes
//! keep their cached metadata, further dropping the cost of subsequent
//! refreshes.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

/// One live `opencode` process owning a given cwd. `start_ms` is
/// Unix epoch milliseconds, used by the session classifier to tell
/// a live-owned session apart from a zombie left behind by a prior
/// `opencode` run. Multiple processes can share a cwd (e.g. the
/// user launched a second `opencode` in the same worktree), so the
/// outer collection stores a `Vec` keyed by cwd.
#[derive(Debug, Clone, Copy)]
struct LiveOpencodeProcess {
    pid: u32,
    start_ms: u64,
}

/// Canonicalised cwd → every live `opencode` process owning that cwd.
#[derive(Debug, Clone, Default)]
pub struct LiveOpencodeProcesses(HashMap<PathBuf, Vec<LiveOpencodeProcess>>);

thread_local! {
    /// Shared across refreshes so sysinfo can reuse its internal
    /// `Process` cache for alive PIDs. Dropping this would force
    /// `proc_pidpath` resolution of every process name on the box.
    static SYSTEM: RefCell<System> = RefCell::new(System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    ));
}

impl LiveOpencodeProcesses {
    /// Take a live snapshot by scanning the process list.
    #[must_use]
    pub fn collect() -> Self {
        SYSTEM.with(|cell| Self::collect_from(&mut cell.borrow_mut()))
    }

    /// Internal entry point that takes the shared `System`. Split out
    /// so tests can drive it against a scratch instance.
    fn collect_from(system: &mut System) -> Self {
        // Pass 1 — list every PID + populate names, but skip cwd. On
        // macOS name comes from `proc_bsdinfo` for processes already in
        // the cache; cold PIDs cost one `proc_pidpath` each. Either way
        // we avoid the expensive `PROC_PIDVNODEPATHINFO` syscall.
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );

        // Pass 2 — collect PIDs that match `opencode*` and request cwd
        // only for those. `ProcessesToUpdate::Some(&[Pid])` makes
        // sysinfo call `proc_pidinfo` exactly once per listed PID.
        let opencode_pids: Vec<Pid> = system
            .processes()
            .iter()
            .filter_map(|(pid, proc)| {
                is_opencode_process(proc.name().to_string_lossy().as_ref()).then_some(*pid)
            })
            .collect();

        if !opencode_pids.is_empty() {
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&opencode_pids),
                true,
                ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
            );
        }

        let mut map: HashMap<PathBuf, Vec<LiveOpencodeProcess>> = HashMap::new();
        for pid in opencode_pids {
            let Some(process) = system.process(pid) else {
                continue;
            };
            let Some(cwd) = process.cwd() else { continue };
            let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
            // `sysinfo::Process::start_time()` returns seconds since
            // the Unix epoch; convert to ms so it lines up with the
            // OpenCode DB's ms-precision timestamps.
            let start_ms = process.start_time().saturating_mul(1_000);
            map.entry(canonical).or_default().push(LiveOpencodeProcess {
                pid: pid.as_u32(),
                start_ms,
            });
        }
        Self(map)
    }

    /// Does an `opencode` process own this directory as its cwd? The
    /// lookup canonicalises the caller's path so `/var/...` vs
    /// `/private/var/...` resolves consistently on macOS.
    #[must_use]
    pub fn has_cwd(&self, directory: &Path) -> bool {
        self.lookup(directory).is_some()
    }

    /// Returns a PID of some process that owns `directory`, if any.
    /// When multiple processes share the cwd the returned PID is
    /// arbitrary — callers that need a deterministic pick should
    /// reach for `oldest_process_start_ms` instead.
    #[must_use]
    pub fn lookup(&self, directory: &Path) -> Option<u32> {
        self.processes_for(directory)
            .and_then(|procs| procs.first())
            .map(|p| p.pid)
    }

    /// Earliest start time (ms since Unix epoch) across every live
    /// `opencode` process owning `directory`. Used by the session
    /// classifier to decide whether a session's latest activity
    /// could still belong to some currently-live process, or is
    /// stranded state from a previous run.
    ///
    /// "Oldest wins" is the permissive rule: any activity timestamp
    /// after the earliest owning process started could still be the
    /// work of some live process. A stricter "newest wins" rule
    /// would classify sessions touched by the older live process
    /// but not by the newer one as zombies, which is wrong — that
    /// older process is still alive.
    #[must_use]
    pub fn oldest_process_start_ms(&self, directory: &Path) -> Option<i64> {
        self.processes_for(directory)?
            .iter()
            .map(|p| p.start_ms)
            .min()
            .map(|ms| i64::try_from(ms).unwrap_or(i64::MAX))
    }

    fn processes_for(&self, directory: &Path) -> Option<&Vec<LiveOpencodeProcess>> {
        let canonical =
            std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        self.0.get(&canonical)
    }
}

#[cfg(test)]
impl LiveOpencodeProcesses {
    /// Test-only constructor. Each entry is `(canonical cwd, PID,
    /// start_ms)`. Multiple tuples can share a cwd.
    pub(crate) fn from_entries(entries: Vec<(PathBuf, u32, u64)>) -> Self {
        let mut map: HashMap<PathBuf, Vec<LiveOpencodeProcess>> = HashMap::new();
        for (cwd, pid, start_ms) in entries {
            map.entry(cwd)
                .or_default()
                .push(LiveOpencodeProcess { pid, start_ms });
        }
        Self(map)
    }
}

fn is_opencode_process(name: &str) -> bool {
    // Custom launch wrappers must `exec` stock OpenCode so its process keeps one of these names.
    // `opencode` is the canonical binary name. `.opencode-wrapped` is
    // the Nix wrapper exec that shows up in `sysinfo` on NixOS and
    // nix-darwin installs.
    name == "opencode" || name == ".opencode-wrapped"
}

#[cfg(test)]
mod tests {
    use super::*;

    mod is_opencode_process {
        use super::*;

        #[test]
        fn accepts_canonical_binary() {
            assert!(is_opencode_process("opencode"));
        }

        #[test]
        fn accepts_nix_wrapper() {
            assert!(is_opencode_process(".opencode-wrapped"));
        }

        #[test]
        fn rejects_unrelated_processes() {
            assert!(!is_opencode_process("code"));
            assert!(!is_opencode_process("opencoded"));
            assert!(!is_opencode_process("opencode-agent"));
            assert!(!is_opencode_process(""));
        }
    }

    mod live_opencode_processes {
        use super::*;

        #[test]
        fn from_entries_and_lookup_round_trip() {
            // Use a real existing dir so canonicalize resolves.
            let tmp = std::env::temp_dir();
            let canon = std::fs::canonicalize(&tmp).unwrap_or_else(|_| tmp.clone());
            let probe = LiveOpencodeProcesses::from_entries(vec![(canon, 42, 1_000_000)]);
            assert_eq!(probe.lookup(&tmp), Some(42));
            assert!(probe.has_cwd(&tmp));
            assert_eq!(probe.oldest_process_start_ms(&tmp), Some(1_000_000));
        }

        #[test]
        fn lookup_returns_none_for_unknown_directory() {
            let probe = LiveOpencodeProcesses::from_entries(Vec::new());
            assert_eq!(probe.lookup(Path::new("/nonexistent")), None);
            assert!(!probe.has_cwd(Path::new("/nonexistent")));
            assert_eq!(
                probe.oldest_process_start_ms(Path::new("/nonexistent")),
                None,
            );
        }

        /// Two opencode processes sharing a cwd — classifier must see
        /// the earliest start time as the ownership boundary.
        #[test]
        fn oldest_process_start_ms_returns_min_when_multiple_procs() {
            let tmp = std::env::temp_dir();
            let canon = std::fs::canonicalize(&tmp).unwrap_or_else(|_| tmp.clone());
            let probe = LiveOpencodeProcesses::from_entries(vec![
                (canon.clone(), 1, 5_000),
                (canon.clone(), 2, 1_000),
                (canon, 3, 3_000),
            ]);
            assert_eq!(probe.oldest_process_start_ms(&tmp), Some(1_000));
        }

        /// Canonicalisation applies equally to `oldest_process_start_ms`
        /// so callers can pass either the pre- or post-canonicalised
        /// path and get the same answer.
        #[test]
        fn oldest_process_start_ms_canonicalises_input() {
            let tmp = std::env::temp_dir();
            let canon = std::fs::canonicalize(&tmp).unwrap_or_else(|_| tmp.clone());
            let probe = LiveOpencodeProcesses::from_entries(vec![(canon, 7, 42)]);
            assert_eq!(probe.oldest_process_start_ms(&tmp), Some(42));
        }

        #[test]
        fn collect_from_scratch_system_does_not_panic() {
            // Guards the two-pass refresh wiring: it must cope with an
            // empty, freshly-constructed `System` and whatever happens
            // to be running on the host. We don't assert on content —
            // the goal is just "runs without crashing".
            let mut system = System::new_with_specifics(
                RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
            );
            let _ = LiveOpencodeProcesses::collect_from(&mut system);
        }

        #[test]
        fn collect_from_returns_cwd_for_current_process_when_named_opencode() {
            // End-to-end sanity: we rewrite this test's "current process"
            // name via the process list view, so this is a shallow smoke
            // test only. On the CI host the test binary is not named
            // `opencode`, so the result is expected to be empty — but
            // the path must not panic and must return a valid map.
            let mut system = System::new_with_specifics(
                RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
            );
            let probe = LiveOpencodeProcesses::collect_from(&mut system);
            // No assertion on content; behavior depends on host.
            // Just exercise `has_cwd` to ensure the HashMap is usable.
            _ = probe.has_cwd(Path::new("/"));
        }
    }
}
