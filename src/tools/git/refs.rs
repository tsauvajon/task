use std::path::Path;

use super::{gitdir::GitDir, run::capture};
use crate::error::Result;

const ORIGIN_FETCH_REFSPEC: &str = "+refs/heads/*:refs/remotes/origin/*";

/// Parse the branch name from the output of `git ls-remote --symref origin HEAD`.
///
/// Returns the branch name (e.g. `"main"`) if a symref line like
/// `ref: refs/heads/main HEAD` is found, otherwise `None`.
pub(crate) fn parse_ls_remote_branch(output: &str) -> Option<&str> {
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("ref: ") {
            // `git ls-remote --symref` output uses tab-separated fields:
            //   ref: refs/heads/main\tHEAD
            // Split on whitespace to handle both tab and space separators.
            let target = rest.split_whitespace().next()?;
            if let Some(branch) = target.strip_prefix("refs/heads/") {
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

    // Fallback: try common default branch names when ls-remote is unavailable.
    for fallback in ["origin/main", "origin/master"] {
        if rev_exists(gitdir, fallback) {
            return fallback.to_string();
        }
    }

    "HEAD".to_string()
}

pub fn fetch_origin_refs(gitdir: &Path) -> Result<()> {
    // Ensure the bare repo has the correct fetch refspec so that plain
    // `git fetch` also works (repairs repos cloned before this fix).
    ensure_origin_fetch_refspec(gitdir)?;

    GitDir::new(gitdir).status(&["fetch", "origin", "--prune", ORIGIN_FETCH_REFSPEC])
}

/// Set `remote.origin.fetch` to the standard non-bare refspec.
///
/// Bare clones map into `refs/heads/*` by default, which means a plain
/// `git fetch origin` inside a linked worktree updates nothing under
/// `refs/remotes/origin/*`. This one-liner fixes that. The call is
/// idempotent — `git config` replaces the existing value.
pub fn ensure_origin_fetch_refspec(gitdir: &Path) -> Result<()> {
    GitDir::new(gitdir).status(&["config", "remote.origin.fetch", ORIGIN_FETCH_REFSPEC])
}

pub fn set_branch_upstream(gitdir: &Path, branch: &str, remote: &str) -> Result<()> {
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    let merge_ref = format!("refs/heads/{branch}");

    let gitdir = GitDir::new(gitdir);
    gitdir.status(&["config", &remote_key, remote])?;
    gitdir.status(&["config", &merge_key, &merge_ref])
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
    use std::{env, fs, process::Command};

    use super::{current_branch, parse_ls_remote_branch, ref_exists, rev_exists};

    /// Create a temporary bare git repository, isolated from user config.
    fn make_bare_repo(name: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("task-rs-refs-bare-{name}.git"));
        let _ = fs::remove_dir_all(&dir);
        let status = Command::new("git")
            .args(["init", "--bare", dir.to_str().expect("valid utf-8")])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git must be available");
        assert!(status.success(), "git init --bare failed");
        dir
    }

    /// Create a regular git repo with an initial commit on `main` and return
    /// its path (the working tree root, which is also the `.git` parent).
    ///
    /// Git subprocesses are isolated from the user's global config to avoid
    /// races with parallel tests that mutate `HOME`.
    fn make_regular_repo_with_commit(name: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("task-rs-refs-regular-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create dir");

        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", &dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git must be available");
            assert!(status.success(), "git {args:?} failed");
        };

        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        // Create an initial commit so HEAD points to a real branch
        fs::write(dir.join("README.md"), "hello").expect("write README");
        run(&["add", "."]);
        run(&["commit", "-m", "initial"]);
        dir
    }

    mod ref_exists_tests {
        use super::*;

        #[test]
        fn returns_false_for_nonexistent_ref() {
            let dir = make_bare_repo("ref-exists-false");
            let exists = ref_exists(&dir, "refs/heads/nonexistent-branch-xyz");
            assert!(!exists, "nonexistent ref should return false");
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn returns_false_on_nonexistent_gitdir() {
            let dir = env::temp_dir().join("task-rs-refs-nonexistent-gitdir.git");
            let _ = fs::remove_dir_all(&dir);
            let exists = ref_exists(&dir, "refs/heads/main");
            assert!(!exists, "nonexistent gitdir should return false");
        }

        #[test]
        fn returns_true_for_existing_branch_in_regular_repo() {
            let repo = make_regular_repo_with_commit("ref-exists-true");
            let gitdir = repo.join(".git");
            let exists = ref_exists(&gitdir, "refs/heads/main");
            assert!(exists, "main branch should exist after initial commit");
            let _ = fs::remove_dir_all(&repo);
        }
    }

    mod rev_exists_tests {
        use super::*;

        #[test]
        fn returns_false_for_nonexistent_revision() {
            let dir = make_bare_repo("rev-exists-false");
            let exists = rev_exists(&dir, "nonexistent-branch-xyz");
            assert!(!exists, "nonexistent revision should return false");
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn returns_false_on_nonexistent_gitdir() {
            let dir = env::temp_dir().join("task-rs-refs-nonexistent-rev-gitdir.git");
            let _ = fs::remove_dir_all(&dir);
            let exists = rev_exists(&dir, "HEAD");
            assert!(!exists, "nonexistent gitdir should return false");
        }

        #[test]
        fn returns_true_for_head_in_repo_with_commit() {
            let repo = make_regular_repo_with_commit("rev-exists-head");
            let gitdir = repo.join(".git");
            assert!(
                rev_exists(&gitdir, "HEAD"),
                "HEAD should exist after commit"
            );
            let _ = fs::remove_dir_all(&repo);
        }

        #[test]
        fn returns_true_for_branch_name_in_repo_with_commit() {
            let repo = make_regular_repo_with_commit("rev-exists-branch");
            let gitdir = repo.join(".git");
            assert!(
                rev_exists(&gitdir, "main"),
                "main should resolve to a commit"
            );
            let _ = fs::remove_dir_all(&repo);
        }
    }

    mod current_branch_tests {
        use super::*;

        #[test]
        fn returns_branch_for_regular_repo_with_commit() {
            let dir = make_regular_repo_with_commit("current-branch-main");
            let branch = current_branch(&dir);
            assert_eq!(
                branch.as_deref(),
                Some("main"),
                "expected 'main' branch, got {branch:?}"
            );
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn returns_none_for_bare_repo() {
            // Bare repos have no working tree and HEAD is symbolic, but
            // `git symbolic-ref --quiet --short HEAD` returns a non-zero exit
            // in a bare repo when there are no commits (detached or unborn).
            // Depending on git defaults, an unborn bare repo can report
            // "master", "main", or nothing; all are acceptable.
            let dir = make_bare_repo("current-branch-bare");
            let branch = current_branch(&dir);
            assert!(matches!(
                branch.as_deref(),
                None | Some("main") | Some("master")
            ));
            let _ = fs::remove_dir_all(&dir);
        }
    }

    mod parse_ls_remote_branch {
        use super::*;

        #[test]
        fn returns_branch_for_main() {
            // Real git output uses tab-separated fields.
            let output = "ref: refs/heads/main\tHEAD\nabc123\trefs/heads/main\n";
            assert_eq!(parse_ls_remote_branch(output), Some("main"));
        }

        #[test]
        fn returns_branch_for_master() {
            let output = "ref: refs/heads/master\tHEAD\nabc123\trefs/heads/master\n";
            assert_eq!(parse_ls_remote_branch(output), Some("master"));
        }

        #[test]
        fn returns_branch_for_nested_name() {
            let output = "ref: refs/heads/feature/my-thing\tHEAD\n";
            assert_eq!(parse_ls_remote_branch(output), Some("feature/my-thing"));
        }

        #[test]
        fn handles_space_separated_output() {
            // Also accept space-separated format for robustness.
            let output = "ref: refs/heads/main HEAD\nabc123\trefs/heads/main\n";
            assert_eq!(parse_ls_remote_branch(output), Some("main"));
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
            let output = "ref: refs/tags/v1.0\tHEAD\n";
            assert_eq!(parse_ls_remote_branch(output), None);
        }

        #[test]
        fn ignores_lines_before_symref() {
            let output = "some-other-line\nref: refs/heads/develop\tHEAD\nabc123\n";
            assert_eq!(parse_ls_remote_branch(output), Some("develop"));
        }

        #[test]
        fn returns_first_matching_line() {
            let output = "ref: refs/heads/first\tHEAD\nref: refs/heads/second\tHEAD\n";
            assert_eq!(parse_ls_remote_branch(output), Some("first"));
        }
    }
}
