use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
    repos_dir: PathBuf,
    wt_dir: PathBuf,
}

impl WorkspacePaths {
    pub fn new(repos_dir: impl AsRef<Path>, wt_dir: impl AsRef<Path>) -> Self {
        let repos_dir = repos_dir.as_ref().to_path_buf();
        let wt_dir = wt_dir.as_ref().to_path_buf();
        Self { repos_dir, wt_dir }
    }

    pub fn repos_dir(&self) -> &Path {
        &self.repos_dir
    }

    pub fn wt_dir(&self) -> &Path {
        &self.wt_dir
    }

    pub fn repo_gitdir_path(&self, repo_key: &str) -> PathBuf {
        self.repos_dir.join(format!("{repo_key}.git"))
    }

    pub fn worktree_path(&self, repo_key: &str, branch: &str) -> PathBuf {
        self.wt_dir.join(repo_key).join(branch)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspacePaths;

    #[test]
    fn workspace_paths_build_expected_paths() {
        let repos_dir = std::env::temp_dir().join("task-tests-repos");
        let wt_dir = std::env::temp_dir().join("task-tests-wt");
        let layout = WorkspacePaths::new(&repos_dir, &wt_dir);
        assert_eq!(
            layout.repo_gitdir_path("github.com/tsauvajon/goto"),
            repos_dir.join("github.com/tsauvajon/goto.git")
        );
        assert_eq!(
            layout.worktree_path("github.com/tsauvajon/goto", "bump-deps"),
            wt_dir.join("github.com/tsauvajon/goto/bump-deps")
        );
    }
}
