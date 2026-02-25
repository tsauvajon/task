use rayon::prelude::*;

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, process, task_rows::TaskRow},
    tools::git,
};

pub fn run(context: &RuntimeEnvironment, repo_arg: Option<&str>) -> Result<()> {
    context.tasks().ensure_layout()?;
    let open_sessions = context.tasks().tmux_sessions();

    let mut rows: Vec<TaskRow> = Vec::new();
    let repo_arg = repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key().map(String::from));

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
            process::log(&format!("No tasks found for {repo_key}"));
        } else {
            context.tasks().print_task_rows_table(&rows);
        }
        return Ok(());
    }

    // Resolve the nix store path for git before entering the parallel section:
    // the OnceLock inside NixRunner would otherwise block every rayon thread on
    // the first caller (~0.5s) while the rest stall idle.
    git::warmup();

    // Collect all (key, gitdir) pairs first (fast sequential scan), then
    // fan out all `git worktree list` subprocess calls in one flat parallel pass.
    let results: Vec<_> = context
        .tasks()
        .available_repos()?
        .into_par_iter()
        .map(|(repo_key, gitdir)| {
            let result = context
                .tasks()
                .repo_task_rows(&repo_key, &gitdir, &open_sessions);
            (repo_key, result)
        })
        .collect();

    let mut skipped_repos = Vec::new();
    for (repo_key, result) in results {
        match result {
            Ok(repo_rows) => rows.extend(repo_rows),
            Err(err) => skipped_repos.push((repo_key, err)),
        }
    }

    if rows.is_empty() {
        process::log(&format!(
            "No tasks found under {}",
            context.layout().wt_dir().display()
        ));
    } else {
        context.tasks().print_task_rows_table(&rows);
    }

    for (repo_key, err) in skipped_repos {
        process::warn(&format!("Skipping {repo_key}: {err}"));
    }

    Ok(())
}
