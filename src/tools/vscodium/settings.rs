use std::{fs, path::Path};

use crate::error::Result;

/// Seeds default VSCodium settings into a fresh user-data-dir profile.
///
/// Skips writing if `User/settings.json` already exists, so that user
/// customizations made through the VSCodium UI are preserved on subsequent
/// `task open` / `task start` calls.
///
/// The defaults disable automatic updates and telemetry because task-managed
/// VSCodium instances are installed through nix — self-updates would conflict
/// with the nix store (which is read-only) and produce confusing error dialogs
/// on every launch.
pub fn seed_default_settings(user_data_dir: &Path) -> Result<()> {
    let settings_path = user_data_dir.join("User/settings.json");
    if settings_path.exists() {
        return Ok(());
    }

    let defaults = serde_json::json!({
        "update.mode": "none",
        "update.showReleaseNotes": false,
        "extensions.autoCheckUpdates": false,
        "extensions.autoUpdate": false,
        "telemetry.telemetryLevel": "off",
    });

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&settings_path, serde_json::to_string_pretty(&defaults)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::seed_default_settings;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("task-vscodium-settings-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_settings_for_fresh_profile() {
        let dir = TempDir::new("fresh");

        seed_default_settings(dir.path()).expect("seed settings");

        let content = fs::read_to_string(dir.path().join("User/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["update.mode"], "none");
        assert_eq!(parsed["update.showReleaseNotes"], false);
        assert_eq!(parsed["extensions.autoCheckUpdates"], false);
        assert_eq!(parsed["extensions.autoUpdate"], false);
        assert_eq!(parsed["telemetry.telemetryLevel"], "off");
    }

    #[test]
    fn skips_when_settings_already_exist() {
        let dir = TempDir::new("existing");
        let settings_path = dir.path().join("User/settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, r#"{"editor.fontSize": 16}"#).unwrap();

        seed_default_settings(dir.path()).expect("seed settings");

        let content = fs::read_to_string(&settings_path).unwrap();
        assert!(
            content.contains("editor.fontSize"),
            "existing settings should be preserved"
        );
        assert!(
            !content.contains("update.mode"),
            "defaults should not be injected"
        );
    }

    #[test]
    fn creates_parent_directories() {
        let dir = TempDir::new("no-user-dir");
        // Don't pre-create User/ — seed_default_settings should handle it.

        seed_default_settings(dir.path()).expect("seed settings");

        assert!(dir.path().join("User/settings.json").is_file());
    }
}
