use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Layout {
    repos_dir: PathBuf,
    wt_dir: PathBuf,
}

impl Layout {
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
