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

impl RepoKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    #[must_use]
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::RepoKey;

    mod construction {
        use super::*;

        #[test]
        fn new_stores_the_string() {
            let k = RepoKey::new("github.com/owner/repo");
            assert_eq!(k.as_str(), "github.com/owner/repo");
        }

        #[test]
        fn from_str_ref_and_from_string_agree() {
            let from_str: RepoKey = "github.com/owner/repo".into();
            let from_string: RepoKey = "github.com/owner/repo".to_string().into();
            assert_eq!(from_str, from_string);
        }
    }

    mod trait_impls {
        use super::*;

        #[test]
        fn display_returns_inner_string() {
            let k = RepoKey::new("github.com/owner/repo");
            assert_eq!(k.to_string(), "github.com/owner/repo");
        }

        #[test]
        fn deref_allows_str_operations() {
            let k = RepoKey::new("github.com/owner/repo");
            assert!(k.contains('/'));
            assert_eq!(k.len(), "github.com/owner/repo".len());
        }

        #[test]
        fn as_ref_str_returns_inner_str() {
            let k = RepoKey::new("github.com/owner/repo");
            let s: &str = k.as_ref();
            assert_eq!(s, "github.com/owner/repo");
        }

        #[test]
        fn as_ref_path_converts_to_path() {
            let k = RepoKey::new("github.com/owner/repo");
            let p: &Path = k.as_ref();
            assert_eq!(p, Path::new("github.com/owner/repo"));
        }

        #[test]
        fn can_be_used_as_hash_map_key() {
            use std::collections::HashMap;
            let mut map: HashMap<RepoKey, u32> = HashMap::new();
            map.insert(RepoKey::new("github.com/owner/repo"), 42);
            assert_eq!(map.get("github.com/owner/repo"), Some(&42));
        }
    }

    mod conversions {
        use super::*;

        #[test]
        fn into_string_round_trips() {
            let k = RepoKey::new("github.com/owner/repo");
            let s: String = k.into();
            assert_eq!(s, "github.com/owner/repo");
        }

        #[test]
        fn into_path_buf_converts_correctly() {
            let k = RepoKey::new("github.com/owner/repo");
            let p: PathBuf = k.into();
            assert_eq!(p, PathBuf::from("github.com/owner/repo"));
        }
    }

    mod ordering {
        use super::*;

        #[test]
        fn ordering_is_lexicographic() {
            let a = RepoKey::new("github.com/a/repo");
            let b = RepoKey::new("github.com/b/repo");
            assert!(a < b);
            assert!(b > a);
            assert_eq!(a, RepoKey::new("github.com/a/repo"));
        }
    }
}
