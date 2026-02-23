use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn clone_bare_repo(repo_url: &str, gitdir: &Path) -> Result<(), String> {
    if gitdir.is_dir() {
        return Ok(());
    }

    if let Some(parent) = gitdir.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    run_git_status(
        &[
            "clone",
            "--bare",
            repo_url,
            gitdir.to_string_lossy().as_ref(),
        ],
        None,
    )
}

pub fn detect_default_base(gitdir: &Path) -> String {
    if let Ok(output) = run_git_capture(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "ls-remote",
            "--symref",
            "origin",
            "HEAD",
        ],
        None,
    ) {
        for line in output.lines() {
            if let Some(target) = line.strip_prefix("ref: ") {
                let target = target.trim();
                if let Some(target) = target.strip_suffix(" HEAD")
                    && let Some(branch) = target.strip_prefix("refs/heads/")
                {
                    let remote_branch = format!("origin/{branch}");
                    if rev_exists(gitdir, &remote_branch) {
                        return remote_branch;
                    }
                    if rev_exists(gitdir, branch) {
                        return branch.to_string();
                    }
                }
            }
        }
    }

    if run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "show-ref",
            "--verify",
            "--quiet",
            "refs/remotes/origin/master",
        ],
        None,
    )
    .is_ok()
    {
        return "origin/master".to_string();
    }

    "HEAD".to_string()
}

pub fn fetch_origin_refs(gitdir: &Path) -> Result<(), String> {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "fetch",
            "origin",
            "--prune",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        None,
    )
}

pub fn ref_exists(gitdir: &Path, reference: &str) -> bool {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "show-ref",
            "--verify",
            "--quiet",
            reference,
        ],
        None,
    )
    .is_ok()
}

pub fn rev_exists(gitdir: &Path, revision: &str) -> bool {
    let value = format!("{revision}^{{commit}}");
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "rev-parse",
            "--verify",
            "--quiet",
            &value,
        ],
        None,
    )
    .is_ok()
}

pub fn worktree_list(gitdir: &Path) -> Result<String, String> {
    run_git_capture(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "list",
        ],
        None,
    )
}

pub fn worktree_list_porcelain(gitdir: &Path) -> Result<String, String> {
    run_git_capture(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "list",
            "--porcelain",
        ],
        None,
    )
}

pub fn worktree_add_existing_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<(), String> {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "add",
            worktree.to_string_lossy().as_ref(),
            branch,
        ],
        None,
    )
}

pub fn worktree_add_tracking_remote_branch(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<(), String> {
    let remote = format!("origin/{branch}");
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "add",
            "--track",
            "-b",
            branch,
            worktree.to_string_lossy().as_ref(),
            &remote,
        ],
        None,
    )
}

pub fn worktree_add_from_base(
    gitdir: &Path,
    worktree: &Path,
    branch: &str,
    base_ref: &str,
) -> Result<(), String> {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "add",
            "-b",
            branch,
            worktree.to_string_lossy().as_ref(),
            base_ref,
        ],
        None,
    )
}

pub fn worktree_prune(gitdir: &Path) -> Result<(), String> {
    run_git_status(
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "prune",
            "--verbose",
        ],
        None,
    )
}

pub fn worktree_remove(gitdir: &Path, worktree: &Path, force: bool) -> Result<(), String> {
    let mut args = vec![
        "--git-dir".to_string(),
        gitdir.to_string_lossy().to_string(),
        "worktree".to_string(),
        "remove".to_string(),
    ];
    if force {
        args.push("--force".to_string());
    }
    args.push(worktree.to_string_lossy().to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git_status(&arg_refs, None)
}

pub fn status_porcelain(worktree: &Path) -> Result<String, String> {
    run_git_capture(
        &[
            "-C",
            worktree.to_string_lossy().as_ref(),
            "status",
            "--porcelain",
        ],
        None,
    )
}

pub fn rebase(worktree: &Path, base_ref: &str) -> Result<(), String> {
    run_git_status(
        &[
            "-C",
            worktree.to_string_lossy().as_ref(),
            "rebase",
            base_ref,
        ],
        None,
    )
}

pub fn current_root() -> Result<PathBuf, String> {
    let root = run_git_capture(&["rev-parse", "--show-toplevel"], None)?;
    Ok(PathBuf::from(root.trim()))
}

pub fn git_common_dir(root: &Path) -> Result<PathBuf, String> {
    let common_dir_raw = run_git_capture(
        &[
            "-C",
            root.to_string_lossy().as_ref(),
            "rev-parse",
            "--git-common-dir",
        ],
        None,
    )?;

    let mut common_dir = PathBuf::from(common_dir_raw.trim());
    if common_dir.is_relative() {
        common_dir = root.join(common_dir);
    }
    fs::canonicalize(common_dir).map_err(|e| e.to_string())
}

pub fn current_branch(root: &Path) -> Option<String> {
    run_git_capture(
        &[
            "-C",
            root.to_string_lossy().as_ref(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ],
        None,
    )
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn run_git_capture(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    run_capture("git", args, cwd)
}

fn run_git_status(args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    run_status("git", args, cwd)
}

fn run_capture(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            return Err(stderr);
        }
        return Err(format!("command failed with status {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_status(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let status = command.status().map_err(|e| e.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(format!("command failed with status {status}"))
}
