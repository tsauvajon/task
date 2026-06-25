//! Shared test fixtures used by the per-module test blocks and by the
//! `real_cases` integration suite.
//!
//! Every helper matches the classifier-visible fields of the real
//! `OpenCode` schema. Anything not classifier-visible is intentionally
//! omitted so the fixtures stay small and evolve alongside the rules
//! that inspect them.

#![cfg(test)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;

use super::snapshot::OpenCodeSnapshot;
use crate::tools::opencode::process::LiveOpencodeProcesses;

pub(super) fn create_schema(conn: &Connection) {
    // Mirrors the real OpenCode schema's classifier-visible
    // columns. `parent_id` in particular is required so subagent
    // child sessions can be linked to their parent exactly as
    // the live DB does it (see `session_parent_idx`).
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY,
            parent_id TEXT,
            directory TEXT NOT NULL DEFAULT '',
            title TEXT NOT NULL DEFAULT '',
            time_updated INTEGER NOT NULL DEFAULT 0,
            time_archived INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL DEFAULT 0,
            time_updated INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL DEFAULT '{}'
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL DEFAULT 0,
            time_updated INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL DEFAULT '{}'
        );",
    )
    .unwrap();
}

pub(super) fn new_db(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let conn = Connection::open(&path).unwrap();
    create_schema(&conn);
    path
}

pub(super) fn insert_session(conn: &Connection, id: &str, directory: &str, time_updated: i64) {
    insert_session_with_parent(conn, id, None, directory, time_updated, None);
}

pub(super) fn insert_session_with_title(
    conn: &Connection,
    id: &str,
    directory: &str,
    title: &str,
    time_updated: i64,
) {
    conn.execute(
        "INSERT INTO session (id, directory, title, time_updated) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, directory, title, time_updated],
    )
    .unwrap();
}

/// Insert a session row with optional `parent_id` and
/// `time_archived`. Subagent child sessions carry a non-NULL
/// `parent_id` pointing at the parent that spawned them.
pub(super) fn insert_session_with_parent(
    conn: &Connection,
    id: &str,
    parent_id: Option<&str>,
    directory: &str,
    time_updated: i64,
    time_archived: Option<i64>,
) {
    conn.execute(
        "INSERT INTO session (id, parent_id, directory, time_updated, time_archived) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, parent_id, directory, time_updated, time_archived],
    )
    .unwrap();
}

pub(super) fn insert_message(
    conn: &Connection,
    session_id: &str,
    created: i64,
    data: serde_json::Value,
) {
    let data_json = data.to_string();
    drop(data);
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![format!("msg_{created}_{session_id}"), session_id, created, data_json],
    )
    .unwrap();
}

pub(super) fn insert_tool_part(
    conn: &Connection,
    session_id: &str,
    updated: i64,
    status: &str,
    start: i64,
) {
    let data = serde_json::json!({
        "type": "tool",
        "tool": "bash",
        "state": {
            "status": status,
            "time": { "start": start }
        }
    });
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            format!("prt_{updated}_{session_id}"),
            format!("msg_any_{session_id}"),
            session_id,
            updated,
            data.to_string(),
        ],
    )
    .unwrap();
}

pub(super) fn temp(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("task-rs-opencode-status-{tag}"));
    _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

/// Build a snapshot with the provided databases and live cwds. Each
/// cwd gets a synthetic live opencode process started at t=0 so the
/// session filter keeps every session — matches the "no zombies"
/// behaviour the original tests relied on before the filter existed.
pub(super) fn snapshot_with(
    dbs: Vec<PathBuf>,
    now_ms: i64,
    live_cwds: Vec<PathBuf>,
) -> OpenCodeSnapshot {
    let cwds_with_start: Vec<(PathBuf, u64)> = live_cwds.into_iter().map(|cwd| (cwd, 0)).collect();
    snapshot_with_proc_starts(dbs, now_ms, cwds_with_start)
}

/// Variant of [`snapshot_with`] that lets a test set each live
/// process's `start_ms`. Used by the session-filter tests where
/// zombie vs live-owned hinges on the boundary.
pub(super) fn snapshot_with_proc_starts(
    dbs: Vec<PathBuf>,
    now_ms: i64,
    live_cwds_with_start: Vec<(PathBuf, u64)>,
) -> OpenCodeSnapshot {
    let entries = live_cwds_with_start
        .into_iter()
        .enumerate()
        .map(|(idx, (cwd, start_ms))| {
            let canon = std::fs::canonicalize(&cwd).unwrap_or(cwd);
            (
                canon,
                u32::try_from(idx).expect("test process index fits u32"),
                start_ms,
            )
        })
        .collect();
    OpenCodeSnapshot::new_for_test(LiveOpencodeProcesses::from_entries(entries), dbs, now_ms)
}
