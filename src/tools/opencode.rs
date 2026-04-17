use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::{
    error::Result,
    runtime::process::{CommandPlan, ExternalTool},
};

pub fn auth_storage_reachable() -> bool {
    crate::runtime::process::run_status("opencode", &["auth", "list"], None).is_ok()
}

/// Returns the full command plan for launching opencode for a worktree.
///
/// If a previous opencode session exists for that exact directory, the command
/// includes `--session <id>` so the TUI resumes it.
pub fn launch_command(directory: &Path) -> CommandPlan {
    match last_session_id(directory) {
        Some(id) => {
            CommandPlan::for_tool(ExternalTool::Opencode, vec!["--session".to_string(), id])
        }
        None => CommandPlan::for_tool(ExternalTool::Opencode, Vec::new()),
    }
}

pub fn rename_latest_session_title(directory: &Path, title: &str) -> Result<bool> {
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
    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let dir_str = directory.to_string_lossy();
    conn.query_row(
        "SELECT id FROM session WHERE directory = ?1 ORDER BY time_updated DESC LIMIT 1",
        rusqlite::params![dir_str],
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
) -> Result<bool> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let dir_str = directory.to_string_lossy();
    let updated = conn.execute(
        "UPDATE session SET title = ?1 WHERE id = (SELECT id FROM session WHERE directory = ?2 ORDER BY time_updated DESC LIMIT 1)",
        rusqlite::params![title, dir_str.as_ref()],
    )?;

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

    mod last_session_id {
        use super::*;

        #[test]
        fn returns_most_recent_for_directory() {
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
        fn returns_none_for_unknown_directory() {
            let dir = std::env::temp_dir().join("task-test-opencode-unknown");
            std::fs::create_dir_all(&dir).unwrap();
            let db_path = create_test_db(&dir, &[("ses_abc", "/wt/repo/branch", 100)]);

            let id = query_last_session(&db_path, "/wt/other/branch");
            assert_eq!(id, None);

            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn returns_single_session_for_directory() {
            let dir = std::env::temp_dir().join("task-test-opencode-single");
            std::fs::create_dir_all(&dir).unwrap();
            let db_path = create_test_db(&dir, &[("only_session", "/wt/repo/branch", 1)]);

            let id = query_last_session(&db_path, "/wt/repo/branch");
            assert_eq!(id, Some("only_session".to_string()));

            std::fs::remove_dir_all(&dir).ok();
        }
    }

    mod launch_command {
        use super::*;

        #[test]
        fn uses_direct_opencode_binary_when_no_db() {
            let plan = launch_command(Path::new("/nonexistent/worktree"));
            assert_eq!(plan.program(), "opencode");
            assert!(plan.args().is_empty());
        }

        #[test]
        fn program_is_opencode() {
            let plan = launch_command(Path::new("/nonexistent/wt/repo"));
            assert_eq!(plan.program(), "opencode");
        }
    }

    mod rename_latest_session_title {
        use super::*;

        #[test]
        fn updates_most_recent_matching_directory() {
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
        fn returns_false_when_no_matching_directory() {
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

        #[test]
        fn only_touches_most_recent_session_when_multiple_exist() {
            let dir = std::env::temp_dir().join("task-test-opencode-rename-only-recent");
            std::fs::create_dir_all(&dir).unwrap();
            let db_path = create_test_db(
                &dir,
                &[
                    ("ses_1", "/wt/repo/branch", 10),
                    ("ses_2", "/wt/repo/branch", 20),
                    ("ses_3", "/wt/repo/branch", 30),
                ],
            );

            rename_latest_session_title_at_db(&db_path, Path::new("/wt/repo/branch"), "new-title")
                .expect("rename title");

            // Only ses_3 (time_updated=30) should be renamed.
            assert_eq!(query_title(&db_path, "ses_1"), "");
            assert_eq!(query_title(&db_path, "ses_2"), "");
            assert_eq!(query_title(&db_path, "ses_3"), "new-title");

            std::fs::remove_dir_all(&dir).ok();
        }
    }
}
