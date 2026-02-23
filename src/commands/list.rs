use crate::git::parsing::TaskRow;
use crate::workspace_paths::WorkspacePaths;

pub fn run(layout: &WorkspacePaths, repo_arg: Option<&str>) -> Result<(), String> {
    super::ensure_layout(layout)?;
    let open_sessions = super::tmux_sessions();

    let mut rows: Vec<TaskRow> = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(super::current_repo_key);

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = super::resolve_repo_key_input(layout, repo_arg)?;
        let gitdir = layout.repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }

        rows.extend(super::repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
        if rows.is_empty() {
            super::log(&format!("No tasks found for {repo_key}"));
        } else {
            super::print_task_rows_table(&rows);
        }
        return Ok(());
    }

    let repo_keys = super::available_repo_keys(layout)?;
    for repo_key in repo_keys {
        let gitdir = layout.repo_gitdir_path(&repo_key);
        rows.extend(super::repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
    }

    if rows.is_empty() {
        super::log(&format!(
            "No tasks found under {}",
            super::default_dev_root().join("wt").display()
        ));
    } else {
        super::print_task_rows_table(&rows);
    }

    Ok(())
}
