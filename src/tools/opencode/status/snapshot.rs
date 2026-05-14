//! Per-refresh snapshot that classifies every visible worktree in one
//! pass.
//!
//! Owning the DB connection cache and the live-process list in a single
//! value keeps the hot path cheap: scanning processes and discovering
//! DB files happens once per refresh, then every classification reuses
//! the same opened connections.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use rusqlite::Connection;

use super::{
    activity::latest_session_activity,
    classify::{OpenCodeState, classify_with_conn},
    sessions::sessions_in_db,
};
use crate::tools::opencode::{db, process::LiveOpencodeProcesses};

/// One-shot snapshot reused across many `classify` calls in a single
/// refresh cycle. Holds the list of live OpenCode cwds, the discovered
/// DB files, and a lazy cache of read-only connections so that
/// classifying N paths does not re-open the same DB 2N+ times.
pub struct OpenCodeSnapshot {
    processes: LiveOpencodeProcesses,
    dbs: Vec<PathBuf>,
    now_ms: i64,
    /// Lazy connection cache keyed by DB path. `Mutex` (not `RefCell`)
    /// because the snapshot may be shared across rayon worker threads
    /// during the initial full scan. Contention is trivial: the cache
    /// fills after the first few paths, then every lookup is a ~100ns
    /// unlock → hashmap probe → unlock.
    connections: Mutex<HashMap<PathBuf, Option<Connection>>>,
}

