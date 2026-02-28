use std::{
    collections::HashSet,
    io::{self, IsTerminal},
};

use dialoguer::{theme::ColorfulTheme, Select};

use crate::{
    error::{Error, Result},
    runtime::{environment::RuntimeEnvironment, task_rows::TaskRow},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatchKind {
    Exact,
    Partial,
}

pub fn run(
    context: &RuntimeEnvironment,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
) -> Result<()> {
    if let (Some(query), None) = (repo_arg, branch_arg) {
        let row = select_task_by_query(context, query)?;
        return context
            .tasks()
            .launch_workspace(&row.repo, &row.branch, &row.path);
    }

    let (repo_key_raw, branch) = context
        .tasks()
        .resolve_repo_branch_inputs(repo_arg, branch_arg)?;
    let repo_key = context.tasks().resolve_repo_key_input(&repo_key_raw)?;
    let worktree = context.tasks().resolve_worktree_path(&repo_key, &branch);
    if !worktree.join(".git").exists() {
        return Err(Error::not_found(format!(
            "Worktree not found: {}",
            worktree.display()
        )));
    }
    context
        .tasks()
        .launch_workspace(&repo_key, &branch, &worktree)
}

fn select_task_by_query(context: &RuntimeEnvironment, query: &str) -> Result<TaskRow> {
    let all_rows = all_task_rows(context)?;
    if all_rows.is_empty() {
        return Err(Error::not_found(
            "No tasks found. Run 'task start <repo> <branch>' first.",
        ));
    }

    let task_matches = collect_matches(&all_rows, query, match_task_name);
    if !task_matches.is_empty() {
        return resolve_match(query, &task_matches, "task name");
    }

    let repo_matches = collect_matches(&all_rows, query, match_repo_name);
    if repo_matches.is_empty() {
        return Err(Error::not_found(format!(
            "No task found for '{query}'. Searched task names first, then repository names."
        )));
    }

    resolve_match(query, &repo_matches, "repository")
}

fn all_task_rows(context: &RuntimeEnvironment) -> Result<Vec<TaskRow>> {
    let mut rows = Vec::new();
    let open_sessions = HashSet::new();

    for repo_key in context.tasks().available_repo_keys()? {
        let gitdir = context.layout().repo_gitdir_path(&repo_key);
        rows.extend(
            context
                .tasks()
                .repo_task_rows(&repo_key, &gitdir, &open_sessions)?,
        );
    }

    rows.sort_by(|left, right| {
        left.repo
            .cmp(&right.repo)
            .then(left.branch.cmp(&right.branch))
    });
    Ok(rows)
}

fn collect_matches(
    rows: &[TaskRow],
    query: &str,
    matcher: fn(&TaskRow, &str) -> Option<MatchKind>,
) -> Vec<(TaskRow, MatchKind)> {
    rows.iter()
        .filter_map(|row| matcher(row, query).map(|kind| (row.clone(), kind)))
        .collect()
}

fn resolve_match(query: &str, matches: &[(TaskRow, MatchKind)], context: &str) -> Result<TaskRow> {
    let allow_prompt = io::stdin().is_terminal() && io::stdout().is_terminal();
    resolve_match_impl(query, matches, context, allow_prompt)
}

/// Inner, testable core of `resolve_match`.
///
/// `allow_prompt` controls whether an interactive `Select` may be shown.
/// Production code derives this from `is_terminal()`; tests pass `false`
/// to guarantee a deterministic, non-blocking result.
fn resolve_match_impl(
    query: &str,
    matches: &[(TaskRow, MatchKind)],
    context: &str,
    allow_prompt: bool,
) -> Result<TaskRow> {
    if matches.len() == 1 {
        return Ok(matches[0].0.clone());
    }

    let exact: Vec<TaskRow> = matches
        .iter()
        .filter(|(_, kind)| *kind == MatchKind::Exact)
        .map(|(row, _)| row.clone())
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0].clone());
    }

    // Still ambiguous — build the label list once for both the error message
    // and the interactive prompt.
    let labels: Vec<String> = matches
        .iter()
        .map(|(row, _)| format!("{} {} ({})", row.repo, row.branch, row.path.display()))
        .collect();

    if !allow_prompt {
        return Err(Error::failed(format!(
            "Multiple tasks match '{query}' by {context}: {}. Use 'task open <repo> <branch>' to disambiguate.",
            labels.join(" | ")
        )));
    }

    choose_task_interactive(query, &labels, matches)
}

