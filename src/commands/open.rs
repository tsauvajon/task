use std::{
    collections::HashSet,
    io::{self, IsTerminal},
};

use dialoguer::{Select, theme::ColorfulTheme};

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

    choose_task_interactive(query, matches, context)
}

fn choose_task_interactive(
    query: &str,
    matches: &[(TaskRow, MatchKind)],
    context: &str,
) -> Result<TaskRow> {
    let labels: Vec<String> = matches
        .iter()
        .map(|(row, _)| format!("{} {} ({})", row.repo, row.branch, row.path.display()))
        .collect();

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::failed(format!(
            "Multiple tasks match '{query}' by {context}: {}. Use 'task open <repo> <branch>' to disambiguate.",
            labels.join(" | ")
        )));
    }

    let prompt = format!("Multiple tasks match '{query}' by {context}. Choose one:");
    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&labels)
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

    use super::{MatchKind, match_repo_name, match_task_name, resolve_match};
    use crate::runtime::{
        BranchName, RepoKey,
        task_rows::{TaskRow, TaskStatus},
    };

    #[test]
    fn task_name_match_prefers_exact() {
        let row = TaskRow {
            status: TaskStatus::Parked,
            repo: RepoKey::new("github.com/acme/tool"),
            branch: BranchName::new("feat/login"),
            path: PathBuf::from("/tmp/wt/tool/feat/login"),
        };
        assert_eq!(match_task_name(&row, "feat/login"), Some(MatchKind::Exact));
        assert_eq!(match_task_name(&row, "login"), Some(MatchKind::Partial));
    }

    #[test]
    fn repo_name_match_supports_short_repo() {
        let row = TaskRow {
            status: TaskStatus::Parked,
            repo: RepoKey::new("github.com/acme/tool"),
            branch: BranchName::new("feat/login"),
            path: PathBuf::from("/tmp/wt/tool/feat/login"),
        };
        assert_eq!(match_repo_name(&row, "tool"), Some(MatchKind::Exact));
        assert_eq!(match_repo_name(&row, "acme"), Some(MatchKind::Partial));
    }

    #[test]
    fn resolve_match_uses_single_exact_without_prompt() {
        let a = TaskRow {
            status: TaskStatus::Parked,
            repo: RepoKey::new("github.com/acme/tool"),
            branch: BranchName::new("feat/login"),
            path: PathBuf::from("/tmp/wt/tool/feat/login"),
        };
        let b = TaskRow {
            status: TaskStatus::Parked,
            repo: RepoKey::new("github.com/acme/other"),
            branch: BranchName::new("login-fix"),
            path: PathBuf::from("/tmp/wt/other/login-fix"),
        };

        let selected = resolve_match(
            "feat/login",
            &[(a.clone(), MatchKind::Exact), (b, MatchKind::Partial)],
            "task name",
        )
        .expect("select exact match");
        assert_eq!(selected, a);
    }
}
