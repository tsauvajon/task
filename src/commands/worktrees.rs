use std::path::Path;

use crate::layout::Layout;

pub fn run(layout: &Layout, repo_arg: Option<&str>) -> Result<(), String> {
    super::ensure_layout(layout)?;

    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(super::current_repo_key);

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
        let gitdir = layout.repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }
        let output = super::run_capture(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "list",
            ],
            None,
        )?;
        print!("{output}");
        return Ok(());
    }

    let repo_keys = super::available_repo_keys(layout)?;
    if repo_keys.is_empty() {
        super::log(&format!(
            "No repositories found in {}",
            layout
                .repo_gitdir_path("")
                .parent()
                .unwrap_or(Path::new("/"))
                .display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!();
        println!("[{repo_key}]");
        let gitdir = layout.repo_gitdir_path(&repo_key);
        let output = super::run_capture(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "list",
            ],
            None,
        )?;
        print!("{output}");
    }

    Ok(())
}
