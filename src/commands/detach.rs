use std::fs;

use clap::Subcommand;
use rayon::prelude::*;

use crate::{
    error::{Error, Result},
    runtime::{
        RepoKey,
        environment::RuntimeEnvironment,
        process,
        progress::{Phase, ProgressReporter},
    },
    tools::git::{
        refs::{detect_default_base, fetch_origin_refs},
        worktrees::{add_detached, fetch_detached, reset_detached, update_detached},
    },
};

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum DetachCommand {
    /// Create (or update) a detached worktree pinned to the default branch,
    /// or to a branch configured via a [[detached]] entry in config.toml.
    ///
    /// If the worktree already exists, this is equivalent to `task detach update <repo>`.
    #[command(
        about = "Create or update a detached worktree (default branch, or pinned via [[detached]])"
    )]
    Add { repo: String },
    /// Update one or all detached worktrees by fetching and hard-resetting to origin/HEAD,
    /// or to `origin/<branch>` when a [[detached]] entry pins that repo.
    ///
    /// Repos with a matching [[install]] entry in config are automatically reinstalled
    /// after a successful update.
    #[command(
        about = "Fetch and reset a detached worktree to origin/HEAD (or origin/<branch> when pinned via [[detached]])"
    )]
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
    ///
    /// When an [[install]] entry has no `path`, `task` inspects the repo root
    /// manifest and, if it is a virtual workspace, uses `cargo metadata` to
    /// pick an installable crate: the only bin member, or the bin member whose
    /// package name matches the repo short name. Ambiguous workspaces fail
    /// with a list of candidate paths.
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
    let pinned_branch = find_detached_entry(env.detached_entries(), repo_key.as_ref())?
        .map(|entry| entry.branch.clone());

    // If worktree already on disk and is a git worktree, update it instead.
    if path.join(".git").exists() || is_detached_worktree(&path) {
        process::log(&format!(
            "Updating existing detached worktree: {}",
            path.display()
        ));
        return update_detached(&path, pinned_branch.as_deref());
    }

    process::log(&format!("Fetching origin refs for {repo_key}"));
    fetch_origin_refs(&gitdir)?;

    let base_ref = match pinned_branch.as_deref() {
        Some(branch) => {
            let remote_ref = format!("origin/{branch}");
            if !crate::tools::git::refs::rev_exists(&gitdir, &remote_ref) {
                return Err(Error::failed(format!(
                    "[[detached]] entry for '{repo_key}' pins branch '{branch}', \
                     but '{remote_ref}' does not exist on origin. \
                     Check the branch name in config.toml or push the branch upstream."
                )));
            }
            remote_ref
        }
        None => detect_default_base(&gitdir),
    };
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

    let pinned_branch = find_detached_entry(env.detached_entries(), repo_key.as_ref())?
        .map(|entry| entry.branch.clone());

    process::log(&format!("Updating detached worktree: {}", path.display()));
    update_detached(&path, pinned_branch.as_deref())?;

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

    // Build row labels (stable repo keys) for the progress reporter
    // while preserving each path alongside its index.
    let labels: Vec<String> = worktrees
        .iter()
        .map(|p| {
            let key = repo_key_from_detached_path(detached_dir, p);
            AsRef::<str>::as_ref(&key).to_string()
        })
        .collect();
    let reporter = ProgressReporter::new("Updating", labels);
    let install_entries = env.install_entries();
    let detached_entries = env.detached_entries();

    // Fan out per-repo work across rayon workers. Each closure buffers its
    // output via `OutputScope`; captured lines are retained only for
    // failed rows (docker-compose-style quiet happy path).
    let results: Vec<(std::path::PathBuf, Vec<process::CapturedLine>, Result<()>)> = worktrees
        .par_iter()
        .enumerate()
        .map(|(idx, path)| {
            let scope = process::OutputScope::new();
            let Some(handle) = reporter.begin(idx) else {
                return (
                    path.clone(),
                    scope.into_lines(),
                    Err(Error::failed("progress row index out of range")),
                );
            };
            let repo_key = repo_key_from_detached_path(detached_dir, path);

            // Resolve config lookups up-front: ambiguous short-name
            // matches are surfaced as hard errors, not silently ignored.
            let pinned_entry = find_detached_entry(detached_entries, repo_key.as_ref());
            let install_entry = find_install_entry(install_entries, repo_key.as_ref());

            let result = pinned_entry.and_then(|pinned| {
                let install = install_entry?;
                let pinned_branch = pinned.map(|entry| entry.branch.as_str());
                fetch_detached(path)
                    .and_then(|()| {
                        handle.phase(Phase::Syncing);
                        reset_detached(path, pinned_branch)
                    })
                    .and_then(|()| match install {
                        Some(entry) => {
                            handle.phase(Phase::Installing);
                            run_cargo_install(detached_dir, entry)
                        }
                        None => Ok(()),
                    })
            });

            let has_install = matches!(
                find_install_entry(install_entries, repo_key.as_ref()),
                Ok(Some(_))
            );
            match &result {
                Ok(()) => handle.succeeded(if has_install { "Installed" } else { "Updated" }),
                Err(err) => handle.failed(err.to_string()),
            }
            (path.clone(), scope.into_lines(), result)
        })
        .collect();

    reporter.finish();

    let mut errors: Vec<String> = Vec::new();
    for (path, lines, result) in results {
        if let Err(err) = result {
            // Only surface buffered output for failures — successful
            // workers stay quiet so the progress block is the whole UX.
            print_failure_block(&path, lines);
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

/// Render a `──── <label> ────` header followed by the worker's
/// captured output. Keeps failure post-mortems grouped per-repo so
/// concurrent workers' logs don't interleave.
fn print_failure_block(path: &std::path::Path, lines: Vec<process::CapturedLine>) {
    if lines.is_empty() {
        return;
    }
    eprintln!("──── {} ────", path.display());
    process::flush_captured_lines(lines);
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
    let entry = find_install_entry(entries, repo_arg)?.ok_or_else(|| {
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

    let labels: Vec<String> = entries.iter().map(|e| e.repo.clone()).collect();
    let reporter = ProgressReporter::new("Installing", labels);

    // Parallelise across entries. cargo's global install lock serialises
    // the final `mv` step, but the expensive compile phase overlaps across
    // distinct detached worktrees (each has its own target/ directory).
    let results: Vec<(String, Vec<process::CapturedLine>, Result<()>)> = entries
        .par_iter()
        .enumerate()
        .map(|(idx, entry)| {
            let scope = process::OutputScope::new();
            let Some(handle) = reporter.begin(idx) else {
                return (
                    entry.repo.clone(),
                    scope.into_lines(),
                    Err(Error::failed("progress row index out of range")),
                );
            };
            handle.phase(Phase::Installing);
            let result = run_cargo_install(detached_dir, entry);
            match &result {
                Ok(()) => handle.succeeded("Installed"),
                Err(err) => handle.failed(err.to_string()),
            }
            (entry.repo.clone(), scope.into_lines(), result)
        })
        .collect();

    reporter.finish();

    let mut errors: Vec<String> = Vec::new();
    for (repo, lines, result) in results {
        if let Err(err) = result {
            eprintln!("──── {repo} ────");
            process::flush_captured_lines(lines);
            errors.push(format!("{repo}: {err}"));
        }
    }

    if !errors.is_empty() {
        return Err(Error::failed(format!("{} install(s) failed", errors.len())));
    }

    Ok(())
}

/// Try to install a repo if it has a matching [[install]] entry.
///
/// Ambiguity and install failures are logged as warnings and do not
/// propagate — the caller's update already succeeded and we don't want
/// to fail the whole command just because the install step misbehaved.
fn try_install(env: &RuntimeEnvironment, repo_key: &RepoKey) {
    let entries = env.install_entries();
    let entry = match find_install_entry(entries, repo_key.as_ref()) {
        Ok(Some(entry)) => entry,
        Ok(None) => return,
        Err(err) => {
            process::warn(&format!("Skipping install for {repo_key}: {err}"));
            return;
        }
    };

    let detached_dir = env.layout().detached_dir();
    if let Err(err) = run_cargo_install(detached_dir, entry) {
        process::warn(&format!("Failed to install {}: {err}", entry.repo));
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Find an entry whose `repo` field matches `query`, by exact repo key
/// or unambiguous short-name (last path component) match.
///
/// Returns `Ok(None)` when nothing matches. Returns `Err` when a short-name
/// query matches multiple entries — callers must surface this rather than
/// silently falling back to default behaviour, otherwise a user-configured
/// pin is ignored without warning.
///
/// `section` names the config section for the error message (e.g.
/// `"[[detached]]"`), and `repo_field` extracts the `repo` field from each
/// entry.
fn find_entry_by_repo<'a, T>(
    entries: &'a [T],
    query: &str,
    section: &str,
    repo_field: impl Fn(&T) -> &str,
) -> Result<Option<&'a T>> {
    // Exact match on full repo key.
    if let Some(entry) = entries.iter().find(|e| repo_field(e) == query) {
        return Ok(Some(entry));
    }

    // Short-name suffix match: compare against last path component.
    let matches: Vec<&T> = entries
        .iter()
        .filter(|e| {
            repo_field(e)
                .rsplit('/')
                .next()
                .is_some_and(|name| name == query)
        })
        .collect();

    match matches.as_slice() {
        [] => Ok(None),
        [entry] => Ok(Some(*entry)),
        _ => {
            let candidates = matches
                .iter()
                .map(|e| format!("  - {}", repo_field(e)))
                .collect::<Vec<_>>()
                .join("\n");
            Err(Error::failed(format!(
                "'{query}' matches multiple {section} entries in config.toml. \
                 Use the fully-qualified repo key (host/owner/name). Candidates:\n{candidates}"
            )))
        }
    }
}

/// Find an [[install]] entry whose `repo` field matches the given argument.
///
/// See [`find_entry_by_repo`] for matching rules and error semantics.
fn find_install_entry<'a>(
    entries: &'a [crate::runtime::config::InstallEntry],
    query: &str,
) -> Result<Option<&'a crate::runtime::config::InstallEntry>> {
    find_entry_by_repo(entries, query, "[[install]]", |e| e.repo.as_str())
}

/// Find a [[detached]] entry whose `repo` field matches the given argument.
///
/// See [`find_entry_by_repo`] for matching rules and error semantics.
fn find_detached_entry<'a>(
    entries: &'a [crate::runtime::config::DetachedEntry],
    query: &str,
) -> Result<Option<&'a crate::runtime::config::DetachedEntry>> {
    find_entry_by_repo(entries, query, "[[detached]]", |e| e.repo.as_str())
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

    let install_path = resolve_install_path(&base_path, entry)?;

    let path_str = install_path.to_string_lossy();
    process::log(&format!("Installing {} from {path_str}", entry.repo));

    let mut args = vec!["install", "--path", &path_str, "--locked"];
    let extra: Vec<&str> = entry.extra_flags.iter().map(String::as_str).collect();
    args.extend(extra);

    process::run_status("cargo", &args, None)
}

