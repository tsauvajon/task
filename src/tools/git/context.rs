use std::fs;
use std::path::{Path, PathBuf};

use super::runner::run_git_capture;

pub fn current_root() -> Result<PathBuf, String> {
    let root = run_git_capture(&["rev-parse", "--show-toplevel"], None)?;
    Ok(PathBuf::from(root.trim()))
}

pub fn git_common_dir(root: &Path) -> Result<PathBuf, String> {
    let common_dir_raw = run_git_capture(
        &[
            "-C",
            root.to_string_lossy().as_ref(),
            "rev-parse",
            "--git-common-dir",
        ],
        None,
    )?;

    let mut common_dir = PathBuf::from(common_dir_raw.trim());
    if common_dir.is_relative() {
        common_dir = root.join(common_dir);
    }
    fs::canonicalize(common_dir).map_err(|e| e.to_string())
}

pub fn repo_key_from_common_dir(common_dir: &Path, repos_dir: &Path) -> Option<String> {
    let relative = common_dir.strip_prefix(repos_dir).ok()?;
    let mut key = relative.to_string_lossy().to_string();
    if key.ends_with(".git") {
        key.truncate(key.len() - 4);
    }
    if key.is_empty() {
        return None;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::repo_key_from_common_dir;

    #[test]
    fn repo_key_from_common_dir_extracts_key() {
        let key = repo_key_from_common_dir(
            Path::new("/tmp/custom/repos/github.com/tsauvajon/task.git"),
            Path::new("/tmp/custom/repos"),
        );
        assert_eq!(key, Some("github.com/tsauvajon/task".to_string()));
    }
}
