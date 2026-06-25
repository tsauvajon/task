//! Last-activity timestamp for a single `OpenCode` session.
//!
//! Shared between the classifier (which uses it to measure subagent
//! child liveness) and the snapshot filter (which uses it to reject
//! zombie sessions whose latest activity predates every currently
//! live `OpenCode` process).

use rusqlite::Connection;

/// Latest `time_updated` across a session's `message` and `part`
/// rows, in milliseconds. Returns `None` when the session has no
/// rows at all — distinct from `Some(0)`, which would be a row
/// explicitly pinned to epoch.
///
/// Both tables mutate `time_updated` during streaming (new message
/// chunks, flipping tool state), so the max across both captures
/// "did anything happen in this session recently".
pub(super) fn latest_session_activity(conn: &Connection, session_id: &str) -> Option<i64> {
    // Separate MAX queries per table keep each leg indexed. Combining
    // with a Rust-side max avoids depending on SQLite's handling of
    // NULL in a single compound query.
    let message_max: Option<i64> = conn
        .query_row(
            "SELECT MAX(time_updated) FROM message WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten();
    let part_max: Option<i64> = conn
        .query_row(
            "SELECT MAX(time_updated) FROM part WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten();
    match (message_max, part_max) {
        (Some(m), Some(p)) => Some(m.max(p)),
        (Some(v), None) | (None, Some(v)) => Some(v),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use rusqlite::Connection;

    use super::{super::test_support::*, *};

    fn conn_for(base: &Path) -> Connection {
        let path = base.join("activity.db");
        let conn = Connection::open(&path).unwrap();
        create_schema(&conn);
        conn
    }

    #[test]
    fn returns_none_when_session_has_no_rows() {
        let base = temp("activity-empty");
        let conn = conn_for(&base);

        assert_eq!(latest_session_activity(&conn, "s1"), None);

        _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn returns_message_time_updated_when_no_parts() {
        let base = temp("activity-msg-only");
        let conn = conn_for(&base);
        insert_message(
            &conn,
            "s1",
            500,
            serde_json::json!({ "role": "user", "time": { "created": 500 } }),
        );

        assert_eq!(latest_session_activity(&conn, "s1"), Some(500));

        _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn returns_part_time_updated_when_no_messages() {
        let base = temp("activity-part-only");
        let conn = conn_for(&base);
        insert_tool_part(&conn, "s1", 777, "completed", 700);

        assert_eq!(latest_session_activity(&conn, "s1"), Some(777));

        _ = fs::remove_dir_all(&base);
    }

    /// Freshest row wins regardless of which table it lives in.
    #[test]
    fn returns_max_across_messages_and_parts() {
        let base = temp("activity-mixed");
        let conn = conn_for(&base);
        insert_message(
            &conn,
            "s1",
            500,
            serde_json::json!({ "role": "user", "time": { "created": 500 } }),
        );
        insert_tool_part(&conn, "s1", 900, "running", 800);

        assert_eq!(latest_session_activity(&conn, "s1"), Some(900));

        // Swap: now the message is freshest.
        insert_message(
            &conn,
            "s1",
            1_500,
            serde_json::json!({ "role": "assistant", "time": { "created": 1_500 } }),
        );
        assert_eq!(latest_session_activity(&conn, "s1"), Some(1_500));

        _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn scopes_by_session_id() {
        let base = temp("activity-scope");
        let conn = conn_for(&base);
        insert_message(
            &conn,
            "s1",
            100,
            serde_json::json!({ "role": "user", "time": { "created": 100 } }),
        );
        insert_message(
            &conn,
            "s2",
            999,
            serde_json::json!({ "role": "user", "time": { "created": 999 } }),
        );

        assert_eq!(latest_session_activity(&conn, "s1"), Some(100));
        assert_eq!(latest_session_activity(&conn, "s2"), Some(999));

        _ = fs::remove_dir_all(&base);
    }

    /// A session whose messages/parts belong to archived cohorts
    /// still reports activity from those rows. The activity lookup
    /// deliberately ignores `session.time_archived` here: callers
    /// that need archive-awareness filter at the session level.
    #[test]
    fn ignores_session_archive_state() {
        let base = temp("activity-archived");
        let conn = conn_for(&base);
        insert_session_with_parent(&conn, "s1", None, "/wt", 100, Some(200));
        insert_message(
            &conn,
            "s1",
            900,
            serde_json::json!({ "role": "assistant", "time": { "created": 900 } }),
        );

        assert_eq!(latest_session_activity(&conn, "s1"), Some(900));

        _ = fs::remove_dir_all(&base);
    }
}