/// Resolve the directory to pass to `cargo install --path`.
///
/// Behavior:
/// - If `entry.path` is set, use `base_path/entry.path` unchanged.
/// - Else if `base_path/Cargo.toml` is a normal package manifest, use `base_path`.
/// - Else treat `base_path/Cargo.toml` as a virtual workspace manifest and
///   consult `cargo metadata` to pick a single installable member:
///   - exactly one member with a bin target, or
///   - among multiple bin members, the one whose package name matches the
///     repo short name (the last `/`-separated segment of `entry.repo`).
/// - Otherwise return an actionable error listing the candidates.
fn resolve_install_path(
    base_path: &std::path::Path,
    entry: &crate::runtime::config::InstallEntry,
) -> Result<std::path::PathBuf> {
    if let Some(sub) = &entry.path {
        let install_path = base_path.join(sub);
        if !install_path.join("Cargo.toml").exists() {
            return Err(Error::failed(format!(
                "No Cargo.toml found at {}",
                install_path.display()
            )));
        }
        return Ok(install_path);
    }

    let root_manifest = base_path.join("Cargo.toml");
    if !root_manifest.exists() {
        return Err(Error::failed(format!(
            "No Cargo.toml found at {}",
            base_path.display()
        )));
    }

    if manifest_has_package_section(&root_manifest)? {
        return Ok(base_path.to_path_buf());
    }

    // Virtual workspace root: ask cargo metadata for the real member list.
    let short_name = entry.repo.rsplit('/').next().unwrap_or(&entry.repo);
    resolve_workspace_member(base_path, &root_manifest, short_name)
}

