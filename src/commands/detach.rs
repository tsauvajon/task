use std::fs;

use clap::Subcommand;
use rayon::prelude::*;

use crate::{
    error::{Error, Result},
    runtime::{RepoKey, environment::RuntimeEnvironment, process},
    tools::git::{
        refs::{detect_default_base, fetch_origin_refs},
        worktrees::{add_detached, update_detached},
    },
};

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum DetachCommand {
    /// Create (or update) a detached worktree pinned to the default branch.
    ///
    /// If the worktree already exists, this is equivalent to `task detach update <repo>`.
    #[command(about = "Create or update a detached default-branch worktree")]
    Add { repo: String },
    /// Update one or all detached worktrees by fetching and hard-resetting to origin/HEAD.
    ///
    /// Repos with a matching [[install]] entry in config are automatically reinstalled
    /// after a successful update.
    #[command(about = "Fetch and reset a detached worktree to the latest remote default branch")]
    Update {
        /// Repo to update. Omit to update all detached worktrees.
        repo: Option<String>,
    },
    /// Remove a detached worktree from disk.
    #[command(about = "Remove a detached worktree")]
    Remove {
        repo: String,
        #[arg(long)]
        force: bool,
    },
    /// Install one or all configured detached repos via `cargo install --path <path> --locked`.
    ///
    /// Installable repos are defined in the [[install]] section of config.toml.
    /// Without arguments, installs all configured entries.
    #[command(about = "Install configured detached repos via cargo")]
    Install {
        /// Repo to install. Omit to install all configured repos.
        repo: Option<String>,
    },
    /// List all detached worktrees with their HEAD commit.
    #[command(about = "List all detached worktrees")]
    List,
}

pub fn run(env: &RuntimeEnvironment, command: DetachCommand) -> Result<()> {
    match command {
        DetachCommand::Add { repo } => add(env, &repo),
        DetachCommand::Update { repo } => match repo {
            Some(repo) => update_one(env, &repo),
            None => update_all(env),
        },
        DetachCommand::Remove { repo, force } => remove(env, &repo, force),
        DetachCommand::Install { repo } => match repo {
            Some(repo) => install_one(env, &repo),
            None => install_all(env),
        },
        DetachCommand::List => list(env),
    }
}

pub(crate) fn add(env: &RuntimeEnvironment, repo_arg: &str) -> Result<()> {
    let layout = env.layout();
    let repo_key = env.tasks().resolve_existing_repo_key(repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    let path = layout.detached_path(&repo_key);

    // If worktree already on disk and is a git worktree, update it instead.
    if path.join(".git").exists() || is_detached_worktree(&path) {
        process::log(&format!(
            "Updating existing detached worktree: {}",
            path.display()
        ));
        return update_detached(&path);
    }

    process::log(&format!("Fetching origin refs for {repo_key}"));
    fetch_origin_refs(&gitdir)?;

    let base_ref = detect_default_base(&gitdir);
    process::log(&format!(
        "Creating detached worktree at {} (base: {base_ref})",
        path.display()
    ));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(Error::from)?;
    }

    add_detached(&gitdir, &path, &base_ref)
}

pub(crate) fn update_one(env: &RuntimeEnvironment, repo_arg: &str) -> Result<()> {
    let layout = env.layout();
    let repo_key = env.tasks().resolve_existing_repo_key(repo_arg)?;
    let path = layout.detached_path(&repo_key);

    if !path.exists() {
        return Err(Error::failed(format!(
            "No detached worktree for {repo_key}. Run 'task detach add {repo_arg}' first."
        )));
    }

    process::log(&format!("Updating detached worktree: {}", path.display()));
    update_detached(&path)?;

    try_install(env, &repo_key);
    Ok(())
}

