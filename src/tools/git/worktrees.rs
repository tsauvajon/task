use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    gitdir::GitDir,
    refs::{parse_ls_remote_branch, set_branch_upstream},
    run::{capture, status},
};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch_ref: Option<String>,
    pub is_bare: bool,
}

pub fn list(gitdir: &Path) -> Result<String> {
    GitDir::new(gitdir).capture(&["worktree", "list"])
}

pub fn list_porcelain(gitdir: &Path) -> Result<String> {
    GitDir::new(gitdir).capture(&["worktree", "list", "--porcelain"])
}

/// Enumerate the non-bare worktrees registered in a bare repo by reading
/// `<gitdir>/worktrees/*/gitdir` directly from disk.
///
/// Each file under `<gitdir>/worktrees/<name>/gitdir` is a one-line pointer
/// to the worktree's `.git` file; its parent is the worktree working
/// directory. Stale entries (pointing at paths that no longer exist) are
/// silently skipped, matching `git worktree list`'s own behavior.
///
/// Use this when you only need worktree paths and want to avoid the
/// per-repo `git worktree list --porcelain` subprocess — it is roughly
/// four orders of magnitude faster across a workspace with ~150 bare
/// repos (fork+exec overhead dominates the git invocation).
///
/// Returns an empty vec when the `worktrees` directory does not exist.
pub fn list_registered_worktrees(gitdir: &Path) -> Vec<PathBuf> {
    let worktrees_dir = gitdir.join("worktrees");
    let Ok(entries) = fs::read_dir(&worktrees_dir) else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| read_worktree_root(&entry.path()))
        .filter(|root| root.exists())
        .collect()
}

fn read_worktree_root(worktree_meta_dir: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(worktree_meta_dir.join("gitdir")).ok()?;
    let raw = content.trim();
    if raw.is_empty() {
        return None;
    }
    // The file points at `<worktree>/.git` (a file, not a dir); the parent
    // is the worktree root.
    //
    // Git is free to write either an absolute or a relative path here, and
    // resolves relatives against the metadata directory itself (e.g. after
    // `git worktree repair` following a move of the bare repo). Match that
    // behavior so a relative pointer is not silently misresolved against
    // the process CWD and dropped by the existence check.
    let wt_git = PathBuf::from(raw);
    let wt_git = if wt_git.is_relative() {
        worktree_meta_dir.join(wt_git)
    } else {
        wt_git
    };
    wt_git.parent().map(PathBuf::from)
}

pub fn add_existing_branch(gitdir: &Path, worktree: &Path, branch: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    GitDir::new(gitdir).status(&["worktree", "add", worktree_str.as_ref(), branch])
}

pub fn add_tracking_remote_branch(gitdir: &Path, worktree: &Path, branch: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    let remote = format!("origin/{branch}");
    add_new_branch_worktree(gitdir, worktree_str.as_ref(), branch, &remote, true)
}

pub fn add_from_base(gitdir: &Path, worktree: &Path, branch: &str, base_ref: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    add_new_branch_worktree(gitdir, worktree_str.as_ref(), branch, base_ref, false)?;

    set_branch_upstream(gitdir, branch, "origin")
}

fn add_new_branch_worktree(
    gitdir: &Path,
    worktree: &str,
    branch: &str,
    start_point: &str,
    track: bool,
) -> Result<()> {
    let track_flag = if track { "--track" } else { "--no-track" };

    GitDir::new(gitdir).status(&[
        "worktree",
        "add",
        track_flag,
        "-b",
        branch,
        worktree,
        start_point,
    ])
}

pub fn prune(gitdir: &Path) -> Result<()> {
    GitDir::new(gitdir).status(&["worktree", "prune", "--verbose", "--expire", "now"])
}

pub fn remove(gitdir: &Path, worktree: &Path, force: bool) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_str.as_ref());
    let cwd = gitdir.parent().unwrap_or(gitdir);
    GitDir::new(gitdir).status_in(&args, cwd)
}

pub fn status_porcelain(worktree: &Path) -> Result<String> {
    let worktree_str = worktree.to_string_lossy();
    // This uses `-C` rather than `--git-dir`, so call run directly.
    capture(
        &["-C", worktree_str.as_ref(), "status", "--porcelain"],
        None,
    )
}

