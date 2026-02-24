use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

const TRUST_MODEL_KEY: &str = "content.trust.model.key";

/// Seeds trusted workspace roots into a task-specific VSCodium profile.
///
/// VSCodium stores workspace trust in `<user-data-dir>/User/globalStorage/state.vscdb`
/// under the `content.trust.model.key` row of `ItemTable`.
///
/// We merge configured trusted roots into that JSON document before opening the editor
/// so task worktrees under those roots open as trusted without disabling workspace trust.
pub fn seed_trusted_roots(user_data_dir: &Path, trusted_roots: &[PathBuf]) -> Result<(), String> {
    let mut normalized_roots = Vec::new();
    for root in trusted_roots {
        normalized_roots.push(normalize_path(root));
    }

    if normalized_roots.is_empty() {
        return Ok(());
    }

    let global_storage_dir = user_data_dir.join("User").join("globalStorage");
    fs::create_dir_all(&global_storage_dir).map_err(|error| error.to_string())?;
    let db_path = global_storage_dir.join("state.vscdb");

    let connection = Connection::open(&db_path).map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
            [],
        )
        .map_err(|error| error.to_string())?;

    let existing: Option<String> = connection
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            params![TRUST_MODEL_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;

    let merged = merge_trust_model(existing.as_deref(), &normalized_roots)?;
    connection
        .execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            params![TRUST_MODEL_KEY, merged],
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn merge_trust_model(existing: Option<&str>, trusted_roots: &[String]) -> Result<String, String> {
    let mut model = existing
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));

    if !model.get("uriTrustInfo").is_some_and(Value::is_array) {
        model["uriTrustInfo"] = Value::Array(Vec::new());
    }

    let entries = model["uriTrustInfo"]
        .as_array_mut()
        .ok_or_else(|| "Could not initialize VSCodium trust model".to_string())?;

    let mut known_paths = HashSet::new();
    for entry in entries.iter() {
        if !entry
            .get("trusted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }

        let Some(uri) = entry.get("uri") else {
            continue;
        };

        let path = uri
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| uri.get("fsPath").and_then(Value::as_str));
        if let Some(path) = path {
            known_paths.insert(normalize_path(Path::new(path)));
        }
    }

    for root in trusted_roots {
        if known_paths.contains(root) {
            continue;
        }
        entries.push(trusted_entry(root));
        known_paths.insert(root.clone());
    }

    serde_json::to_string(&model).map_err(|error| error.to_string())
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
    use std::{env, fs};

    use rusqlite::Connection;

    use super::seed_trusted_roots;

    fn read_trust_model(path: &std::path::Path) -> String {
        let db_path = path.join("User").join("globalStorage").join("state.vscdb");
        let connection = Connection::open(db_path).expect("open sqlite");
        connection
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'content.trust.model.key'",
                [],
                |row| row.get(0),
            )
            .expect("read trust model")
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