fn choose_task_interactive(
    query: &str,
    labels: &[String],
    matches: &[(TaskRow, MatchKind)],
) -> Result<TaskRow> {
    let prompt = format!("Multiple tasks match '{query}'. Choose one:");
    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(labels)
        .default(0)
        .interact_opt()?;

    let Some(i) = index else {
        return Err(Error::Cancelled);
    };
    Ok(matches[i].0.clone())
}

fn match_task_name(row: &TaskRow, query: &str) -> Option<MatchKind> {
    if row.branch.as_str() == query {
        return Some(MatchKind::Exact);
    }

    let query_lower = query.to_lowercase();
    if row.branch.to_lowercase().contains(&query_lower) {
        return Some(MatchKind::Partial);
    }

    None
}

fn match_repo_name(row: &TaskRow, query: &str) -> Option<MatchKind> {
    let short = row.repo.rsplit('/').next().unwrap_or_default();
    if row.repo.as_str() == query || short == query {
        return Some(MatchKind::Exact);
    }

    let query_lower = query.to_lowercase();
    if row.repo.to_lowercase().contains(&query_lower) || short.to_lowercase().contains(&query_lower)
    {
        return Some(MatchKind::Partial);
    }

    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{match_repo_name, match_task_name, resolve_match_impl, MatchKind};
    use crate::runtime::{
        task_rows::{TaskRow, TaskStatus},
        BranchName, RepoKey,
    };

    mod match_task_name {
        use super::*;

        fn row(branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new("github.com/acme/tool"),
                branch: BranchName::new(branch),
                path: PathBuf::from("/tmp/wt/tool/feat/login"),
            }
        }

        #[test]
        fn prefers_exact() {
            let r = row("feat/login");
            assert_eq!(match_task_name(&r, "feat/login"), Some(MatchKind::Exact));
            assert_eq!(match_task_name(&r, "login"), Some(MatchKind::Partial));
        }

        #[test]
        fn returns_none_when_no_match() {
            let r = row("feat/login");
            assert_eq!(match_task_name(&r, "unrelated"), None);
        }

        #[test]
        fn case_insensitive_partial_match() {
            let r = row("feat/Login");
            assert_eq!(match_task_name(&r, "login"), Some(MatchKind::Partial));
        }
    }

    mod match_repo_name {
        use super::*;

        fn row(repo: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new(repo),
                branch: BranchName::new("feat/login"),
                path: PathBuf::from("/tmp/wt/tool/feat/login"),
            }
        }

        #[test]
        fn supports_short_repo() {
            let r = row("github.com/acme/tool");
            assert_eq!(match_repo_name(&r, "tool"), Some(MatchKind::Exact));
            assert_eq!(match_repo_name(&r, "acme"), Some(MatchKind::Partial));
        }

        #[test]
        fn full_key_exact_match() {
            let r = row("github.com/acme/tool");
            assert_eq!(
                match_repo_name(&r, "github.com/acme/tool"),
                Some(MatchKind::Exact)
            );
        }

        #[test]
        fn returns_none_when_no_match() {
            let r = row("github.com/acme/tool");
            assert_eq!(match_repo_name(&r, "unrelated-org"), None);
        }

        #[test]
        fn case_insensitive_partial_match() {
            let r = row("github.com/Acme/Tool");
            assert_eq!(match_repo_name(&r, "acme"), Some(MatchKind::Partial));
        }
    }

    mod resolve_match {
        use super::*;

        fn make_row(repo: &str, branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new(repo),
                branch: BranchName::new(branch),
                path: PathBuf::from(format!("/tmp/wt/{repo}/{branch}")),
            }
        }

        /// Thin wrapper that always passes `allow_prompt = false` so no test
        /// ever blocks waiting for terminal input, regardless of whether the
        /// test runner is attached to a TTY.
        fn resolve(
            query: &str,
            matches: &[(TaskRow, MatchKind)],
            context: &str,
        ) -> crate::error::Result<TaskRow> {
            resolve_match_impl(query, matches, context, false)
        }

        #[test]
        fn uses_single_exact_without_prompt() {
            let a = make_row("github.com/acme/tool", "feat/login");
            let b = make_row("github.com/acme/other", "login-fix");

            let selected = resolve(
                "feat/login",
                &[(a.clone(), MatchKind::Exact), (b, MatchKind::Partial)],
                "task name",
            )
            .expect("select exact match");
            assert_eq!(selected, a);
        }

        #[test]
        fn single_partial_match_returned_directly() {
            let a = make_row("github.com/acme/tool", "feat/login");

            let selected = resolve("login", &[(a.clone(), MatchKind::Partial)], "task name")
                .expect("single partial match");
            assert_eq!(selected, a);
        }

        #[test]
        fn multiple_exact_matches_errors_when_prompt_disallowed() {
            // Call resolve_match_impl with allow_prompt=false so no dialoguer
            // Select is ever constructed, regardless of whether stdin is a TTY.
            let a = make_row("github.com/acme/tool", "login");
            let b = make_row("github.com/acme/other", "login");

            let result = resolve_match_impl(
                "login",
                &[(a, MatchKind::Exact), (b, MatchKind::Exact)],
                "task name",
                false,
            );
            assert!(result.is_err(), "two exact matches must yield an error");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Multiple tasks match"),
                "error should explain the ambiguity: {msg}"
            );
            assert!(
                msg.contains("disambiguate"),
                "error should suggest how to fix it: {msg}"
            );
        }

        #[test]
        fn multiple_partial_matches_without_exact_errors_when_prompt_disallowed() {
            let a = make_row("github.com/acme/tool", "feature-login");
            let b = make_row("github.com/acme/other", "admin-login");

            let result = resolve_match_impl(
                "login",
                &[(a, MatchKind::Partial), (b, MatchKind::Partial)],
                "task name",
                false,
            );
            assert!(result.is_err(), "two partial matches must yield an error");
            let msg = result.unwrap_err().to_string();
            assert!(msg.contains("Multiple tasks match"), "{msg}");
        }

        #[test]
        fn error_message_contains_query() {
            let a = make_row("github.com/acme/tool", "login-alpha");
            let b = make_row("github.com/acme/other", "login-beta");

            let result = resolve_match_impl(
                "myquery",
                &[(a, MatchKind::Partial), (b, MatchKind::Partial)],
                "task name",
                false,
            );
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("myquery"),
                "error should contain the query: {msg}"
            );
        }

        #[test]
        fn error_message_contains_context() {
            let a = make_row("github.com/acme/tool", "login-alpha");
            let b = make_row("github.com/acme/other", "login-beta");

            let result = resolve_match_impl(
                "login",
                &[(a, MatchKind::Partial), (b, MatchKind::Partial)],
                "repository",
                false,
            );
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("repository"),
                "error should name the context: {msg}"
            );
        }
    }

    mod match_task_name_extra {
        use super::*;

        fn row(branch: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new("github.com/acme/tool"),
                branch: BranchName::new(branch),
                path: PathBuf::from("/tmp/wt/tool/feat"),
            }
        }

        #[test]
        fn empty_branch_does_not_match_non_empty_query() {
            let r = row("");
            assert_eq!(match_task_name(&r, "login"), None);
        }

        #[test]
        fn empty_query_matches_any_branch_as_partial() {
            // An empty query string is a prefix of everything.
            let r = row("feat/login");
            assert_eq!(match_task_name(&r, ""), Some(MatchKind::Partial));
        }

        #[test]
        fn slash_in_branch_matches_via_contains() {
            let r = row("feat/JIRA-123");
            assert_eq!(match_task_name(&r, "JIRA"), Some(MatchKind::Partial));
        }
    }

    mod match_repo_name_extra {
        use super::*;

        fn row(repo: &str) -> TaskRow {
            TaskRow {
                status: TaskStatus::Parked,
                repo: RepoKey::new(repo),
                branch: BranchName::new("feat/login"),
                path: PathBuf::from("/tmp/wt/tool/feat/login"),
            }
        }

        #[test]
        fn short_name_partial_match_is_partial() {
            let r = row("github.com/acme/toolbox");
            assert_eq!(match_repo_name(&r, "tool"), Some(MatchKind::Partial));
        }

        #[test]
        fn short_name_exact_match_is_exact() {
            let r = row("github.com/acme/tool");
            assert_eq!(match_repo_name(&r, "tool"), Some(MatchKind::Exact));
        }

        #[test]
        fn case_insensitive_short_name_exact_becomes_partial() {
            // "Tool" != "tool" so it won't be exact, but partial via lowercase
            let r = row("github.com/acme/Tool");
            // short is "Tool", query is "tool"; "Tool" != "tool" so not exact,
            // but "tool".to_lowercase().contains("tool") is true → Partial
            assert_eq!(match_repo_name(&r, "tool"), Some(MatchKind::Partial));
        }
    }
}
