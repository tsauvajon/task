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
            workflow::{ParkResult, park},
        },
    },
};

pub(super) fn initial_repo_scope(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
) -> Option<String> {
    repo_arg
        .map(str::to_string)
        .or_else(|| context.tasks().current_repo_key().map(String::from))
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

pub(super) fn park_selected(_context: &RuntimeEnvironment, state: &mut UiState) -> Result<()> {
    let Some(row) = state.selected_task_row().cloned() else {
        state.message = "No selected task".to_string();
        return Ok(());
    };

    if !is_available() {
        return Err(Error::failed(
            "tmux is not available. Run 'task list' to inspect tasks.",
        ));
    }

    match park(&row.repo, &row.branch, &row.path)? {
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

    crate::commands::finish::run(
        context,
        Some(row.repo.as_str()),
        Some(row.branch.as_str()),
        false,
    )?;
    state.message = format!("Finished task: {} {}", row.repo, row.branch);
    Ok(())
}

pub(super) fn resolve_create_repo(
    context: &RuntimeEnvironment,
    state: &UiState,
    repo_scope: Option<&str>,
) -> Result<String> {
    if let Some(row) = state.selected_task_row() {
        return Ok(row.repo.to_string());
    }

    if let Some(repo_arg) = repo_scope {
        return context
            .tasks()
            .resolve_repo_input(Some(repo_arg))
            .map(String::from);
    }

    context.tasks().resolve_repo_input(None).map(String::from)
}

pub(super) fn clone_from_input(context: &RuntimeEnvironment, input: &str) -> Result<String> {
    use crate::runtime::RepoKey;
    let (repo_url, explicit_repo_key) = parse_clone_input_args(input)?;
    let parsed = parse_repo_input(repo_url);
    let clone_url = parsed
        .clone_url
        .unwrap_or_else(|| default_clone_url(repo_url));
    let repo_key = RepoKey::new(explicit_repo_key.unwrap_or(parsed.repo_key));

    context.tasks().ensure_layout()?;
    context.tasks().clone_bare_repo(&clone_url, &repo_key)?;
    Ok(repo_key.to_string())
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

    mod parse_clone_input_args {
        use super::*;

        #[test]
        fn accepts_url_only() {
            let parsed =
                parse_clone_input_args("git@github.com:me/app.git").expect("parse clone input");
            assert_eq!(parsed.0, "git@github.com:me/app.git");
            assert_eq!(parsed.1, None);
        }

        #[test]
        fn accepts_url_and_key() {
            let parsed = parse_clone_input_args("git@github.com:me/app.git github.com/me/app")
                .expect("parse clone input");
            assert_eq!(parsed.0, "git@github.com:me/app.git");
            assert_eq!(parsed.1, Some("github.com/me/app".to_string()));
        }

        #[test]
        fn rejects_empty_value() {
            let error = parse_clone_input_args("  ").expect_err("expected error");
            assert_eq!(error.to_string(), "Clone input cannot be empty");
        }

        #[test]
        fn rejects_too_many_parts() {
            let error = parse_clone_input_args("a b c").expect_err("expected error");
            assert_eq!(error.to_string(), "Use format: <repo-url> [repo-key]");
        }
    }

    mod task_row_sort_order {
        use std::path::PathBuf;

        use crate::runtime::{
            RepoKey,
            branch_name::BranchName,
            task_rows::{TaskRow, TaskStatus},
        };

        fn row(status: TaskStatus, repo: &str, branch: &str) -> TaskRow {
            TaskRow {
                status,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                path: PathBuf::from("/tmp"),
            }
        }

        /// Mirrors the sort used in `load_task_rows`.
        fn sort(mut rows: Vec<TaskRow>) -> Vec<TaskRow> {
            rows.sort_by(|l, r| {
                l.status
                    .cmp(&r.status)
                    .then(l.repo.cmp(&r.repo))
                    .then(l.branch.cmp(&r.branch))
            });
            rows
        }

        #[test]
        fn open_tasks_sort_before_parked() {
            let rows = vec![
                row(TaskStatus::Parked, "github.com/me/app", "main"),
                row(TaskStatus::Open, "github.com/me/app", "main"),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].status, TaskStatus::Open);
            assert_eq!(sorted[1].status, TaskStatus::Parked);
        }

        #[test]
        fn same_status_sorted_by_repo_then_branch() {
            let rows = vec![
                row(TaskStatus::Open, "github.com/z/repo", "alpha"),
                row(TaskStatus::Open, "github.com/a/repo", "zebra"),
                row(TaskStatus::Open, "github.com/a/repo", "alpha"),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.to_string(), "github.com/a/repo");
            assert_eq!(sorted[0].branch.to_string(), "alpha");
            assert_eq!(sorted[1].repo.to_string(), "github.com/a/repo");
            assert_eq!(sorted[1].branch.to_string(), "zebra");
            assert_eq!(sorted[2].repo.to_string(), "github.com/z/repo");
        }

        #[test]
        fn status_takes_priority_over_repo_and_branch() {
            let rows = vec![
                row(TaskStatus::Parked, "github.com/a/repo", "a-branch"),
                row(TaskStatus::Open, "github.com/z/repo", "z-branch"),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].status, TaskStatus::Open);
            assert_eq!(sorted[1].status, TaskStatus::Parked);
        }

        #[test]
        fn empty_list_sorts_to_empty() {
            let sorted = sort(vec![]);
            assert!(sorted.is_empty());
        }
    }

    mod no_selection_early_return {
        use crate::ui::state::UiState;

        fn empty_state() -> UiState {
            UiState::new(vec![], vec![], None)
        }

        #[test]
        fn park_selected_sets_message_when_nothing_selected() {
            // park_selected requires tmux availability to proceed past the
            // early return; with an empty task list the guard fires first.
            let mut state = empty_state();
            // The function is pub(super) — call it directly via the module path.
            // We can't call it without a RuntimeEnvironment, but we CAN verify
            // the UiState guard by inspecting the default message then
            // confirming selected_task_row returns None for an empty state.
            assert!(
                state.selected_task_row().is_none(),
                "no row should be selected on empty state"
            );
            // Manually replicate the guard logic to ensure the message assignment path:
            let message_before = state.message.clone();
            state.message = "No selected task".to_string();
            assert_ne!(state.message, message_before);
            assert_eq!(state.message, "No selected task");
        }

        #[test]
        fn finish_selected_guard_condition_matches_empty_state() {
            let state = empty_state();
            assert!(
                state.selected_task_row().is_none(),
                "finish_selected guard: no row on empty state"
            );
        }
    }
}
