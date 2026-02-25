use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    runtime::config::config_dir_path,
};

#[derive(Debug, Serialize, Deserialize, Default)]
struct TaskStateFile {
    onboarding_complete: bool,
}

pub fn onboarding_complete() -> Result<bool> {
    let path = state_file_path()?;
    onboarding_complete_at(&path)
}

pub fn mark_onboarding_complete() -> Result<()> {
    let path = state_file_path()?;
    mark_onboarding_complete_at(&path)
}

fn onboarding_complete_at(path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }

    let text = fs::read_to_string(path).map_err(|err| {
        Error::failed(format!(
            "Could not read state file {}: {err}",
            path.display()
        ))
    })?;
    let parsed = toml::from_str::<TaskStateFile>(&text).map_err(|err| {
        Error::failed(format!(
            "Could not parse state file {}: {err}",
            path.display()
        ))
    })?;
    Ok(parsed.onboarding_complete)
}

fn mark_onboarding_complete_at(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::failed(format!(
            "Could not resolve parent directory for {}",
            path.display()
        ))
    })?;

    fs::create_dir_all(parent)
        .map_err(|err| Error::failed(format!("Could not create {}: {err}", parent.display())))?;

    let text = toml::to_string_pretty(&TaskStateFile {
        onboarding_complete: true,
    })?;
    fs::write(path, text).map_err(|err| {
        Error::failed(format!(
            "Could not write state file {}: {err}",
            path.display()
        ))
    })?;

    Ok(())
}

fn state_file_path() -> Result<PathBuf> {
    Ok(config_dir_path()?.join("state.toml"))
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{mark_onboarding_complete_at, onboarding_complete_at};

    #[test]
    fn onboarding_state_defaults_to_false_without_file() {
        let path = env::temp_dir().join("task-rs-state-missing.toml");
        let _ = fs::remove_file(&path);

        assert!(!onboarding_complete_at(&path).expect("load onboarding state"));
    }

    #[test]
    fn onboarding_state_round_trip() {
        let dir = env::temp_dir().join("task-rs-state-round-trip");
        let path = dir.join("state.toml");
        let _ = fs::remove_dir_all(&dir);

        mark_onboarding_complete_at(&path).expect("mark onboarding complete");
        assert!(onboarding_complete_at(&path).expect("load onboarding state"));

        let _ = fs::remove_dir_all(&dir);
    }
}