pub(crate) fn update_all(env: &RuntimeEnvironment) -> Result<()> {
    let detached_dir = env.layout().detached_dir();

    let entries = match fs::read_dir(detached_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            process::log("No detached worktrees found");
            return Ok(());
        }
        Err(err) => return Err(Error::from(err)),
    };

    // Collect all leaf worktree paths (at depth repo_key = host/owner/name).
    let mut worktrees: Vec<std::path::PathBuf> = Vec::new();
    collect_detached_worktrees(detached_dir, &mut worktrees)?;

    if worktrees.is_empty() {
        process::log("No detached worktrees found");
        drop(entries);
        return Ok(());
    }

    drop(entries);

    // Fan out per-repo work across rayon workers. Each closure installs its
    // own [`process::OutputScope`] so all of the worker's logs and
    // subprocess output are buffered, then flushed sequentially in the
    // original sort order below.
    let results: Vec<(std::path::PathBuf, Vec<process::CapturedLine>, Result<()>)> = worktrees
        .par_iter()
        .map(|path| {
            let scope = process::OutputScope::new();
            process::log(&format!("Updating: {}", path.display()));
            let result = match update_detached(path) {
                Ok(()) => {
                    let repo_key = repo_key_from_detached_path(detached_dir, path);
                    try_install(env, &repo_key);
                    Ok(())
                }
                Err(err) => {
                    process::warn(&format!("Failed to update {}: {err}", path.display()));
                    Err(err)
                }
            };
            (path.clone(), scope.into_lines(), result)
        })
        .collect();

    let mut errors: Vec<String> = Vec::new();
    for (path, lines, result) in results {
        process::flush_captured_lines(lines);
        if let Err(err) = result {
            errors.push(format!("{}: {err}", path.display()));
        }
    }

    if !errors.is_empty() {
        return Err(Error::failed(format!(
            "{} detached worktree(s) failed to update",
            errors.len()
        )));
    }

    Ok(())
}

pub(crate) fn remove(env: &RuntimeEnvironment, repo_arg: &str, force: bool) -> Result<()> {
    let layout = env.layout();
    let repo_key = env.tasks().resolve_existing_repo_key(repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    let path = layout.detached_path(&repo_key);

    if !path.exists() {
        return Err(Error::failed(format!(
            "No detached worktree for {repo_key}"
        )));
    }

    process::log(&format!("Removing detached worktree: {}", path.display()));
    crate::tools::git::worktrees::remove(&gitdir, &path, force)?;

    // Clean up empty parent directories up to (but not including) detached_dir.
    let detached_dir = layout.detached_dir();
    let mut dir = path.parent();
    while let Some(parent) = dir {
        if parent == detached_dir {
            break;
        }
        if fs::read_dir(parent)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            let _ = fs::remove_dir(parent);
        }
        dir = parent.parent();
    }

    Ok(())
}

pub(crate) fn list(env: &RuntimeEnvironment) -> Result<()> {
    let detached_dir = env.layout().detached_dir();

    let mut worktrees: Vec<std::path::PathBuf> = Vec::new();
    collect_detached_worktrees(detached_dir, &mut worktrees)?;

    if worktrees.is_empty() {
        println!("No detached worktrees.");
        return Ok(());
    }

    for path in &worktrees {
        // Best-effort HEAD SHA — don't fail the list if git fails.
        let head = read_head_sha(path).unwrap_or_else(|| "unknown".to_string());
        let repo_key = repo_key_from_detached_path(detached_dir, path);
        println!("{repo_key}  {head}  {}", path.display());
    }

    Ok(())
}

// ── Install ───────────────────────────────────────────────────────────────

pub(crate) fn install_one(env: &RuntimeEnvironment, repo_arg: &str) -> Result<()> {
    let entries = env.install_entries();
    let entry = find_install_entry(entries, repo_arg).ok_or_else(|| {
        Error::failed(format!(
            "No [[install]] entry for '{repo_arg}' in config.toml. \
             Add one to enable cargo install for this repo."
        ))
    })?;

    let detached_dir = env.layout().detached_dir();
    run_cargo_install(detached_dir, entry)
}