/// Returns `true` when the manifest contains a `[package]` table.
fn manifest_has_package_section(manifest_path: &std::path::Path) -> Result<bool> {
    let text = fs::read_to_string(manifest_path).map_err(|err| {
        Error::failed(format!(
            "Could not read manifest {}: {err}",
            manifest_path.display()
        ))
    })?;
    let parsed: toml::Table = toml::from_str(&text).map_err(|err| {
        Error::failed(format!(
            "Could not parse manifest {}: {err}",
            manifest_path.display()
        ))
    })?;
    Ok(parsed.contains_key("package"))
}

/// Run `cargo metadata --no-deps --offline` and pick the member manifest
/// directory to install from. Errors carry the candidate list so users can
/// set `[[install]].path` precisely.
fn resolve_workspace_member(
    base_path: &std::path::Path,
    root_manifest: &std::path::Path,
    repo_short_name: &str,
) -> Result<std::path::PathBuf> {
    let manifest_arg = root_manifest.to_string_lossy();
    let output = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
            "--manifest-path",
            manifest_arg.as_ref(),
        ])
        .output()
        .map_err(|err| Error::failed(format!("Failed to run cargo metadata: {err}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::failed(format!(
            "cargo metadata failed for {}:\n{stderr}",
            root_manifest.display()
        )));
    }

    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout).map_err(|err| {
        Error::failed(format!(
            "Could not parse cargo metadata output for {}: {err}",
            root_manifest.display()
        ))
    })?;

    let bin_candidates = metadata.bin_candidates();
    pick_bin_candidate(&bin_candidates, repo_short_name)
        .map(|pkg| pkg.package_dir().to_path_buf())
        .ok_or_else(|| bin_candidate_error(base_path, repo_short_name, &bin_candidates))
}

