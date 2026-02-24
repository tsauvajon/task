use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::runtime::process::ProcessRunner;

pub fn auth_storage_reachable(process: ProcessRunner) -> bool {
    process
        .run_status("opencode", &["auth", "list"], None)
        .is_ok()
}

/// Returns the opencode launch arguments for a given worktree directory.
///
/// If a previous opencode session exists for that exact directory, returns
/// `["opencode", "--session", "<id>"]` so the TUI resumes it. Otherwise
/// returns `["opencode"]` to start a fresh session.
pub fn launch_args(directory: &Path) -> Vec<String> {
    match last_session_id(directory) {
        Some(id) => vec!["opencode".to_string(), "--session".to_string(), id],
        None => vec!["opencode".to_string()],
    }
}

pub fn rename_latest_session_title(directory: &Path, title: &str) -> Result<bool, String> {
    let Some(db_path) = opencode_db_path() else {
        return Ok(false);
    };

    rename_latest_session_title_at_db(&db_path, directory, title)
}

/// Looks up the most recently updated opencode session for a given worktree
/// directory path. Returns `None` if no session is found or if the opencode
/// database is not accessible.
fn last_session_id(directory: &Path) -> Option<String> {
    let db_path = opencode_db_path()?;
    let connection = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let directory_str = directory.to_string_lossy();
    connection
        .query_row(
            "SELECT id FROM session WHERE directory = ?1 ORDER BY time_updated DESC LIMIT 1",
            rusqlite::params![directory_str],
            |row| row.get::<_, String>(0),
        )
        .ok()
}

fn opencode_db_path() -> Option<PathBuf> {
    let data_dir = if let Ok(base) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(base)
    } else {
        let home = std::env::var("HOME").ok()?;
        PathBuf::from(home).join(".local/share")
    };

    let path = data_dir.join("opencode/opencode.db");
    path.exists().then_some(path)
}

fn rename_latest_session_title_at_db(
    db_path: &Path,
    directory: &Path,
    title: &str,
) -> Result<bool, String> {
    let connection = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;

    let directory_str = directory.to_string_lossy();
    let updated = connection
        .execute(
            "UPDATE session SET title = ?1 WHERE id = (SELECT id FROM session WHERE directory = ?2 ORDER BY time_updated DESC LIMIT 1)",
            rusqlite::params![title, directory_str.as_ref()],
        )
        .map_err(|error| error.to_string())?;

    Ok(updated > 0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rusqlite::Connection;

    use super::*;

    fn create_test_db(dir: &std::path::Path, sessions: &[(&str, &str, i64)]) -> PathBuf {
        let db_path = dir.join("opencode.db");
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                time_updated INTEGER NOT NULL DEFAULT 0
            )",
        )
        .expect("create table");
        for (id, directory, time_updated) in sessions {
            conn.execute(
                "INSERT INTO session (id, directory, time_updated) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, directory, time_updated],
            )
            .expect("insert");
        }
        db_path
    }

    fn query_last_session(db_path: &std::path::Path, directory: &str) -> Option<String> {
        let conn = Connection::open(db_path).expect("open");
        conn.query_row(
            "SELECT id FROM session WHERE directory = ?1 ORDER BY time_updated DESC LIMIT 1",
            rusqlite::params![directory],
            |row| row.get(0),
        )
        .ok()
    }

    fn query_title(db_path: &std::path::Path, id: &str) -> String {
        let conn = Connection::open(db_path).expect("open");
        conn.query_row(
            "SELECT title FROM session WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .expect("title")
    }

    #[test]
    fn last_session_id_returns_most_recent_for_directory() {
        let dir = std::env::temp_dir().join("task-test-opencode-recent");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = create_test_db(
            &dir,
            &[
                ("ses_old", "/wt/repo/branch", 100),
                ("ses_new", "/wt/repo/branch", 200),
                ("ses_other", "/wt/repo/other", 300),
            ],
        );

        let id = query_last_session(&db_path, "/wt/repo/branch");
        assert_eq!(id, Some("ses_new".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn last_session_id_returns_none_for_unknown_directory() {
        let dir = std::env::temp_dir().join("task-test-opencode-unknown");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = create_test_db(&dir, &[("ses_abc", "/wt/repo/branch", 100)]);

        let id = query_last_session(&db_path, "/wt/other/branch");
        assert_eq!(id, None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn launch_args_returns_plain_opencode_when_no_db() {
        // Point at a non-existent path — opencode_db_path() returns None.
        let args = launch_args(Path::new("/nonexistent/worktree"));
        assert_eq!(args, vec!["opencode".to_string()]);
    }

    #[test]
    fn rename_latest_session_title_updates_most_recent_matching_directory() {
        let dir = std::env::temp_dir().join("task-test-opencode-rename-latest");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = create_test_db(
            &dir,
            &[
                ("ses_old", "/wt/repo/branch", 100),
                ("ses_new", "/wt/repo/branch", 200),
                ("ses_other", "/wt/repo/other", 300),
            ],
        );

        let updated = rename_latest_session_title_at_db(
            &db_path,
            Path::new("/wt/repo/branch"),
            "github.com/acme/repo feat/branch",
        )
        .expect("rename title");
        assert!(updated);
        assert_eq!(query_title(&db_path, "ses_old"), "");
        assert_eq!(
            query_title(&db_path, "ses_new"),
            "github.com/acme/repo feat/branch"
        );
        assert_eq!(query_title(&db_path, "ses_other"), "");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_latest_session_title_returns_false_when_no_matching_directory() {
        let dir = std::env::temp_dir().join("task-test-opencode-rename-missing");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = create_test_db(&dir, &[("ses_abc", "/wt/repo/branch", 100)]);

        let updated = rename_latest_session_title_at_db(
            &db_path,
            Path::new("/wt/repo/other"),
            "github.com/acme/repo feat/other",
        )
        .expect("rename title");
        assert!(!updated);
        assert_eq!(query_title(&db_path, "ses_abc"), "");

        std::fs::remove_dir_all(&dir).ok();
    }
}
