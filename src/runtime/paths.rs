use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
    repos_dir: PathBuf,
    wt_dir: PathBuf,
}

impl WorkspacePaths {
    pub fn new(dev_root: impl AsRef<Path>) -> Self {
        let dev_root = dev_root.as_ref().to_path_buf();
        let repos_dir = dev_root.join("repos");
        let wt_dir = dev_root.join("wt");
        Self { repos_dir, wt_dir }
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
        let root = std::env::temp_dir().join("task-tests-dev-root");
        let layout = WorkspacePaths::new(&root);
        assert_eq!(
            layout.repo_gitdir_path("github.com/tsauvajon/goto"),
            root.join("repos/github.com/tsauvajon/goto.git")
        );
        assert_eq!(
            layout.worktree_path("github.com/tsauvajon/goto", "bump-deps"),
            root.join("wt/github.com/tsauvajon/goto/bump-deps")
        );
    }
}
