//! Classification of a single `OpenCode` session into an [`OpenCodeState`].
//!
//! The entry point is [`classify_with_conn`]. It inspects the running
//! tool parts first, then falls back to the latest message's role and
//! timing fields. The helpers in this file (running-parts rollup,
//! subagent-aware elapsed-time rule, session-activity liveness) are all
//! private details of that decision.

use rusqlite::Connection;
use strum::Display;

use super::{activity::latest_session_activity, message::latest_message};

/// Rolled-up state of every `OpenCode` session associated with a worktree
/// directory. Variant names match the labels rendered in the TUI so
/// that code, tests, and UI all read the same vocabulary.
///
/// Variant declaration order **is** the priority, from least to most
/// attention-worthy (`None` < `Gone` < `Idle` < `Busy` < `Hung`). The
/// derived `Ord` impl compares enum discriminants in source order, so
/// rolling up multiple sessions is just `iter.max()` — no custom
/// comparator needed. Adding, reordering, or inserting variants
/// changes the priority, so do so deliberately.
///
/// Why `Busy > Idle`: if a directory has one waiting-for-user session
/// and one actively running session, the running one is the signal
/// the user most likely cares about — the waiting one will still be
/// there after the running one completes, but the running one is
/// what the agent is actually doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Display)]
#[strum(serialize_all = "lowercase")]
pub enum OpenCodeState {
    /// No session row exists for the directory. Rendered as `·`.
    #[default]
    #[strum(serialize = "·")]
    None,
    /// A session exists but no live `opencode` process owns the cwd.
    /// The session history is still on disk and can be reopened, but
    /// no agent is attached right now — the process may have exited
    /// cleanly, been killed, crashed, or simply never been started
    /// in the current shell. Rendered as `gone`.
    Gone,
    /// Nothing autonomous is progressing and the next move is the
    /// user's. Covers two superficially different but UX-identical
    /// situations:
    ///
    /// 1. The agent just finished a turn cleanly; the ball is in the
    ///    user's court.
    /// 2. The user typed a message and walked away; no reply is in
    ///    flight and nothing is broken, so the session is just
    ///    sitting there waiting for the user's next action.
    ///
    /// Both read as "your move" to someone scanning the dashboard, so
    /// they collapse into a single state. Rendered as `idle`.
    Idle,
    /// A live session with a model response currently in flight (the
    /// agent is working). Rendered as `busy`.
    Busy,
    /// Something is wrong: stalled tool call, stalled assistant
    /// response, or a model error stored on the last assistant
    /// message. Rendered as `hung`.
    Hung,
}

/// Classification threshold in milliseconds. A session that has been
/// mid-turn for longer than this is reported as `Hung`.
///
/// Set at 3 minutes to cover the realistic upper bound of legitimate
/// work: long `bash`/`cargo` tool runs, slow provider streams, and
/// reasoning-heavy turns all routinely exceed a minute but rarely a
/// three-minute ceiling. Subagent (`task` tool) calls can and do run
/// for much longer; they are handled separately rather than by
/// loosening this threshold, so "normal" tools keep a tight bound.
const STUCK_THRESHOLD_MS: i64 = 180_000;