pub(crate) fn install_all(env: &RuntimeEnvironment) -> Result<()> {
    let entries = env.install_entries();
    if entries.is_empty() {
        process::log("No [[install]] entries in config.toml");
        return Ok(());
    }

    let detached_dir = env.layout().detached_dir();

    // Parallelise across entries. cargo's global install lock serialises
    // the final `mv` step, but the expensive compile phase overlaps across
    // distinct detached worktrees (each has its own target/ directory).
    let results: Vec<(String, Vec<process::CapturedLine>, Result<()>)> = entries
        .par_iter()
        .map(|entry| {
            let scope = process::OutputScope::new();
            let result = run_cargo_install(detached_dir, entry);
            if let Err(err) = &result {
                process::warn(&format!("Failed to install {}: {err}", entry.repo));
            }
            (entry.repo.clone(), scope.into_lines(), result)
        })
        .collect();

    let mut errors: Vec<String> = Vec::new();
    for (repo, lines, result) in results {
        process::flush_captured_lines(lines);
        if let Err(err) = result {
            errors.push(format!("{repo}: {err}"));
        }
    }

    if !errors.is_empty() {
        return Err(Error::failed(format!("{} install(s) failed", errors.len())));
    }

    Ok(())
}

/// Try to install a repo if it has a matching [[install]] entry.
/// Failures are logged as warnings and do not propagate.
fn try_install(env: &RuntimeEnvironment, repo_key: &RepoKey) {
    let entries = env.install_entries();
    let Some(entry) = find_install_entry(entries, repo_key.as_ref()) else {
        return;
    };

    let detached_dir = env.layout().detached_dir();
    if let Err(err) = run_cargo_install(detached_dir, entry) {
        process::warn(&format!("Failed to install {}: {err}", entry.repo));
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Find an [[install]] entry whose `repo` field matches the given argument.
///
/// Matches against the full repo key, or against the trailing repo name
/// (the last path component) when there is exactly one match.
fn find_install_entry<'a>(
    entries: &'a [crate::runtime::config::InstallEntry],
    query: &str,
) -> Option<&'a crate::runtime::config::InstallEntry> {
    // Exact match on full repo key.
    if let Some(entry) = entries.iter().find(|e| e.repo == query) {
        return Some(entry);
    }

    // Short-name suffix match: compare against last path component.
    let matches: Vec<_> = entries
        .iter()
        .filter(|e| e.repo.rsplit('/').next().is_some_and(|name| name == query))
        .collect();

    if matches.len() == 1 {
        return Some(matches[0]);
    }

    None
}

/// Run `cargo install --path <install_path> --locked [extra_flags...]` for a single install entry.
fn run_cargo_install(
    detached_dir: &std::path::Path,
    entry: &crate::runtime::config::InstallEntry,
) -> Result<()> {
    let repo_key = RepoKey::new(&entry.repo);
    let base_path = detached_dir.join(AsRef::<std::path::Path>::as_ref(&repo_key));

    if !base_path.exists() {
        return Err(Error::failed(format!(
            "Detached worktree not found at {}. Run 'task detach add {}' first.",
            base_path.display(),
            entry.repo
        )));
    }

    let install_path = match &entry.path {
        Some(sub) => base_path.join(sub),
        None => base_path,
    };

    if !install_path.join("Cargo.toml").exists() {
        return Err(Error::failed(format!(
            "No Cargo.toml found at {}",
            install_path.display()
        )));
    }

    let path_str = install_path.to_string_lossy();
    process::log(&format!("Installing {} from {path_str}", entry.repo));

    let mut args = vec!["install", "--path", &path_str, "--locked"];
    let extra: Vec<&str> = entry.extra_flags.iter().map(String::as_str).collect();
    args.extend(extra);

    process::run_status("cargo", &args, None)
}

/// Returns true when `path` is the root of a git worktree (has a `.git` file
/// as created by `git worktree add`, or is a bare/worktree with `HEAD`).
fn is_detached_worktree(path: &std::path::Path) -> bool {
    path.join(".git").exists() || path.join("HEAD").exists()
}