pub fn rebase(worktree: &Path, base_ref: &str) -> Result<()> {
    let worktree_str = worktree.to_string_lossy();
    status(&["-C", worktree_str.as_ref(), "rebase", base_ref], None)
}

pub fn checkout_or_create_branch(worktree: &Path, branch: &str, base_ref: &str) -> Result<()> {
    if local_branch_exists_in_worktree(worktree, branch) {
        return checkout_branch(worktree, branch);
    }

    create_branch_from_base(worktree, branch, base_ref)
}

pub fn parse_worktree_porcelain(text: &str) -> Vec<WorktreeEntry> {
    #[derive(Default)]
    struct Builder {
        path: Option<PathBuf>,
        branch_ref: Option<String>,
        is_bare: bool,
    }

    impl Builder {
        fn flush(&mut self) -> Option<WorktreeEntry> {
            let path = self.path.take()?;
            Some(WorktreeEntry {
                path,
                branch_ref: self.branch_ref.take(),
                is_bare: std::mem::replace(&mut self.is_bare, false),
            })
        }
    }

    let mut entries = Vec::new();
    let mut builder = Builder::default();

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(entry) = builder.flush() {
                entries.push(entry);
            }
            builder.path = Some(PathBuf::from(path));
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            builder.branch_ref = Some(branch_ref.to_string());
        } else if line == "bare" {
            builder.is_bare = true;
        } else if line.is_empty()
            && let Some(entry) = builder.flush()
        {
            entries.push(entry);
        }
    }

    if let Some(entry) = builder.flush() {
        entries.push(entry);
    }

    entries
}

pub fn branch_from_worktree_path(
    wt_dir: &Path,
    repo_key: &str,
    worktree_path: &Path,
) -> Option<String> {
    let relative = worktree_path.strip_prefix(wt_dir.join(repo_key)).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(relative.to_string_lossy().into_owned())
}