/// Classify a single session using an already-open DB connection.
/// Result is always one of `Busy`, `Idle`, or `Hung`.
pub(super) fn classify_with_conn(
    conn: &Connection,
    session_id: &str,
    now_ms: i64,
) -> OpenCodeState {
    // 1. A tool in `running` state implies the model/agent is waiting
    //    on an external call; treat it as busy unless it's been running
    //    for more than the threshold. Sessions can run tools in
    //    parallel, so every running part contributes a vote and the
    //    worst severity wins (`Hung > Busy`). Subagent (`task`) parts
    //    delegate their vote to the child session's activity — see
    //    `classify_running_part`.
    if let Some(state) = classify_running_parts(conn, &running_tool_parts(conn, session_id), now_ms)
    {
        return state;
    }

    // 2. Otherwise fall back to the latest message.
    let Some(last) = latest_message(conn, session_id) else {
        // No messages stored yet — brand-new session or a corrupted row
        // we can't reason about. `Idle` keeps the row visible without
        // crying wolf.
        return OpenCodeState::Idle;
    };

    if last.role == "assistant" {
        // Any error on the last assistant message (rate limit, auth
        // expired, API failure, context overflow, …) means the agent
        // stopped and the user needs to take action to restart the
        // work. `MessageAbortedError` is the one exception: it is the
        // upstream error class set when the user themselves cancelled
        // the turn with ESC. In that case the ball is already in the
        // user's court — treat it exactly like a clean completion
        // (falls through to the `Idle` return below). See
        // `packages/opencode/src/session/message-v2.ts` in sst/opencode.
        if last.has_error && !last.is_aborted {
            return OpenCodeState::Hung;
        }
        if last.time_completed.is_none() {
            return if now_ms.saturating_sub(last.time_created) > STUCK_THRESHOLD_MS {
                OpenCodeState::Hung
            } else {
                OpenCodeState::Busy
            };
        }
        return OpenCodeState::Idle;
    }

    if last.role == "user" {
        // The OpenCode schema only persists a user message once the
        // user has sent it (`User.time` has `created` but no completed
        // or draft field — see sst/opencode `packages/opencode/src/
        // session/message-v2.ts`), so "user last" can only mean the
        // model owes a reply. Anything older than the stuck threshold
        // is a red flag.
        return if now_ms.saturating_sub(last.time_created) > STUCK_THRESHOLD_MS {
            OpenCodeState::Hung
        } else {
            OpenCodeState::Busy
        };
    }

    // Unknown role — stay visible without classifying as a problem.
    OpenCodeState::Idle
}

/// A single currently-running tool part, enriched with the fields the
/// classifier needs to decide whether the part is still alive.
///
/// For ordinary tools (`bash`, `read`, `edit`, …) only `start_ms` is
/// consulted. The remaining fields exist to support the subagent
/// (`task` tool) branch, which measures liveness via the child
/// session's activity rather than the parent part's elapsed time.
#[derive(Debug, Clone)]
struct RunningToolPart {
    /// `state.time.start` in ms. Drives the elapsed-time rule for
    /// non-subagent tools and acts as the fallback for subagents
    /// whose child session is not (yet) resolvable.
    start_ms: i64,
    /// `data.tool` — e.g. `"bash"`, `"task"`, `"read"`. Only the
    /// literal `"task"` triggers the subagent-aware branch; any other
    /// value (including future unknown tools) classifies via the
    /// conservative elapsed-time rule.
    tool: String,
    /// `state.metadata.sessionId`. Present for `task` parts once the
    /// subagent has been spawned and `OpenCode` has recorded the child
    /// session id on the part. Absent for non-subagent tools and for
    /// the narrow window before the child id is written.
    child_session_id: Option<String>,
}

