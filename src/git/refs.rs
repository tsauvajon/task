use std::path::Path;

use super::runner::{run_git_capture, run_git_status};

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