impl OpenCodeSnapshot {
    #[must_use]
    pub fn collect() -> Self {
        Self {
            processes: LiveOpencodeProcesses::collect(),
            dbs: db::discover_opencode_dbs(),
            now_ms: db::now_millis(),
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// Test-only constructor that stitches together a synthetic
    /// snapshot from pre-built components. Lives here rather than in
    /// `test_support` so the `connections` field can stay private.
    #[cfg(test)]
    pub(super) fn new_for_test(
        processes: LiveOpencodeProcesses,
        dbs: Vec<PathBuf>,
        now_ms: i64,
    ) -> Self {
        Self {
            processes,
            dbs,
            now_ms,
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// Classify every live-owned session associated with `directory`
    /// and roll them up by severity.
    ///
    /// "Live-owned" means:
    ///
    /// 1. An `opencode` process has this directory as its cwd.
    /// 2. The session's latest activity (max across `message` and
    ///    `part` rows) is at or after the oldest such process's
    ///    start time.
    ///
    /// Sessions failing either condition are zombies from a previous
    /// `opencode` run — their in-flight state (leaked running tool
    /// parts, uncompleted assistant messages) is stale bookkeeping,
    /// not a live signal, so they must not contribute to the rollup.
    ///
    /// Return values:
    ///
    /// - No sessions in this directory → `None`.
    /// - Sessions exist but no live process owns the cwd → `Gone`.
    /// - Sessions exist, process is live, but every session is a
    ///   zombie → `None` (treated as "no live-owned sessions").
    /// - Otherwise → max severity across live-owned sessions.
    pub fn state_for(&self, directory: &Path) -> OpenCodeState {
        let sessions = self.sessions_for_directory(directory);
        if sessions.is_empty() {
            return OpenCodeState::None;
        }
        let Some(proc_start_ms) = self.processes.oldest_process_start_ms(directory) else {
            // Raw sessions exist but no live opencode owns the cwd:
            // every session is definitionally a zombie. Today's UI
            // surfaces this as `Gone` so the user sees the row but
            // knows nothing is running — preserve that.
            return OpenCodeState::Gone;
        };

        let live_owned = self.filter_live_owned(sessions, proc_start_ms);
        if live_owned.is_empty() {
            // Live process exists but nothing in this directory has
            // been touched since it started. Treat as "no sessions"
            // rather than inventing a state for zombies — matches the
            // agreed rule that classification only works on alive
            // OpenCode processes.
            return OpenCodeState::None;
        }

        // `OpenCodeState` derives `Ord` with declaration order equal
        // to rollup priority, so `Iterator::max` already picks the
        // highest-priority classification across sessions.
        live_owned
            .into_iter()
            .map(|(db_path, session)| self.classify_session(&db_path, &session))
            .max()
            .unwrap_or(OpenCodeState::None)
    }

    /// Return the latest non-empty OpenCode session title for `directory`.
    #[must_use]
    pub fn title_for(&self, directory: &Path) -> Option<String> {
        self.sessions_for_directory(directory)
            .into_iter()
            .map(|(_, session)| session)
            .max_by_key(|session| session.time_updated)
            .and_then(|session| normalized_title(&session.title))
    }

    #[must_use]
    pub fn last_activity_for(&self, directory: &Path) -> Option<i64> {
        self.sessions_for_directory(directory)
            .into_iter()
            .map(|(db_path, session)| self.session_activity(&db_path, &session))
            .max()
    }

    /// Drop sessions whose latest activity predates the oldest live
    /// `opencode` process for the cwd.
    ///
    /// Activity is the max of `message.time_updated`, `part.time_updated`,
    /// and `session.time_updated`. The `session` fallback matters for
    /// brand-new sessions that a live opencode just created but has
    /// not yet flushed any messages/parts for — they classify as
    /// `Idle` today and must keep doing so.
    fn filter_live_owned(
        &self,
        sessions: Vec<(PathBuf, db::SessionMeta)>,
        proc_start_ms: i64,
    ) -> Vec<(PathBuf, db::SessionMeta)> {
        sessions
            .into_iter()
            .filter(|(db_path, session)| self.session_activity(db_path, session) >= proc_start_ms)
            .collect()
    }

    fn session_activity(&self, db_path: &Path, session: &db::SessionMeta) -> i64 {
        self.with_conn(db_path, |conn| latest_session_activity(conn, &session.id))
            .flatten()
            .unwrap_or(session.time_updated)
            .max(session.time_updated)
    }

    /// Run `f` against the cached connection for `db_path`, opening
    /// one on first use. Returns `None` when the DB cannot be opened.
    ///
    /// The cache lock is held for the duration of `f`. That serialises
    /// classification across rayon workers, but each call runs a tiny
    /// bounded SQL query (`LIMIT 1` / `LIMIT 10`), so holding the lock
    /// across it is fine in practice. Per-rayon-thread connections
    /// would be an option if contention shows up in a profile.
    fn with_conn<R>(&self, db_path: &Path, f: impl FnOnce(&Connection) -> R) -> Option<R> {
        let mut cache = self
            .connections
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let entry = cache
            .entry(db_path.to_path_buf())
            .or_insert_with(|| db::open_ro(db_path));
        entry.as_ref().map(f)
    }

    fn sessions_for_directory(&self, directory: &Path) -> Vec<(PathBuf, db::SessionMeta)> {
        if self.dbs.is_empty() {
            return Vec::new();
        }
        let canonical = db::canonical_dir(directory);
        let candidates = db::directory_candidates(directory, &canonical);

        let mut out = Vec::new();
        for db_path in &self.dbs {
            let sessions = self
                .with_conn(db_path, |conn| sessions_in_db(conn, &candidates))
                .unwrap_or_default();
            for session in sessions {
                out.push((db_path.clone(), session));
            }
        }
        out
    }

    /// Classify a single session against its owning DB. Callers guard
    /// on process liveness before calling — the result is always one
    /// of `Busy`, `Idle`, or `Hung`.
    fn classify_session(&self, db_path: &Path, session: &db::SessionMeta) -> OpenCodeState {
        let Some(state) = self.with_conn(db_path, |conn| {
            classify_with_conn(conn, &session.id, self.now_ms)
        }) else {
            // DB couldn't be opened; treat as inconclusive rather than
            // hiding the session entirely.
            return OpenCodeState::Idle;
        };
        state
    }
}

fn normalized_title(title: &str) -> Option<String> {
    let title = title.trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use rusqlite::Connection;

    use super::{super::test_support::*, *};

    mod state_for {
        use super::*;

        #[test]
        fn no_dbs_returns_none() {
            let snap = snapshot_with(Vec::new(), 1_000, Vec::new());
            assert_eq!(snap.state_for(Path::new("/wt/a")), OpenCodeState::None);
        }

        #[test]
        fn no_session_for_directory_returns_none() {
            let base = temp("no-session");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", "/wt/elsewhere", 100);

            let snap = snapshot_with(vec![db], 1_000, Vec::new());
            assert_eq!(snap.state_for(Path::new("/wt/here")), OpenCodeState::None);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn session_without_live_process_is_shut() {
            let base = temp("closed");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", "/wt/here", 100);

            let snap = snapshot_with(vec![db], 1_000, Vec::new());
            assert_eq!(snap.state_for(Path::new("/wt/here")), OpenCodeState::Gone);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn completed_assistant_is_idle() {
            let base = temp("idle");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990, "completed": 995 }
                }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Idle);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn streaming_assistant_within_threshold_is_busy() {
            let base = temp("running");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                9_500,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 9_500 }
                }),
            );

            let snap = snapshot_with(vec![db], 10_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Busy);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn streaming_assistant_beyond_threshold_is_hung() {
            let base = temp("stuck-stream");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                1_000,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 1_000 }
                }),
            );

            let snap = snapshot_with(vec![db], 200_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Hung);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn assistant_api_error_is_hung() {
            // An APIError (rate limit, 5xx, etc.) means the agent
            // stopped and the user needs to restart work — `Hung`.
            let base = temp("stuck-api-error");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990, "completed": 995 },
                    "error": { "name": "APIError", "message": "rate limited" }
                }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Hung);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn assistant_auth_error_is_hung() {
            // An auth error is still an agent-stopped-needs-attention
            // signal — only `MessageAbortedError` is excluded.
            let base = temp("stuck-auth-error");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990, "completed": 995 },
                    "error": { "name": "ProviderAuthError", "providerID": "anthropic", "message": "expired" }
                }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Hung);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn user_aborted_turn_is_done_not_hung() {
            // The user pressed ESC to cancel the turn. OpenCode records
            // this as `MessageAbortedError` on the assistant message.
            // Flagging the user's own action as needing attention would
            // be noise, so this one error class is excluded.
            let base = temp("aborted-turn");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990, "completed": 995 },
                    "error": { "name": "MessageAbortedError", "message": "Aborted" }
                }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Idle);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn running_tool_part_within_threshold_is_busy() {
            let base = temp("tool-running");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990 }
                }),
            );
            insert_tool_part(&conn, "s1", 990, "running", 995);

            let snap = snapshot_with(vec![db], 1_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Busy);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn running_tool_part_beyond_threshold_is_hung() {
            let base = temp("tool-stuck");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990 }
                }),
            );
            insert_tool_part(&conn, "s1", 990, "running", 1_000);

            let snap = snapshot_with(vec![db], 200_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Hung);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn rollup_picks_highest_priority_across_sessions() {
            let base = temp("rollup");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 200);
            insert_session(&conn, "s2", &base.to_string_lossy(), 100);

            // s1: idle (completed assistant).
            insert_message(
                &conn,
                "s1",
                190,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 190, "completed": 195 }
                }),
            );
            // s2: stuck (model error).
            insert_message(
                &conn,
                "s2",
                90,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 90, "completed": 95 },
                    "error": { "name": "oops" }
                }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![base.clone()]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Hung);

            let _ = fs::remove_dir_all(&base);
        }
    }
    mod live_owned_filter {
        use super::*;

        /// A session whose latest activity predates the live opencode
        /// process is a zombie from a previous run. The classifier
        /// must not see it even though the owning directory is
        /// currently live.
        #[test]
        fn zombie_session_is_filtered_out() {
            let base = temp("filter-zombie");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            // Assistant message with no completion, created well
            // before the live process started. Unfiltered this would
            // classify as `Hung` under the elapsed-time rule.
            insert_message(
                &conn,
                "s1",
                100,
                serde_json::json!({ "role": "assistant", "time": { "created": 100 } }),
            );

            // Live process started at t=10_000, well after the
            // zombie session's activity at t=100.
            let snap = snapshot_with_proc_starts(vec![db], 200_000, vec![(base.clone(), 10_000)]);
            assert_eq!(snap.state_for(&base), OpenCodeState::None);

            let _ = fs::remove_dir_all(&base);
        }

        /// Session whose activity crosses the process-start boundary
        /// is live-owned and must classify normally.
        #[test]
        fn session_with_activity_after_proc_start_classifies_normally() {
            let base = temp("filter-live");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            // In-flight assistant message created after proc_start,
            // still within the busy threshold.
            insert_message(
                &conn,
                "s1",
                10_500,
                serde_json::json!({ "role": "assistant", "time": { "created": 10_500 } }),
            );

            let snap = snapshot_with_proc_starts(vec![db], 11_000, vec![(base.clone(), 10_000)]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Busy);

            let _ = fs::remove_dir_all(&base);
        }

        /// Live-owned Busy session plus zombie Hung session in the
        /// same cwd → rollup surfaces the live state, not the zombie.
        /// This is the exact shape of the bug that motivated the
        /// filter.
        #[test]
        fn mix_of_live_and_zombie_rolls_up_to_live_state() {
            let base = temp("filter-mix");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();

            // Zombie: ancient in-flight assistant message.
            insert_session(&conn, "s_zombie", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s_zombie",
                100,
                serde_json::json!({ "role": "assistant", "time": { "created": 100 } }),
            );

            // Live: fresh in-flight message.
            insert_session(&conn, "s_live", &base.to_string_lossy(), 10_500);
            insert_message(
                &conn,
                "s_live",
                10_500,
                serde_json::json!({ "role": "assistant", "time": { "created": 10_500 } }),
            );

            let snap = snapshot_with_proc_starts(vec![db], 11_000, vec![(base.clone(), 10_000)]);
            assert_eq!(snap.state_for(&base), OpenCodeState::Busy);

            let _ = fs::remove_dir_all(&base);
        }

        /// Every session in the cwd is a zombie → classifier returns
        /// `None` even though a live opencode owns the cwd. Matches
        /// the agreed rule that zombies don't exist from the
        /// classifier's perspective.
        #[test]
        fn all_zombies_with_live_process_returns_none() {
            let base = temp("filter-all-zombies");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                100,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 100, "completed": 200 },
                    "error": { "name": "APIError", "message": "rate limited" }
                }),
            );

            let snap = snapshot_with_proc_starts(vec![db], 200_000, vec![(base.clone(), 10_000)]);
            assert_eq!(snap.state_for(&base), OpenCodeState::None);

            let _ = fs::remove_dir_all(&base);
        }

        /// Two live opencode processes in the same cwd: the oldest
        /// start time sets the filter boundary. A session touched
        /// after the older process started (but before the newer one)
        /// is still live-owned.
        #[test]
        fn oldest_process_bounds_when_multiple_procs_share_cwd() {
            let base = temp("filter-multi-proc");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                2_500,
                serde_json::json!({ "role": "assistant", "time": { "created": 2_500 } }),
            );

            // Two procs: one started at 2_000 (older), one at 5_000.
            // Oldest wins → session activity at 2_500 is live-owned.
            let snap = snapshot_with_proc_starts(
                vec![db],
                3_000,
                vec![(base.clone(), 2_000), (base.clone(), 5_000)],
            );
            assert_eq!(snap.state_for(&base), OpenCodeState::Busy);

            let _ = fs::remove_dir_all(&base);
        }

        /// `Gone` still applies when a session exists but no live
        /// opencode owns the cwd — independently of the filter. Pins
        /// the decision-table row where raw sessions exist but no
        /// live process does.
        #[test]
        fn session_with_no_live_process_is_gone_unchanged() {
            let base = temp("filter-gone");
            let db = new_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "s1", &base.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s1",
                10_500,
                serde_json::json!({ "role": "assistant", "time": { "created": 10_500 } }),
            );

            // No live cwds — should return `Gone` regardless of the
            // session's activity timestamp.
            let snap = snapshot_with(vec![db], 11_000, Vec::new());
            assert_eq!(snap.state_for(&base), OpenCodeState::Gone);

            let _ = fs::remove_dir_all(&base);
        }
    }
    mod with_conn_cache {
        use super::*;

        #[test]
        fn second_call_reuses_cached_connection_after_file_is_removed() {
            let base = temp("cache-reuse");
            let db = new_db(&base, "opencode-stable.db");

            let snapshot = OpenCodeSnapshot::new_for_test(
                LiveOpencodeProcesses::default(),
                vec![db.clone()],
                0,
            );

            // First call opens the connection and caches it.
            let first = snapshot.with_conn(&db, |_| 1u32);
            assert_eq!(first, Some(1));

            // Remove the file. Because the connection is already cached,
            // the second call should still succeed (proving it did not
            // re-open).
            std::fs::remove_file(&db).unwrap();

            let second = snapshot.with_conn(&db, |_| 2u32);
            assert_eq!(
                second,
                Some(2),
                "cached connection must be reused even after the file is gone"
            );

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn unreadable_path_caches_none_and_short_circuits() {
            let base = temp("cache-none");
            let missing = base.join("not-a-real-file.db");

            let snapshot =
                OpenCodeSnapshot::new_for_test(LiveOpencodeProcesses::default(), Vec::new(), 0);

            // First call: can't open → returns None, closure not called.
            let mut calls = 0;
            let first = snapshot.with_conn(&missing, |_| {
                calls += 1;
            });
            assert!(first.is_none());
            assert_eq!(calls, 0);

            // Second call: the None is cached; closure still not called.
            let second = snapshot.with_conn(&missing, |_| {
                calls += 1;
            });
            assert!(second.is_none());
            assert_eq!(calls, 0, "closure must not be invoked for a cached None");

            let _ = fs::remove_dir_all(&base);
        }
    }

    mod title_for {
        use super::*;

        #[test]
        fn returns_latest_non_empty_title() {
            let base = temp("title-for");
            let db = new_db(&base, "opencode-stable.db");
            let dir = base.join("worktree");
            std::fs::create_dir_all(&dir).unwrap();

            let conn = Connection::open(&db).unwrap();
            insert_session_with_title(&conn, "old", &dir.to_string_lossy(), "Old title", 100);
            insert_session_with_title(&conn, "new", &dir.to_string_lossy(), "Ship cards", 200);

            let snap = snapshot_with(vec![db], 1_000, vec![dir.clone()]);
            assert_eq!(snap.title_for(&dir).as_deref(), Some("Ship cards"));

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn blank_title_returns_none() {
            let base = temp("title-blank");
            let db = new_db(&base, "opencode-stable.db");
            let dir = base.join("worktree");
            std::fs::create_dir_all(&dir).unwrap();

            let conn = Connection::open(&db).unwrap();
            insert_session_with_title(&conn, "session", &dir.to_string_lossy(), "   ", 100);

            let snap = snapshot_with(vec![db], 1_000, vec![dir.clone()]);
            assert!(snap.title_for(&dir).is_none());

            let _ = fs::remove_dir_all(&base);
        }
    }

    mod last_activity_for {
        use super::*;

        #[test]
        fn returns_latest_activity_across_matching_sessions() {
            let base = temp("last-activity");
            let db = new_db(&base, "opencode-stable.db");
            let dir = base.join("worktree");
            std::fs::create_dir_all(&dir).unwrap();

            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "older", &dir.to_string_lossy(), 100);
            insert_session(&conn, "newer", &dir.to_string_lossy(), 200);
            insert_message(
                &conn,
                "older",
                900,
                serde_json::json!({ "role": "assistant", "time": { "created": 900 } }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![dir.clone()]);
            assert_eq!(snap.last_activity_for(&dir), Some(900));

            let _ = fs::remove_dir_all(&base);
        }
    }

    mod state_for_multi_dir {
        use super::*;

        /// Two directories share the same DB; the snapshot should
        /// classify each independently by directory.
        #[test]
        fn directories_sharing_one_db_are_classified_independently() {
            let base = temp("multi-dir");
            let db = new_db(&base, "opencode-stable.db");
            let dir_a = base.join("dir-a");
            let dir_b = base.join("dir-b");
            std::fs::create_dir_all(&dir_a).unwrap();
            std::fs::create_dir_all(&dir_b).unwrap();

            let conn = Connection::open(&db).unwrap();
            // a → idle (completed assistant).
            insert_session(&conn, "s_a", &dir_a.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s_a",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990, "completed": 995 }
                }),
            );
            // b → stuck (model error).
            insert_session(&conn, "s_b", &dir_b.to_string_lossy(), 100);
            insert_message(
                &conn,
                "s_b",
                990,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990, "completed": 995 },
                    "error": { "name": "oops" }
                }),
            );

            let snap = snapshot_with(vec![db], 1_000, vec![dir_a.clone(), dir_b.clone()]);
            assert_eq!(snap.state_for(&dir_a), OpenCodeState::Idle);
            assert_eq!(snap.state_for(&dir_b), OpenCodeState::Hung);

            let _ = fs::remove_dir_all(&base);
        }
    }
}
