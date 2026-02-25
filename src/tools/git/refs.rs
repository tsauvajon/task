use std::path::Path;

use super::runner::{run_git_capture, run_git_status};
use crate::error::Result;

pub fn detect_default_base(gitdir: &Path) -> String {
    let gitdir_str = gitdir.to_string_lossy();
    if let Ok(output) = run_git_capture(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
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

    let gitdir_str = gitdir.to_string_lossy();
    if run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
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

pub fn fetch_origin_refs(gitdir: &Path) -> Result<()> {
    let gitdir_str = gitdir.to_string_lossy();
    run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
            "fetch",
            "origin",
            "--prune",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        None,
    )
}

pub fn ref_exists(gitdir: &Path, reference: &str) -> bool {
    let gitdir_str = gitdir.to_string_lossy();
    run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
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
    let gitdir_str = gitdir.to_string_lossy();
    let value = format!("{revision}^{{commit}}");
    run_git_status(
        &[
            "--git-dir",
            gitdir_str.as_ref(),
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
    let root_str = root.to_string_lossy();
    run_git_capture(
        &["-C", root_str.as_ref(), "symbolic-ref", "--quiet", "--short", "HEAD"],
        None,
    )
    .ok()
    .map(|s| s.trim().to_owned())
    .filter(|s| !s.is_empty())
}
