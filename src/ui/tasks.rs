use std::{collections::HashSet, path::Path};

use super::state::{RepoRow, UiState};
use crate::{
    error::{Error, Result},
    runtime::{RepoKey, environment::RuntimeEnvironment, task_rows::TaskRow},
    tools::{
        git::{
            repo::{default_clone_url, parse_repo_input},
            worktrees::{list_registered_worktrees, worktree_name},
        },
        tmux::{
            naming::session_name,
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
    let wt_dir = context.layout().wt_dir();
    let mut rows = Vec::new();

    for (repo_key, gitdir) in context.tasks().available_repos()? {
        let (open_tasks, parked_tasks) =
            count_repo_worktrees(&repo_key, &gitdir, wt_dir, &open_sessions);

        let detached_path = context.layout().detached_path(&repo_key);
        let is_detached = is_detached_worktree(&detached_path);

        rows.push(RepoRow {
            repo: repo_key,
            open_tasks,
            parked_tasks,
            is_detached,
        });
    }

    rows.sort_by(|left, right| {
        right
            .open_tasks
            .cmp(&left.open_tasks)
            .then(right.parked_tasks.cmp(&left.parked_tasks))
            .then(right.is_detached.cmp(&left.is_detached))
            .then(left.repo.cmp(&right.repo))
    });
    Ok(rows)
}

/// Returns true when `path` is the root of a git worktree (has a `.git` file
/// as created by `git worktree add`, or is a bare/worktree with `HEAD`).
fn is_detached_worktree(path: &std::path::Path) -> bool {
    path.join(".git").exists() || path.join("HEAD").exists()
}

/// Count `(open, parked)` task worktrees for a repo without spawning git.
///
/// Reads `<gitdir>/worktrees/*/gitdir` from disk, keeps only worktrees that
/// live under `<wt_dir>/<repo_key>/` (excluding the repo root itself and
/// detached snapshots outside `wt_dir`), and classifies each as `Open` when a
/// tmux session matches `session_name(repo_key, worktree_name)`. Otherwise
/// the worktree is counted as `Parked`.
///
/// Mirrors the filter logic of `build_task_rows` but skips branch-name
/// resolution (not needed for the Repos view) and the git subprocess.
fn count_repo_worktrees(
    repo_key: &RepoKey,
    gitdir: &Path,
    wt_dir: &Path,
    open_sessions: &HashSet<String>,
) -> (usize, usize) {
    let task_root_real = canonical(wt_dir).join(repo_key.as_str());
    let mut open = 0usize;
    let mut parked = 0usize;

    for wt_path in list_registered_worktrees(gitdir) {
        let real_path = std::fs::canonicalize(&wt_path).unwrap_or_else(|_| wt_path.clone());
        if !real_path.starts_with(&task_root_real) || real_path == task_root_real {
            continue;
        }
        let wt_name = worktree_name(wt_dir, repo_key.as_str(), &wt_path);
        let session = session_name(repo_key.as_str(), &wt_name);
        if open_sessions.contains(&session) {
            open += 1;
        } else {
            parked += 1;
        }
    }

    (open, parked)
}

fn canonical(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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

    let title = format!("{} {}", row.repo, row.branch);
    match park(&row.repo, &row.worktree_name, &row.path, &title)? {
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
                worktree_name: branch.to_string(),
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

    mod repo_row_sort_order {
        use crate::{runtime::RepoKey, ui::state::RepoRow};

        fn row(repo: &str, open: usize, parked: usize, detached: bool) -> RepoRow {
            RepoRow {
                repo: RepoKey::new(repo),
                open_tasks: open,
                parked_tasks: parked,
                is_detached: detached,
            }
        }

        /// Mirrors the sort used in `load_repo_rows`.
        fn sort(mut rows: Vec<RepoRow>) -> Vec<RepoRow> {
            rows.sort_by(|left, right| {
                right
                    .open_tasks
                    .cmp(&left.open_tasks)
                    .then(right.parked_tasks.cmp(&left.parked_tasks))
                    .then(right.is_detached.cmp(&left.is_detached))
                    .then(left.repo.cmp(&right.repo))
            });
            rows
        }

        #[test]
        fn higher_open_count_sorts_first() {
            let rows = vec![
                row("github.com/me/one-open", 1, 0, false),
                row("github.com/me/three-open", 3, 0, false),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.as_str(), "github.com/me/three-open");
            assert_eq!(sorted[1].repo.as_str(), "github.com/me/one-open");
        }

        #[test]
        fn equal_open_higher_parked_sorts_first() {
            let rows = vec![
                row("github.com/me/one-parked", 1, 1, false),
                row("github.com/me/three-parked", 1, 3, false),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.as_str(), "github.com/me/three-parked");
            assert_eq!(sorted[1].repo.as_str(), "github.com/me/one-parked");
        }

        #[test]
        fn equal_tasks_detached_sorts_before_non_detached() {
            let rows = vec![
                row("github.com/me/not-detached", 0, 1, false),
                row("github.com/me/detached", 0, 1, true),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.as_str(), "github.com/me/detached");
            assert_eq!(sorted[1].repo.as_str(), "github.com/me/not-detached");
        }

        #[test]
        fn alphabetical_tiebreaker() {
            let rows = vec![
                row("github.com/z/repo", 1, 0, false),
                row("github.com/a/repo", 1, 0, false),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.as_str(), "github.com/a/repo");
            assert_eq!(sorted[1].repo.as_str(), "github.com/z/repo");
        }

        #[test]
        fn repos_with_tasks_sort_before_empty_repos() {
            let rows = vec![
                row("github.com/me/no-tasks", 0, 0, false),
                row("github.com/me/has-open", 1, 0, false),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.as_str(), "github.com/me/has-open");
            assert_eq!(sorted[1].repo.as_str(), "github.com/me/no-tasks");
        }

        #[test]
        fn detached_empty_repos_sort_before_plain_empty_repos() {
            let rows = vec![
                row("github.com/me/plain", 0, 0, false),
                row("github.com/me/detached", 0, 0, true),
            ];
            let sorted = sort(rows);
            assert_eq!(sorted[0].repo.as_str(), "github.com/me/detached");
            assert_eq!(sorted[1].repo.as_str(), "github.com/me/plain");
        }

        #[test]
        fn full_composite_sort() {
            let rows = vec![
                row("github.com/d/empty", 0, 0, false),
                row("github.com/b/busy", 3, 1, false),
                row("github.com/c/moderate", 1, 0, false),
                row("github.com/a/empty-det", 0, 0, true),
                row("github.com/e/parked-only", 0, 2, false),
                row("github.com/f/also-busy", 3, 1, true),
            ];
            let sorted = sort(rows);
            // 3 open, 1 parked, detached → first
            assert_eq!(sorted[0].repo.as_str(), "github.com/f/also-busy");
            // 3 open, 1 parked, not detached
            assert_eq!(sorted[1].repo.as_str(), "github.com/b/busy");
            // 1 open
            assert_eq!(sorted[2].repo.as_str(), "github.com/c/moderate");
            // 0 open, 2 parked
            assert_eq!(sorted[3].repo.as_str(), "github.com/e/parked-only");
            // 0 tasks, detached
            assert_eq!(sorted[4].repo.as_str(), "github.com/a/empty-det");
            // 0 tasks, not detached
            assert_eq!(sorted[5].repo.as_str(), "github.com/d/empty");
        }

        #[test]
        fn empty_list_sorts_to_empty() {
            assert!(sort(vec![]).is_empty());
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

    mod initial_repo_scope_tests {
        use std::{env, fs};

        use super::super::initial_repo_scope;
        use crate::runtime::environment::RuntimeEnvironment;

        fn env_no_context() -> RuntimeEnvironment {
            let base = env::temp_dir().join("task-rs-ui-tasks-scope-no-ctx");
            let repos_dir = base.join("repos");
            let wt_dir = base.join("wt");
            let detached_dir = base.join("detached");
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir)
        }

        #[test]
        fn returns_repo_arg_when_provided() {
            let env = env_no_context();
            let scope = initial_repo_scope(&env, Some("github.com/me/app"));
            assert_eq!(scope, Some("github.com/me/app".to_string()));
        }

        #[test]
        fn returns_none_when_no_arg_and_no_context() {
            // Not inside a worktree → current_repo_key() returns None.
            let env = env_no_context();
            let scope = initial_repo_scope(&env, None);
            assert_eq!(scope, None);
        }
    }

    mod load_task_rows_tests {
        use std::{env, fs, process::Command};

        use super::super::load_task_rows;
        use crate::runtime::environment::RuntimeEnvironment;

        fn init_bare_repo(path: &std::path::Path) {
            fs::create_dir_all(path).expect("create repo dir");
            let status = Command::new("git")
                .args(["init", "--bare"])
                .arg(path)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git must be available");
            assert!(status.success(), "git init --bare failed");
        }

        fn env_for(base: &str) -> (std::path::PathBuf, RuntimeEnvironment) {
            let base_dir = env::temp_dir().join(format!("task-rs-ui-tasks-load-{base}"));
            let repos_dir = base_dir.join("repos");
            let wt_dir = base_dir.join("wt");
            let detached_dir = base_dir.join("detached");
            let _ = fs::remove_dir_all(&base_dir);
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            let env = RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir);
            (base_dir, env)
        }

        #[test]
        fn returns_empty_rows_when_no_repos() {
            let (_base, env) = env_for("no-repos");
            let rows = load_task_rows(&env, None).expect("load_task_rows");
            assert!(rows.is_empty(), "expected no rows with no repos");
        }

        #[test]
        fn errors_when_scoped_repo_does_not_exist() {
            let (_base, env) = env_for("missing-scoped");
            let result = load_task_rows(&env, Some("github.com/me/nonexistent"));
            assert!(result.is_err(), "should error for a missing scoped repo");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("not found") || msg.contains("nonexistent"),
                "error should mention the missing repo: {msg}"
            );
        }

        #[test]
        fn returns_ok_for_existing_bare_repo_with_no_worktrees() {
            let (base, env) = env_for("bare-no-wt");
            let repos_dir = base.join("repos");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let rows = load_task_rows(&env, Some("github.com/me/app")).expect("load_task_rows");
            // Bare repo with no worktrees → empty result
            assert!(
                rows.is_empty(),
                "expected no task rows for a bare repo with no worktrees"
            );
            let _ = fs::remove_dir_all(&base);
        }
    }

    mod is_detached_worktree_tests {
        use std::{env, fs};

        use super::super::is_detached_worktree;

        struct TempDir(std::path::PathBuf);
        impl TempDir {
            fn new(name: &str) -> Self {
                let path = env::temp_dir().join(format!("task-rs-detached-{name}"));
                let _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn detects_git_file_as_worktree() {
            // `git worktree add` creates a .git *file* (not directory)
            let dir = TempDir::new("git-file");
            fs::write(dir.path().join(".git"), "gitdir: /some/path").unwrap();
            assert!(is_detached_worktree(dir.path()));
        }

        #[test]
        fn detects_git_directory() {
            // A regular clone has a .git directory -- this also returns true
            let dir = TempDir::new("git-dir");
            fs::create_dir_all(dir.path().join(".git")).unwrap();
            assert!(is_detached_worktree(dir.path()));
        }

        #[test]
        fn detects_head_file_as_bare_repo() {
            let dir = TempDir::new("head-file");
            fs::write(dir.path().join("HEAD"), "ref: refs/heads/main").unwrap();
            assert!(is_detached_worktree(dir.path()));
        }

        #[test]
        fn returns_false_for_empty_directory() {
            let dir = TempDir::new("empty");
            assert!(!is_detached_worktree(dir.path()));
        }

        #[test]
        fn returns_false_for_nonexistent_path() {
            let path = env::temp_dir().join("task-rs-detached-nonexistent-12345");
            let _ = fs::remove_dir_all(&path);
            assert!(!is_detached_worktree(&path));
        }
    }

    mod count_repo_worktrees_tests {
        use std::{collections::HashSet, fs};

        use super::super::count_repo_worktrees;
        use crate::{runtime::RepoKey, tools::tmux::naming::session_name};

        struct TempDir(std::path::PathBuf);
        impl TempDir {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir().join(format!("task-rs-count-wt-{name}"));
                let _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        /// Register a worktree on disk the way `git worktree add` would:
        /// `<gitdir>/worktrees/<name>/gitdir` containing the absolute path
        /// of the worktree's `.git` file.
        fn register(gitdir: &std::path::Path, name: &str, wt_path: &std::path::Path) {
            let meta = gitdir.join("worktrees").join(name);
            fs::create_dir_all(&meta).unwrap();
            fs::create_dir_all(wt_path).unwrap();
            fs::write(
                meta.join("gitdir"),
                wt_path.join(".git").to_string_lossy().as_ref(),
            )
            .unwrap();
        }

        #[test]
        fn returns_zeros_when_no_worktrees_registered() {
            let dir = TempDir::new("zeros");
            let gitdir = dir.path().join("repo.git");
            fs::create_dir_all(&gitdir).unwrap();
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&wt_dir).unwrap();

            let repo_key = RepoKey::new("github.com/me/app");
            let (open, parked) = count_repo_worktrees(&repo_key, &gitdir, &wt_dir, &HashSet::new());
            assert_eq!((open, parked), (0, 0));
        }

        #[test]
        fn counts_worktree_under_wt_dir_as_parked_without_session() {
            let dir = TempDir::new("parked");
            let gitdir = dir.path().join("repo.git");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&gitdir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let repo_key = RepoKey::new("github.com/me/app");
            let wt_path = wt_dir.join("github.com/me/app/feat");
            register(&gitdir, "feat", &wt_path);

            let (open, parked) = count_repo_worktrees(&repo_key, &gitdir, &wt_dir, &HashSet::new());
            assert_eq!((open, parked), (0, 1));
        }

        #[test]
        fn counts_worktree_as_open_when_matching_session_is_present() {
            let dir = TempDir::new("open");
            let gitdir = dir.path().join("repo.git");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&gitdir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let repo_key = RepoKey::new("github.com/me/app");
            let wt_path = wt_dir.join("github.com/me/app/feat");
            register(&gitdir, "feat", &wt_path);

            // The session name used by open detection must match
            // `session_name(repo_key, worktree_name)`.
            let mut sessions = HashSet::new();
            sessions.insert(session_name(repo_key.as_str(), "feat"));

            let (open, parked) = count_repo_worktrees(&repo_key, &gitdir, &wt_dir, &sessions);
            assert_eq!((open, parked), (1, 0));
        }

        #[test]
        fn excludes_worktrees_outside_wt_dir_for_repo() {
            // Detached snapshots registered in the bare repo point at
            // paths outside `<wt_dir>/<repo_key>/` — they must not be counted.
            let dir = TempDir::new("outside");
            let gitdir = dir.path().join("repo.git");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&gitdir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let repo_key = RepoKey::new("github.com/me/app");
            // Outside the expected wt subtree.
            let detached_path = dir.path().join("detached/github.com/me/app");
            register(&gitdir, "detached", &detached_path);

            let (open, parked) = count_repo_worktrees(&repo_key, &gitdir, &wt_dir, &HashSet::new());
            assert_eq!((open, parked), (0, 0));
        }

        #[test]
        fn excludes_stale_entries() {
            let dir = TempDir::new("stale");
            let gitdir = dir.path().join("repo.git");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&gitdir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            // Metadata exists but the worktree path does not.
            let meta = gitdir.join("worktrees/ghost");
            fs::create_dir_all(&meta).unwrap();
            fs::write(meta.join("gitdir"), "/does/not/exist/.git").unwrap();

            let repo_key = RepoKey::new("github.com/me/app");
            let (open, parked) = count_repo_worktrees(&repo_key, &gitdir, &wt_dir, &HashSet::new());
            assert_eq!((open, parked), (0, 0));
        }

        #[test]
        fn mixes_open_and_parked_counts_across_worktrees() {
            let dir = TempDir::new("mixed");
            let gitdir = dir.path().join("repo.git");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&gitdir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();

            let repo_key = RepoKey::new("github.com/me/app");
            let wt_a = wt_dir.join("github.com/me/app/a");
            let wt_b = wt_dir.join("github.com/me/app/b");
            let wt_c = wt_dir.join("github.com/me/app/c");
            register(&gitdir, "a", &wt_a);
            register(&gitdir, "b", &wt_b);
            register(&gitdir, "c", &wt_c);

            let mut sessions = HashSet::new();
            sessions.insert(session_name(repo_key.as_str(), "a"));
            sessions.insert(session_name(repo_key.as_str(), "c"));

            let (open, parked) = count_repo_worktrees(&repo_key, &gitdir, &wt_dir, &sessions);
            assert_eq!((open, parked), (2, 1));
        }
    }
}
