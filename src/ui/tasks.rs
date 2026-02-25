use super::state::{RepoRow, UiState};
use crate::{
    error::{Error, Result},
    runtime::{
        environment::RuntimeEnvironment,
        task_rows::{TaskRow, TaskStatus},
    },
    tools::{
        git::repo::{default_clone_url, parse_repo_input},
        tmux::{
            sessions::is_available,
            workflow::{park_task, ParkResult},
        },
    },
};

pub(super) fn initial_repo_scope(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
) -> Option<String> {
    repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key())
}

pub(super) fn load_task_rows(
    context: &RuntimeEnvironment,
    repo_scope: Option<&str>,
) -> Result<Vec<TaskRow>> {
    let open_sessions = context.tasks().tmux_sessions();
    let mut rows = Vec::new();

    if let Some(repo_arg) = repo_scope {
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
    } else {
        for repo_key in context.tasks().available_repo_keys()? {
            let gitdir = context.layout().repo_gitdir_path(&repo_key);
            rows.extend(
                context
                    .tasks()
                    .repo_task_rows(&repo_key, &gitdir, &open_sessions)?,
            );
        }
    }

    rows.sort_by(|left, right| {
        left.status
            .cmp(&right.status)
            .then(left.repo.cmp(&right.repo))
            .then(left.branch.cmp(&right.branch))
    });

    Ok(rows)
}

pub(super) fn load_repo_rows(context: &RuntimeEnvironment) -> Result<Vec<RepoRow>> {
    let open_sessions = context.tasks().tmux_sessions();
    let mut rows = Vec::new();

    for repo_key in context.tasks().available_repo_keys()? {
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            continue;
        }

        let task_rows = context
            .tasks()
            .repo_task_rows(&repo_key, &gitdir, &open_sessions)?;
        let open_tasks = task_rows
            .iter()
            .filter(|row| row.status == TaskStatus::Open)
            .count();
        let parked_tasks = task_rows.len().saturating_sub(open_tasks);

        rows.push(RepoRow {
            repo: repo_key,
            open_tasks,
            parked_tasks,
        });
    }

    rows.sort_by(|left, right| left.repo.cmp(&right.repo));
    Ok(rows)
}

pub(super) fn park_selected(context: &RuntimeEnvironment, state: &mut UiState) -> Result<()> {
    let Some(row) = state.selected_task_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    if !is_available(context.process()) {
        return Err(Error::failed(
            "tmux is not available. Run 'task list' to inspect tasks.",
        ));
    }

    match park_task(context.process(), &row.repo, &row.branch, &row.path)? {
        ParkResult::Parked => state.message = format!("Parked task: {} {}", row.repo, row.branch),
        ParkResult::AlreadyParked => {
            state.message = format!("Task already parked: {} {}", row.repo, row.branch)
        }
    }

    Ok(())
}

pub(super) fn finish_selected(context: &RuntimeEnvironment, state: &mut UiState) -> Result<()> {
    let Some(row) = state.selected_task_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    crate::commands::finish::run(context, Some(&row.repo), Some(&row.branch), false)?;
    state.message = format!("Finished task: {} {}", row.repo, row.branch);
    Ok(())
}

pub(super) fn resolve_create_repo(
    context: &RuntimeEnvironment,
    state: &UiState,
    repo_scope: Option<&str>,
) -> Result<String> {
    if let Some(row) = state.selected_task_row() {
        return Ok(row.repo.clone());
    }

    if let Some(repo_arg) = repo_scope {
        return context.tasks().resolve_repo_input(Some(repo_arg));
    }

    context.tasks().resolve_repo_input(None)
}

pub(super) fn clone_from_input(context: &RuntimeEnvironment, input: &str) -> Result<String> {
    let (repo_url, explicit_repo_key) = parse_clone_input_args(input)?;
    let parsed = parse_repo_input(repo_url);
    let clone_url = parsed
        .clone_url
        .unwrap_or_else(|| default_clone_url(repo_url));
    let repo_key = explicit_repo_key.unwrap_or(parsed.repo_key);

    context.tasks().ensure_layout()?;
    context.tasks().clone_bare_repo(&clone_url, &repo_key)?;
    Ok(repo_key)
}

fn parse_clone_input_args(input: &str) -> Result<(&str, Option<String>)> {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(Error::failed("Clone input cannot be empty"));
    }
    if tokens.len() > 2 {
        return Err(Error::failed("Use format: <repo-url> [repo-key]"));
    }

    Ok((tokens[0], tokens.get(1).map(|token| (*token).to_string())))
}

#[cfg(test)]
mod tests {
    use super::parse_clone_input_args;

    #[test]
    fn parse_clone_input_accepts_url_only() {
        let parsed =
            parse_clone_input_args("git@github.com:me/app.git").expect("parse clone input");
        assert_eq!(parsed.0, "git@github.com:me/app.git");
        assert_eq!(parsed.1, None);
    }

    #[test]
    fn parse_clone_input_accepts_url_and_key() {
        let parsed = parse_clone_input_args("git@github.com:me/app.git github.com/me/app")
            .expect("parse clone input");
        assert_eq!(parsed.0, "git@github.com:me/app.git");
        assert_eq!(parsed.1, Some("github.com/me/app".to_string()));
    }

    #[test]
    fn parse_clone_input_rejects_empty_value() {
        let error = parse_clone_input_args("  ").expect_err("expected error");
        assert_eq!(error.to_string(), "Clone input cannot be empty");
    }

    #[test]
    fn parse_clone_input_rejects_too_many_parts() {
        let error = parse_clone_input_args("a b c").expect_err("expected error");
        assert_eq!(error.to_string(), "Use format: <repo-url> [repo-key]");
    }
}
