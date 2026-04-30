//! Locate OpenCode SQLite databases and query them read-only.
//!
//! OpenCode ships multiple release channels (stable, dev, …) and each
//! channel writes to its own `opencode*.db` file under the OpenCode data
//! directory. This module discovers every such file and resolves the
//! most recently-updated session for a given worktree across all of
//! them.
//!
//! All reads open the database with
//! `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`, so they never block
//! writers and are safe to run on hot paths.

use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use rusqlite::{Connection, OpenFlags};

/// Minimal session metadata needed by callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMeta {
    pub id: String,
    /// Milliseconds since UNIX_EPOCH, as stored by OpenCode.
    pub time_updated: i64,
}

/// Returned session paired with the DB it belongs to, so follow-up
/// queries hit the same file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSession {
    pub db_path: PathBuf,
    pub session: SessionMeta,
}

/// Discover every `opencode*.db` file under the OpenCode data directory
/// that exists and is readable. Returns an empty vector when the
/// directory is missing.
#[must_use]
pub fn discover_opencode_dbs() -> Vec<PathBuf> {
    let Some(dir) = opencode_data_dir() else {
        return Vec::new();
    };
    discover_in(&dir)
}

/// Testable variant: scans a specific directory for `opencode*.db`
/// files.
pub(crate) fn discover_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_opencode_db_name(name) && path.is_file() {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Resolve the most recently-updated non-archived session for
/// `directory` across every installed OpenCode database. Returns
/// `None` when no session exists for that directory.
#[must_use]
pub fn latest_session_for(directory: &Path) -> Option<SessionMeta> {
    latest_owned_session_for(directory).map(|owned| owned.session)
}

/// Like [`latest_session_for`] but also reports which database owns the
/// winning session. Needed by callers that issue follow-up queries
/// against the same DB (messages, parts, permissions).
#[must_use]
pub fn latest_owned_session_for(directory: &Path) -> Option<OwnedSession> {
    latest_owned_session_in_dbs(&discover_opencode_dbs(), directory)
}

pub(crate) fn latest_owned_session_in_dbs(
    dbs: &[PathBuf],
    directory: &Path,
) -> Option<OwnedSession> {
    if dbs.is_empty() {
        return None;
    }

    let canonical = canonical_dir(directory);
    let candidates = directory_candidates(directory, &canonical);

    let mut best: Option<OwnedSession> = None;
    for db_path in dbs {
        let Some(session) = latest_session_in_db(db_path, &candidates) else {
            continue;
        };
        match &best {
            Some(current) if current.session.time_updated >= session.time_updated => {}
            _ => {
                best = Some(OwnedSession {
                    db_path: db_path.clone(),
                    session,
                });
            }
        }
    }
    best
}

fn latest_session_in_db(db_path: &Path, directories: &[String]) -> Option<SessionMeta> {
    // An empty `IN ()` clause is a SQL syntax error in SQLite; callers
    // today always pass a non-empty slice but let's not trust that
    // forever.
    if directories.is_empty() {
        return None;
    }
    let conn = open_ro(db_path)?;

    // Build a parameter list like "?1, ?2, ?3" for an IN() clause.
    let placeholders: Vec<String> = (1..=directories.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT id, time_updated FROM session \
         WHERE time_archived IS NULL AND directory IN ({}) \
         ORDER BY time_updated DESC LIMIT 1",
        placeholders.join(", ")
    );

    let params = rusqlite::params_from_iter(directories.iter().map(String::as_str));
    conn.query_row(&sql, params, |row| {
        Ok(SessionMeta {
            id: row.get::<_, String>(0)?,
            time_updated: row.get::<_, i64>(1)?,
        })
    })
    .ok()
}

/// Open a database read-only without taking the mutex, so the call is
/// non-blocking even while OpenCode is writing through its WAL.
pub(crate) fn open_ro(path: &Path) -> Option<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

/// Candidate strings to probe the `directory` column with. OpenCode may
/// record the path before macOS canonicalised `/var` → `/private/var`,
/// so we try both the caller's path and its canonical form.
pub(crate) fn directory_candidates(directory: &Path, canonical: &Path) -> Vec<String> {
    let raw = directory.to_string_lossy().to_string();
    let canonical = canonical.to_string_lossy().to_string();
    if raw == canonical {
        vec![raw]
    } else {
        vec![raw, canonical]
    }
}

pub(crate) fn canonical_dir(directory: &Path) -> PathBuf {
    std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf())
}

