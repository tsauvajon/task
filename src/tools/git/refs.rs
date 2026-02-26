use std::path::Path;

use super::{gitdir::GitDir, run::capture};
use crate::error::Result;

/// Parse the branch name from the output of `git ls-remote --symref origin HEAD`.
///
/// Returns the branch name (e.g. `"main"`) if a symref line like
/// `ref: refs/heads/main HEAD` is found, otherwise `None`.
pub(crate) fn parse_ls_remote_branch(output: &str) -> Option<&str> {
    for line in output.lines() {
        if let Some(target) = line.strip_prefix("ref: ") {
            let target = target.trim();
            if let Some(target) = target.strip_suffix(" HEAD")
                && let Some(branch) = target.strip_prefix("refs/heads/")
            {
                return Some(branch);
            }
        }
    }
    None
}

pub fn detect_default_base(gitdir: &Path) -> String {
    let gd = GitDir::new(gitdir);
    if let Ok(output) = gd.capture(&["ls-remote", "--symref", "origin", "HEAD"])
        && let Some(branch) = parse_ls_remote_branch(&output)
    {
        let remote_branch = format!("origin/{branch}");
        if rev_exists(gitdir, &remote_branch) {
            return remote_branch;
        }
        if rev_exists(gitdir, branch) {
            return branch.to_string();
        }
    }

    if gd
        .status(&[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/remotes/origin/master",
        ])
        .is_ok()
    {
        return "origin/master".to_string();
    }

    "HEAD".to_string()
}

pub fn fetch_origin_refs(gitdir: &Path) -> Result<()> {
    GitDir::new(gitdir).status(&[
        "fetch",
        "origin",
        "--prune",
        "+refs/heads/*:refs/remotes/origin/*",
    ])
}

pub fn ref_exists(gitdir: &Path, reference: &str) -> bool {
    GitDir::new(gitdir)
        .status(&["show-ref", "--verify", "--quiet", reference])
        .is_ok()
}

pub fn rev_exists(gitdir: &Path, revision: &str) -> bool {
    let value = format!("{revision}^{{commit}}");
    GitDir::new(gitdir)
        .status(&["rev-parse", "--verify", "--quiet", &value])
        .is_ok()
}

pub fn current_branch(root: &Path) -> Option<String> {
    let root_str = root.to_string_lossy();
    capture(
        &[
            "-C",
            root_str.as_ref(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ],
        None,
    )
    .ok()
    .map(|s| s.trim().to_owned())
    .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::parse_ls_remote_branch;

    mod parse_ls_remote_branch {
        use super::*;

        #[test]
        fn returns_branch_for_main() {
            // Supply the space-separated form that the function is designed to parse.
            let output = "ref: refs/heads/main HEAD\nabc123\trefs/heads/main\n";
            assert_eq!(parse_ls_remote_branch(output), Some("main"));
        }

        #[test]
        fn returns_branch_for_master() {
            let output = "ref: refs/heads/master HEAD\nabc123\trefs/heads/master\n";
            assert_eq!(parse_ls_remote_branch(output), Some("master"));
        }

        #[test]
        fn returns_branch_for_nested_name() {
            let output = "ref: refs/heads/feature/my-thing HEAD\n";
            assert_eq!(parse_ls_remote_branch(output), Some("feature/my-thing"));
        }

        #[test]
        fn returns_none_for_empty_output() {
            assert_eq!(parse_ls_remote_branch(""), None);
        }

        #[test]
        fn returns_none_when_no_symref_line() {
            let output = "abc123\trefs/heads/main\n";
            assert_eq!(parse_ls_remote_branch(output), None);
        }

        #[test]
        fn returns_none_for_non_heads_ref() {
            // A tag symref – should not match refs/heads/ prefix
            let output = "ref: refs/tags/v1.0 HEAD\n";
            assert_eq!(parse_ls_remote_branch(output), None);
        }

        #[test]
        fn ignores_lines_before_symref() {
            let output = "some-other-line\nref: refs/heads/develop HEAD\nabc123\n";
            assert_eq!(parse_ls_remote_branch(output), Some("develop"));
        }

        #[test]
        fn returns_first_matching_line() {
            let output = "ref: refs/heads/first HEAD\nref: refs/heads/second HEAD\n";
            assert_eq!(parse_ls_remote_branch(output), Some("first"));
        }
    }
}
