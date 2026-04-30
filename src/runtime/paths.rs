use std::path::{Path, PathBuf};

use crate::runtime::{BranchName, RepoKey};

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
    repos_dir: PathBuf,
    wt_dir: PathBuf,
    detached_dir: PathBuf,
}

impl WorkspacePaths {
    pub fn new(
        repos_dir: impl AsRef<Path>,
        wt_dir: impl AsRef<Path>,
        detached_dir: impl AsRef<Path>,
    ) -> Self {
        let repos_dir = repos_dir.as_ref().to_path_buf();
        let wt_dir = wt_dir.as_ref().to_path_buf();
        let detached_dir = detached_dir.as_ref().to_path_buf();
        Self {
            repos_dir,
            wt_dir,
            detached_dir,
        }
    }

    #[must_use]
    pub fn repos_dir(&self) -> &Path {
        &self.repos_dir
    }

    #[must_use]
    pub fn wt_dir(&self) -> &Path {
        &self.wt_dir
    }

    #[must_use]
    pub fn detached_dir(&self) -> &Path {
        &self.detached_dir
    }

    #[must_use]
    pub fn repo_gitdir_path(&self, repo_key: &RepoKey) -> PathBuf {
        self.repos_dir.join(format!("{repo_key}.git"))
    }

    #[must_use]
    pub fn worktree_path(&self, repo_key: &RepoKey, branch: &BranchName) -> PathBuf {
        self.wt_dir.join(repo_key.as_str()).join(branch.as_str())
    }

    #[must_use]
    pub fn detached_path(&self, repo_key: &RepoKey) -> PathBuf {
        self.detached_dir.join(repo_key.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspacePaths;
    use crate::runtime::{branch_name::BranchName, repo_key::RepoKey};

    mod workspace_paths {
        use super::*;

        #[test]
        fn new_accepts_pathbuf_and_path_ref() {
            let repos_dir = std::path::PathBuf::from("/repos");
            let wt_dir = std::path::PathBuf::from("/wt");
            let detached_dir = std::path::PathBuf::from("/detached");
            let layout = WorkspacePaths::new(&repos_dir, &wt_dir, &detached_dir);
            assert_eq!(layout.repos_dir(), repos_dir.as_path());
            assert_eq!(layout.wt_dir(), wt_dir.as_path());
            assert_eq!(layout.detached_dir(), detached_dir.as_path());
        }

        #[test]
        fn repos_dir_accessor_returns_correct_path() {
            let repos_dir = std::path::Path::new("/tmp/my-repos");
            let wt_dir = std::path::Path::new("/tmp/my-wt");
            let layout = WorkspacePaths::new(repos_dir, wt_dir, "/tmp/my-detached");
            assert_eq!(layout.repos_dir(), repos_dir);
        }

        #[test]
        fn wt_dir_accessor_returns_correct_path() {
            let repos_dir = std::path::Path::new("/tmp/my-repos");
            let wt_dir = std::path::Path::new("/tmp/my-wt");
            let layout = WorkspacePaths::new(repos_dir, wt_dir, "/tmp/my-detached");
            assert_eq!(layout.wt_dir(), wt_dir);
        }

        #[test]
        fn detached_dir_accessor_returns_correct_path() {
            let detached_dir = std::path::Path::new("/tmp/my-detached");
            let layout = WorkspacePaths::new("/tmp/my-repos", "/tmp/my-wt", detached_dir);
            assert_eq!(layout.detached_dir(), detached_dir);
        }

        #[test]
        fn clone_preserves_all_fields() {
            let layout = WorkspacePaths::new("/a", "/b", "/c");
            let cloned = layout.clone();
            assert_eq!(cloned.repos_dir(), layout.repos_dir());
            assert_eq!(cloned.wt_dir(), layout.wt_dir());
            assert_eq!(cloned.detached_dir(), layout.detached_dir());
        }

        #[test]
        fn repo_gitdir_path_appends_git_suffix() {
            let layout = WorkspacePaths::new("/repos", "/wt", "/detached");
            let key = RepoKey::new("github.com/org/name");
            let gitdir = layout.repo_gitdir_path(&key);
            assert!(gitdir.to_string_lossy().ends_with(".git"));
            assert_eq!(
                gitdir,
                std::path::PathBuf::from("/repos/github.com/org/name.git")
            );
        }

        #[test]
        fn worktree_path_combines_repo_and_branch() {
            let layout = WorkspacePaths::new("/wts", "/wts", "/detached");
            let key = RepoKey::new("gitlab.com/team/project");
            let branch = BranchName::new("feat/new-thing");
            let wt = layout.worktree_path(&key, &branch);
            assert_eq!(
                wt,
                std::path::PathBuf::from("/wts/gitlab.com/team/project/feat/new-thing")
            );
        }

        #[test]
        fn detached_path_uses_repo_key_without_branch() {
            let layout = WorkspacePaths::new("/repos", "/wt", "/detached");
            let key = RepoKey::new("github.com/org/name");
            let det = layout.detached_path(&key);
            assert_eq!(
                det,
                std::path::PathBuf::from("/detached/github.com/org/name")
            );
        }

        #[test]
        fn build_expected_paths() {
            let repos_dir = std::env::temp_dir().join("task-tests-repos");
            let wt_dir = std::env::temp_dir().join("task-tests-wt");
            let detached_dir = std::env::temp_dir().join("task-tests-detached");
            let layout = WorkspacePaths::new(&repos_dir, &wt_dir, &detached_dir);
            let repo_key = RepoKey::new("github.com/tsauvajon/goto");
            let branch = BranchName::new("bump-deps");
            assert_eq!(
                layout.repo_gitdir_path(&repo_key),
                repos_dir.join("github.com/tsauvajon/goto.git")
            );
            assert_eq!(
                layout.worktree_path(&repo_key, &branch),
                wt_dir.join("github.com/tsauvajon/goto/bump-deps")
            );
            assert_eq!(
                layout.detached_path(&repo_key),
                detached_dir.join("github.com/tsauvajon/goto")
            );
        }
    }
}
