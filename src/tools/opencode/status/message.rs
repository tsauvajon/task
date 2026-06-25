//! Latest-message summary used by the classifier's fallback branches.
//!
//! The shape captured here is intentionally tiny: only the fields
//! [`classify_with_conn`](super::classify::classify_with_conn) reads
//! — role, timestamps, and the error classification needed to
//! distinguish a real failure from a user-initiated abort.

use rusqlite::Connection;

/// Classifier-visible slice of the most recent message in a session.
#[derive(Debug, Clone)]
pub(super) struct MessageSummary {
    pub(super) role: String,
    pub(super) time_created: i64,
    pub(super) time_completed: Option<i64>,
    pub(super) has_error: bool,
    /// True only when `error.name == "MessageAbortedError"` — i.e. the
    /// user pressed ESC. Lets the classifier treat that one case as a
    /// clean stop instead of lighting up the row as `Hung`.
    pub(super) is_aborted: bool,
}

pub(super) fn latest_message(conn: &Connection, session_id: &str) -> Option<MessageSummary> {
    conn.query_row(
        "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created DESC LIMIT 1",
        rusqlite::params![session_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|data| parse_message_summary(&data))
}

fn parse_message_summary(data: &str) -> Option<MessageSummary> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    let role = value.get("role")?.as_str()?.to_owned();
    let time = value.get("time")?;
    let time_created = time.get("created")?.as_i64()?;
    let time_completed = time.get("completed").and_then(serde_json::Value::as_i64);
    let error = value.get("error");
    let has_error = error.is_some_and(|v| !v.is_null());
    let is_aborted = error
        .and_then(|e| e.get("name"))
        .and_then(serde_json::Value::as_str)
        == Some("MessageAbortedError");
    Some(MessageSummary {
        role,
        time_created,
        time_completed,
        has_error,
        is_aborted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_helpers {
        use super::*;

        #[test]
        fn parse_message_extracts_role_and_timings() {
            let data = serde_json::json!({
                "role": "user",
                "time": { "created": 1, "completed": 2 }
            })
            .to_string();
            let msg = parse_message_summary(&data).unwrap();
            assert_eq!(msg.role, "user");
            assert_eq!(msg.time_created, 1);
            assert_eq!(msg.time_completed, Some(2));
            assert!(!msg.has_error);
        }

        #[test]
        fn parse_message_flags_error_present() {
            let data = serde_json::json!({
                "role": "assistant",
                "time": { "created": 1 },
                "error": { "name": "APIError", "message": "overloaded" }
            })
            .to_string();
            let msg = parse_message_summary(&data).unwrap();
            assert!(msg.has_error);
            assert!(
                !msg.is_aborted,
                "APIError is not an abort — `is_aborted` must be false"
            );
        }

        #[test]
        fn parse_message_flags_abort_specifically() {
            let data = serde_json::json!({
                "role": "assistant",
                "time": { "created": 1, "completed": 2 },
                "error": { "name": "MessageAbortedError", "message": "aborted" }
            })
            .to_string();
            let msg = parse_message_summary(&data).unwrap();
            assert!(msg.has_error);
            assert!(msg.is_aborted, "MessageAbortedError must flag is_aborted");
        }

        #[test]
        fn parse_message_is_aborted_false_without_error() {
            let data = serde_json::json!({
                "role": "assistant",
                "time": { "created": 1, "completed": 2 }
            })
            .to_string();
            let msg = parse_message_summary(&data).unwrap();
            assert!(!msg.has_error);
            assert!(!msg.is_aborted);
        }

        #[test]
        fn parse_message_is_aborted_false_when_error_has_no_name() {
            let data = serde_json::json!({
                "role": "assistant",
                "time": { "created": 1 },
                "error": { "message": "something" }
            })
            .to_string();
            let msg = parse_message_summary(&data).unwrap();
            assert!(msg.has_error);
            assert!(!msg.is_aborted);
        }

        #[test]
        fn parse_message_without_time_returns_none() {
            let data = serde_json::json!({ "role": "assistant" }).to_string();
            assert!(parse_message_summary(&data).is_none());
        }
    }
}
