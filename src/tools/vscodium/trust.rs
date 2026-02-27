use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::error::{Error, Result};

const TRUST_MODEL_KEY: &str = "content.trust.model.key";

/// Seeds trusted workspace roots into a task-specific VSCodium profile.
///
/// VSCodium stores workspace trust in `<user-data-dir>/User/globalStorage/state.vscdb`
/// under the `content.trust.model.key` row of `ItemTable`.
///
/// We merge configured trusted roots into that JSON document before opening the editor
/// so task worktrees under those roots open as trusted without disabling workspace trust.
pub fn seed_trusted_roots(user_data_dir: &Path, trusted_roots: &[PathBuf]) -> Result<()> {
    let normalized_roots: Vec<String> = trusted_roots.iter().map(|r| normalize_path(r)).collect();

    if normalized_roots.is_empty() {
        return Ok(());
    }

    let global_storage_dir = user_data_dir.join("User/globalStorage");
    fs::create_dir_all(&global_storage_dir)?;
    let db_path = global_storage_dir.join("state.vscdb");

    let conn = Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
        [],
    )?;

    let existing: Option<String> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            params![TRUST_MODEL_KEY],
            |row| row.get(0),
        )
        .optional()?;

    let merged = merge_trust_model(existing.as_deref(), &normalized_roots)?;
    conn.execute(
        "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
        params![TRUST_MODEL_KEY, merged],
    )?;

    Ok(())
}

fn merge_trust_model(existing: Option<&str>, trusted_roots: &[String]) -> Result<String> {
    let mut model = existing
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));

    if !model.get("uriTrustInfo").is_some_and(Value::is_array) {
        model["uriTrustInfo"] = Value::Array(Vec::new());
    }

    let entries = model["uriTrustInfo"]
        .as_array_mut()
        .ok_or_else(|| Error::failed("Could not initialize VSCodium trust model"))?;

    // Collect paths that are already trusted so we don't add duplicates.
    let known_paths: HashSet<String> = entries
        .iter()
        .filter(|e| e.get("trusted").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|e| e.get("uri"))
        .filter_map(|uri| {
            uri.get("path")
                .and_then(Value::as_str)
                .or_else(|| uri.get("fsPath").and_then(Value::as_str))
        })
        .map(|p| normalize_path(Path::new(p)))
        .collect();

    for root in trusted_roots {
        if !known_paths.contains(root) {
            entries.push(trusted_entry(root));
        }
    }

    Ok(serde_json::to_string(&model)?)
}

fn trusted_entry(root: &str) -> Value {
    json!({
        "uri": {
            "$mid": 1,
            "scheme": "file",
            "path": root,
            "fsPath": root,
            "external": format!("file://{root}"),
        },
        "trusted": true,
    })
}

