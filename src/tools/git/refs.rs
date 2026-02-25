use std::path::Path;

use super::{gitdir::GitDir, run::run_git_capture};
use crate::error::Result;

pub fn detect_default_base(gitdir: &Path) -> String {
    let gd = GitDir::new(gitdir);
    if let Ok(output) = gd.capture(&["ls-remote", "--symref", "origin", "HEAD"]) {
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
    run_git_capture(
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
