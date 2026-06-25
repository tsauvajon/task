//! Table-driven integration suite hitting real `OpenCode` session shapes.
//!
//! Pulled into the module tree via `#[path]` from `mod.rs` under the
//! module name `real_cases`, so tests here run as
//! `tools::opencode::status::real_cases::<case>`.

#![cfg(test)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;

use super::{OpenCodeState, test_support::*};

/// Minimal session fixture. Directory is a single path segment
/// so no personal directory tree leaks into the suite.
struct SessionSeed {
    id: &'static str,
    directory: &'static str,
    time_updated: i64,
    /// DB file name this session lives in. Real machines host
    /// both `opencode-stable.db` and the legacy `opencode.db`
    /// side by side — defaulting to the stable DB matches the
    /// primary case; overriding lets a case span both files.
    db_name: &'static str,
    /// Non-NULL for subagent child sessions. Mirrors the real
    /// schema's `session.parent_id` column.
    parent_id: Option<&'static str>,
    /// Non-NULL when the session is archived. Archived child
    /// sessions still count toward activity measurement, so
    /// seeding this explicitly is useful for pinning that
    /// invariant.
    time_archived: Option<i64>,
}

/// Minimal message fixture. Only the fields
/// `classify_with_conn` inspects are expressed here.
struct MessageSeed {
    id: &'static str,
    session_id: &'static str,
    created_ms: i64,
    role: &'static str,
    completed_ms: Option<i64>,
    error_name: Option<&'static str>,
}

/// Minimal tool-part fixture. The classifier only reacts to
/// `running` parts (with a `state.time.start`), but `pending`,
/// `completed`, and `error` statuses all exist in the wild —
/// seeding them lets us pin that they are correctly ignored.
struct ToolPartSeed {
    id: &'static str,
    session_id: &'static str,
    message_id: &'static str,
    status: &'static str,
    /// Present for `running` parts (drives the stuck check),
    /// absent for `pending` parts (not yet started).
    start_ms: Option<i64>,
    /// Tool name (`"bash"`, `"task"`, `"read"`, …). `None`
    /// defaults to `"bash"` so existing cases keep their
    /// behaviour.
    tool: Option<&'static str>,
    /// `state.metadata.sessionId` for `task` parts — the child
    /// subagent session id. `None` for every non-subagent tool.
    child_session_id: Option<&'static str>,
}

struct Case {
    name: &'static str,
    now_ms: i64,
    /// Directories that an `opencode` process is currently
    /// cwd'd into. Entries not listed here classify as `Gone`
    /// (when a session exists) or `None` (when no session
    /// exists).
    live_cwds: &'static [&'static str],
    /// Start time (ms since epoch) applied to every live
    /// `opencode` process seeded in this case. `0` keeps every
    /// session live-owned — matches the pre-filter default and
    /// lets existing cases opt out of the zombie check.
    /// Cases that exercise the live-ownership boundary set a
    /// value after which only live-owned sessions survive.
    proc_start_ms: i64,
    sessions: &'static [SessionSeed],
    messages: &'static [MessageSeed],
    tool_parts: &'static [ToolPartSeed],
    expected: &'static [(&'static str, OpenCodeState)],
}

// Base epoch for all relative timings in the fixtures. Picking
// a concrete millisecond keeps each case readable without
// leaking real observation times.
const T0: i64 = 10_000_000;

const STABLE_DB: &str = "opencode-stable.db";
const LEGACY_DB: &str = "opencode.db";