fn normalize_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if text == "/" {
        return text.into_owned();
    }
    text.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path};

    use rusqlite::Connection;

    use super::{merge_trust_model, normalize_path, seed_trusted_roots};

    fn read_trust_model(path: &std::path::Path) -> String {
        let db_path = path.join("User/globalStorage/state.vscdb");
        let connection = Connection::open(db_path).expect("open sqlite");
        connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'content.trust.model.key'",
                [],
                |row| row.get(0),
            )
            .expect("read trust model")
    }

    mod normalize_path_tests {
        use super::*;

        #[test]
        fn strips_trailing_slash() {
            assert_eq!(normalize_path(Path::new("/home/user/")), "/home/user");
        }

        #[test]
        fn preserves_root_slash() {
            assert_eq!(normalize_path(Path::new("/")), "/");
        }

        #[test]
        fn leaves_path_without_trailing_slash_unchanged() {
            assert_eq!(normalize_path(Path::new("/home/user")), "/home/user");
        }
    }

    mod merge_trust_model_tests {
        use super::*;

        #[test]
        fn creates_new_model_when_existing_is_none() {
            let result = merge_trust_model(None, &["/home/user/dev".to_string()])
                .expect("merge trust model");
            let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
            let entries = parsed["uriTrustInfo"].as_array().expect("array");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["uri"]["path"], "/home/user/dev");
        }

        #[test]
        fn does_not_duplicate_existing_entry() {
            let first = merge_trust_model(None, &["/home/user/dev".to_string()]).unwrap();
            let second = merge_trust_model(Some(&first), &["/home/user/dev".to_string()]).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&second).expect("valid json");
            let entries = parsed["uriTrustInfo"].as_array().expect("array");
            assert_eq!(entries.len(), 1, "duplicate should not be inserted");
        }

        #[test]
        fn adds_new_root_without_removing_existing() {
            let first = merge_trust_model(None, &["/home/user/dev".to_string()]).unwrap();
            let second = merge_trust_model(
                Some(&first),
                &["/home/user/dev".to_string(), "/mnt/repos".to_string()],
            )
            .unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&second).expect("valid json");
            let entries = parsed["uriTrustInfo"].as_array().expect("array");
            assert_eq!(entries.len(), 2);
        }

        #[test]
        fn handles_invalid_existing_json_as_empty() {
            let result = merge_trust_model(Some("not valid json"), &["/home/user".to_string()])
                .expect("merge trust model");
            let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
            let entries = parsed["uriTrustInfo"].as_array().expect("array");
            assert_eq!(entries.len(), 1);
        }

        #[test]
        fn empty_roots_list_leaves_uri_trust_info_empty() {
            let result = merge_trust_model(None, &[]).expect("merge trust model");
            let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
            let entries = parsed["uriTrustInfo"].as_array().expect("array");
            assert!(entries.is_empty());
        }

        #[test]
        fn trusted_entry_has_expected_fields() {
            let result = merge_trust_model(None, &["/mnt/work".to_string()]).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            let entry = &parsed["uriTrustInfo"][0];
            assert_eq!(entry["trusted"], true);
            assert_eq!(entry["uri"]["scheme"], "file");
            assert_eq!(entry["uri"]["path"], "/mnt/work");
            assert_eq!(entry["uri"]["fsPath"], "/mnt/work");
            assert_eq!(entry["uri"]["external"], "file:///mnt/work");
        }
    }

    #[test]
    fn seed_trusted_roots_creates_trust_model_for_empty_profile() {
        let base = env::temp_dir().join("task-vscodium-trust-empty");
        let _ = fs::remove_dir_all(&base);
        let user_data_dir = base.join("profile");

        seed_trusted_roots(
            &user_data_dir,
            &["/home/thomas/dev/wt/github.com/tsauvajon".into()],
        )
        .expect("seed trusted roots");

        let value = read_trust_model(&user_data_dir);
        assert!(value.contains("/home/thomas/dev/wt/github.com/tsauvajon"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn seed_trusted_roots_merges_without_duplicates() {
        let base = env::temp_dir().join("task-vscodium-trust-merge");
        let _ = fs::remove_dir_all(&base);
        let user_data_dir = base.join("profile");

        seed_trusted_roots(
            &user_data_dir,
            &["/home/thomas/dev/wt/github.com/tsauvajon".into()],
        )
        .expect("seed initial trusted root");

        seed_trusted_roots(
            &user_data_dir,
            &[
                "/home/thomas/dev/wt/github.com/tsauvajon".into(),
                "/mnt/linux/dev/repos/github.com/tsauvajon".into(),
            ],
        )
        .expect("seed additional trusted roots");

        let value = read_trust_model(&user_data_dir);
        let parsed: serde_json::Value = serde_json::from_str(&value).expect("parse trust model");
        let entries = parsed
            .get("uriTrustInfo")
            .and_then(serde_json::Value::as_array)
            .expect("uriTrustInfo array");

        let trusted_count = entries
            .iter()
            .filter_map(|entry| entry.get("uri"))
            .filter_map(|uri| uri.get("path"))
            .filter_map(serde_json::Value::as_str)
            .filter(|path| path.contains("tsauvajon"))
            .count();
        assert_eq!(trusted_count, 2);

        let _ = fs::remove_dir_all(&base);
    }
}
