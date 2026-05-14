//! SQL lookup of sessions owned by a given set of candidate directories.
//!
//! The query is the hot path of a snapshot refresh: it runs once per
//! (DB, directory) pair and ultimately feeds every classifier call.

use rusqlite::Connection;

use crate::tools::opencode::db::SessionMeta;

/// Upper bound on sessions inspected per directory in one refresh.
///
/// No real OpenCode installation is anywhere near this — the busiest
/// directory observed in a dev laptop had 116 active sessions — but
/// we keep a ceiling so a pathological DB can't stall a refresh.
/// Raising from the original 20 ensures that a stuck session in a
/// busy directory is never silently hidden behind newer chatter.
const MAX_SESSIONS_PER_DIRECTORY: usize = 1_000;

pub(super) fn sessions_in_db(conn: &Connection, directories: &[String]) -> Vec<SessionMeta> {
    // Defensive: an empty `IN ()` clause is a SQL syntax error in
    // SQLite. Callers today always pass a non-empty slice, but let's
    // not trust that to hold forever.
    if directories.is_empty() {
        return Vec::new();
    }
    let placeholders: Vec<String> = (1..=directories.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT id, title, time_updated FROM session \
         WHERE time_archived IS NULL AND directory IN ({}) \
         ORDER BY time_updated DESC LIMIT {}",
        placeholders.join(", "),
        MAX_SESSIONS_PER_DIRECTORY,
    );

    let params = rusqlite::params_from_iter(directories.iter().map(String::as_str));
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let rows = stmt
        .query_map(params, |row| {
            Ok(SessionMeta {
                id: row.get::<_, String>(0)?,
                title: row.get::<_, String>(1)?,
                time_updated: row.get::<_, i64>(2)?,
            })
        })
        .ok();
    let Some(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|row| row.ok()).collect()
}