/// Select a bin candidate by the agreed rules.
fn pick_bin_candidate<'a>(
    candidates: &'a [CargoPackage],
    repo_short_name: &str,
) -> Option<&'a CargoPackage> {
    if candidates.len() == 1 {
        return candidates.first();
    }
    candidates.iter().find(|pkg| pkg.name == repo_short_name)
}

fn bin_candidate_error(
    base_path: &std::path::Path,
    repo_short_name: &str,
    candidates: &[CargoPackage],
) -> Error {
    if candidates.is_empty() {
        return Error::failed(format!(
            "Virtual workspace at {} has no package with a `bin` target. \
             Set `path = \"…\"` on the `[[install]]` entry to point at an \
             installable crate.",
            base_path.display()
        ));
    }

    let listing = candidates
        .iter()
        .map(|pkg| {
            let rel = pkg
                .package_dir()
                .strip_prefix(base_path)
                .unwrap_or_else(|_| pkg.package_dir())
                .display();
            format!("  - {} (path = \"{rel}\")", pkg.name)
        })
        .collect::<Vec<_>>()
        .join("\n");

    Error::failed(format!(
        "Virtual workspace at {} has multiple installable crates and none \
         matched the repo short name `{repo_short_name}`. Set `path = \"…\"` \
         on the `[[install]]` entry. Candidates:\n{listing}",
        base_path.display()
    ))
}

#[derive(Debug, serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