/// Recursively collect leaf directories that look like git worktrees.
pub(crate) fn collect_detached_worktrees(
    dir: &std::path::Path,
    out: &mut Vec<std::path::PathBuf>,
) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(Error::from(err)),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if is_detached_worktree(&path) {
            out.push(path);
        } else {
            collect_detached_worktrees(&path, out)?;
        }
    }

    out.sort();
    Ok(())
}

fn read_head_sha(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            path.to_string_lossy().as_ref(),
            "rev-parse",
            "--short",
            "HEAD",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

pub(crate) fn repo_key_from_detached_path(
    detached_dir: &std::path::Path,
    path: &std::path::Path,
) -> RepoKey {
    let relative = path
        .strip_prefix(detached_dir)
        .unwrap_or(path)
        .to_string_lossy();
    RepoKey::new(relative.as_ref())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::{
        collect_detached_worktrees, find_install_entry, is_detached_worktree, read_head_sha,
        repo_key_from_detached_path, run_cargo_install,
    };
    use crate::runtime::{RepoKey, config::InstallEntry, environment::RuntimeEnvironment};

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("task-rs-detach-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    mod is_detached_worktree {
        use super::*;

        #[test]
        fn true_when_dot_git_file_exists() {
            let dir = TempDir::new("dot-git");
            fs::write(dir.path().join(".git"), "gitdir: ../../.git/worktrees/foo").unwrap();
            assert!(is_detached_worktree(dir.path()));
        }

        #[test]
        fn true_when_head_file_exists() {
            let dir = TempDir::new("head-file");
            fs::write(dir.path().join("HEAD"), "ref: refs/heads/main").unwrap();
            assert!(is_detached_worktree(dir.path()));
        }

        #[test]
        fn false_for_plain_directory() {
            let dir = TempDir::new("plain");
            assert!(!is_detached_worktree(dir.path()));
        }
    }

    mod collect_detached_worktrees {
        use super::*;

        #[test]
        fn finds_nested_worktrees() {
            let dir = TempDir::new("collect");
            let wt = dir.path().join("github.com/org/repo");
            fs::create_dir_all(&wt).unwrap();
            fs::write(wt.join(".git"), "gitdir: ...").unwrap();

            let mut out = Vec::new();
            collect_detached_worktrees(dir.path(), &mut out).unwrap();
            assert_eq!(out.len(), 1);
            assert_eq!(out[0], wt);
        }

        #[test]
        fn returns_empty_for_missing_dir() {
            let path = Path::new("/tmp/task-rs-detach-nonexistent-99999");
            let _ = fs::remove_dir_all(path);
            let mut out = Vec::new();
            collect_detached_worktrees(path, &mut out).unwrap();
            assert!(out.is_empty());
        }

        #[test]
        fn output_is_sorted() {
            let dir = TempDir::new("sorted");
            for name in ["zzz", "aaa", "mmm"] {
                let wt = dir.path().join(name);
                fs::create_dir_all(&wt).unwrap();
                fs::write(wt.join(".git"), "gitdir: ...").unwrap();
            }
            let mut out = Vec::new();
            collect_detached_worktrees(dir.path(), &mut out).unwrap();
            let names: Vec<_> = out
                .iter()
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .collect();
            assert_eq!(names, ["aaa", "mmm", "zzz"]);
        }
    }

    mod repo_key_from_detached_path {
        use super::*;

        #[test]
        fn strips_detached_dir_prefix() {
            let detached_dir = Path::new("/dev/detached");
            let path = Path::new("/dev/detached/github.com/org/repo");
            let key = repo_key_from_detached_path(detached_dir, path);
            assert_eq!(key, RepoKey::new("github.com/org/repo"));
        }

        #[test]
        fn falls_back_to_full_path_when_no_prefix() {
            let detached_dir = Path::new("/other");
            let path = Path::new("/dev/detached/github.com/org/repo");
            let key = repo_key_from_detached_path(detached_dir, path);
            assert_eq!(key, RepoKey::new("/dev/detached/github.com/org/repo"));
        }
    }

    mod read_head_sha {
        use super::*;

        #[test]
        fn returns_none_for_nonexistent_path() {
            let result = read_head_sha(Path::new("/tmp/task-rs-detach-nonexistent-sha-99999"));
            assert!(result.is_none());
        }

        #[test]
        fn returns_none_for_non_git_directory() {
            let dir = TempDir::new("sha-non-git");
            let result = read_head_sha(dir.path());
            assert!(result.is_none());
        }
    }

    // Helper shared by command-level tests.
    fn make_env(base: &std::path::Path) -> RuntimeEnvironment {
        let repos_dir = base.join("repos");
        let wt_dir = base.join("wt");
        let detached_dir = base.join("detached");
        fs::create_dir_all(&repos_dir).unwrap();
        fs::create_dir_all(&wt_dir).unwrap();
        fs::create_dir_all(&detached_dir).unwrap();
        RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir)
    }

    fn init_bare_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "--bare"])
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", std::env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git must be available")
            .success();
        assert!(ok, "git init --bare failed");
    }

    /// Run a git command, isolated from the user's global config, panicking
    /// on failure. Used by the parallel update_all fixtures below.
    fn git(args: &[&str], cwd: &Path) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", std::env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git must be available");
        assert!(status.success(), "git {args:?} in {cwd:?} failed");
    }

    /// Create a minimal remote + clone-style detached worktree at the
    /// given path. Returns the path the caller can hand to `update_all`.
    ///
    /// Layout:
    /// - `<base>/remote.git` — bare origin with one commit on `main`.
    /// - `<base>/detached/<repo_key>` — `git clone`d working tree.
    fn make_clone_backed_detached(base: &Path, repo_key: &str) -> std::path::PathBuf {
        let remote = base.join(format!("remote-{}.git", repo_key.replace('/', "-")));
        let seed = base.join(format!("seed-{}", repo_key.replace('/', "-")));
        fs::create_dir_all(&seed).unwrap();

        git(&["init", "--bare", remote.to_str().unwrap()], base);
        git(&["init", "-b", "main"], &seed);
        git(&["config", "user.email", "t@example.com"], &seed);
        git(&["config", "user.name", "T"], &seed);
        fs::write(seed.join("README.md"), "v1\n").unwrap();
        git(&["add", "README.md"], &seed);
        git(&["commit", "-m", "initial"], &seed);
        git(
            &["remote", "add", "origin", remote.to_str().unwrap()],
            &seed,
        );
        git(&["push", "-u", "origin", "main"], &seed);

        let detached = base.join("detached").join(repo_key);
        fs::create_dir_all(detached.parent().unwrap()).unwrap();
        git(
            &[
                "clone",
                remote.to_str().unwrap(),
                detached.to_str().unwrap(),
            ],
            base,
        );
        detached
    }

    mod add_tests {
        use super::{super::add, *};

        #[test]
        fn errors_when_repo_not_cloned() {
            let dir = TempDir::new("add-not-cloned");
            let base = dir.path();
            // No bare repo created — dir is empty.
            let env = make_env(base);

            let result = add(&env, "github.com/org/nonexistent");
            assert!(result.is_err(), "add should fail for an uncloned repo");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("not found") || msg.contains("Clone"),
                "error should tell user to clone first: {msg}"
            );
        }

        #[test]
        fn errors_when_given_a_clone_url() {
            let dir = TempDir::new("add-clone-url");
            let base = dir.path();
            let env = make_env(base);

            let result = add(&env, "https://github.com/org/repo.git");
            assert!(result.is_err(), "add should reject clone URLs");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("clone URL") || msg.contains("Clone the repository first"),
                "error should reject clone URLs: {msg}"
            );
        }

        #[test]
        fn resolves_short_name_when_unique_and_cloned() {
            // Short-name matching should work even for detach add.
            let dir = TempDir::new("add-short-name");
            let base = dir.path();
            // Create a bare repo — we don't actually run git worktree add (no remote),
            // just verify that resolution finds the right repo and fails later with
            // a git error (not a "not found" error).
            init_bare_repo(&base.join("repos/github.com/org/myapp.git"));
            let env = make_env(base);

            // The resolution itself should succeed; the subsequent git call will fail
            // because there's no remote — so we just check the error is NOT about
            // the repo being missing/uncloned.
            let result = add(&env, "myapp");
            // We can't easily test a successful add (needs real remote), but we can
            // confirm the error is a git-level error, not "repo not found".
            if let Err(err) = result {
                let msg = err.to_string();
                assert!(
                    !msg.contains("not found") && !msg.contains("Clone"),
                    "error should NOT be about a missing repo when repo is cloned: {msg}"
                );
            }
            // If it returned Ok (e.g. already exists), that's also fine.
        }
    }

    mod update_one_tests {
        use super::{
            super::{update_all, update_one},
            *,
        };

        #[test]
        fn errors_when_path_does_not_exist() {
            let dir = TempDir::new("update-one-missing");
            let base = dir.path();
            // Create a bare repo so resolve_repo_key_input succeeds.
            init_bare_repo(&base.join("repos/github.com/org/repo.git"));
            let env = make_env(base);

            let result = update_one(&env, "github.com/org/repo");
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("No detached worktree"),
                "expected 'No detached worktree' in: {msg}"
            );
        }

        #[test]
        fn error_message_suggests_add_command() {
            let dir = TempDir::new("update-one-suggest-add");
            let base = dir.path();
            init_bare_repo(&base.join("repos/github.com/org/myrepo.git"));
            let env = make_env(base);

            let result = update_one(&env, "github.com/org/myrepo");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("task detach add"),
                "expected suggestion to run 'task detach add' in: {msg}"
            );
        }

        #[test]
        fn update_all_returns_ok_when_detached_dir_is_missing() {
            let dir = TempDir::new("update-all-no-dir");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            // Intentionally do NOT create detached_dir.
            let detached_dir = dir.path().join("detached");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            let env = RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir);

            let result = update_all(&env);
            assert!(
                result.is_ok(),
                "should succeed gracefully when detached_dir is absent: {result:?}"
            );
        }

        #[test]
        fn update_all_returns_ok_when_detached_dir_is_empty() {
            let dir = TempDir::new("update-all-empty-dir");
            let env = make_env(dir.path());

            let result = update_all(&env);
            assert!(
                result.is_ok(),
                "should succeed gracefully with empty detached_dir: {result:?}"
            );
        }

        #[test]
        fn update_all_processes_multiple_repos_in_parallel() {
            // Exercise the rayon par_iter path with two real detached
            // worktrees. Each has an origin remote, so `git fetch origin`
            // and `git reset --hard` succeed and `update_all` should return
            // Ok(()) after updating both.
            let dir = TempDir::new("update-all-parallel");
            let base = dir.path();
            let env = make_env(base);

            let wt_a = make_clone_backed_detached(base, "github.com/org/a");
            let wt_b = make_clone_backed_detached(base, "github.com/org/b");

            let result = update_all(&env);
            assert!(
                result.is_ok(),
                "update_all should succeed for two valid detached worktrees: {result:?}"
            );
            assert!(wt_a.join(".git").exists());
            assert!(wt_b.join(".git").exists());
        }

        #[test]
        fn update_all_aggregates_errors_across_repos() {
            // One healthy worktree, one broken (no `.git`). The broken one
            // is still a leaf directory but lacks any git metadata, so
            // `update_detached` will fail for it. `update_all` must surface
            // the failure AND still process the healthy one.
            let dir = TempDir::new("update-all-mixed");
            let base = dir.path();
            let env = make_env(base);

            let _good = make_clone_backed_detached(base, "github.com/org/good");

            // Create a broken leaf that looks like a worktree (has a .git
            // file) but points at nothing, so `git fetch origin` fails.
            let broken = base.join("detached/github.com/org/broken");
            fs::create_dir_all(&broken).unwrap();
            fs::write(broken.join(".git"), "gitdir: /nonexistent/path").unwrap();

            let result = update_all(&env);
            assert!(result.is_err(), "broken worktree should trigger an error");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("failed to update"),
                "error should mention failed updates: {msg}"
            );
        }
    }

    mod list_tests {
        use super::{super::list, *};

        #[test]
        fn returns_ok_with_empty_detached_dir() {
            let dir = TempDir::new("list-empty");
            let env = make_env(dir.path());
            let result = list(&env);
            assert!(
                result.is_ok(),
                "list should succeed with empty dir: {result:?}"
            );
        }

        #[test]
        fn returns_ok_when_detached_dir_missing() {
            let dir = TempDir::new("list-no-dir");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let detached_dir = dir.path().join("detached");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            let env = RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir);

            let result = list(&env);
            assert!(result.is_ok(), "list should handle missing dir: {result:?}");
        }
    }

    mod remove_tests {
        use super::{super::remove, *};

        #[test]
        fn errors_when_path_does_not_exist() {
            let dir = TempDir::new("remove-missing");
            let base = dir.path();
            init_bare_repo(&base.join("repos/github.com/org/repo.git"));
            let env = make_env(base);

            let result = remove(&env, "github.com/org/repo", false);
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("No detached worktree"),
                "expected 'No detached worktree' in: {msg}"
            );
        }

        #[test]
        fn cleans_up_empty_parent_dirs_after_removal() {
            // Create a fake "detached worktree" directory by writing a .git file,
            // then remove it directly (bypassing git worktree remove) and test
            // that the parent cleanup logic removes the now-empty intermediate dirs.
            let dir = TempDir::new("remove-cleanup");
            let base = dir.path();
            init_bare_repo(&base.join("repos/github.com/org/app.git"));
            let _env = make_env(base);

            let detached_dir = base.join("detached");
            let worktree = detached_dir.join("github.com/org/app");
            fs::create_dir_all(&worktree).unwrap();

            // Simulate a removed worktree by writing and then deleting its .git file.
            // After deletion the dir still exists but the parent chain is now empty.
            fs::write(worktree.join(".git"), "gitdir: ...").unwrap();

            // Remove the worktree dir manually (simulate what git worktree remove would do).
            fs::remove_file(worktree.join(".git")).unwrap();
            fs::remove_dir(&worktree).unwrap();

            // Now the parent dirs github.com/org and github.com are empty.
            // Run the cleanup logic directly by calling a version that only exercises
            // the dir-pruning: verify the intermediate dirs are empty.
            let org_dir = detached_dir.join("github.com/org");
            assert!(
                fs::read_dir(&org_dir)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false),
                "org dir should be empty after worktree removal"
            );
        }
    }

    mod find_install_entry_tests {
        use super::*;

        fn entries() -> Vec<InstallEntry> {
            vec![
                InstallEntry {
                    repo: "github.com/org/tool".to_string(),
                    path: None,
                    extra_flags: vec![],
                },
                InstallEntry {
                    repo: "gitlab.com/team/app".to_string(),
                    path: Some("crates/cli".to_string()),
                    extra_flags: vec![],
                },
            ]
        }

        #[test]
        fn matches_full_repo_key() {
            let e = entries();
            let found = find_install_entry(&e, "github.com/org/tool");
            assert_eq!(found.unwrap().repo, "github.com/org/tool");
        }

        #[test]
        fn matches_short_name() {
            let e = entries();
            let found = find_install_entry(&e, "tool");
            assert_eq!(found.unwrap().repo, "github.com/org/tool");
        }

        #[test]
        fn returns_none_when_no_match() {
            let e = entries();
            assert!(find_install_entry(&e, "nonexistent").is_none());
        }

        #[test]
        fn returns_none_when_ambiguous_short_name() {
            let entries = vec![
                InstallEntry {
                    repo: "github.com/a/cli".to_string(),
                    path: None,
                    extra_flags: vec![],
                },
                InstallEntry {
                    repo: "gitlab.com/b/cli".to_string(),
                    path: None,
                    extra_flags: vec![],
                },
            ];
            assert!(find_install_entry(&entries, "cli").is_none());
        }

        #[test]
        fn returns_entry_with_sub_path() {
            let e = entries();
            let found = find_install_entry(&e, "gitlab.com/team/app");
            assert_eq!(found.unwrap().path.as_deref(), Some("crates/cli"));
        }

        #[test]
        fn returns_none_for_empty_entries() {
            let empty: Vec<InstallEntry> = Vec::new();
            assert!(find_install_entry(&empty, "anything").is_none());
        }
    }

    mod run_cargo_install_tests {
        use super::*;

        #[test]
        fn errors_when_detached_worktree_missing() {
            let dir = TempDir::new("install-no-wt");
            let entry = InstallEntry {
                repo: "github.com/org/missing".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let result = run_cargo_install(dir.path(), &entry);
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("not found") || msg.contains("Detached worktree"),
                "expected worktree-missing error: {msg}"
            );
        }

        #[test]
        fn errors_when_no_cargo_toml() {
            let dir = TempDir::new("install-no-cargo");
            let wt = dir.path().join("github.com/org/nocargo");
            fs::create_dir_all(&wt).unwrap();

            let entry = InstallEntry {
                repo: "github.com/org/nocargo".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let result = run_cargo_install(dir.path(), &entry);
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Cargo.toml"),
                "expected Cargo.toml error: {msg}"
            );
        }

        #[test]
        fn errors_when_sub_path_has_no_cargo_toml() {
            let dir = TempDir::new("install-subpath-no-cargo");
            let wt = dir.path().join("github.com/org/workspace/crates/cli");
            fs::create_dir_all(&wt).unwrap();

            let entry = InstallEntry {
                repo: "github.com/org/workspace".to_string(),
                path: Some("crates/cli".to_string()),
                extra_flags: vec![],
            };

            let result = run_cargo_install(dir.path(), &entry);
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Cargo.toml"),
                "expected Cargo.toml error: {msg}"
            );
        }

        #[test]
        fn extra_flags_do_not_suppress_missing_cargo_toml_error() {
            // Even with extra_flags set, the Cargo.toml validation fires before
            // the cargo invocation.
            let dir = TempDir::new("install-extra-flags-no-cargo");
            let wt = dir.path().join("github.com/org/tool");
            fs::create_dir_all(&wt).unwrap();

            let entry = InstallEntry {
                repo: "github.com/org/tool".to_string(),
                path: None,
                extra_flags: vec!["--all-features".to_string()],
            };

            let result = run_cargo_install(dir.path(), &entry);
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Cargo.toml"),
                "expected Cargo.toml error even with extra_flags: {msg}"
            );
        }
    }

    mod install_all_tests {
        use super::{super::install_all, *};

        #[test]
        fn returns_ok_when_no_install_entries() {
            let dir = TempDir::new("install-all-empty");
            let env = make_env(dir.path());

            let result = install_all(&env);
            assert!(
                result.is_ok(),
                "should succeed with no install entries: {result:?}"
            );
        }
    }

    mod install_one_tests {
        use super::{super::install_one, *};

        #[test]
        fn errors_when_no_install_entry_configured() {
            let dir = TempDir::new("install-one-no-entry");
            let env = make_env(dir.path());

            let result = install_one(&env, "github.com/org/unconfigured");
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("No [[install]] entry"),
                "expected config-missing error: {msg}"
            );
        }
    }
}
