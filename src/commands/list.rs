use crate::runtime::environment::RuntimeEnvironment;
use crate::runtime::task_rows::TaskRow;

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<(), String> {
    context.ensure_layout()?;
    let open_sessions = context.tmux_sessions();

    let mut rows: Vec<TaskRow> = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.current_repo_key());

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = context.resolve_repo_key_input(repo_arg)?;
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }

        rows.extend(context.repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
        if rows.is_empty() {
            context.log(&format!("No tasks found for {repo_key}"));
        } else {
            context.print_task_rows_table(&rows);
        }
        return Ok(());
    }

    let repo_keys = context.available_repo_keys()?;
    for repo_key in repo_keys {
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        rows.extend(context.repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
    }

    if rows.is_empty() {
        context.log(&format!(
            "No tasks found under {}",
            context.wt_dir().display()
        ));
    } else {
        context.print_task_rows_table(&rows);
    }

    Ok(())
}