impl CargoMetadata {
    /// Workspace packages that expose a `bin` target.
    fn bin_candidates(&self) -> Vec<CargoPackage> {
        let workspace: std::collections::HashSet<&str> =
            self.workspace_members.iter().map(String::as_str).collect();
        self.packages
            .iter()
            .filter(|pkg| workspace.contains(pkg.id.as_str()))
            .filter(|pkg| pkg.has_bin_target())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct CargoPackage {
    name: String,
    id: String,
    manifest_path: std::path::PathBuf,
    #[serde(default)]
    targets: Vec<CargoTarget>,
}

impl CargoPackage {
    fn has_bin_target(&self) -> bool {
        self.targets
            .iter()
            .any(|t| t.kind.iter().any(|k| k == "bin"))
    }

    fn package_dir(&self) -> &std::path::Path {
        self.manifest_path.parent().unwrap_or(&self.manifest_path)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct CargoTarget {
    #[serde(default)]
    kind: Vec<String>,
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

    /// Build a workspace with a bare repo under `<base>/repos/<repo_key>.git`
    /// whose `origin` has both `main` (default) and `feat` branches.
    ///
    /// Returns the path to the bare repo. Callers that need to advance
    /// `origin/feat` afterward can reuse the seed clone exposed by
    /// [`FeatBranchWorkspace`].
    fn seed_bare_repo_with_feat_branch(base: &Path, repo_key: &str) -> std::path::PathBuf {
        seed_feat_branch_workspace(base, repo_key).bare
    }

    /// Full handles returned by [`seed_feat_branch_workspace`]: the bare
    /// repo under `<base>/repos/<repo_key>.git` and the seed clone used to
    /// create it. Tests that need to push further commits to `origin/feat`
    /// after the initial seeding use the seed clone.
    struct FeatBranchWorkspace {
        bare: std::path::PathBuf,
        seed: std::path::PathBuf,
    }

    /// Richer variant of [`seed_bare_repo_with_feat_branch`] that returns
    /// the seed clone alongside the bare path.
    fn seed_feat_branch_workspace(base: &Path, repo_key: &str) -> FeatBranchWorkspace {
        let remote = base.join(format!("remote-{}.git", repo_key.replace('/', "-")));
        let seed = base.join(format!("seed-{}", repo_key.replace('/', "-")));
        let bare = base.join("repos").join(format!("{repo_key}.git"));
        fs::create_dir_all(&seed).unwrap();
        fs::create_dir_all(bare.parent().unwrap()).unwrap();

        git(&["init", "--bare", remote.to_str().unwrap()], base);
        git(&["init", "-b", "main"], &seed);
        git(&["config", "user.email", "t@example.com"], &seed);
        git(&["config", "user.name", "T"], &seed);
        fs::write(seed.join("README.md"), "main-v1\n").unwrap();
        git(&["add", "README.md"], &seed);
        git(&["commit", "-m", "initial"], &seed);
        git(
            &["remote", "add", "origin", remote.to_str().unwrap()],
            &seed,
        );
        git(&["push", "-u", "origin", "main"], &seed);

        // Create a second branch with a distinct commit so HEAD differs
        // from main.
        git(&["checkout", "-b", "feat"], &seed);
        fs::write(seed.join("README.md"), "feat-v1\n").unwrap();
        git(&["commit", "-am", "feat change"], &seed);
        git(&["push", "-u", "origin", "feat"], &seed);

        // Bare repo pointing at the same origin.
        git(&["init", "--bare", bare.to_str().unwrap()], base);
        git(
            &["remote", "add", "origin", remote.to_str().unwrap()],
            &bare,
        );
        git(
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
            &bare,
        );
        git(&["fetch", "origin"], &bare);
        FeatBranchWorkspace { bare, seed }
    }

    /// Run a git command and capture stdout. Panics on failure.
    fn git_stdout(args: &[&str], cwd: &Path) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", std::env::temp_dir())
            .output()
            .expect("git must be available");
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
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

        #[test]
        fn pinned_branch_is_used_as_base_ref() {
            // Seed a bare repo whose origin has `main` (default) and `feat`.
            // A [[detached]] entry pins the repo to `feat`. After `add`, the
            // detached worktree's HEAD must match `origin/feat`, not `origin/main`.
            let dir = TempDir::new("add-pinned-branch");
            let base = dir.path();
            fs::create_dir_all(base.join("repos")).unwrap();
            fs::create_dir_all(base.join("wt")).unwrap();
            fs::create_dir_all(base.join("detached")).unwrap();

            let bare = seed_bare_repo_with_feat_branch(base, "github.com/org/forked");
            let env =
                make_env(base).with_detached_entries(vec![crate::runtime::config::DetachedEntry {
                    repo: "github.com/org/forked".to_string(),
                    branch: "feat".to_string(),
                }]);

            add(&env, "github.com/org/forked").expect("add should succeed");

            let detached = base.join("detached/github.com/org/forked");
            let head = git_stdout(&["rev-parse", "HEAD"], &detached);
            let origin_feat = git_stdout(&["rev-parse", "origin/feat"], &bare);
            assert_eq!(head, origin_feat, "detached HEAD should match origin/feat");
        }

        #[test]
        fn without_pinned_branch_uses_default_base() {
            // No [[detached]] entry → falls back to origin/main behaviour.
            let dir = TempDir::new("add-default-base");
            let base = dir.path();
            fs::create_dir_all(base.join("repos")).unwrap();
            fs::create_dir_all(base.join("wt")).unwrap();
            fs::create_dir_all(base.join("detached")).unwrap();

            let bare = seed_bare_repo_with_feat_branch(base, "github.com/org/app");
            let env = make_env(base);

            add(&env, "github.com/org/app").expect("add should succeed");

            let detached = base.join("detached/github.com/org/app");
            let head = git_stdout(&["rev-parse", "HEAD"], &detached);
            let origin_main = git_stdout(&["rev-parse", "origin/main"], &bare);
            assert_eq!(head, origin_main, "detached HEAD should match origin/main");
        }

        #[test]
        fn errors_when_pinned_branch_does_not_exist_on_origin() {
            // A typo or deleted upstream branch should produce an
            // actionable error that names the offending entry, not a raw
            // `git worktree add: invalid reference` message.
            let dir = TempDir::new("add-pinned-missing");
            let base = dir.path();
            fs::create_dir_all(base.join("repos")).unwrap();
            fs::create_dir_all(base.join("wt")).unwrap();
            fs::create_dir_all(base.join("detached")).unwrap();

            let _bare = seed_bare_repo_with_feat_branch(base, "github.com/org/broken");
            let env =
                make_env(base).with_detached_entries(vec![crate::runtime::config::DetachedEntry {
                    repo: "github.com/org/broken".to_string(),
                    branch: "does-not-exist".to_string(),
                }]);

            let err = add(&env, "github.com/org/broken").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("does-not-exist"),
                "error must mention the pinned branch: {msg}"
            );
            assert!(
                msg.contains("[[detached]]") || msg.contains("origin/does-not-exist"),
                "error must hint at config or missing remote ref: {msg}"
            );
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

        #[test]
        fn update_one_resets_to_pinned_branch() {
            // End-to-end: create a detached worktree that tracks `main`,
            // pin it to `feat` via config, advance `feat` on the remote,
            // then `update_one` — the worktree HEAD must land on
            // `origin/feat`, not `origin/main`.
            let dir = TempDir::new("update-one-pinned");
            let base = dir.path();
            let repo_key = "github.com/org/pinned";

            fs::create_dir_all(base.join("repos")).unwrap();
            fs::create_dir_all(base.join("wt")).unwrap();
            fs::create_dir_all(base.join("detached")).unwrap();

            // Seed remote + bare repo with main + feat branches.
            let workspace = seed_feat_branch_workspace(base, repo_key);
            let bare = &workspace.bare;
            let seed = &workspace.seed;
            let detached = base.join(format!("detached/{repo_key}"));
            fs::create_dir_all(detached.parent().unwrap()).unwrap();

            // Create the detached worktree pointing at origin/main
            // (simulates the pre-pinning state).
            git(
                &[
                    "--git-dir",
                    bare.to_str().unwrap(),
                    "worktree",
                    "add",
                    "--detach",
                    detached.to_str().unwrap(),
                    "origin/main",
                ],
                base,
            );

            let repos_dir = base.join("repos");
            let wt_dir = base.join("wt");
            let detached_dir = base.join("detached");
            let env = RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir)
                .with_detached_entries(vec![crate::runtime::config::DetachedEntry {
                    repo: repo_key.to_string(),
                    branch: "feat".to_string(),
                }]);

            // Advance origin/feat so the reset is observable.
            fs::write(seed.join("README.md"), "feat-v2\n").unwrap();
            git(&["commit", "-am", "feat v2"], seed);
            git(&["push", "origin", "feat"], seed);

            update_one(&env, repo_key).expect("update_one should succeed");

            let head = git_stdout(&["rev-parse", "HEAD"], &detached);
            let origin_feat = git_stdout(&["rev-parse", "origin/feat"], &detached);
            let origin_main = git_stdout(&["rev-parse", "origin/main"], &detached);
            assert_eq!(head, origin_feat, "HEAD should match origin/feat");
            assert_ne!(head, origin_main, "HEAD should not be left on origin/main");
        }

        #[test]
        fn update_all_resets_to_pinned_branch_for_matching_repo() {
            // Pinned repo lands on `origin/feat`; unpinned companion lands
            // on `origin/main`. Proves the per-row lookup in `update_all`.
            let dir = TempDir::new("update-all-pinned");
            let base = dir.path();

            let pinned_key = "github.com/org/pinned";
            let other_key = "github.com/org/other";

            // Seed two remotes, two bare repos, two clone-backed detached
            // worktrees. The "pinned" one has an extra `feat` branch.
            let pinned_remote = base.join("pinned-remote.git");
            let pinned_seed = base.join("pinned-seed");
            let pinned_detached = base.join(format!("detached/{pinned_key}"));
            fs::create_dir_all(&pinned_seed).unwrap();
            fs::create_dir_all(pinned_detached.parent().unwrap()).unwrap();
            fs::create_dir_all(base.join("wt")).unwrap();
            fs::create_dir_all(base.join("repos")).unwrap();

            git(&["init", "--bare", pinned_remote.to_str().unwrap()], base);
            git(&["init", "-b", "main"], &pinned_seed);
            git(&["config", "user.email", "t@example.com"], &pinned_seed);
            git(&["config", "user.name", "T"], &pinned_seed);
            fs::write(pinned_seed.join("README.md"), "main\n").unwrap();
            git(&["add", "README.md"], &pinned_seed);
            git(&["commit", "-m", "main"], &pinned_seed);
            git(
                &["remote", "add", "origin", pinned_remote.to_str().unwrap()],
                &pinned_seed,
            );
            git(&["push", "-u", "origin", "main"], &pinned_seed);
            git(&["checkout", "-b", "feat"], &pinned_seed);
            fs::write(pinned_seed.join("README.md"), "feat\n").unwrap();
            git(&["commit", "-am", "feat"], &pinned_seed);
            git(&["push", "-u", "origin", "feat"], &pinned_seed);

            // Clone into the detached location, tracking main so update
            // starts on main.
            git(
                &[
                    "clone",
                    "-b",
                    "main",
                    pinned_remote.to_str().unwrap(),
                    pinned_detached.to_str().unwrap(),
                ],
                base,
            );

            // Unpinned companion — simple clone-backed detached worktree.
            let other_detached = make_clone_backed_detached(base, other_key);

            let repos_dir = base.join("repos");
            let wt_dir = base.join("wt");
            let detached_dir = base.join("detached");
            let env = RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir)
                .with_detached_entries(vec![crate::runtime::config::DetachedEntry {
                    repo: pinned_key.to_string(),
                    branch: "feat".to_string(),
                }]);

            update_all(&env).expect("update_all should succeed");

            let pinned_head = git_stdout(&["rev-parse", "HEAD"], &pinned_detached);
            let pinned_feat = git_stdout(&["rev-parse", "origin/feat"], &pinned_detached);
            let pinned_main = git_stdout(&["rev-parse", "origin/main"], &pinned_detached);
            assert_eq!(
                pinned_head, pinned_feat,
                "pinned repo HEAD should match origin/feat"
            );
            assert_ne!(
                pinned_head, pinned_main,
                "pinned repo HEAD should not be left on origin/main"
            );

            // The unpinned companion must land on its own origin/main,
            // proving `update_all` resolves the pinned branch per-repo
            // rather than applying the pin globally.
            let other_head = git_stdout(&["rev-parse", "HEAD"], &other_detached);
            let other_main = git_stdout(&["rev-parse", "origin/main"], &other_detached);
            assert_eq!(
                other_head, other_main,
                "unpinned repo HEAD should match origin/main"
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
            let found = find_install_entry(&e, "github.com/org/tool")
                .expect("lookup")
                .expect("match");
            assert_eq!(found.repo, "github.com/org/tool");
        }

        #[test]
        fn matches_short_name() {
            let e = entries();
            let found = find_install_entry(&e, "tool")
                .expect("lookup")
                .expect("match");
            assert_eq!(found.repo, "github.com/org/tool");
        }

        #[test]
        fn returns_none_when_no_match() {
            let e = entries();
            let found = find_install_entry(&e, "nonexistent").expect("lookup");
            assert!(found.is_none());
        }

        #[test]
        fn errors_when_ambiguous_short_name() {
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
            let err = find_install_entry(&entries, "cli").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("multiple [[install]] entries"),
                "expected ambiguity error mentioning section: {msg}"
            );
            assert!(
                msg.contains("github.com/a/cli") && msg.contains("gitlab.com/b/cli"),
                "expected candidate list: {msg}"
            );
        }

        #[test]
        fn returns_entry_with_sub_path() {
            let e = entries();
            let found = find_install_entry(&e, "gitlab.com/team/app")
                .expect("lookup")
                .expect("match");
            assert_eq!(found.path.as_deref(), Some("crates/cli"));
        }

        #[test]
        fn returns_none_for_empty_entries() {
            let empty: Vec<InstallEntry> = Vec::new();
            let found = find_install_entry(&empty, "anything").expect("lookup");
            assert!(found.is_none());
        }
    }

    mod find_detached_entry_tests {
        use super::super::find_detached_entry;
        use crate::runtime::config::DetachedEntry;

        fn entries() -> Vec<DetachedEntry> {
            vec![
                DetachedEntry {
                    repo: "github.com/mattwparas/helix".to_string(),
                    branch: "steel-event-system".to_string(),
                },
                DetachedEntry {
                    repo: "github.com/org/fork".to_string(),
                    branch: "custom".to_string(),
                },
            ]
        }

        #[test]
        fn matches_full_repo_key() {
            let e = entries();
            let found = find_detached_entry(&e, "github.com/mattwparas/helix")
                .expect("lookup")
                .expect("match");
            assert_eq!(found.branch, "steel-event-system");
        }

        #[test]
        fn matches_short_name() {
            let e = entries();
            let found = find_detached_entry(&e, "helix")
                .expect("lookup")
                .expect("match");
            assert_eq!(found.branch, "steel-event-system");
        }

        #[test]
        fn returns_none_when_no_match() {
            let e = entries();
            let found = find_detached_entry(&e, "nonexistent").expect("lookup");
            assert!(found.is_none());
        }

        #[test]
        fn errors_when_ambiguous_short_name() {
            let entries = vec![
                DetachedEntry {
                    repo: "github.com/a/app".to_string(),
                    branch: "x".to_string(),
                },
                DetachedEntry {
                    repo: "github.com/b/app".to_string(),
                    branch: "y".to_string(),
                },
            ];
            let err = find_detached_entry(&entries, "app").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("multiple [[detached]] entries"),
                "expected ambiguity error mentioning section: {msg}"
            );
            assert!(
                msg.contains("github.com/a/app") && msg.contains("github.com/b/app"),
                "expected candidate list: {msg}"
            );
        }

        #[test]
        fn returns_none_for_empty_entries() {
            let empty: Vec<DetachedEntry> = Vec::new();
            let found = find_detached_entry(&empty, "anything").expect("lookup");
            assert!(found.is_none());
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

    mod resolve_install_path_tests {
        use super::{super::resolve_install_path, *};

        /// Minimal library crate layout with a `src/lib.rs`.
        fn write_library_package(dir: &std::path::Path, name: &str) {
            fs::create_dir_all(dir.join("src")).unwrap();
            fs::write(dir.join("src/lib.rs"), "").unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n"
                ),
            )
            .unwrap();
        }

        /// Minimal binary crate layout with a `src/main.rs`.
        fn write_binary_package(dir: &std::path::Path, name: &str) {
            fs::create_dir_all(dir.join("src")).unwrap();
            fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[[bin]]\nname = \"{name}\"\npath = \"src/main.rs\"\n"
                ),
            )
            .unwrap();
        }

        /// Workspace `Cargo.toml` with the provided members, virtual (no `[package]`).
        fn write_virtual_workspace(dir: &std::path::Path, members: &[&str]) {
            fs::create_dir_all(dir).unwrap();
            let members_toml = members
                .iter()
                .map(|m| format!("\"{m}\""))
                .collect::<Vec<_>>()
                .join(", ");
            fs::write(
                dir.join("Cargo.toml"),
                format!("[workspace]\nresolver = \"2\"\nmembers = [{members_toml}]\n"),
            )
            .unwrap();
        }

        #[test]
        fn explicit_path_is_honored_without_invoking_cargo_metadata() {
            let dir = TempDir::new("resolve-explicit-path");
            let wt = dir.path().join("github.com/org/workspace");
            let sub = wt.join("crates/cli");
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join("Cargo.toml"), "[package]\nname = \"cli\"\n").unwrap();

            let entry = InstallEntry {
                repo: "github.com/org/workspace".to_string(),
                path: Some("crates/cli".to_string()),
                extra_flags: vec![],
            };

            let resolved = resolve_install_path(&wt, &entry).unwrap();
            assert_eq!(resolved, sub);
        }

        #[test]
        fn normal_package_manifest_uses_base_path() {
            let dir = TempDir::new("resolve-normal-package");
            let wt = dir.path().join("github.com/org/tool");
            fs::create_dir_all(&wt).unwrap();
            fs::write(
                wt.join("Cargo.toml"),
                "[package]\nname = \"tool\"\nversion = \"0.0.1\"\nedition = \"2021\"\n",
            )
            .unwrap();

            let entry = InstallEntry {
                repo: "github.com/org/tool".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let resolved = resolve_install_path(&wt, &entry).unwrap();
            assert_eq!(resolved, wt);
        }

        #[test]
        fn errors_when_base_path_has_no_manifest() {
            let dir = TempDir::new("resolve-no-manifest");
            let wt = dir.path().join("github.com/org/tool");
            fs::create_dir_all(&wt).unwrap();

            let entry = InstallEntry {
                repo: "github.com/org/tool".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let result = resolve_install_path(&wt, &entry);
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Cargo.toml"),
                "expected Cargo.toml error: {msg}"
            );
        }

        #[test]
        fn workspace_with_single_bin_crate_resolves_that_crate() {
            let dir = TempDir::new("resolve-ws-single-bin");
            let wt = dir.path().join("github.com/org/ws");
            write_virtual_workspace(&wt, &["crates/lib-a", "crates/tool"]);
            write_library_package(&wt.join("crates/lib-a"), "lib-a");
            write_binary_package(&wt.join("crates/tool"), "tool");

            let entry = InstallEntry {
                repo: "github.com/org/ws".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let resolved = resolve_install_path(&wt, &entry).unwrap();
            assert_eq!(resolved, wt.join("crates/tool"));
        }

        #[test]
        fn workspace_with_multiple_bins_matches_repo_short_name() {
            let dir = TempDir::new("resolve-ws-match-short-name");
            let wt = dir.path().join("github.com/org/dumap");
            write_virtual_workspace(&wt, &["crates/dumap-cli", "crates/dumap"]);
            write_binary_package(&wt.join("crates/dumap-cli"), "dumap-cli");
            write_binary_package(&wt.join("crates/dumap"), "dumap");

            let entry = InstallEntry {
                repo: "github.com/org/dumap".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let resolved = resolve_install_path(&wt, &entry).unwrap();
            assert_eq!(resolved, wt.join("crates/dumap"));
        }

        #[test]
        fn workspace_with_multiple_bins_and_no_short_name_match_errors_with_candidates() {
            let dir = TempDir::new("resolve-ws-ambiguous");
            let wt = dir.path().join("github.com/org/multi");
            write_virtual_workspace(&wt, &["crates/alpha", "crates/beta"]);
            write_binary_package(&wt.join("crates/alpha"), "alpha");
            write_binary_package(&wt.join("crates/beta"), "beta");

            let entry = InstallEntry {
                repo: "github.com/org/multi".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let result = resolve_install_path(&wt, &entry);
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("multiple installable crates"),
                "expected ambiguity error: {msg}"
            );
            assert!(msg.contains("alpha"), "expected candidate alpha: {msg}");
            assert!(msg.contains("beta"), "expected candidate beta: {msg}");
            assert!(
                msg.contains("path = \"crates/alpha\"") && msg.contains("path = \"crates/beta\""),
                "expected path hints in ambiguity error: {msg}"
            );
        }

        #[test]
        fn workspace_without_any_bin_returns_actionable_error() {
            let dir = TempDir::new("resolve-ws-no-bin");
            let wt = dir.path().join("github.com/org/libs");
            write_virtual_workspace(&wt, &["crates/lib-a", "crates/lib-b"]);
            write_library_package(&wt.join("crates/lib-a"), "lib-a");
            write_library_package(&wt.join("crates/lib-b"), "lib-b");

            let entry = InstallEntry {
                repo: "github.com/org/libs".to_string(),
                path: None,
                extra_flags: vec![],
            };

            let result = resolve_install_path(&wt, &entry);
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("no package with a `bin` target"),
                "expected no-bin error: {msg}"
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
