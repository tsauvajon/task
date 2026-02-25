use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use super::runner::run_git_capture;
use crate::{
    error::{Error, Result},
    types::RepoKey,
};

pub fn current_root() -> Result<PathBuf> {
    let root = run_git_capture(&["rev-parse", "--show-toplevel"], None)?;
    Ok(PathBuf::from(root.trim()))
}

pub fn git_common_dir(root: &Path) -> Result<PathBuf> {
    let root_str = root.to_string_lossy();
    let common_dir_raw = run_git_capture(
        &["-C", root_str.as_ref(), "rev-parse", "--git-common-dir"],
        None,
    )?;

    let mut common_dir = PathBuf::from(common_dir_raw.trim());
    if common_dir.is_relative() {
        common_dir = root.join(common_dir);
    }
    fs::canonicalize(common_dir).map_err(Error::from)
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(err) => Err(Error::from(err)),
    }
}

pub fn repo_key_from_common_dir(common_dir: &Path, repos_dir: &Path) -> Result<Option<RepoKey>> {
    let normalized_common_dir = normalize_path(common_dir)?;
    let normalized_repos_dir = normalize_path(repos_dir)?;

    let Ok(relative) = normalized_common_dir.strip_prefix(&normalized_repos_dir) else {
        return Ok(None);
    };

    let key = relative.to_string_lossy();
    let key = key.strip_suffix(".git").unwrap_or(&key);
    if key.is_empty() {
        return Ok(None);
    }
    Ok(Some(RepoKey::new(key)))
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::repo_key_from_common_dir;

    #[test]
    fn repo_key_from_common_dir_extracts_key() {
        use crate::types::RepoKey;
        let key = repo_key_from_common_dir(
            Path::new("/tmp/custom/repos/github.com/tsauvajon/task.git"),
            Path::new("/tmp/custom/repos"),
        )
        .expect("resolve repo key");
        assert_eq!(key, Some(RepoKey::new("github.com/tsauvajon/task")));
    }

    #[cfg(unix)]
    #[test]
    fn repo_key_from_common_dir_resolves_symlinked_repos_dir() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("task-context-test-{unique}"));

        let real_repos_dir = base.join("real").join("repos");
        let repo_common_dir = real_repos_dir
            .join("github.com")
            .join("tsauvajon")
            .join("task.git");
        fs::create_dir_all(&repo_common_dir).expect("create real repos dir");

        let symlinked_repos_dir = base.join("linked").join("repos");
        fs::create_dir_all(
            symlinked_repos_dir
                .parent()
                .expect("symlinked repos has parent"),
        )
        .expect("create symlink parent dir");
        symlink(&real_repos_dir, &symlinked_repos_dir).expect("create repos symlink");

        use crate::types::RepoKey;
        let key = repo_key_from_common_dir(&repo_common_dir, &symlinked_repos_dir)
            .expect("resolve repo key with symlink");
        assert_eq!(key, Some(RepoKey::new("github.com/tsauvajon/task")));

        fs::remove_dir_all(base).expect("cleanup temp dirs");
    }
}