/// List every currently-running tool part for `session_id`, ordered
/// oldest-first by `state.time.start`.
///
/// Filters and orders entirely in SQL via `json_extract` so a session
/// with many recently-completed tools cannot hide an older still-running
/// tool. `OpenCode` runs tool calls in parallel, so a long-stuck `bash`
/// invocation can easily coexist with many freshly-completed tools in
/// the same session — a "top N by `time_updated`" shortcut would let the
/// stuck tool fall out of the window entirely and mis-classify the
/// session as `Idle`.
fn running_tool_parts(conn: &Connection, session_id: &str) -> Vec<RunningToolPart> {
    let sql = "SELECT \
                 json_extract(data, '$.state.time.start')       AS start, \
                 json_extract(data, '$.tool')                   AS tool, \
                 json_extract(data, '$.state.metadata.sessionId') AS child \
               FROM part \
               WHERE session_id = ?1 \
                 AND json_extract(data, '$.type') = 'tool' \
                 AND json_extract(data, '$.state.status') = 'running' \
                 AND json_extract(data, '$.state.time.start') IS NOT NULL \
               ORDER BY start ASC";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let rows = stmt.query_map(rusqlite::params![session_id], |row| {
        Ok(RunningToolPart {
            start_ms: row.get::<_, i64>(0)?,
            tool: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            child_session_id: row.get::<_, Option<String>>(2)?,
        })
    });
    match rows {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

/// Roll up every running tool part in a session into a single state,
/// returning `None` when there are no running parts at all (so the
/// caller can fall through to the message-based branches).
///
/// Each part votes `Busy` or `Hung` independently; the max severity
/// wins. That matches the "any stuck tool poisons the session" policy
/// documented on `OpenCodeState`.
fn classify_running_parts(
    conn: &Connection,
    parts: &[RunningToolPart],
    now_ms: i64,
) -> Option<OpenCodeState> {
    parts
        .iter()
        .map(|part| classify_running_part(conn, part, now_ms))
        .max()
}

/// Decide whether a single running tool part is `Busy` or `Hung`.
///
/// Subagent (`task`) parts are measured against the child session's
/// most recent activity rather than the parent part's elapsed time:
/// a long-running subagent that is still streaming messages or tool
/// results into its child session is legitimately `Busy`, while a
/// subagent whose child has gone silent (including the "child
/// finished but parent never reacted" failure mode) is `Hung`.
///
/// The subagent branch falls back to the elapsed-time rule whenever
/// child-session liveness cannot be measured: the `task` part has
/// no `metadata.sessionId` yet, or the referenced child session has
/// no messages or parts at all. Both cases are transient windows
/// where the parent's `state.time.start` is the best signal we have.
fn classify_running_part(conn: &Connection, part: &RunningToolPart, now_ms: i64) -> OpenCodeState {
    if let Some(last_activity) = latest_subagent_activity(conn, part) {
        return state_from_last_activity(now_ms, last_activity);
    }

    state_from_last_activity(now_ms, part.start_ms)
}

fn latest_subagent_activity(conn: &Connection, part: &RunningToolPart) -> Option<i64> {
    if part.tool != "task" {
        return None;
    }

    let child = part.child_session_id.as_deref()?;
    latest_session_activity(conn, child)
}

const fn state_from_last_activity(now_ms: i64, last_activity_ms: i64) -> OpenCodeState {
    if now_ms.saturating_sub(last_activity_ms) > STUCK_THRESHOLD_MS {
        OpenCodeState::Hung
    } else {
        OpenCodeState::Busy
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use rusqlite::Connection;

    use super::{super::test_support::*, *};

    mod opencode_state_ordering {
        use super::*;

        #[test]
        fn priority_goes_none_gone_idle_busy_hung() {
            assert!(OpenCodeState::None < OpenCodeState::Gone);
            assert!(OpenCodeState::Gone < OpenCodeState::Idle);
            assert!(OpenCodeState::Idle < OpenCodeState::Busy);
            assert!(OpenCodeState::Busy < OpenCodeState::Hung);
        }

        #[test]
        fn max_picks_highest_priority() {
            assert_eq!(
                OpenCodeState::Busy.max(OpenCodeState::Hung),
                OpenCodeState::Hung
            );
            // Busy outranks Idle: a session actively running is the
            // signal the user most likely cares about right now.
            assert_eq!(
                OpenCodeState::Idle.max(OpenCodeState::Busy),
                OpenCodeState::Busy
            );
            assert_eq!(
                OpenCodeState::Gone.max(OpenCodeState::None),
                OpenCodeState::Gone
            );
        }

        #[test]
        fn display_labels() {
            assert_eq!(OpenCodeState::None.to_string(), "·");
            assert_eq!(OpenCodeState::Gone.to_string(), "gone");
            assert_eq!(OpenCodeState::Busy.to_string(), "busy");
            assert_eq!(OpenCodeState::Idle.to_string(), "idle");
            assert_eq!(OpenCodeState::Hung.to_string(), "hung");
        }
    }
    mod running_tool_parts {
        use super::*;

        fn conn_for(base: &Path) -> Connection {
            let path = base.join("run.db");
            let conn = Connection::open(&path).unwrap();
            create_schema(&conn);
            conn
        }

        /// Describes one tool-part row to insert into the test DB.
        /// The generic `insert_tool_part` helper hardcodes
        /// `tool: "bash"` which is fine for elapsed-time tests but
        /// not for subagent metadata tests.
        #[derive(Clone, Copy)]
        struct CustomToolPart<'a> {
            id: &'a str,
            session_id: &'a str,
            updated: i64,
            tool: &'a str,
            status: &'a str,
            start: Option<i64>,
            child_session_id: Option<&'a str>,
        }

        fn insert_custom_tool_part(conn: &Connection, spec: CustomToolPart<'_>) {
            let mut data = serde_json::json!({
                "type": "tool",
                "tool": spec.tool,
                "state": {
                    "status": spec.status,
                }
            });
            if let Some(start_ms) = spec.start {
                data["state"]["time"] = serde_json::json!({ "start": start_ms });
            }
            if let Some(child) = spec.child_session_id {
                data["state"]["metadata"] = serde_json::json!({ "sessionId": child });
            }
            conn.execute(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
                rusqlite::params![
                    spec.id,
                    format!("msg_any_{}", spec.session_id),
                    spec.session_id,
                    spec.updated,
                    data.to_string(),
                ],
            )
            .unwrap();
        }

        #[test]
        fn returns_empty_when_no_running_tools() {
            let base = temp("parts-none");
            let conn = conn_for(&base);
            insert_tool_part(&conn, "s1", 10, "completed", 5);
            insert_tool_part(&conn, "s1", 20, "completed", 15);

            assert!(super::super::running_tool_parts(&conn, "s1").is_empty());

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn returns_single_running_tool_with_defaults() {
            let base = temp("parts-one");
            let conn = conn_for(&base);
            insert_tool_part(&conn, "s1", 100, "running", 42);

            let parts = super::super::running_tool_parts(&conn, "s1");
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].start_ms, 42);
            assert_eq!(parts[0].tool, "bash");
            assert!(parts[0].child_session_id.is_none());

            _ = fs::remove_dir_all(&base);
        }

        /// A `task` tool part carries `state.metadata.sessionId`
        /// pointing at the subagent's child session. Classifier uses
        /// this to delegate liveness measurement to the child; commit
        /// 1 just pins that the field is plumbed through.
        #[test]
        fn extracts_child_session_id_for_task_tool() {
            let base = temp("parts-task-meta");
            let conn = conn_for(&base);
            insert_custom_tool_part(
                &conn,
                CustomToolPart {
                    id: "p1",
                    session_id: "s1",
                    updated: 100,
                    tool: "task",
                    status: "running",
                    start: Some(90),
                    child_session_id: Some("ses_child_01"),
                },
            );

            let parts = super::super::running_tool_parts(&conn, "s1");
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].tool, "task");
            assert_eq!(parts[0].child_session_id.as_deref(), Some("ses_child_01"));

            _ = fs::remove_dir_all(&base);
        }

        /// `metadata.sessionId` can be missing briefly before the
        /// subagent is fully spawned (observed 2 out of 366 historical
        /// task parts). The struct must cope with `None` there.
        #[test]
        fn child_session_id_none_when_task_metadata_missing() {
            let base = temp("parts-task-no-meta");
            let conn = conn_for(&base);
            insert_custom_tool_part(
                &conn,
                CustomToolPart {
                    id: "p1",
                    session_id: "s1",
                    updated: 100,
                    tool: "task",
                    status: "running",
                    start: Some(90),
                    child_session_id: None,
                },
            );

            let parts = super::super::running_tool_parts(&conn, "s1");
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].tool, "task");
            assert!(parts[0].child_session_id.is_none());

            _ = fs::remove_dir_all(&base);
        }

        /// Regression for the "many recent completed tools hide an
        /// older stuck running tool" bug: the prior implementation only
        /// looked at the 10 most-recently-updated parts, so a stuck tool
        /// behind 10+ newer completions would be missed and the session
        /// would be mis-classified as `Idle`.
        #[test]
        fn oldest_running_wins_despite_many_newer_completed_tools() {
            let base = temp("parts-many-completed");
            let conn = conn_for(&base);

            // One running tool started long ago, with a very low
            // time_updated so it ranks far down by time_updated.
            insert_tool_part(&conn, "s1", 10, "running", 5);

            // 15 completed tools with much newer time_updated.
            for i in 0..15 {
                insert_tool_part(&conn, "s1", 1_000 + i, "completed", 500 + i);
            }

            let parts = super::super::running_tool_parts(&conn, "s1");
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].start_ms, 5);

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn returns_running_parts_ordered_by_start_asc() {
            let base = temp("parts-order");
            let conn = conn_for(&base);
            // Insert in scrambled order; query must return ascending.
            insert_tool_part(&conn, "s1", 100, "running", 300);
            insert_tool_part(&conn, "s1", 110, "running", 100);
            insert_tool_part(&conn, "s1", 120, "running", 200);

            let parts = super::super::running_tool_parts(&conn, "s1");
            let starts: Vec<i64> = parts.iter().map(|p| p.start_ms).collect();
            assert_eq!(starts, vec![100, 200, 300]);

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn ignores_non_tool_parts() {
            // A reasoning part with a `state.status = running`
            // shouldn't count — only `type = tool` parts do.
            let base = temp("parts-non-tool");
            let conn = conn_for(&base);
            let reasoning = serde_json::json!({
                "type": "reasoning",
                "text": "",
                "state": {
                    "status": "running",
                    "time": { "start": 1 }
                }
            });
            conn.execute(
                "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
                rusqlite::params![
                    "p1",
                    "m1",
                    "s1",
                    100,
                    reasoning.to_string(),
                ],
            )
            .unwrap();

            assert!(super::super::running_tool_parts(&conn, "s1").is_empty());

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn scopes_by_session_id() {
            let base = temp("parts-scope");
            let conn = conn_for(&base);
            insert_tool_part(&conn, "s1", 100, "running", 42);
            insert_tool_part(&conn, "s2", 200, "running", 7);

            let s1 = super::super::running_tool_parts(&conn, "s1");
            let s2 = super::super::running_tool_parts(&conn, "s2");
            assert_eq!(s1.len(), 1);
            assert_eq!(s1[0].start_ms, 42);
            assert_eq!(s2.len(), 1);
            assert_eq!(s2[0].start_ms, 7);

            _ = fs::remove_dir_all(&base);
        }
    }
    mod classify_running_parts {
        use super::*;

        fn part(tool: &str, start_ms: i64, child: Option<&str>) -> RunningToolPart {
            RunningToolPart {
                start_ms,
                tool: tool.to_owned(),
                child_session_id: child.map(str::to_owned),
            }
        }

        /// These tests only exercise non-subagent parts, so the
        /// connection is never queried. A schema-less in-memory DB is
        /// enough to satisfy the signature.
        fn unused_conn() -> Connection {
            Connection::open_in_memory().unwrap()
        }

        #[test]
        fn empty_slice_returns_none() {
            assert_eq!(classify_running_parts(&unused_conn(), &[], 10_000), None);
        }

        #[test]
        fn single_fresh_tool_is_busy() {
            let parts = [part("bash", 9_500, None)];
            assert_eq!(
                classify_running_parts(&unused_conn(), &parts, 10_000),
                Some(OpenCodeState::Busy)
            );
        }

        #[test]
        fn single_stale_tool_is_hung() {
            let parts = [part("bash", 100, None)];
            // Elapsed = 999_900ms, well past STUCK_THRESHOLD_MS.
            assert_eq!(
                classify_running_parts(&unused_conn(), &parts, 1_000_000),
                Some(OpenCodeState::Hung)
            );
        }

        /// Any stuck tool poisons the rollup regardless of how many
        /// healthy siblings are running in parallel.
        #[test]
        fn stuck_tool_poisons_rollup_across_healthy_siblings() {
            let parts = [
                part("bash", 999_000, None), // fresh
                part("read", 998_000, None), // fresh
                part("bash", 100, None),     // stuck
            ];
            assert_eq!(
                classify_running_parts(&unused_conn(), &parts, 1_000_000),
                Some(OpenCodeState::Hung)
            );
        }

        #[test]
        fn all_healthy_siblings_roll_up_to_busy() {
            let parts = [
                part("bash", 999_000, None),
                part("read", 998_000, None),
                part("grep", 997_000, None),
            ];
            assert_eq!(
                classify_running_parts(&unused_conn(), &parts, 1_000_000),
                Some(OpenCodeState::Busy)
            );
        }
    }
    mod classify_running_part_subagent {
        use super::*;

        fn conn_for(base: &Path) -> Connection {
            let path = base.join("subagent.db");
            let conn = Connection::open(&path).unwrap();
            create_schema(&conn);
            conn
        }

        fn task_part(start_ms: i64, child: Option<&str>) -> RunningToolPart {
            RunningToolPart {
                start_ms,
                tool: "task".to_owned(),
                child_session_id: child.map(str::to_owned),
            }
        }

        /// Child has fresh activity (within threshold) → parent task
        /// part votes `Busy` even though the parent itself started
        /// well beyond `STUCK_THRESHOLD_MS` ago.
        #[test]
        fn fresh_child_activity_makes_long_running_task_busy() {
            let base = temp("subagent-fresh");
            let conn = conn_for(&base);
            insert_session_with_parent(&conn, "ses_child", Some("ses_parent"), "/wt", 0, None);
            insert_message(
                &conn,
                "ses_child",
                999_000,
                serde_json::json!({ "role": "assistant", "time": { "created": 999_000 } }),
            );

            let part = task_part(0, Some("ses_child"));
            // Parent started at 0, now = 1_000_000 → elapsed 1M ms
            // (far past threshold). Child activity at 999_000 → fresh.
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Busy,
            );

            _ = fs::remove_dir_all(&base);
        }

        /// Child has been silent longer than the threshold → `Hung`,
        /// even though parent elapsed time alone might look identical.
        #[test]
        fn stale_child_activity_makes_task_hung() {
            let base = temp("subagent-stale");
            let conn = conn_for(&base);
            insert_session_with_parent(&conn, "ses_child", Some("ses_parent"), "/wt", 0, None);
            insert_message(
                &conn,
                "ses_child",
                100_000,
                serde_json::json!({ "role": "assistant", "time": { "created": 100_000 } }),
            );

            let part = task_part(0, Some("ses_child"));
            // Child last touched at 100_000, now = 1_000_000 → silent
            // for 900_000 ms, well past threshold.
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Hung,
            );

            _ = fs::remove_dir_all(&base);
        }

        /// Task part without `metadata.sessionId` falls back to the
        /// elapsed-since-start rule. Observed 2/366 historical task
        /// parts lacked this field.
        #[test]
        fn missing_child_session_id_falls_back_to_elapsed_rule_busy() {
            let base = temp("subagent-no-child");
            let conn = conn_for(&base);

            let part = task_part(999_000, None);
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Busy,
            );

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn missing_child_session_id_falls_back_to_elapsed_rule_hung() {
            let base = temp("subagent-no-child-stuck");
            let conn = conn_for(&base);

            let part = task_part(0, None);
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Hung,
            );

            _ = fs::remove_dir_all(&base);
        }

        /// Child session id is present but no such session / rows
        /// exist yet (brand-new subagent, first few ms). Fall back to
        /// elapsed rule.
        #[test]
        fn unknown_child_session_falls_back_to_elapsed_rule() {
            let base = temp("subagent-unknown-child");
            let conn = conn_for(&base);

            // No session rows inserted; child id is a dangling pointer.
            let part = task_part(999_000, Some("ses_nonexistent"));
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Busy,
            );

            let old_part = task_part(0, Some("ses_nonexistent"));
            assert_eq!(
                classify_running_part(&conn, &old_part, 1_000_000),
                OpenCodeState::Hung,
            );

            _ = fs::remove_dir_all(&base);
        }

        /// Covers the "child is idle but parent never reacted" failure
        /// mode: child's last message is a completed assistant turn,
        /// but the parent task part is still `running` and nothing has
        /// moved in the child since. Activity check catches it.
        #[test]
        fn child_done_but_parent_not_reacting_is_hung() {
            let base = temp("subagent-done-not-reacting");
            let conn = conn_for(&base);
            insert_session_with_parent(&conn, "ses_child", Some("ses_parent"), "/wt", 0, None);
            insert_message(
                &conn,
                "ses_child",
                100_000,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 90_000, "completed": 100_000 }
                }),
            );

            let part = task_part(0, Some("ses_child"));
            // Child wrapped up at 100_000, now = 1_000_000 → 900_000ms
            // silent. Past threshold.
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Hung,
            );

            _ = fs::remove_dir_all(&base);
        }

        /// Symmetric case: child just wrapped up; parent is in its
        /// legitimate wrap-up window. Still `Busy`.
        #[test]
        fn child_done_recently_keeps_parent_busy() {
            let base = temp("subagent-done-wrapping-up");
            let conn = conn_for(&base);
            insert_session_with_parent(&conn, "ses_child", Some("ses_parent"), "/wt", 0, None);
            insert_message(
                &conn,
                "ses_child",
                999_000,
                serde_json::json!({
                    "role": "assistant",
                    "time": { "created": 990_000, "completed": 999_000 }
                }),
            );

            let part = task_part(0, Some("ses_child"));
            // Child last touched 1_000ms ago → still within threshold.
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Busy,
            );

            _ = fs::remove_dir_all(&base);
        }

        /// Non-task tool never consults the child session id even if
        /// one happens to be present. Defensive: extraction is
        /// oblivious to tool type.
        #[test]
        fn non_task_tool_ignores_child_session_id() {
            let base = temp("subagent-non-task");
            let conn = conn_for(&base);
            // Seed a stale child to prove it's not consulted.
            insert_session_with_parent(&conn, "ses_child", Some("ses_parent"), "/wt", 0, None);
            insert_message(
                &conn,
                "ses_child",
                0,
                serde_json::json!({ "role": "user", "time": { "created": 0 } }),
            );

            let part = RunningToolPart {
                start_ms: 999_000,
                tool: "bash".to_owned(),
                child_session_id: Some("ses_child".to_owned()),
            };
            // Would classify Hung if the child were consulted; Busy
            // is correct because we must not consult it.
            assert_eq!(
                classify_running_part(&conn, &part, 1_000_000),
                OpenCodeState::Busy,
            );

            _ = fs::remove_dir_all(&base);
        }
    }
    mod classify_with_conn_branches {
        use super::*;

        fn conn_with_session(base: &Path, session_id: &str) -> Connection {
            let path = base.join("classify.db");
            let conn = Connection::open(&path).unwrap();
            create_schema(&conn);
            insert_session(&conn, session_id, "/worktree", 100);
            conn
        }

        #[test]
        fn no_messages_returns_done() {
            let base = temp("classify-empty");
            let conn = conn_with_session(&base, "s1");

            // No messages, no parts → the model has nothing in flight
            // and there's no visible error, so the caller sees `Idle`.
            assert_eq!(classify_with_conn(&conn, "s1", 10_000), OpenCodeState::Idle);

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn last_message_from_user_within_threshold_is_busy() {
            let base = temp("classify-user-run");
            let conn = conn_with_session(&base, "s1");
            insert_message(
                &conn,
                "s1",
                9_500,
                serde_json::json!({
                    "role": "user",
                    "time": { "created": 9_500 }
                }),
            );

            assert_eq!(classify_with_conn(&conn, "s1", 10_000), OpenCodeState::Busy);

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn last_message_from_user_beyond_threshold_is_hung() {
            let base = temp("classify-user-stuck");
            let conn = conn_with_session(&base, "s1");
            insert_message(
                &conn,
                "s1",
                1_000,
                serde_json::json!({
                    "role": "user",
                    "time": { "created": 1_000 }
                }),
            );

            assert_eq!(
                classify_with_conn(&conn, "s1", 200_000),
                OpenCodeState::Hung
            );

            _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn unknown_role_falls_through_to_done() {
            // Defensive: an unknown role (OpenCode schema growth) must
            // not explode and must not falsely flag the row as busy
            // or hung. `Idle` keeps the session visible without
            // claiming anything is wrong or pending.
            let base = temp("classify-unknown-role");
            let conn = conn_with_session(&base, "s1");
            insert_message(
                &conn,
                "s1",
                9_500,
                serde_json::json!({
                    "role": "system",
                    "time": { "created": 9_500 }
                }),
            );

            assert_eq!(classify_with_conn(&conn, "s1", 10_000), OpenCodeState::Idle);

            _ = fs::remove_dir_all(&base);
        }
    }
}
