use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, task_rows::TaskRow},
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;
    let open_sessions = context.tasks().tmux_sessions();

    let mut rows: Vec<TaskRow> = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key());

    if let Some(repo_arg) = repo_arg.as_deref() {
        let repo_key = context.tasks().resolve_repo_key_input(repo_arg)?;
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(Error::not_found(format!("Repo not found: {repo_key}")));
        }

        rows.extend(
            context
                .tasks()
                .repo_task_rows(&repo_key, &gitdir, &open_sessions)?,
        );
        if rows.is_empty() {
            context
                .process()
                .log(&format!("No tasks found for {repo_key}"));
        } else {
            context.tasks().print_task_rows_table(&rows);
        }
        return Ok(());
    }

    let repo_keys = context.tasks().available_repo_keys()?;
    let mut skipped_repos = Vec::new();
    for repo_key in repo_keys {
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        match context
            .tasks()
            .repo_task_rows(&repo_key, &gitdir, &open_sessions)
        {
            Ok(repo_rows) => rows.extend(repo_rows),
            Err(err) => skipped_repos.push((repo_key, err)),
        }
    }

    if rows.is_empty() {
        context.process().log(&format!(
            "No tasks found under {}",
            context.layout().wt_dir().display()
        ));
    } else {
        context.tasks().print_task_rows_table(&rows);
    }

    for (repo_key, err) in skipped_repos {
        context
            .process()
            .warn(&format!("Skipping {repo_key}: {err}"));
    }

    Ok(())
}
