use std::{
    borrow::Borrow,
    fmt,
    ops::Deref,
    path::{Path, PathBuf},
};

/// A normalized repository key, e.g. `"github.com/owner/repo"`.
///
/// The key is the canonical identifier for a bare git repo inside the
/// workspace's `repos_dir`.  It never contains a leading `/` or a `.git`
/// suffix — use [`crate::runtime::paths::WorkspacePaths`] to obtain the
/// actual filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoKey(String);

/// A git branch name, e.g. `"feat/login"` or `"main"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BranchName(String);

// ── RepoKey ──────────────────────────────────────────────────────────────────

impl RepoKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for RepoKey {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RepoKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for RepoKey {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Borrow<str> for RepoKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for RepoKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RepoKey {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<RepoKey> for String {
    fn from(k: RepoKey) -> Self {
        k.0
    }
}

impl From<RepoKey> for PathBuf {
    fn from(k: RepoKey) -> Self {
        PathBuf::from(k.0)
    }
}

// ── BranchName ───────────────────────────────────────────────────────────────

impl BranchName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for BranchName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for BranchName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for BranchName {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Borrow<str> for BranchName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for BranchName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for BranchName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<BranchName> for String {
    fn from(b: BranchName) -> Self {
        b.0
    }
}

impl From<BranchName> for PathBuf {
    fn from(b: BranchName) -> Self {
        PathBuf::from(b.0)
    }
}