const CASES: &[Case] = &[
    // Clean idle: assistant finished, no error, live process
    // owns the cwd.
    Case {
        name: "clean_idle_real",
        now_ms: T0 + 30_000,
        live_cwds: &["see-current-opencode-status"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R01",
            directory: "see-current-opencode-status",
            time_updated: T0 + 10_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R01",
            session_id: "ses_R01",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 9_000),
            error_name: None,
        }],
        tool_parts: &[],
        expected: &[("see-current-opencode-status", OpenCodeState::Idle)],
    },
    // User pressed ESC mid-turn: assistant message carries
    // `MessageAbortedError`. That is not an unresolved error —
    // the user is in control, so it rolls up to `Idle`.
    Case {
        name: "aborted_turn_is_done_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R02",
            directory: "task",
            time_updated: T0 + 11_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R02",
            session_id: "ses_R02",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 10_000),
            error_name: Some("MessageAbortedError"),
        }],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Idle)],
    },
    // Assistant message is mid-flight (`completed` is null) and
    // was created less than the stuck threshold ago → `Busy`.
    Case {
        name: "assistant_incomplete_within_threshold_is_busy_real",
        now_ms: T0 + 30_000,
        live_cwds: &["install-custom-commands"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R03",
            directory: "install-custom-commands",
            time_updated: T0,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R03",
            session_id: "ses_R03",
            created_ms: T0,
            role: "assistant",
            completed_ms: None,
            error_name: None,
        }],
        tool_parts: &[],
        expected: &[("install-custom-commands", OpenCodeState::Busy)],
    },
    // Same shape but past the 180s stuck threshold → `Hung`.
    Case {
        name: "assistant_incomplete_beyond_threshold_is_hung_real",
        now_ms: T0 + 181_000,
        live_cwds: &["install-custom-commands"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R04",
            directory: "install-custom-commands",
            time_updated: T0,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R04",
            session_id: "ses_R04",
            created_ms: T0,
            role: "assistant",
            completed_ms: None,
            error_name: None,
        }],
        tool_parts: &[],
        expected: &[("install-custom-commands", OpenCodeState::Hung)],
    },
    // A running tool whose `state.time.start` is recent →
    // `Busy`, even if the latest message is already completed.
    Case {
        name: "running_tool_within_threshold_is_busy_real",
        now_ms: T0 + 30_000,
        live_cwds: &["install-custom-commands"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R05",
            directory: "install-custom-commands",
            time_updated: T0,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R05",
            session_id: "ses_R05",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 5_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R05",
            session_id: "ses_R05",
            message_id: "msg_R05",
            status: "running",
            start_ms: Some(T0 + 10_000),
            tool: None,
            child_session_id: None,
        }],
        expected: &[("install-custom-commands", OpenCodeState::Busy)],
    },
    // A tool stuck in `running` for long past the threshold →
    // `Hung`, regardless of the latest message being completed.
    // Mirrors a sticky non-subagent tool (e.g. `bash`) observed
    // in the source DB. Subagent (`task`) tool parts are handled
    // separately and will not classify as `Hung` purely on
    // elapsed time — see the dedicated subagent cases.
    Case {
        name: "running_tool_beyond_threshold_is_hung_real",
        now_ms: T0 + 240_000,
        live_cwds: &["see-current-opencode-status"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R06",
            directory: "see-current-opencode-status",
            time_updated: T0 + 40_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R06",
            session_id: "ses_R06",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 35_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R06",
            session_id: "ses_R06",
            message_id: "msg_R06",
            // Started well before `now_ms - STUCK_THRESHOLD_MS`.
            status: "running",
            start_ms: Some(T0),
            tool: None,
            child_session_id: None,
        }],
        expected: &[("see-current-opencode-status", OpenCodeState::Hung)],
    },
    // Assistant finished but with an `APIError` — the turn
    // stopped with an error the user likely needs to restart.
    Case {
        name: "api_error_is_hung_real",
        now_ms: T0 + 30_000,
        live_cwds: &["detach-commands"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R07",
            directory: "detach-commands",
            time_updated: T0 + 18_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R07",
            session_id: "ses_R07",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 17_000),
            error_name: Some("APIError"),
        }],
        tool_parts: &[],
        expected: &[("detach-commands", OpenCodeState::Hung)],
    },
    // Last message is from the user, within threshold →
    // `Busy`. Source case was in an unrelated repo; rewritten
    // with a `task` directory placeholder per sanitization
    // rules, the behavioural shape is what we care about here.
    Case {
        name: "user_last_within_threshold_is_busy_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R08",
            directory: "task",
            time_updated: T0,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R08",
            session_id: "ses_R08",
            created_ms: T0,
            role: "user",
            completed_ms: None,
            error_name: None,
        }],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Last message is from the user, past threshold → `Hung`.
    Case {
        name: "user_last_beyond_threshold_is_hung_real",
        now_ms: T0 + 181_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R09",
            directory: "task",
            time_updated: T0,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R09",
            session_id: "ses_R09",
            created_ms: T0,
            role: "user",
            completed_ms: None,
            error_name: None,
        }],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // A real session row exists for a directory but no live
    // `opencode` process claims that cwd → `Gone`.
    Case {
        name: "done_session_without_live_process_is_shut_real",
        now_ms: T0 + 30_000,
        live_cwds: &[],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R10",
            directory: "helix-instead-of-codium",
            time_updated: T0 + 10_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R10",
            session_id: "ses_R10",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 9_000),
            error_name: None,
        }],
        tool_parts: &[],
        expected: &[("helix-instead-of-codium", OpenCodeState::Gone)],
    },
    // Rollup: a single directory hosts multiple sessions with
    // mixed states (real machine: the same repo's worktree has
    // 3 busy + 9 idle + 1 aborted simultaneously). The rollup
    // picks the most attention-worthy state, and `Busy`
    // outranks `Idle`: the active session is the one the user
    // is most likely focused on right now; the waiting one
    // will still be there afterwards.
    Case {
        name: "rollup_busy_wins_over_idle_real",
        now_ms: T0 + 30_000,
        live_cwds: &["see-current-opencode-status"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R11a",
                directory: "see-current-opencode-status",
                time_updated: T0 + 10_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R11b",
                directory: "see-current-opencode-status",
                time_updated: T0 + 11_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R11c",
                directory: "see-current-opencode-status",
                time_updated: T0 + 12_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
        ],
        messages: &[
            // One session cleanly idle (assistant turn completed).
            MessageSeed {
                id: "msg_R11a",
                session_id: "ses_R11a",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 9_000),
                error_name: None,
            },
            // One session still streaming assistant text
            // (this drives the rollup under the new ordering).
            MessageSeed {
                id: "msg_R11b",
                session_id: "ses_R11b",
                created_ms: T0 + 11_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            // One session aborted (rolls up as `Idle`).
            MessageSeed {
                id: "msg_R11c",
                session_id: "ses_R11c",
                created_ms: T0 + 12_000,
                role: "assistant",
                completed_ms: Some(T0 + 12_500),
                error_name: Some("MessageAbortedError"),
            },
        ],
        tool_parts: &[],
        expected: &[("see-current-opencode-status", OpenCodeState::Busy)],
    },
    // Rollup: one session is `Hung` (APIError), others are
    // `Idle` — Hung is the highest priority, so the whole
    // directory rolls up to `Hung`.
    Case {
        name: "rollup_hung_wins_over_done_real",
        now_ms: T0 + 30_000,
        live_cwds: &["see-current-opencode-status"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R12a",
                directory: "see-current-opencode-status",
                time_updated: T0 + 10_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R12b",
                directory: "see-current-opencode-status",
                time_updated: T0 + 11_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R12a",
                session_id: "ses_R12a",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 9_000),
                error_name: None,
            },
            MessageSeed {
                id: "msg_R12b",
                session_id: "ses_R12b",
                created_ms: T0 + 10_000,
                role: "assistant",
                completed_ms: Some(T0 + 10_500),
                error_name: Some("APIError"),
            },
        ],
        tool_parts: &[],
        expected: &[("see-current-opencode-status", OpenCodeState::Hung)],
    },
    // `UnknownError` — observed 3 times in the real DB as a
    // 429 retry path through the provider wrapper. Same
    // treatment as APIError: `Hung`.
    Case {
        name: "unknown_error_is_hung_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R13",
            directory: "task",
            time_updated: T0 + 5_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R13",
            session_id: "ses_R13",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 4_000),
            error_name: Some("UnknownError"),
        }],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Parallel tool calls: two tools in `running` state at
    // the same time, one well past the stuck threshold. The
    // oldest running tool drives the classification (see the
    // `running_tool_parts` ordering contract), so the
    // newer tool's fresh start time must not hide the older
    // stuck one.
    Case {
        name: "parallel_running_tools_oldest_wins_real",
        now_ms: T0 + 240_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R14",
            directory: "task",
            time_updated: T0 + 40_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R14",
            session_id: "ses_R14",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 35_000),
            error_name: None,
        }],
        tool_parts: &[
            // Old tool — past the stuck threshold.
            ToolPartSeed {
                id: "prt_R14a",
                session_id: "ses_R14",
                message_id: "msg_R14",
                status: "running",
                start_ms: Some(T0),
                tool: None,
                child_session_id: None,
            },
            // Newer sibling tool — would look `Busy` alone.
            ToolPartSeed {
                id: "prt_R14b",
                session_id: "ses_R14",
                message_id: "msg_R14",
                status: "running",
                start_ms: Some(T0 + 235_000),
                tool: None,
                child_session_id: None,
            },
        ],
        // Oldest wins, so the session is `Hung`.
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Brand-new session: the row exists, a live process owns
    // its cwd, but no message has been stored yet. One such
    // session exists in the real DB. `Idle` is the documented
    // fallback: nothing to classify, so keep the row visible
    // without crying wolf.
    Case {
        name: "session_without_messages_is_done_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R15",
            directory: "task",
            time_updated: T0,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Idle)],
    },
    // Directory has no session row at all (untouched
    // worktree). Classifies as `None`, distinct from `Gone`
    // which implies a session exists but no live process.
    Case {
        name: "directory_without_any_session_is_none_real",
        now_ms: T0 + 30_000,
        live_cwds: &[],
        proc_start_ms: 0,
        sessions: &[],
        messages: &[],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::None)],
    },
    // Two DBs side by side on the same machine: the legacy
    // `opencode.db` and the newer `opencode-stable.db`. A
    // directory with a `Busy` session in one DB and a
    // `Hung` session in the other must roll up to `Hung`.
    // Pins the multi-DB discovery path inside
    // `sessions_for_directory` — if either DB were missed,
    // the rollup would collapse to the survivor's state.
    Case {
        name: "rollup_across_stable_and_legacy_dbs_real",
        now_ms: T0 + 30_000,
        live_cwds: &["see-current-opencode-status"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R16a",
                directory: "see-current-opencode-status",
                time_updated: T0 + 10_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R16b",
                directory: "see-current-opencode-status",
                time_updated: T0 + 9_000,
                db_name: LEGACY_DB,
                parent_id: None,
                time_archived: None,
            },
        ],
        messages: &[
            // Busy session lives in the stable DB.
            MessageSeed {
                id: "msg_R16a",
                session_id: "ses_R16a",
                created_ms: T0 + 10_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            // Hung session (APIError) lives in the legacy DB.
            MessageSeed {
                id: "msg_R16b",
                session_id: "ses_R16b",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 5_000),
                error_name: Some("APIError"),
            },
        ],
        tool_parts: &[],
        expected: &[("see-current-opencode-status", OpenCodeState::Hung)],
    },
    // `pending` tool parts exist in the real DB (3 rows) —
    // they are tool calls the agent has queued but not yet
    // started streaming. The classifier must ignore them;
    // otherwise a session with a queued-but-not-running tool
    // and a completed last message would flap to `Busy`.
    Case {
        name: "pending_tool_is_not_busy_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R17",
            directory: "task",
            time_updated: T0 + 5_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R17",
            session_id: "ses_R17",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 4_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R17",
            session_id: "ses_R17",
            message_id: "msg_R17",
            status: "pending",
            start_ms: None,
            tool: None,
            child_session_id: None,
        }],
        expected: &[("task", OpenCodeState::Idle)],
    },
    // `error` tool parts exist in the real DB (485 rows) —
    // a tool failed mid-execution. The assistant message is
    // what carries the overall error signal; the tool part's
    // `error` status is ignored by the classifier, which only
    // reacts to `running` parts.
    Case {
        name: "errored_tool_is_not_running_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R18",
            directory: "task",
            time_updated: T0 + 5_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R18",
            session_id: "ses_R18",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 4_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R18",
            session_id: "ses_R18",
            message_id: "msg_R18",
            status: "error",
            start_ms: Some(T0 + 1_000),
            tool: None,
            child_session_id: None,
        }],
        expected: &[("task", OpenCodeState::Idle)],
    },
    // Completed tool parts are the overwhelming majority in
    // the real DB (36 605 rows). Same rule: ignored unless
    // `status = running`. Pinned so a future refactor that
    // broadens the SQL filter cannot silently light up every
    // long-completed session as `Busy`.
    Case {
        name: "completed_tool_is_not_running_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R19",
            directory: "task",
            time_updated: T0 + 5_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R19",
            session_id: "ses_R19",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 4_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R19",
            session_id: "ses_R19",
            message_id: "msg_R19",
            status: "completed",
            start_ms: Some(T0 + 1_000),
            tool: None,
            child_session_id: None,
        }],
        expected: &[("task", OpenCodeState::Idle)],
    },
    // Mixed tool statuses in a single session: one running
    // part + several completed + one pending. Only the one
    // `running` part drives classification; the others are
    // noise from the classifier's perspective.
    Case {
        name: "mixed_tool_statuses_only_running_counts_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R20",
            directory: "task",
            time_updated: T0 + 20_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R20",
            session_id: "ses_R20",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 15_000),
            error_name: None,
        }],
        tool_parts: &[
            ToolPartSeed {
                id: "prt_R20a",
                session_id: "ses_R20",
                message_id: "msg_R20",
                status: "completed",
                start_ms: Some(T0 + 1_000),
                tool: None,
                child_session_id: None,
            },
            ToolPartSeed {
                id: "prt_R20b",
                session_id: "ses_R20",
                message_id: "msg_R20",
                status: "pending",
                start_ms: None,
                tool: None,
                child_session_id: None,
            },
            // The one part that should drive classification.
            ToolPartSeed {
                id: "prt_R20c",
                session_id: "ses_R20",
                message_id: "msg_R20",
                status: "running",
                start_ms: Some(T0 + 20_000),
                tool: None,
                child_session_id: None,
            },
            ToolPartSeed {
                id: "prt_R20d",
                session_id: "ses_R20",
                message_id: "msg_R20",
                status: "error",
                start_ms: Some(T0 + 10_000),
                tool: None,
                child_session_id: None,
            },
        ],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Clock-skew defence: a running tool whose start time is
    // slightly in the future. `now_ms.saturating_sub(start)`
    // clamps to zero, so the stuck threshold is not tripped
    // and the session stays `Busy`. Not observed in the real
    // DB but a realistic concern on laptops that suspend and
    // resume with a drifted clock, so worth pinning.
    Case {
        name: "running_tool_with_future_start_is_busy_real",
        now_ms: T0 + 10_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R21",
            directory: "task",
            time_updated: T0 + 5_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R21",
            session_id: "ses_R21",
            created_ms: T0,
            role: "assistant",
            completed_ms: None,
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R21",
            session_id: "ses_R21",
            message_id: "msg_R21",
            status: "running",
            // 5 seconds ahead of `now_ms`.
            start_ms: Some(T0 + 15_000),
            tool: None,
            child_session_id: None,
        }],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Multiple errored sessions in the same directory. Real
    // DB: the `fx_www_admin` directory had several consecutive
    // `UnknownError` sessions from token-refresh 429s. The
    // rollup must stay `Hung` — one is bad enough, three is
    // not three-times-worse in the UI.
    Case {
        name: "rollup_multiple_errored_sessions_stays_hung_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R22a",
                directory: "task",
                time_updated: T0 + 5_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R22b",
                directory: "task",
                time_updated: T0 + 6_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R22c",
                directory: "task",
                time_updated: T0 + 7_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R22a",
                session_id: "ses_R22a",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 4_000),
                error_name: Some("UnknownError"),
            },
            MessageSeed {
                id: "msg_R22b",
                session_id: "ses_R22b",
                created_ms: T0 + 5_000,
                role: "assistant",
                completed_ms: Some(T0 + 5_500),
                error_name: Some("UnknownError"),
            },
            MessageSeed {
                id: "msg_R22c",
                session_id: "ses_R22c",
                created_ms: T0 + 6_000,
                role: "assistant",
                completed_ms: Some(T0 + 6_500),
                error_name: Some("APIError"),
            },
        ],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Mid-session error, clean last message. Real DB has
    // 107 sessions of this shape — the user hit an APIError
    // earlier in the conversation, continued, and the final
    // turn completed cleanly. The classifier reads the
    // latest message only, so the buried error must not
    // leak through. Pins that `classify_with_conn` is
    // memoryless about prior turns.
    Case {
        name: "mid_session_error_then_clean_last_is_done_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R23",
            directory: "task",
            time_updated: T0 + 15_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[
            // Early turn errored.
            MessageSeed {
                id: "msg_R23a",
                session_id: "ses_R23",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 1_000),
                error_name: Some("APIError"),
            },
            // User resumed.
            MessageSeed {
                id: "msg_R23b",
                session_id: "ses_R23",
                created_ms: T0 + 2_000,
                role: "user",
                completed_ms: None,
                error_name: None,
            },
            // Latest turn completed cleanly.
            MessageSeed {
                id: "msg_R23c",
                session_id: "ses_R23",
                created_ms: T0 + 5_000,
                role: "assistant",
                completed_ms: Some(T0 + 10_000),
                error_name: None,
            },
        ],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Idle)],
    },
    // Same shape, opposite polarity: several clean messages
    // followed by a final errored turn. Real DB has sessions
    // like this (token-refresh failure after a long clean
    // conversation). The latest message's error must light up
    // the row, regardless of how many clean messages preceded.
    Case {
        name: "clean_history_then_errored_last_is_hung_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R24",
            directory: "task",
            time_updated: T0 + 15_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[
            MessageSeed {
                id: "msg_R24a",
                session_id: "ses_R24",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 1_000),
                error_name: None,
            },
            MessageSeed {
                id: "msg_R24b",
                session_id: "ses_R24",
                created_ms: T0 + 2_000,
                role: "assistant",
                completed_ms: Some(T0 + 3_000),
                error_name: None,
            },
            // Latest errored.
            MessageSeed {
                id: "msg_R24c",
                session_id: "ses_R24",
                created_ms: T0 + 10_000,
                role: "assistant",
                completed_ms: Some(T0 + 10_500),
                error_name: Some("UnknownError"),
            },
        ],
        tool_parts: &[],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // OpenCode's `task` tool spawns a child session whose id
    // is recorded on the parent part as
    // `state.metadata.sessionId`. The classifier uses the
    // child session's `MAX(time_updated)` across messages +
    // parts as the liveness signal — see the design notes in
    // `classify_running_part`. The cases below pin each branch
    // of that logic against payload shapes observed in a real
    // `opencode-stable.db`.

    // Long-running subagent that's actively chatting: parent
    // task part started 10 min ago, child has a fresh part
    // update 1s ago. Without the subagent branch this would
    // wrongly classify as `Hung` because the parent elapsed
    // time is well past threshold.
    Case {
        name: "subagent_child_fresh_activity_is_busy_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R25p",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R25c",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R25p"),
                time_archived: None,
            },
        ],
        messages: &[
            // Parent's latest message — completed assistant
            // turn that dispatched the subagent.
            MessageSeed {
                id: "msg_R25p",
                session_id: "ses_R25p",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 5_000),
                error_name: None,
            },
            // Child's most recent message — an assistant turn
            // still in flight, updated to "just now".
            MessageSeed {
                id: "msg_R25c",
                session_id: "ses_R25c",
                created_ms: T0 + 599_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[ToolPartSeed {
            id: "prt_R25",
            session_id: "ses_R25p",
            message_id: "msg_R25p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            child_session_id: Some("ses_R25c"),
        }],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Wedged subagent: parent running 10 min, child has been
    // silent for 400s. `Hung`.
    Case {
        name: "subagent_child_stale_activity_is_hung_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R26p",
                directory: "task",
                time_updated: T0 + 200_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R26c",
                directory: "task",
                time_updated: T0 + 200_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R26p"),
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R26p",
                session_id: "ses_R26p",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 5_000),
                error_name: None,
            },
            // Child's freshest row is at T0 + 200_000; now is
            // T0 + 600_000 → silent for 400s.
            MessageSeed {
                id: "msg_R26c",
                session_id: "ses_R26c",
                created_ms: T0 + 200_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[ToolPartSeed {
            id: "prt_R26",
            session_id: "ses_R26p",
            message_id: "msg_R26p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            child_session_id: Some("ses_R26c"),
        }],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Child finished cleanly but parent never reacted: child
    // wrote its final completed assistant message 9.5 min ago
    // and nothing has moved in the child since, yet the
    // parent's task part is still `running`. The activity
    // check catches this exact failure mode — the user
    // specifically asked about it during design.
    Case {
        name: "subagent_child_done_but_parent_not_reacting_is_hung_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R27p",
                directory: "task",
                time_updated: T0 + 30_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R27c",
                directory: "task",
                time_updated: T0 + 30_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R27p"),
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R27p",
                session_id: "ses_R27p",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 5_000),
                error_name: None,
            },
            // Child wrote a completed assistant turn at
            // T0 + 30_000 and stayed silent afterwards.
            MessageSeed {
                id: "msg_R27c",
                session_id: "ses_R27c",
                created_ms: T0 + 20_000,
                role: "assistant",
                completed_ms: Some(T0 + 30_000),
                error_name: None,
            },
        ],
        tool_parts: &[ToolPartSeed {
            id: "prt_R27",
            session_id: "ses_R27p",
            message_id: "msg_R27p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            child_session_id: Some("ses_R27c"),
        }],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Parent in the legitimate wrap-up window: child just
    // completed 1s ago. Must stay `Busy` so the brief window
    // between child completion and parent acknowledging
    // doesn't flash `Hung` on every tool call.
    Case {
        name: "subagent_child_done_and_parent_wrapping_up_is_busy_real",
        now_ms: T0 + 31_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R28p",
                directory: "task",
                time_updated: T0 + 30_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R28c",
                directory: "task",
                time_updated: T0 + 30_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R28p"),
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R28p",
                session_id: "ses_R28p",
                created_ms: T0,
                role: "assistant",
                completed_ms: Some(T0 + 5_000),
                error_name: None,
            },
            MessageSeed {
                id: "msg_R28c",
                session_id: "ses_R28c",
                created_ms: T0 + 20_000,
                role: "assistant",
                completed_ms: Some(T0 + 30_000),
                error_name: None,
            },
        ],
        tool_parts: &[ToolPartSeed {
            id: "prt_R28",
            session_id: "ses_R28p",
            message_id: "msg_R28p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            child_session_id: Some("ses_R28c"),
        }],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Task part has `metadata.sessionId` but the referenced
    // child session does not yet exist in the DB (common
    // brand-new subagent race). Falls back to elapsed-since-
    // start rule; parent started 30s ago → still `Busy`.
    Case {
        name: "subagent_child_session_missing_falls_back_to_start_time_real",
        now_ms: T0 + 30_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R29p",
            directory: "task",
            time_updated: T0 + 5_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R29p",
            session_id: "ses_R29p",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 5_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R29",
            session_id: "ses_R29p",
            message_id: "msg_R29p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            // Dangling pointer — no matching SessionSeed.
            child_session_id: Some("ses_R29_missing"),
        }],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Task part without `metadata.sessionId` at all (observed
    // 2/366 historical task parts). Falls back to elapsed
    // rule; parent started well past threshold → `Hung`.
    Case {
        name: "subagent_without_metadata_session_id_falls_back_to_start_time_real",
        now_ms: T0 + 240_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[SessionSeed {
            id: "ses_R30p",
            directory: "task",
            time_updated: T0 + 10_000,
            db_name: STABLE_DB,
            parent_id: None,
            time_archived: None,
        }],
        messages: &[MessageSeed {
            id: "msg_R30p",
            session_id: "ses_R30p",
            created_ms: T0,
            role: "assistant",
            completed_ms: Some(T0 + 5_000),
            error_name: None,
        }],
        tool_parts: &[ToolPartSeed {
            id: "prt_R30",
            session_id: "ses_R30p",
            message_id: "msg_R30p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            child_session_id: None,
        }],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Parallel tools of mixed types: a healthy subagent
    // (fresh child) running alongside a wedged `bash` past
    // the stuck threshold. Rollup picks the worst vote →
    // `Hung`, because the bash tool genuinely is.
    Case {
        name: "mixed_healthy_subagent_and_stuck_bash_is_hung_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R31p",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R31c",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R31p"),
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R31p",
                session_id: "ses_R31p",
                created_ms: T0,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            MessageSeed {
                id: "msg_R31c",
                session_id: "ses_R31c",
                created_ms: T0 + 599_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[
            // Healthy subagent.
            ToolPartSeed {
                id: "prt_R31_task",
                session_id: "ses_R31p",
                message_id: "msg_R31p",
                status: "running",
                start_ms: Some(T0),
                tool: Some("task"),
                child_session_id: Some("ses_R31c"),
            },
            // Stuck bash: started well past threshold.
            ToolPartSeed {
                id: "prt_R31_bash",
                session_id: "ses_R31p",
                message_id: "msg_R31p",
                status: "running",
                start_ms: Some(T0),
                tool: Some("bash"),
                child_session_id: None,
            },
        ],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Reverse mix: wedged subagent (stale child) alongside a
    // fresh bash. Subagent's `Hung` vote still wins.
    Case {
        name: "mixed_stuck_subagent_and_fresh_bash_is_hung_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R32p",
                directory: "task",
                time_updated: T0 + 200_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R32c",
                directory: "task",
                time_updated: T0 + 200_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R32p"),
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R32p",
                session_id: "ses_R32p",
                created_ms: T0,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            MessageSeed {
                id: "msg_R32c",
                session_id: "ses_R32c",
                created_ms: T0 + 200_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[
            // Stale subagent — child silent for 400s.
            ToolPartSeed {
                id: "prt_R32_task",
                session_id: "ses_R32p",
                message_id: "msg_R32p",
                status: "running",
                start_ms: Some(T0),
                tool: Some("task"),
                child_session_id: Some("ses_R32c"),
            },
            // Fresh bash.
            ToolPartSeed {
                id: "prt_R32_bash",
                session_id: "ses_R32p",
                message_id: "msg_R32p",
                status: "running",
                start_ms: Some(T0 + 599_000),
                tool: Some("bash"),
                child_session_id: None,
            },
        ],
        expected: &[("task", OpenCodeState::Hung)],
    },
    // Parallel-subagent dispatch is common in real usage
    // (e.g. two @explore calls in one turn). Both children
    // are actively chatting → parent rolls up as `Busy`.
    Case {
        name: "two_healthy_subagents_in_parallel_are_busy_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R33p",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R33a",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R33p"),
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R33b",
                directory: "task",
                time_updated: T0 + 598_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R33p"),
                time_archived: None,
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R33p",
                session_id: "ses_R33p",
                created_ms: T0,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            MessageSeed {
                id: "msg_R33a",
                session_id: "ses_R33a",
                created_ms: T0 + 599_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            MessageSeed {
                id: "msg_R33b",
                session_id: "ses_R33b",
                created_ms: T0 + 598_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[
            ToolPartSeed {
                id: "prt_R33a",
                session_id: "ses_R33p",
                message_id: "msg_R33p",
                status: "running",
                start_ms: Some(T0),
                tool: Some("task"),
                child_session_id: Some("ses_R33a"),
            },
            ToolPartSeed {
                id: "prt_R33b",
                session_id: "ses_R33p",
                message_id: "msg_R33p",
                status: "running",
                start_ms: Some(T0),
                tool: Some("task"),
                child_session_id: Some("ses_R33b"),
            },
        ],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Archived-child invariant: OpenCode can archive a child
    // session mid-run. The activity check must still consult
    // the child's message/part rows regardless of
    // `session.time_archived` — otherwise a genuinely busy
    // archived subagent would wrongly classify as `Hung`.
    Case {
        name: "subagent_with_archived_child_still_uses_activity_real",
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        proc_start_ms: 0,
        sessions: &[
            SessionSeed {
                id: "ses_R34p",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            SessionSeed {
                id: "ses_R34c",
                directory: "task",
                time_updated: T0 + 599_000,
                db_name: STABLE_DB,
                parent_id: Some("ses_R34p"),
                // Archived but rows still land — mirroring a
                // child that was archived by the UI while the
                // parent is still reacting to it.
                time_archived: Some(T0 + 300_000),
            },
        ],
        messages: &[
            MessageSeed {
                id: "msg_R34p",
                session_id: "ses_R34p",
                created_ms: T0,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            MessageSeed {
                id: "msg_R34c",
                session_id: "ses_R34c",
                created_ms: T0 + 599_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[ToolPartSeed {
            id: "prt_R34",
            session_id: "ses_R34p",
            message_id: "msg_R34p",
            status: "running",
            start_ms: Some(T0),
            tool: Some("task"),
            child_session_id: Some("ses_R34c"),
        }],
        expected: &[("task", OpenCodeState::Busy)],
    },
    // Regression for the real observed shape where a
    // directory's rollup was `Hung` because zombie sessions
    // from previous `opencode` runs still carried running
    // tool parts and uncompleted assistant messages. The
    // live session (running bash inside the threshold) must
    // dominate the rollup; every zombie must be invisible to
    // the classifier.
    //
    // Shape captured from the user's live DB:
    // - One live session: fresh bash running.
    // - One live session: previous turn completed cleanly.
    // - One zombie session: parent of a stale `task` part
    //   whose child subagent stopped moving ~14h ago.
    // - Two zombie sessions: last assistant message has no
    //   `time.completed` and was created ~14h ago.
    //
    // Timings are stretched so every zombie artifact is
    // well past the 180s stuck threshold — without the
    // live-ownership filter the rollup would be `Hung`.
    Case {
        name: "live_session_wins_over_zombies_real",
        // Now is 400s after the live process started.
        now_ms: T0 + 600_000,
        live_cwds: &["task"],
        // Live opencode started at T0 + 200_000: zombies all
        // touched before this; live sessions all touched
        // after.
        proc_start_ms: T0 + 200_000,
        sessions: &[
            // Live — fresh running bash.
            SessionSeed {
                id: "ses_R35_live_busy",
                directory: "task",
                time_updated: T0 + 595_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            // Live — turn completed cleanly.
            SessionSeed {
                id: "ses_R35_live_idle",
                directory: "task",
                time_updated: T0 + 300_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            // Zombie — stale leaked `task` part. Last message
            // completed long before proc_start.
            SessionSeed {
                id: "ses_R35_zombie_leak",
                directory: "task",
                time_updated: T0 + 50_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            // Zombie — in-flight assistant message from before
            // proc_start, created far beyond the stuck
            // threshold from `now_ms`.
            SessionSeed {
                id: "ses_R35_zombie_inflight_a",
                directory: "task",
                time_updated: T0 + 10_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
            // Zombie — another in-flight assistant message.
            SessionSeed {
                id: "ses_R35_zombie_inflight_b",
                directory: "task",
                time_updated: T0 + 20_000,
                db_name: STABLE_DB,
                parent_id: None,
                time_archived: None,
            },
        ],
        messages: &[
            // Live busy: assistant message in-flight, created
            // well after proc_start. Fresh enough that the
            // elapsed classifier would vote `Busy` — but a
            // running bash is what actually drives the state.
            MessageSeed {
                id: "msg_R35_live_busy",
                session_id: "ses_R35_live_busy",
                created_ms: T0 + 595_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            // Live idle: turn completed cleanly.
            MessageSeed {
                id: "msg_R35_live_idle",
                session_id: "ses_R35_live_idle",
                created_ms: T0 + 250_000,
                role: "assistant",
                completed_ms: Some(T0 + 300_000),
                error_name: None,
            },
            // Zombie leak parent: turn completed before
            // proc_start. Whole session is filtered, so this
            // contributes nothing.
            MessageSeed {
                id: "msg_R35_zombie_leak",
                session_id: "ses_R35_zombie_leak",
                created_ms: T0 + 40_000,
                role: "assistant",
                completed_ms: Some(T0 + 50_000),
                error_name: None,
            },
            // Zombie in-flight A: assistant message with no
            // completion, elapsed 590s (>> 180s threshold).
            // Without the filter this classifies as `Hung`.
            MessageSeed {
                id: "msg_R35_zombie_inflight_a",
                session_id: "ses_R35_zombie_inflight_a",
                created_ms: T0 + 10_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
            MessageSeed {
                id: "msg_R35_zombie_inflight_b",
                session_id: "ses_R35_zombie_inflight_b",
                created_ms: T0 + 20_000,
                role: "assistant",
                completed_ms: None,
                error_name: None,
            },
        ],
        tool_parts: &[
            // Live busy: running bash started after proc_start.
            ToolPartSeed {
                id: "prt_R35_live_bash",
                session_id: "ses_R35_live_busy",
                message_id: "msg_R35_live_busy",
                status: "running",
                start_ms: Some(T0 + 595_000),
                tool: Some("bash"),
                child_session_id: None,
            },
            // Zombie leak: stale running `task` part, started
            // 595s ago. Without the filter this classifies as
            // `Hung` via `classify_running_part` (no live
            // child session → fall back to elapsed rule, far
            // past threshold).
            ToolPartSeed {
                id: "prt_R35_zombie_leak",
                session_id: "ses_R35_zombie_leak",
                message_id: "msg_R35_zombie_leak",
                status: "running",
                start_ms: Some(T0 + 5_000),
                tool: Some("task"),
                child_session_id: None,
            },
        ],
        expected: &[("task", OpenCodeState::Busy)],
    },
];

fn insert_real_message(conn: &Connection, seed: &MessageSeed) {
    let mut data = serde_json::json!({
        "role": seed.role,
        "time": { "created": seed.created_ms },
    });
    if let Some(completed) = seed.completed_ms {
        data["time"]["completed"] = serde_json::json!(completed);
    }
    if let Some(name) = seed.error_name {
        data["error"] = serde_json::json!({ "name": name });
    }
    let updated = seed.completed_ms.unwrap_or(seed.created_ms);
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            seed.id,
            seed.session_id,
            seed.created_ms,
            updated,
            data.to_string()
        ],
    )
    .unwrap();
}

fn insert_real_tool_part(conn: &Connection, seed: &ToolPartSeed) {
    let mut state = serde_json::json!({ "status": seed.status });
    if let Some(start) = seed.start_ms {
        state["time"] = serde_json::json!({ "start": start });
    }
    if let Some(child) = seed.child_session_id {
        state["metadata"] = serde_json::json!({ "sessionId": child });
    }
    let data = serde_json::json!({
        "type": "tool",
        "tool": seed.tool.unwrap_or("bash"),
        "state": state,
    });
    // `time_created` is just metadata here; `start_ms` is the
    // closest real analogue when available, otherwise fall
    // back to a fixed timestamp so the INSERT still succeeds.
    let time_created = seed.start_ms.unwrap_or(T0);
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            seed.id,
            seed.message_id,
            seed.session_id,
            time_created,
            data.to_string()
        ],
    )
    .unwrap();
}

/// Seed one or more DB files under `base` and return every DB
/// path created. A case may reference multiple `db_name`s to
/// reproduce the real-machine layout where both the stable and
/// the legacy DB coexist; each is created on demand.
fn seed_case_db(base: &Path, case: &Case) -> Vec<PathBuf> {
    use std::collections::BTreeMap;

    // Group sessions by DB so each DB is opened once and every
    // relevant session / message / running-tool is written into
    // the right file.
    let mut sessions_by_db: BTreeMap<&str, Vec<&SessionSeed>> = BTreeMap::new();
    for s in case.sessions {
        sessions_by_db.entry(s.db_name).or_default().push(s);
    }

    let mut dbs = Vec::new();
    for (db_name, sessions) in sessions_by_db {
        let db = new_db(base, db_name);
        let conn = Connection::open(&db).unwrap();
        let session_ids: std::collections::HashSet<&str> = sessions.iter().map(|s| s.id).collect();

        for s in &sessions {
            insert_session_with_parent(
                &conn,
                s.id,
                s.parent_id,
                s.directory,
                s.time_updated,
                s.time_archived,
            );
        }
        for m in case
            .messages
            .iter()
            .filter(|m| session_ids.contains(m.session_id))
        {
            insert_real_message(&conn, m);
        }
        for p in case
            .tool_parts
            .iter()
            .filter(|p| session_ids.contains(p.session_id))
        {
            insert_real_tool_part(&conn, p);
        }
        dbs.push(db);
    }
    dbs
}

#[test]
fn every_real_case_matches_expected_state() {
    for case in CASES {
        let base = temp(&format!("real-{}", case.name));
        let dbs = seed_case_db(&base, case);
        let live_cwds_with_start = case
            .live_cwds
            .iter()
            .map(|cwd| {
                (
                    PathBuf::from(cwd),
                    u64::try_from(case.proc_start_ms)
                        .expect("real case start time is non-negative"),
                )
            })
            .collect::<Vec<_>>();
        let snap = snapshot_with_proc_starts(dbs, case.now_ms, live_cwds_with_start);

        for (directory, expected) in case.expected {
            assert_eq!(
                snap.state_for(Path::new(directory)),
                *expected,
                "case `{}` mis-classified directory `{}`",
                case.name,
                directory,
            );
        }

        _ = fs::remove_dir_all(&base);
    }
}

/// Real DB has sessions with up to 712 messages. Pin that
/// `ORDER BY time_created DESC LIMIT 1` in `latest_message`
/// picks the actual latest regardless of how many messages
/// precede it — the final errored turn must light the row
/// up as `Hung`, not any of the 99 clean messages before it.
#[test]
fn latest_message_picked_from_large_history_real() {
    let base = temp("real-large-history");
    let db = new_db(&base, "opencode-stable.db");
    let conn = Connection::open(&db).unwrap();

    let dir = "task";
    insert_session(&conn, "ses_large", dir, T0 + 150_000);

    // 99 filler messages: alternating clean assistant + user,
    // spread over 99s in the past.
    for i in 0..99 {
        let role = if i % 2 == 0 { "assistant" } else { "user" };
        let created = T0 + i * 1_000;
        let completed = (role == "assistant").then(|| serde_json::json!(created + 500));
        let mut time = serde_json::json!({ "created": created });
        if let Some(c) = completed {
            time["completed"] = c;
        }
        let data = serde_json::json!({ "role": role, "time": time });
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) \
             VALUES (?1, ?2, ?3, ?3, ?4)",
            rusqlite::params![
                format!("msg_large_{i:03}"),
                "ses_large",
                created,
                data.to_string()
            ],
        )
        .unwrap();
    }

    // 100th message: errored assistant, newest by time_created.
    let err = serde_json::json!({
        "role": "assistant",
        "time": { "created": T0 + 140_000, "completed": T0 + 140_500 },
        "error": { "name": "APIError" }
    });
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params!["msg_large_last", "ses_large", T0 + 140_000, err.to_string()],
    )
    .unwrap();

    let snap = snapshot_with(vec![db], T0 + 200_000, vec![PathBuf::from(dir)]);
    assert_eq!(snap.state_for(Path::new(dir)), OpenCodeState::Hung);

    _ = fs::remove_dir_all(&base);
}

/// Real DB has one directory with 116 active sessions. The
/// rollup used to cap at `LIMIT 20` and silently hide any
/// `Hung` session outside that window — the exact opposite of
/// the classifier's purpose. With the cap raised to 1000 the
/// old hung session now surfaces correctly.
#[test]
fn rollup_surfaces_hung_session_regardless_of_age_real() {
    let base = temp("real-hung-past-recent");
    let db = new_db(&base, "opencode-stable.db");
    let conn = Connection::open(&db).unwrap();

    let dir = "task";

    // 50 recent sessions, all clean Idle. Well beyond the
    // previous 20-session cap so a regression that re-
    // introduces a tight LIMIT will still truncate these.
    for i in 0..50 {
        let ses_id = format!("ses_recent_{i:02}");
        let msg_id = format!("msg_recent_{i:02}");
        let created = T0 + 10_000 + i * 100;
        insert_session(&conn, &ses_id, dir, created);
        let data = serde_json::json!({
            "role": "assistant",
            "time": { "created": created, "completed": created + 10 }
        });
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data) \
             VALUES (?1, ?2, ?3, ?3, ?4)",
            rusqlite::params![msg_id, ses_id, created, data.to_string()],
        )
        .unwrap();
    }

    // One much older session is Hung (APIError). Its
    // `time_updated` is before every recent session, so a
    // tight LIMIT would truncate it out of the rollup.
    insert_session(&conn, "ses_old_hung", dir, T0);
    let err = serde_json::json!({
        "role": "assistant",
        "time": { "created": T0, "completed": T0 + 500 },
        "error": { "name": "APIError" }
    });
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) \
         VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params!["msg_old_hung", "ses_old_hung", T0, err.to_string()],
    )
    .unwrap();

    let snap = snapshot_with(vec![db], T0 + 30_000, vec![PathBuf::from(dir)]);
    assert_eq!(
        snap.state_for(Path::new(dir)),
        OpenCodeState::Hung,
        "regression: if this returns Idle, the session limit has been \
         tightened again — a buried Hung session must not be hidden"
    );

    _ = fs::remove_dir_all(&base);
}