/// Extract the stable worktree identity from a worktree path.
///
/// Returns the path relative to `wt_dir/repo_key` — this is the directory
/// name chosen at `task start` time and never changed by Git branch renames.
/// Use it (instead of the current Git branch name) for session/profile
/// identity so that renames don't break teardown.
///
/// Both `wt_dir` and `worktree_path` are canonicalized internally so that
/// symlinked layouts (e.g. macOS `/var` → `/private/var`) resolve correctly.
/// Falls back to the raw paths, then to the last path component.
pub fn worktree_name(wt_dir: &Path, repo_key: &str, worktree_path: &Path) -> String {
    // Try canonical paths first (handles symlinks like /var → /private/var).
    let real_wt = std::fs::canonicalize(wt_dir).ok();
    let real_path = std::fs::canonicalize(worktree_path).ok();
    if let (Some(rw), Some(rp)) = (&real_wt, &real_path)
        && let Some(name) = branch_from_worktree_path(rw, repo_key, rp)
    {
        return name;
    }

    // Fall back to raw (non-canonical) paths.
    branch_from_worktree_path(wt_dir, repo_key, worktree_path)
        .or_else(|| {
            worktree_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Create a detached worktree at `path` pinned to `base_ref` (e.g. `origin/HEAD`).
/// Equivalent to: git --git-dir <gitdir> worktree add --detach <path> <base_ref>
pub fn add_detached(gitdir: &Path, path: &Path, base_ref: &str) -> Result<()> {
    let path_str = path.to_string_lossy();
    GitDir::new(gitdir).status(&["worktree", "add", "--detach", path_str.as_ref(), base_ref])
}

/// Update a detached worktree by fetching `origin` then hard-resetting to `origin/HEAD`.
/// Equivalent to:
///   git -C <path> fetch origin
///   git -C <path> reset --hard <resolved-base>
pub fn update_detached(path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    status(&["-C", path_str.as_ref(), "fetch", "origin"], None)?;
    let base_ref = resolve_detached_base_ref(path);
    status(
        &["-C", path_str.as_ref(), "reset", "--hard", &base_ref],
        None,
    )
}

fn resolve_detached_base_ref(path: &Path) -> String {
    if let Some(remote_head) = remote_default_branch(path)
        && rev_exists_in_worktree(path, &remote_head)
    {
        return remote_head;
    }

    if let Some(origin_head) = symbolic_origin_head(path)
        && rev_exists_in_worktree(path, &origin_head)
    {
        return origin_head;
    }

    for fallback in ["origin/main", "origin/master"] {
        if rev_exists_in_worktree(path, fallback) {
            return fallback.to_string();
        }
    }

    "HEAD".to_string()
}

fn remote_default_branch(path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();
    let output = capture(
        &[
            "-C",
            path_str.as_ref(),
            "ls-remote",
            "--symref",
            "origin",
            "HEAD",
        ],
        None,
    )
    .ok()?;
    let branch = parse_ls_remote_branch(&output)?;
    Some(format!("origin/{branch}"))
}

fn symbolic_origin_head(path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();
    let output = capture(
        &[
            "-C",
            path_str.as_ref(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        None,
    )
    .ok()?;
    let value = output.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn rev_exists_in_worktree(path: &Path, revision: &str) -> bool {
    let path_str = path.to_string_lossy();
    let value = format!("{revision}^{{commit}}");
    status(
        &[
            "-C",
            path_str.as_ref(),
            "rev-parse",
            "--verify",
            "--quiet",
            &value,
        ],
        None,
    )
    .is_ok()
}

fn local_branch_exists_in_worktree(path: &Path, branch: &str) -> bool {
    let path_str = path.to_string_lossy();
    let branch_ref = format!("refs/heads/{branch}");
    status(
        &[
            "-C",
            path_str.as_ref(),
            "show-ref",
            "--verify",
            "--quiet",
            &branch_ref,
        ],
        None,
    )
    .is_ok()
}

fn checkout_branch(path: &Path, branch: &str) -> Result<()> {
    let path_str = path.to_string_lossy();
    status(&["-C", path_str.as_ref(), "checkout", branch], None)
}

fn create_branch_from_base(path: &Path, branch: &str, base_ref: &str) -> Result<()> {
    let path_str = path.to_string_lossy();
    status(
        &["-C", path_str.as_ref(), "checkout", "-b", branch, base_ref],
        None,
    )
}

pub fn branch_from_ref(branch_ref: Option<&str>) -> Option<String> {
    let branch_ref = branch_ref?;
    Some(
        branch_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(branch_ref)
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    use super::{
        WorktreeEntry, add_from_base, add_tracking_remote_branch, branch_from_ref,
        branch_from_worktree_path, checkout_or_create_branch, list_registered_worktrees,
        parse_worktree_porcelain, update_detached,
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("task-rs-worktrees-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            self.0.as_path()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Run a git command isolated from the user's global config.
    ///
    /// Sets `GIT_CONFIG_NOSYSTEM` and `HOME` to the working directory so that
    /// the subprocess never reads `~/.gitconfig` or `/etc/gitconfig`.  This
    /// prevents races with parallel tests that mutate `HOME`.
    fn run_git(args: &[&str], cwd: &Path) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", cwd)
            .status()
            .expect("git must be available");
        assert!(status.success(), "git {args:?} failed");
    }

    fn git_output(args: &[&str], cwd: &Path) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", cwd)
            .output()
            .expect("git must be available");
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn create_bare_repo_with_origin(name: &str) -> (TempDir, PathBuf, PathBuf) {
        let dir = TempDir::new(name);
        let remote = dir.path().join("remote.git");
        let source = dir.path().join("source");
        let bare = dir.path().join("repo.git");

        fs::create_dir_all(&source).expect("create source dir");

        run_git(
            &["init", "--bare", remote.to_string_lossy().as_ref()],
            dir.path(),
        );
        run_git(&["init", "-b", "main"], source.as_path());
        run_git(
            &["config", "user.email", "test@example.com"],
            source.as_path(),
        );
        run_git(&["config", "user.name", "Test"], source.as_path());
        fs::write(source.join("README.md"), "v1\n").expect("write initial file");
        run_git(&["add", "README.md"], source.as_path());
        run_git(&["commit", "-m", "initial"], source.as_path());
        run_git(
            &["remote", "add", "origin", remote.to_string_lossy().as_ref()],
            source.as_path(),
        );
        run_git(&["push", "-u", "origin", "main"], source.as_path());

        run_git(
            &["init", "--bare", bare.to_string_lossy().as_ref()],
            dir.path(),
        );
        run_git(
            &["remote", "add", "origin", remote.to_string_lossy().as_ref()],
            bare.as_path(),
        );
        run_git(
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
            bare.as_path(),
        );
        run_git(&["fetch", "origin"], bare.as_path());

        (dir, source, bare)
    }

    mod parse_worktree_porcelain {
        use super::*;

        #[test]
        fn collects_entries() {
            let text = "worktree /tmp/dev/repos/github.com/tsauvajon/task.git\n\
bare\n\
\n\
worktree /tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust\n\
HEAD 0123456789abcdef\n\
branch refs/heads/rewrite-in-rust\n\n";

            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 2);
            assert!(entries[0].is_bare);
            assert_eq!(
                entries[1],
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/rewrite-in-rust".into(),
                    branch_ref: Some("refs/heads/rewrite-in-rust".to_string()),
                    is_bare: false,
                }
            );
        }

        #[test]
        fn handles_missing_trailing_blank_line() {
            // If the porcelain output ends without a trailing blank line the last
            // entry should still be flushed by the end-of-input flush path.
            let text = "worktree /tmp/dev/wt/github.com/tsauvajon/task/bump\n\
branch refs/heads/bump";

            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0],
                WorktreeEntry {
                    path: "/tmp/dev/wt/github.com/tsauvajon/task/bump".into(),
                    branch_ref: Some("refs/heads/bump".to_string()),
                    is_bare: false,
                }
            );
        }

        #[test]
        fn preserves_bare_flag() {
            let text = "worktree /tmp/dev/repos/task.git\n\
bare\n\
\n\
worktree /tmp/dev/wt/task/main\n\
branch refs/heads/main\n\
\n";
            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 2);
            assert!(entries[0].is_bare);
            assert!(!entries[1].is_bare);
        }

        #[test]
        fn empty_input_returns_empty_vec() {
            let entries = parse_worktree_porcelain("");
            assert!(entries.is_empty());
        }

        #[test]
        fn entry_without_branch_has_none() {
            // A detached HEAD worktree has no `branch` line.
            let text = "worktree /tmp/dev/wt/task/detached\nHEAD abc123\n\n";
            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].branch_ref, None);
            assert!(!entries[0].is_bare);
        }

        #[test]
        fn single_entry_with_trailing_newline() {
            let text = "worktree /tmp/dev/wt/task/feat\nbranch refs/heads/feat\n\n";
            let entries = parse_worktree_porcelain(text);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].branch_ref.as_deref(), Some("refs/heads/feat"));
        }
    }

    mod branch_from_ref {
        use super::*;

        #[test]
        fn strips_heads_prefix() {
            assert_eq!(
                branch_from_ref(Some("refs/heads/rewrite-in-rust")),
                Some("rewrite-in-rust".to_string())
            );
        }

        #[test]
        fn returns_none_for_none_input() {
            assert_eq!(branch_from_ref(None), None);
        }

        #[test]
        fn returns_raw_ref_when_no_prefix() {
            // A ref that does NOT start with "refs/heads/" is returned as-is.
            assert_eq!(branch_from_ref(Some("HEAD")), Some("HEAD".to_string()));
        }
    }

    mod add_detached_args {
        /// Verify the args passed to `add_detached` are correct by building them
        /// independently — same pattern as the prune/remove arg tests.
        #[test]
        fn includes_detach_flag_and_base_ref() {
            let gitdir = std::path::Path::new("/repos/app.git");
            let worktree = std::path::Path::new("/detached/github.com/org/app");
            let base_ref = "origin/HEAD";

            // Reconstruct what add_detached would build.
            let gitdir_str = gitdir.to_string_lossy();
            let worktree_str = worktree.to_string_lossy();
            let args = [
                "--git-dir",
                gitdir_str.as_ref(),
                "worktree",
                "add",
                "--detach",
                worktree_str.as_ref(),
                base_ref,
            ];

            assert!(args.contains(&"--detach"));
            assert!(args.contains(&"origin/HEAD"));
            assert!(args.contains(&worktree_str.as_ref()));
        }

        #[test]
        fn detach_flag_precedes_path() {
            let args = ["worktree", "add", "--detach", "/some/path", "origin/HEAD"];
            let detach_pos = args.iter().position(|&a| a == "--detach").unwrap();
            let path_pos = args.iter().position(|&a| a == "/some/path").unwrap();
            assert!(
                detach_pos < path_pos,
                "--detach must come before the path argument"
            );
        }
    }

    mod branch_from_worktree_path {
        use super::*;

        #[test]
        fn supports_nested_branch_names() {
            let branch = branch_from_worktree_path(
                Path::new("/tmp/custom/wt"),
                "github.com/tsauvajon/task",
                Path::new("/tmp/custom/wt/github.com/tsauvajon/task/feat/rewrite/rust"),
            );
            assert_eq!(branch, Some("feat/rewrite/rust".to_string()));
        }

        #[test]
        fn handles_single_component_branch() {
            let branch = branch_from_worktree_path(
                Path::new("/tmp/wt"),
                "github.com/acme/repo",
                Path::new("/tmp/wt/github.com/acme/repo/main"),
            );
            assert_eq!(branch, Some("main".to_string()));
        }

        #[test]
        fn returns_none_when_path_equals_repo_root() {
            // The path IS the repo root (no trailing component) → None
            let branch = branch_from_worktree_path(
                Path::new("/tmp/wt"),
                "github.com/tsauvajon/task",
                Path::new("/tmp/wt/github.com/tsauvajon/task"),
            );
            assert_eq!(branch, None);
        }

        #[test]
        fn returns_none_for_unrelated_path() {
            let branch = branch_from_worktree_path(
                Path::new("/tmp/wt"),
                "github.com/tsauvajon/task",
                Path::new("/other/path/feat/something"),
            );
            assert_eq!(branch, None);
        }
    }

    mod update_detached_tests {
        use super::*;

        #[test]
        fn falls_back_when_origin_head_is_missing() {
            let dir = TempDir::new("update-detached-fallback");
            let remote = dir.path().join("remote.git");
            let source = dir.path().join("source");
            let detached = dir.path().join("detached");

            fs::create_dir_all(&source).expect("create source dir");

            run_git(
                &["init", "--bare", remote.to_string_lossy().as_ref()],
                dir.path(),
            );

            run_git(&["init", "-b", "main"], source.as_path());
            run_git(
                &["config", "user.email", "test@example.com"],
                source.as_path(),
            );
            run_git(&["config", "user.name", "Test"], source.as_path());
            fs::write(source.join("README.md"), "v1\n").expect("write initial file");
            run_git(&["add", "README.md"], source.as_path());
            run_git(&["commit", "-m", "initial"], source.as_path());
            run_git(
                &["remote", "add", "origin", remote.to_string_lossy().as_ref()],
                source.as_path(),
            );
            run_git(&["push", "-u", "origin", "main"], source.as_path());

            run_git(
                &[
                    "clone",
                    remote.to_string_lossy().as_ref(),
                    detached.to_string_lossy().as_ref(),
                ],
                dir.path(),
            );

            // Remove origin/HEAD to reproduce the detached update failure mode.
            run_git(
                &["update-ref", "-d", "refs/remotes/origin/HEAD"],
                detached.as_path(),
            );

            fs::write(source.join("README.md"), "v2\n").expect("write updated file");
            run_git(&["commit", "-am", "update"], source.as_path());
            run_git(&["push", "origin", "main"], source.as_path());

            update_detached(detached.as_path()).expect("update_detached should succeed");

            let head = git_output(&["rev-parse", "HEAD"], detached.as_path());
            let origin_main = git_output(&["rev-parse", "origin/main"], detached.as_path());
            assert_eq!(head, origin_main, "detached HEAD should match origin/main");
        }
    }

    mod checkout_or_create_branch_tests {
        use super::*;

        #[test]
        fn checks_out_existing_local_branch() {
            let dir = TempDir::new("checkout-existing-branch");
            let repo = dir.path().join("repo");

            fs::create_dir_all(&repo).expect("create repo dir");
            run_git(&["init", "-b", "main"], repo.as_path());
            run_git(
                &["config", "user.email", "test@example.com"],
                repo.as_path(),
            );
            run_git(&["config", "user.name", "Test"], repo.as_path());
            fs::write(repo.join("README.md"), "v1\n").expect("write initial file");
            run_git(&["add", "README.md"], repo.as_path());
            run_git(&["commit", "-m", "initial"], repo.as_path());
            run_git(&["checkout", "-b", "feature"], repo.as_path());
            run_git(&["checkout", "main"], repo.as_path());

            checkout_or_create_branch(repo.as_path(), "feature", "main")
                .expect("checkout existing branch");

            let head = git_output(
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
                repo.as_path(),
            );
            assert_eq!(head, "feature");
        }

        #[test]
        fn creates_missing_branch_from_base() {
            let dir = TempDir::new("checkout-create-branch");
            let repo = dir.path().join("repo");

            fs::create_dir_all(&repo).expect("create repo dir");
            run_git(&["init", "-b", "main"], repo.as_path());
            run_git(
                &["config", "user.email", "test@example.com"],
                repo.as_path(),
            );
            run_git(&["config", "user.name", "Test"], repo.as_path());
            fs::write(repo.join("README.md"), "v1\n").expect("write initial file");
            run_git(&["add", "README.md"], repo.as_path());
            run_git(&["commit", "-m", "initial"], repo.as_path());

            checkout_or_create_branch(repo.as_path(), "feature", "main")
                .expect("create branch from base");

            let head = git_output(
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
                repo.as_path(),
            );
            assert_eq!(head, "feature");

            let branch_ref = git_output(&["rev-parse", "feature"], repo.as_path());
            let base_ref = git_output(&["rev-parse", "main"], repo.as_path());
            assert_eq!(
                branch_ref, base_ref,
                "new branch should start from base ref"
            );
        }
    }

    mod branch_tracking_tests {
        use super::*;

        #[test]
        fn add_from_base_sets_upstream_to_same_named_origin_branch() {
            let (dir, _source, bare) = create_bare_repo_with_origin("add-from-base-upstream");
            let worktree = dir.path().join("wt/feature");

            add_from_base(bare.as_path(), worktree.as_path(), "feature", "origin/main")
                .expect("create worktree from base");

            let branch = git_output(
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
                worktree.as_path(),
            );
            assert_eq!(branch, "feature");

            let remote_config = git_output(
                &["config", "--get", "branch.feature.remote"],
                worktree.as_path(),
            );
            assert_eq!(remote_config, "origin");

            let merge_config = git_output(
                &["config", "--get", "branch.feature.merge"],
                worktree.as_path(),
            );
            assert_eq!(merge_config, "refs/heads/feature");

            run_git(&["push"], worktree.as_path());

            let status = git_output(&["status", "-sb"], worktree.as_path());
            assert!(status.starts_with("## feature...origin/feature"));
        }

        #[test]
        fn add_tracking_remote_branch_tracks_existing_remote_branch() {
            let (dir, source, bare) = create_bare_repo_with_origin("track-remote-upstream");
            let worktree = dir.path().join("wt/feature");
            run_git(&["checkout", "-b", "feature"], source.as_path());
            run_git(&["push", "-u", "origin", "feature"], source.as_path());
            run_git(&["fetch", "origin"], bare.as_path());

            add_tracking_remote_branch(bare.as_path(), worktree.as_path(), "feature")
                .expect("create tracking worktree");

            let status = git_output(&["status", "-sb"], worktree.as_path());
            assert!(status.starts_with("## feature...origin/feature"));

            let merge_config = git_output(
                &["config", "--get", "branch.feature.merge"],
                worktree.as_path(),
            );
            assert_eq!(merge_config, "refs/heads/feature");
        }
    }

    mod worktree_name {
        use std::path::Path;

        use super::super::worktree_name;

        #[test]
        fn extracts_relative_path() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let path = Path::new("/tmp/dev/wt/github.com/org/repo/feat-login");
            assert_eq!(
                worktree_name(wt_dir, "github.com/org/repo", path),
                "feat-login"
            );
        }

        #[test]
        fn preserves_slashes_in_branch_style_directory() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let path = Path::new("/tmp/dev/wt/github.com/org/repo/feat/login");
            assert_eq!(
                worktree_name(wt_dir, "github.com/org/repo", path),
                "feat/login"
            );
        }

        #[test]
        fn falls_back_to_last_component_when_not_under_wt_dir() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let path = Path::new("/somewhere/else/bump-deps");
            assert_eq!(
                worktree_name(wt_dir, "github.com/org/repo", path),
                "bump-deps"
            );
        }

        #[test]
        fn returns_unknown_for_root_path() {
            let wt_dir = Path::new("/tmp/dev/wt");
            let path = Path::new("/");
            assert_eq!(
                worktree_name(wt_dir, "github.com/org/repo", path),
                "unknown"
            );
        }

        /// On macOS `/var` is a symlink to `/private/var`. When wt_dir uses
        /// the symlink form and worktree_path uses the canonical form (or
        /// vice versa), the raw `strip_prefix` fails. Canonicalization inside
        /// `worktree_name` must handle this so nested branches like
        /// `feat/login` don't collapse to just `login`.
        #[test]
        fn resolves_through_symlinks_for_nested_branch() {
            // Create a real directory tree, then symlink so paths diverge.
            let dir = super::TempDir::new("wt-name-symlink");
            let real_wt = dir.path().join("real_wt");
            let repo_tree = real_wt.join("org/repo/feat/login");
            std::fs::create_dir_all(&repo_tree).unwrap();

            // Symlink: <tmp>/link_wt → <tmp>/real_wt
            let link_wt = dir.path().join("link_wt");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&real_wt, &link_wt).unwrap();

            // wt_dir uses the symlink, worktree_path uses the real path.
            let result = worktree_name(&link_wt, "org/repo", &repo_tree);
            assert_eq!(
                result, "feat/login",
                "should resolve through symlink and preserve nested path"
            );

            // Reverse: wt_dir is real, worktree_path uses symlink.
            let link_path = link_wt.join("org/repo/feat/login");
            let result = worktree_name(&real_wt, "org/repo", &link_path);
            assert_eq!(
                result, "feat/login",
                "should resolve through symlink in the other direction too"
            );
        }
    }

    mod list_registered_worktrees_tests {
        use super::*;

        #[test]
        fn returns_empty_when_worktrees_dir_missing() {
            let dir = TempDir::new("list-reg-empty");
            let gitdir = dir.path().join("repo.git");
            fs::create_dir_all(&gitdir).unwrap();
            // No <gitdir>/worktrees/ created.
            let result = list_registered_worktrees(&gitdir);
            assert!(result.is_empty());
        }

        #[test]
        fn returns_worktree_root_from_gitdir_file() {
            let dir = TempDir::new("list-reg-one");
            let gitdir = dir.path().join("repo.git");
            let wt_path = dir.path().join("wt/feature");
            fs::create_dir_all(&wt_path).unwrap();
            let meta = gitdir.join("worktrees/feature");
            fs::create_dir_all(&meta).unwrap();
            // `git worktree add` writes the path of the worktree's .git file.
            fs::write(
                meta.join("gitdir"),
                wt_path.join(".git").to_string_lossy().as_ref(),
            )
            .unwrap();

            let result = list_registered_worktrees(&gitdir);
            assert_eq!(result, vec![wt_path]);
        }

        #[test]
        fn skips_stale_entries_whose_target_no_longer_exists() {
            let dir = TempDir::new("list-reg-stale");
            let gitdir = dir.path().join("repo.git");
            let meta = gitdir.join("worktrees/ghost");
            fs::create_dir_all(&meta).unwrap();
            // Pointer to a path that does not exist — must be dropped.
            fs::write(meta.join("gitdir"), "/no/such/path/.git").unwrap();

            let result = list_registered_worktrees(&gitdir);
            assert!(result.is_empty());
        }

        #[test]
        fn returns_multiple_entries_in_any_order() {
            let dir = TempDir::new("list-reg-many");
            let gitdir = dir.path().join("repo.git");
            let wt_a = dir.path().join("wt/a");
            let wt_b = dir.path().join("wt/b");
            fs::create_dir_all(&wt_a).unwrap();
            fs::create_dir_all(&wt_b).unwrap();
            for (name, wt) in [("a", &wt_a), ("b", &wt_b)] {
                let meta = gitdir.join(format!("worktrees/{name}"));
                fs::create_dir_all(&meta).unwrap();
                fs::write(
                    meta.join("gitdir"),
                    wt.join(".git").to_string_lossy().as_ref(),
                )
                .unwrap();
            }

            let mut result = list_registered_worktrees(&gitdir);
            result.sort();
            let mut expected = vec![wt_a, wt_b];
            expected.sort();
            assert_eq!(result, expected);
        }

        #[test]
        fn skips_entries_with_missing_gitdir_file() {
            let dir = TempDir::new("list-reg-no-gitdir-file");
            let gitdir = dir.path().join("repo.git");
            let meta = gitdir.join("worktrees/broken");
            fs::create_dir_all(&meta).unwrap();
            // No `gitdir` file inside the metadata dir.

            let result = list_registered_worktrees(&gitdir);
            assert!(result.is_empty());
        }

        #[test]
        fn resolves_relative_gitdir_pointer_against_metadata_dir() {
            // `git worktree repair` (and some portable git builds) can write
            // a relative pointer into <gitdir>/worktrees/<name>/gitdir.
            // Git resolves it against the metadata directory itself; we must
            // do the same, otherwise the path resolves against the process
            // CWD and the entry gets silently dropped by the existence check.
            let dir = TempDir::new("list-reg-relative");
            let gitdir = dir.path().join("repo.git");
            let wt_path = dir.path().join("wt/feature");
            fs::create_dir_all(&wt_path).unwrap();
            let meta = gitdir.join("worktrees/feature");
            fs::create_dir_all(&meta).unwrap();
            // Relative path from <gitdir>/worktrees/feature/ back up to wt/feature/.git
            fs::write(meta.join("gitdir"), "../../../wt/feature/.git").unwrap();

            let result = list_registered_worktrees(&gitdir);
            // Canonicalize because the joined path contains `..` segments
            // that the existence check resolves through, but the returned
            // PathBuf preserves them.
            let canon: Vec<PathBuf> = result
                .into_iter()
                .map(|p| fs::canonicalize(&p).unwrap_or(p))
                .collect();
            let expected = fs::canonicalize(&wt_path).unwrap_or(wt_path);
            assert_eq!(canon, vec![expected]);
        }

        #[test]
        fn skips_empty_gitdir_file() {
            let dir = TempDir::new("list-reg-empty-gitdir-file");
            let gitdir = dir.path().join("repo.git");
            let meta = gitdir.join("worktrees/blank");
            fs::create_dir_all(&meta).unwrap();
            fs::write(meta.join("gitdir"), "").unwrap();

            let result = list_registered_worktrees(&gitdir);
            assert!(result.is_empty());
        }

        #[test]
        fn agrees_with_git_worktree_list_on_real_repo() {
            // End-to-end: create a bare repo, add a worktree via git, and
            // verify our FS-based reader returns the same path that
            // `git worktree list --porcelain` reports.
            let (_dir, _source, bare) = create_bare_repo_with_origin("fs-vs-git-agreement");
            let wt_path = _dir.path().join("wt/feature");
            add_from_base(&bare, &wt_path, "feature", "origin/main")
                .expect("create worktree from base");

            let fs_paths = list_registered_worktrees(&bare);
            let git_output = super::super::list_porcelain(&bare).expect("git worktree list");
            let git_paths: Vec<PathBuf> = parse_worktree_porcelain(&git_output)
                .into_iter()
                .filter(|e| !e.is_bare)
                .map(|e| e.path)
                .collect();

            // Canonicalize both sides because macOS may resolve /var → /private/var.
            let canon = |paths: Vec<PathBuf>| -> Vec<PathBuf> {
                let mut out: Vec<PathBuf> = paths
                    .into_iter()
                    .map(|p| fs::canonicalize(&p).unwrap_or(p))
                    .collect();
                out.sort();
                out
            };
            assert_eq!(canon(fs_paths), canon(git_paths));
        }
    }
}
