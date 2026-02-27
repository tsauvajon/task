use std::{
    borrow::Borrow,
    fmt,
    ops::Deref,
    path::{Path, PathBuf},
};

/// A git branch name, e.g. `"feat/login"` or `"main"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BranchName(String);

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::BranchName;

    mod construction {
        use super::*;

        #[test]
        fn new_stores_the_string() {
            let b = BranchName::new("feat/login");
            assert_eq!(b.as_str(), "feat/login");
        }

        #[test]
        fn from_str_ref_and_from_string_agree() {
            let from_str: BranchName = "feat/login".into();
            let from_string: BranchName = "feat/login".to_string().into();
            assert_eq!(from_str, from_string);
        }
    }

    mod trait_impls {
        use super::*;

        #[test]
        fn display_returns_inner_string() {
            let b = BranchName::new("main");
            assert_eq!(b.to_string(), "main");
        }

        #[test]
        fn deref_allows_str_operations() {
            let b = BranchName::new("feat/login");
            assert!(b.contains('/'));
            assert_eq!(b.len(), "feat/login".len());
        }

        #[test]
        fn as_ref_str_returns_inner_str() {
            let b = BranchName::new("bump-deps");
            let s: &str = b.as_ref();
            assert_eq!(s, "bump-deps");
        }

        #[test]
        fn as_ref_path_converts_to_path() {
            let b = BranchName::new("feat/login");
            let p: &Path = b.as_ref();
            assert_eq!(p, Path::new("feat/login"));
        }

        #[test]
        fn can_be_used_as_hash_map_key() {
            use std::collections::HashMap;
            let mut map: HashMap<BranchName, u32> = HashMap::new();
            map.insert(BranchName::new("main"), 1);
            assert_eq!(map.get("main"), Some(&1));
        }
    }

    mod conversions {
        use super::*;

        #[test]
        fn into_string_round_trips() {
            let b = BranchName::new("bump-deps");
            let s: String = b.into();
            assert_eq!(s, "bump-deps");
        }

        #[test]
        fn into_path_buf_converts_correctly() {
            let b = BranchName::new("feat/login");
            let p: PathBuf = b.into();
            assert_eq!(p, PathBuf::from("feat/login"));
        }
    }

    mod ordering {
        use super::*;

        #[test]
        fn ordering_is_lexicographic() {
            let a = BranchName::new("a");
            let b = BranchName::new("b");
            assert!(a < b);
            assert!(b > a);
            assert_eq!(a, BranchName::new("a"));
        }
    }
}