fn opencode_data_dir() -> Option<PathBuf> {
    if let Ok(base) = std::env::var("XDG_DATA_HOME") {
        return Some(PathBuf::from(base).join("opencode"));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".local/share/opencode"))
}

/// Matches `opencode.db`, `opencode-stable.db`, `opencode-beta.db`, …
/// Rejects sibling files like WAL/SHM or snapshots.
fn is_opencode_db_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".db") else {
        return false;
    };
    stem == "opencode" || stem.starts_with("opencode-")
}

/// Epoch millis helper kept in this module so callers don't need to
/// duplicate the conversion.
#[must_use]
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn create_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                time_updated INTEGER NOT NULL DEFAULT 0,
                time_archived INTEGER
            );",
        )
        .unwrap();
    }

    fn insert_session(
        conn: &Connection,
        id: &str,
        directory: &str,
        time_updated: i64,
        archived: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO session (id, directory, time_updated, time_archived) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, directory, time_updated, archived],
        )
        .unwrap();
    }

    fn create_db(dir: &Path, filename: &str) -> PathBuf {
        let path = dir.join(filename);
        let conn = Connection::open(&path).unwrap();
        create_schema(&conn);
        path
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("task-rs-opencode-db-{tag}"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    mod is_opencode_db_name {
        use super::*;

        #[test]
        fn accepts_canonical_names() {
            assert!(is_opencode_db_name("opencode.db"));
            assert!(is_opencode_db_name("opencode-stable.db"));
            assert!(is_opencode_db_name("opencode-beta.db"));
            assert!(is_opencode_db_name("opencode-dev.db"));
        }

        #[test]
        fn rejects_wal_shm_sidecars() {
            assert!(!is_opencode_db_name("opencode.db-wal"));
            assert!(!is_opencode_db_name("opencode.db-shm"));
        }

        #[test]
        fn rejects_unrelated_files() {
            assert!(!is_opencode_db_name("auth.json"));
            assert!(!is_opencode_db_name("encode.db"));
            assert!(!is_opencode_db_name("opencodex.db"));
        }
    }

    mod discover_in {
        use super::*;

        #[test]
        fn returns_empty_when_dir_missing() {
            let base = temp_dir("discover-empty");
            let _ = fs::remove_dir_all(&base);
            assert!(discover_in(&base).is_empty());
        }

        #[test]
        fn returns_every_matching_db() {
            let base = temp_dir("discover-many");
            let _ = create_db(&base, "opencode.db");
            let _ = create_db(&base, "opencode-stable.db");
            fs::write(base.join("opencode.db-wal"), b"").unwrap();
            fs::write(base.join("auth.json"), b"{}").unwrap();

            let dbs = discover_in(&base);
            assert_eq!(dbs.len(), 2);
            assert!(dbs.iter().any(|p| p.ends_with("opencode.db")));
            assert!(dbs.iter().any(|p| p.ends_with("opencode-stable.db")));

            let _ = fs::remove_dir_all(&base);
        }
    }

    mod latest_owned_session_in_dbs {
        use super::*;

        #[test]
        fn picks_newest_across_multiple_dbs() {
            let base = temp_dir("latest-across");
            let stable = create_db(&base, "opencode-stable.db");
            let legacy = create_db(&base, "opencode.db");

            let conn_stable = Connection::open(&stable).unwrap();
            insert_session(&conn_stable, "ses_stable", "/worktree", 200, None);
            let conn_legacy = Connection::open(&legacy).unwrap();
            insert_session(&conn_legacy, "ses_legacy", "/worktree", 100, None);

            let dbs = discover_in(&base);
            let got = latest_owned_session_in_dbs(&dbs, Path::new("/worktree")).expect("session");
            assert_eq!(got.session.id, "ses_stable");
            assert_eq!(got.session.time_updated, 200);
            assert_eq!(got.db_path, stable);

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn ignores_archived_sessions() {
            let base = temp_dir("latest-archived");
            let db = create_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "ses_archived", "/worktree", 500, Some(600));
            insert_session(&conn, "ses_live", "/worktree", 100, None);

            let got = latest_owned_session_in_dbs(&discover_in(&base), Path::new("/worktree"))
                .expect("session");
            assert_eq!(got.session.id, "ses_live");

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn returns_none_when_directory_unknown() {
            let base = temp_dir("latest-unknown");
            let db = create_db(&base, "opencode-stable.db");
            let conn = Connection::open(&db).unwrap();
            insert_session(&conn, "ses_other", "/elsewhere", 100, None);

            let got = latest_owned_session_in_dbs(&discover_in(&base), Path::new("/worktree"));
            assert!(got.is_none());

            let _ = fs::remove_dir_all(&base);
        }

        #[test]
        fn no_dbs_returns_none() {
            assert!(latest_owned_session_in_dbs(&[], Path::new("/worktree")).is_none());
        }

        /// On a tie in `time_updated`, the function keeps the first
        /// observed session (`>=` rather than `>`). Pin that ordering so
        /// a future change is a conscious decision.
        #[test]
        fn tie_on_time_updated_keeps_first_observed() {
            let base = temp_dir("latest-tie");
            let first = create_db(&base, "opencode-a.db");
            let second = create_db(&base, "opencode-b.db");

            let conn_first = Connection::open(&first).unwrap();
            insert_session(&conn_first, "ses_a", "/worktree", 100, None);
            let conn_second = Connection::open(&second).unwrap();
            insert_session(&conn_second, "ses_b", "/worktree", 100, None);

            // `discover_in` sorts alphabetically, so opencode-a.db
            // arrives first. With equal time_updated the tie must go
            // to the first-scanned DB.
            let dbs = discover_in(&base);
            let got = latest_owned_session_in_dbs(&dbs, Path::new("/worktree")).expect("session");
            assert_eq!(got.session.id, "ses_a");

            let _ = fs::remove_dir_all(&base);
        }
    }

    mod directory_candidates_tests {
        use super::*;

        #[test]
        fn returns_single_entry_when_raw_equals_canonical() {
            let raw = Path::new("/exact/match");
            let canonical = Path::new("/exact/match");
            let got = directory_candidates(raw, canonical);
            assert_eq!(got, vec!["/exact/match".to_string()]);
        }

        #[test]
        fn returns_both_entries_when_raw_differs_from_canonical() {
            let raw = Path::new("/var/folders/x/y/worktree");
            let canonical = Path::new("/private/var/folders/x/y/worktree");
            let got = directory_candidates(raw, canonical);
            assert_eq!(
                got,
                vec![
                    "/var/folders/x/y/worktree".to_string(),
                    "/private/var/folders/x/y/worktree".to_string(),
                ]
            );
        }
    }

    mod canonical_dir_tests {
        use super::*;

        #[test]
        fn returns_input_path_unchanged_when_canonicalize_fails() {
            let missing = Path::new("/definitely/does/not/exist/task-rs");
            let got = canonical_dir(missing);
            // Fallback is the input path verbatim.
            assert_eq!(got, missing.to_path_buf());
        }

        #[test]
        fn resolves_real_directory_to_absolute_canonical_form() {
            let base = temp_dir("canonical-real");
            let got = canonical_dir(&base);
            // On macOS tempdir may sit under /var → /private/var; on
            // Linux it's usually unchanged. Either way the result must
            // be absolute and readable.
            assert!(got.is_absolute(), "canonical must be absolute: {got:?}");
            assert!(got.exists(), "canonical must resolve: {got:?}");

            let _ = fs::remove_dir_all(&base);
        }
    }
}
