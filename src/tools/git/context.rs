use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use super::run::capture;
use crate::{
    error::{Error, Result},
    runtime::RepoKey,
};

pub fn current_root() -> Result<PathBuf> {
    let root = capture(&["rev-parse", "--show-toplevel"], None)?;
    Ok(PathBuf::from(root.trim()))
}

pub fn git_common_dir(root: &Path) -> Result<PathBuf> {
    let root_str = root.to_string_lossy();
    let common_dir_raw = capture(
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

    use super::{normalize_path, repo_key_from_common_dir};

    mod normalize_path {
        use super::*;

        #[test]
        fn returns_path_when_not_found() {
            let path = Path::new("/tmp/nonexistent-task-test-path-xyz-12345");
            let result = normalize_path(path).expect("normalize should not error for NotFound");
            assert_eq!(result, path);
        }
    }

    mod repo_key_from_common_dir {
        use super::*;
        use crate::runtime::RepoKey;

        #[test]
        fn extracts_key() {
            let key = repo_key_from_common_dir(
                Path::new("/tmp/custom/repos/github.com/tsauvajon/task.git"),
                Path::new("/tmp/custom/repos"),
            )
            .expect("resolve repo key");
            assert_eq!(key, Some(RepoKey::new("github.com/tsauvajon/task")));
        }

        #[test]
        fn returns_none_when_outside_repos_dir() {
            let key = repo_key_from_common_dir(
                Path::new("/other/path/github.com/tsauvajon/task.git"),
                Path::new("/tmp/custom/repos"),
            )
            .expect("resolve repo key");
            assert_eq!(key, None);
        }

        #[test]
        fn returns_none_when_paths_match_exactly() {
            // If common_dir == repos_dir the relative part is empty → None.
            let key = repo_key_from_common_dir(
                Path::new("/tmp/custom/repos"),
                Path::new("/tmp/custom/repos"),
            )
            .expect("resolve repo key");
            assert_eq!(key, None);
        }

        #[test]
        fn strips_dot_git_suffix() {
            let key = repo_key_from_common_dir(
                Path::new("/repos/github.com/acme/proj.git"),
                Path::new("/repos"),
            )
            .expect("resolve repo key");
            assert_eq!(key, Some(RepoKey::new("github.com/acme/proj")));
        }

        #[test]
        fn accepts_path_without_git_suffix() {
            let key = repo_key_from_common_dir(
                Path::new("/repos/github.com/acme/proj"),
                Path::new("/repos"),
            )
            .expect("resolve repo key");
            assert_eq!(key, Some(RepoKey::new("github.com/acme/proj")));
        }

        #[cfg(unix)]
        #[test]
        fn resolves_symlinked_repos_dir() {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time went backwards")
                .as_nanos();
            let base = std::env::temp_dir().join(format!("task-context-test-{unique}"));

            let real_repos_dir = base.join("real/repos");
            let repo_common_dir = real_repos_dir.join("github.com/tsauvajon/task.git");
            fs::create_dir_all(&repo_common_dir).expect("create real repos dir");

            let symlinked_repos_dir = base.join("linked/repos");
            fs::create_dir_all(
                symlinked_repos_dir
                    .parent()
                    .expect("symlinked repos has parent"),
            )
            .expect("create symlink parent dir");
            symlink(&real_repos_dir, &symlinked_repos_dir).expect("create repos symlink");

            let key = repo_key_from_common_dir(&repo_common_dir, &symlinked_repos_dir)
                .expect("resolve repo key with symlink");
            assert_eq!(key, Some(RepoKey::new("github.com/tsauvajon/task")));

            fs::remove_dir_all(base).expect("cleanup temp dirs");
        }
    }
}
