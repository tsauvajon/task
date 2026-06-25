use std::collections::HashSet;

use crate::{
    commands::detach::{collect_detached_worktrees, repo_key_from_detached_path},
    error::Result,
    runtime::{environment::RuntimeEnvironment, process},
};

pub fn run(context: Option<&RuntimeEnvironment>, words: &[String]) -> Result<()> {
    for value in completion_values(context, words)? {
        process::write_stdout_line(value)?;
    }
    Ok(())
}

fn completion_values(
    context: Option<&RuntimeEnvironment>,
    words: &[String],
) -> Result<Vec<String>> {
    let Some((command, args)) = words.split_first() else {
        return Ok(top_level_commands());
    };
    if args.is_empty() && command.is_empty() {
        return Ok(top_level_commands());
    }

    let command = command.as_str();

    // Single word that isn't an exact command match — filter top-level commands by prefix.
    if args.is_empty() {
        if command.starts_with('-') {
            return Ok(filter_prefix(global_flags(), command));
        }
        let top = top_level_commands();
        if !top.iter().any(|c| c == command) {
            return Ok(filter_prefix(top, command));
        }
    }

    let current = args.last().map(String::as_str).unwrap_or_default();
    let arg_count = args.len();

    let mut values = match command {
        "start" => {
            if current.starts_with('-') {
                vec!["--no-open".to_owned()]
            } else if arg_count <= 1 {
                repo_candidates(context)?
            } else {
                Vec::new()
            }
        }
        "open" | "rebase" => {
            if arg_count <= 1 {
                let mut values = task_candidates(context, None)?;
                values.extend(repo_candidates(context)?);
                values
            } else if arg_count == 2 {
                task_candidates(context, args.first().map(String::as_str))?
            } else {
                Vec::new()
            }
        }
        "path" => {
            if arg_count <= 1 {
                repo_candidates(context)?
            } else if arg_count == 2 {
                task_candidates(context, args.first().map(String::as_str))?
            } else {
                Vec::new()
            }
        }
        "finish" => {
            if arg_count <= 1 {
                if current.starts_with('-') {
                    vec!["--force".to_owned()]
                } else {
                    finish_task_candidates(context, args)?
                }
            } else if current.starts_with('-') {
                vec!["--force".to_owned()]
            } else {
                finish_task_candidates(context, args)?
            }
        }
        "list" | "ui" => {
            if arg_count <= 1 {
                repo_candidates(context)?
            } else {
                Vec::new()
            }
        }
        "repo" => repo_subcommand_completions(context, args)?,
        "detach" => detach_subcommand_completions(context, args)?,
        "completions" => vec!["bash".to_owned(), "fish".to_owned(), "zsh".to_owned()],
        _ => Vec::new(),
    };

    if current.starts_with('-') {
        values.extend(subcommand_help_flags());
    }

    values.sort();
    values.dedup();
    Ok(filter_prefix(values, current))
}

fn top_level_commands() -> Vec<String> {
    [
        "doctor",
        "repo",
        "start",
        "open",
        "park",
        "path",
        "list",
        "ui",
        "finish",
        "rebase",
        "detach",
        "completions",
    ]
    .iter()
    .map(|&s| s.to_owned())
    .collect()
}

fn global_flags() -> Vec<String> {
    ["-h", "-V", "--help", "--version"]
        .iter()
        .map(|&s| s.to_owned())
        .collect()
}

fn subcommand_help_flags() -> Vec<String> {
    ["-h", "--help"].iter().map(|&s| s.to_owned()).collect()
}

fn repo_subcommand_completions(
    context: Option<&RuntimeEnvironment>,
    args: &[String],
) -> Result<Vec<String>> {
    let repo_subcommands = vec!["list".to_owned(), "clone".to_owned(), "prune".to_owned()];

    if args.len() <= 1 {
        return Ok(repo_subcommands);
    }

    let Some((subcmd, sub_args)) = args.split_first() else {
        return Ok(repo_subcommands);
    };
    let subcmd = subcmd.as_str();
    let sub_arg_count = sub_args.len();

    match subcmd {
        "prune" => {
            if sub_arg_count <= 1 {
                repo_candidates(context)
            } else {
                Ok(Vec::new())
            }
        }
        _ => Ok(Vec::new()),
    }
}

fn detach_subcommand_completions(
    context: Option<&RuntimeEnvironment>,
    args: &[String],
) -> Result<Vec<String>> {
    let detach_subcommands = vec![
        "add".to_owned(),
        "update".to_owned(),
        "remove".to_owned(),
        "list".to_owned(),
    ];

    if args.len() <= 1 {
        return Ok(detach_subcommands);
    }

    let Some((subcmd, sub_args)) = args.split_first() else {
        return Ok(detach_subcommands);
    };
    let subcmd = subcmd.as_str();
    let sub_arg_count = sub_args.len();
    let current = sub_args.last().map(String::as_str).unwrap_or_default();

    // All detach subcommands take at most one positional (repo), plus optional flags.
    // For `remove`, a `--force` flag is also valid.
    if sub_arg_count >= 3 {
        return Ok(Vec::new());
    }

    match subcmd {
        "add" => {
            if sub_arg_count <= 1 {
                repo_candidates(context)
            } else {
                Ok(Vec::new())
            }
        }
        "update" => {
            if sub_arg_count <= 1 {
                detached_repo_candidates(context)
            } else {
                Ok(Vec::new())
            }
        }
        "remove" => {
            if current.starts_with('-') {
                Ok(vec!["--force".to_owned()])
            } else if sub_arg_count <= 1 {
                detached_repo_candidates(context)
            } else {
                Ok(Vec::new())
            }
        }
        _ => Ok(Vec::new()),
    }
}

fn detached_repo_candidates(context: Option<&RuntimeEnvironment>) -> Result<Vec<String>> {
    let Some(context) = context else {
        return Ok(Vec::new());
    };

    let detached_dir = context.layout().detached_dir();
    let mut worktrees: Vec<std::path::PathBuf> = Vec::new();
    collect_detached_worktrees(detached_dir, &mut worktrees)?;

    let mut short_names: Vec<String> = worktrees
        .into_iter()
        .map(|path| repo_key_from_detached_path(detached_dir, &path))
        .filter_map(|key| key.rsplit('/').next().map(str::to_owned))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    short_names.sort();
    Ok(short_names)
}

fn repo_candidates(context: Option<&RuntimeEnvironment>) -> Result<Vec<String>> {
    let Some(context) = context else {
        return Ok(Vec::new());
    };

    let mut short_names: Vec<String> = context
        .tasks()
        .available_repo_keys()?
        .into_iter()
        .filter_map(|key| key.rsplit('/').next().map(str::to_owned))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    short_names.sort();
    Ok(short_names)
}

fn filter_prefix(values: Vec<String>, current: &str) -> Vec<String> {
    if current.is_empty() {
        return values;
    }
    let current_lower = current.to_lowercase();
    values
        .into_iter()
        .filter(|v| v.to_lowercase().starts_with(&current_lower))
        .collect()
}

fn task_candidates(
    context: Option<&RuntimeEnvironment>,
    repo_hint: Option<&str>,
) -> Result<Vec<String>> {
    let Some(context) = context else {
        return Ok(Vec::new());
    };

    let keys = match repo_hint {
        Some(repo) => vec![context.tasks().resolve_repo_key_input(repo)?],
        None => context.tasks().available_repo_keys()?,
    };

    // Empty set — completions don't need real Zellij session state.
    let open_sessions = HashSet::new();
    let mut branches: Vec<String> = keys
        .into_iter()
        .flat_map(|repo_key| {
            let gitdir = context.layout().repo_gitdir_path(&repo_key);
            context
                .tasks()
                .repo_task_rows(&repo_key, &gitdir, &open_sessions)
                .unwrap_or_default()
                .into_iter()
                .map(|row| row.branch.to_string())
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    branches.sort();
    Ok(branches)
}

fn finish_task_candidates(
    context: Option<&RuntimeEnvironment>,
    args: &[String],
) -> Result<Vec<String>> {
    let used: HashSet<&str> = args
        .iter()
        .take(args.len().saturating_sub(1))
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect();
    let mut values = task_candidates(context, None)?;
    values.retain(|value| !used.contains(value.as_str()));
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{completion_values, filter_prefix, top_level_commands};

    mod filter_prefix {
        use super::*;

        #[test]
        fn returns_all_when_empty_prefix() {
            let values = vec!["alpha".to_owned(), "beta".to_owned()];
            assert_eq!(filter_prefix(values.clone(), ""), values);
        }

        #[test]
        fn filters_case_insensitive() {
            let values = vec!["alpha".to_owned(), "beta".to_owned(), "Aleph".to_owned()];
            let filtered = filter_prefix(values, "al");
            assert_eq!(filtered, vec!["alpha", "Aleph"]);
        }

        #[test]
        fn returns_empty_when_nothing_matches() {
            let values = vec!["alpha".to_owned(), "beta".to_owned()];
            let filtered = filter_prefix(values, "zzz");
            assert!(filtered.is_empty(), "no values should match 'zzz'");
        }

        #[test]
        fn empty_input_returns_empty() {
            let filtered = filter_prefix(vec![], "al");
            assert!(filtered.is_empty());
        }
    }

    mod top_level_commands {
        use super::*;

        #[test]
        fn includes_all_expected() {
            let cmds = top_level_commands();
            for expected in [
                "doctor",
                "repo",
                "start",
                "open",
                "park",
                "path",
                "list",
                "ui",
                "finish",
                "rebase",
                "detach",
                "completions",
            ] {
                assert!(
                    cmds.contains(&expected.to_owned()),
                    "missing command: {expected}"
                );
            }
        }

        #[test]
        fn has_no_duplicates() {
            let cmds = top_level_commands();
            let mut seen = std::collections::HashSet::new();
            for cmd in &cmds {
                assert!(seen.insert(cmd.clone()), "duplicate command: {cmd}");
            }
        }
    }

    mod completion_values {
        use super::*;

        #[test]
        fn top_level_available_without_configured_context() {
            let values = completion_values(None, &[]).expect("top-level completion values");
            assert!(values.contains(&"doctor".to_owned()));
            assert!(values.contains(&"start".to_owned()));
        }

        #[test]
        fn top_level_available_with_single_empty_word() {
            let values = completion_values(None, &[String::new()])
                .expect("top-level completion from empty word");
            assert!(values.contains(&"doctor".to_owned()));
            assert!(values.contains(&"start".to_owned()));
        }

        #[test]
        fn partial_command_filters_top_level() {
            let values =
                completion_values(None, &["re".to_owned()]).expect("partial command prefix");
            assert!(values.contains(&"rebase".to_owned()));
            assert!(values.contains(&"repo".to_owned()));
            assert!(!values.contains(&"start".to_owned()));
        }

        #[test]
        fn partial_command_no_match_returns_empty() {
            let values = completion_values(None, &["xyz".to_owned()]).expect("non-matching prefix");
            assert!(values.is_empty());
        }

        #[test]
        fn dash_suggests_all_global_flags() {
            let values = completion_values(None, &["-".to_owned()]).expect("dash global flags");
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"-V".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
            assert!(values.contains(&"--version".to_owned()));
        }

        #[test]
        fn double_dash_suggests_long_global_flags() {
            let values =
                completion_values(None, &["--".to_owned()]).expect("double dash global flags");
            assert!(values.contains(&"--help".to_owned()));
            assert!(values.contains(&"--version".to_owned()));
            assert!(!values.contains(&"-h".to_owned()));
            assert!(!values.contains(&"-V".to_owned()));
        }

        #[test]
        fn double_dash_h_completes_help() {
            let values = completion_values(None, &["--h".to_owned()]).expect("--h prefix");
            assert_eq!(values, vec!["--help"]);
        }

        #[test]
        fn dash_v_completes_version() {
            let values = completion_values(None, &["-V".to_owned()]).expect("-V prefix");
            assert_eq!(values, vec!["-V"]);
        }

        #[test]
        fn start_dash_includes_help_and_no_open() {
            let values = completion_values(None, &["start".to_owned(), "-".to_owned()])
                .expect("start dash flags");
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
            assert!(values.contains(&"--no-open".to_owned()));
        }

        #[test]
        fn open_double_dash_suggests_help() {
            let values = completion_values(None, &["open".to_owned(), "--".to_owned()])
                .expect("open double dash");
            assert_eq!(values, vec!["--help"]);
        }

        #[test]
        fn doctor_dash_includes_help_flags() {
            let values = completion_values(None, &["doctor".to_owned(), "-".to_owned()])
                .expect("doctor dash flags");
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
        }

        #[test]
        fn unknown_command_returns_empty() {
            let values = completion_values(None, &["nonexistent".to_owned(), String::new()])
                .expect("unknown command completions");
            assert!(values.is_empty());
        }

        #[test]
        fn doctor_takes_no_positional_args() {
            let values = completion_values(None, &["doctor".to_owned(), String::new()])
                .expect("doctor completion values");
            assert!(values.is_empty());
        }

        #[test]
        fn start_returns_empty_without_context() {
            let values = completion_values(None, &["start".to_owned(), String::new()])
                .expect("start completions");
            assert!(values.is_empty());
        }

        #[test]
        fn start_returns_empty_for_second_arg() {
            let values = completion_values(
                None,
                &["start".to_owned(), "some-repo".to_owned(), String::new()],
            )
            .expect("start completions 2nd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn start_suggests_no_open_on_dash_prefix() {
            let values = completion_values(None, &["start".to_owned(), "-".to_owned()])
                .expect("start flag completions");
            assert!(values.contains(&"--no-open".to_owned()));
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
        }

        #[test]
        fn start_suggests_no_open_on_double_dash_prefix() {
            let values = completion_values(None, &["start".to_owned(), "--".to_owned()])
                .expect("start flag completions");
            assert!(values.contains(&"--no-open".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
            assert!(!values.contains(&"-h".to_owned()));
        }

        #[test]
        fn start_suggests_no_open_when_typing_flag_mid_args() {
            let values = completion_values(
                None,
                &["start".to_owned(), "some-repo".to_owned(), "-".to_owned()],
            )
            .expect("start mid-args flag completions");
            assert!(values.contains(&"--no-open".to_owned()));
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
        }

        #[test]
        fn open_returns_empty_without_context() {
            let values = completion_values(None, &["open".to_owned(), String::new()])
                .expect("open completions");
            assert!(values.is_empty());
        }

        #[test]
        fn open_second_arg_returns_empty_without_context() {
            let values = completion_values(
                None,
                &["open".to_owned(), "some-repo".to_owned(), String::new()],
            )
            .expect("open 2nd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn open_third_arg_returns_empty() {
            let values = completion_values(
                None,
                &[
                    "open".to_owned(),
                    "repo".to_owned(),
                    "branch".to_owned(),
                    String::new(),
                ],
            )
            .expect("open 3rd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn path_returns_empty_without_context() {
            let values = completion_values(None, &["path".to_owned(), String::new()])
                .expect("path completions");
            assert!(values.is_empty());
        }

        #[test]
        fn finish_suggests_force_flag_on_dash_prefix() {
            let values = completion_values(None, &["finish".to_owned(), "-".to_owned()])
                .expect("finish flag completions");
            assert!(values.contains(&"--force".to_owned()));
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
        }

        #[test]
        fn finish_returns_empty_repos_without_context() {
            let values = completion_values(None, &["finish".to_owned(), String::new()])
                .expect("finish completions");
            assert!(values.is_empty());
        }

        #[test]
        fn finish_suggests_force_on_third_arg_dash() {
            let values = completion_values(
                None,
                &[
                    "finish".to_owned(),
                    "repo".to_owned(),
                    "branch".to_owned(),
                    "-".to_owned(),
                ],
            )
            .expect("finish 3rd arg flag");
            assert!(values.contains(&"--force".to_owned()));
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
        }

        #[test]
        fn rebase_returns_empty_without_context() {
            let values = completion_values(None, &["rebase".to_owned(), String::new()])
                .expect("rebase completions");
            assert!(values.is_empty());
        }

        #[test]
        fn rebase_third_arg_returns_empty() {
            let values = completion_values(
                None,
                &[
                    "rebase".to_owned(),
                    "repo".to_owned(),
                    "branch".to_owned(),
                    String::new(),
                ],
            )
            .expect("rebase 3rd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn repo_suggests_subcommands() {
            let values = completion_values(None, &["repo".to_owned(), String::new()])
                .expect("repo subcommand completions");
            assert!(values.contains(&"list".to_owned()));
            assert!(values.contains(&"clone".to_owned()));
            assert!(values.contains(&"prune".to_owned()));
        }

        #[test]
        fn repo_prune_returns_empty_without_context() {
            let values = completion_values(
                None,
                &["repo".to_owned(), "prune".to_owned(), String::new()],
            )
            .expect("repo prune completions");
            assert!(values.is_empty());
        }

        #[test]
        fn repo_prune_second_arg_returns_empty() {
            let values = completion_values(
                None,
                &[
                    "repo".to_owned(),
                    "prune".to_owned(),
                    "some-repo".to_owned(),
                    String::new(),
                ],
            )
            .expect("repo prune 2nd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn list_returns_empty_without_context() {
            let values = completion_values(None, &["list".to_owned(), String::new()])
                .expect("list completions");
            assert!(values.is_empty());
        }

        #[test]
        fn ui_returns_empty_without_context() {
            let values =
                completion_values(None, &["ui".to_owned(), String::new()]).expect("ui completions");
            assert!(values.is_empty());
        }

        #[test]
        fn completions_suggests_all_shells() {
            let values =
                completion_values(None, &["completions".to_owned()]).expect("shell completions");
            assert!(values.contains(&"bash".to_owned()));
            assert!(values.contains(&"fish".to_owned()));
            assert!(values.contains(&"zsh".to_owned()));
        }

        #[test]
        fn completions_with_partial_shell_prefix_filters_correctly() {
            let values = completion_values(None, &["completions".to_owned(), "b".to_owned()])
                .expect("shell prefix completions");
            assert_eq!(values, vec!["bash"]);
        }

        #[test]
        fn completions_with_fish_prefix_returns_fish() {
            let values = completion_values(None, &["completions".to_owned(), "f".to_owned()])
                .expect("fish prefix completions");
            assert_eq!(values, vec!["fish"]);
        }
    }

    mod finish {
        use std::{fs, path::Path, process::Command};

        use super::completion_values;
        use crate::runtime::environment::RuntimeEnvironment;

        struct TempDir(std::path::PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir().join(format!("task-rs-complete-finish-{name}"));
                _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                _ = fs::remove_dir_all(&self.0);
            }
        }

        fn make_env(base: &Path) -> RuntimeEnvironment {
            let repos_dir = base.join("repos");
            let wt_dir = base.join("wt");
            let detached_dir = base.join("detached");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            fs::create_dir_all(&detached_dir).unwrap();
            RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir)
        }

        fn setup_worktree(env: &RuntimeEnvironment, repo_slug: &str, branch: &str) {
            let gitdir = env.layout().repos_dir().join(format!("{repo_slug}.git"));
            fs::create_dir_all(gitdir.parent().expect("gitdir parent")).unwrap();

            let init_status = Command::new("git")
                .args(["init", "--bare", gitdir.to_str().expect("gitdir path")])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", std::env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git init --bare");
            assert!(init_status.success(), "git init --bare failed");

            let wt_path = env.layout().wt_dir().join(repo_slug).join(branch);
            fs::create_dir_all(wt_path.parent().expect("worktree parent")).unwrap();

            let add_status = Command::new("git")
                .args([
                    "--git-dir",
                    gitdir.to_str().expect("gitdir path"),
                    "worktree",
                    "add",
                    "--orphan",
                    "-b",
                    branch,
                    wt_path.to_str().expect("worktree path"),
                ])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", std::env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git worktree add");
            assert!(add_status.success(), "git worktree add --orphan failed");
        }

        #[test]
        fn first_positional_suggests_task_names() {
            let dir = TempDir::new("first-positional");
            let env = make_env(dir.path());
            setup_worktree(&env, "github.com/org/alpha", "alpha-task");
            setup_worktree(&env, "github.com/org/beta", "beta-task");

            let values = completion_values(Some(&env), &["finish".to_owned(), String::new()])
                .expect("finish completions");
            assert_eq!(values, vec!["alpha-task", "beta-task"]);
        }

        #[test]
        fn later_positionals_keep_suggesting_task_names() {
            let dir = TempDir::new("later-positionals");
            let env = make_env(dir.path());
            setup_worktree(&env, "github.com/org/alpha", "alpha-task");
            setup_worktree(&env, "github.com/org/beta", "beta-task");

            let values = completion_values(
                Some(&env),
                &["finish".to_owned(), "alpha-task".to_owned(), "b".to_owned()],
            )
            .expect("finish second task completions");
            assert_eq!(values, vec!["beta-task"]);
        }

        #[test]
        fn later_positionals_exclude_entered_task_names() {
            let dir = TempDir::new("excludes-entered");
            let env = make_env(dir.path());
            setup_worktree(&env, "github.com/org/alpha", "alpha-task");
            setup_worktree(&env, "github.com/org/beta", "beta-task");

            let values = completion_values(
                Some(&env),
                &["finish".to_owned(), "alpha-task".to_owned(), String::new()],
            )
            .expect("finish deduped task completions");
            assert_eq!(values, vec!["beta-task"]);
        }
    }

    mod detach {
        use std::fs;

        use super::completion_values;
        use crate::runtime::environment::RuntimeEnvironment;

        struct TempDir(std::path::PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir().join(format!("task-rs-complete-detach-{name}"));
                _ = fs::remove_dir_all(&path);
                fs::create_dir_all(&path).expect("create temp dir");
                Self(path)
            }

            fn path(&self) -> &std::path::Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                _ = fs::remove_dir_all(&self.0);
            }
        }

        fn make_env(base: &std::path::Path) -> RuntimeEnvironment {
            let repos_dir = base.join("repos");
            let wt_dir = base.join("wt");
            let detached_dir = base.join("detached");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            fs::create_dir_all(&detached_dir).unwrap();
            RuntimeEnvironment::from_paths(&repos_dir, &wt_dir, &detached_dir)
        }

        fn make_detached_worktree(detached_dir: &std::path::Path, repo_key: &str) {
            let path = detached_dir.join(repo_key);
            fs::create_dir_all(&path).expect("create detached worktree dir");
            fs::write(path.join(".git"), "gitdir: ...").expect("write .git marker");
        }

        #[test]
        fn top_level_includes_detach() {
            let values =
                completion_values(None, &["de".to_owned()]).expect("detach prefix completions");
            assert!(
                values.contains(&"detach".to_owned()),
                "expected 'detach' in: {values:?}"
            );
        }

        #[test]
        fn detach_alone_suggests_subcommands() {
            let values = completion_values(None, &["detach".to_owned(), String::new()])
                .expect("detach subcommand completions");
            for expected in ["add", "update", "remove", "list"] {
                assert!(
                    values.contains(&expected.to_owned()),
                    "missing {expected} in {values:?}"
                );
            }
        }

        #[test]
        fn detach_partial_subcommand_filters() {
            let values = completion_values(None, &["detach".to_owned(), "u".to_owned()])
                .expect("detach partial subcommand");
            assert_eq!(values, vec!["update"]);
        }

        #[test]
        fn detach_update_returns_empty_without_context() {
            let values = completion_values(
                None,
                &["detach".to_owned(), "update".to_owned(), String::new()],
            )
            .expect("detach update without context");
            assert!(values.is_empty());
        }

        #[test]
        fn detach_update_suggests_detached_repos() {
            let dir = TempDir::new("update-suggests");
            let env = make_env(dir.path());
            make_detached_worktree(env.layout().detached_dir(), "github.com/org/alpha");
            make_detached_worktree(env.layout().detached_dir(), "gitlab.com/team/beta");

            let values = completion_values(
                Some(&env),
                &["detach".to_owned(), "update".to_owned(), String::new()],
            )
            .expect("detach update completions");
            assert_eq!(values, vec!["alpha", "beta"]);
        }

        #[test]
        fn detach_update_filters_by_prefix() {
            let dir = TempDir::new("update-prefix");
            let env = make_env(dir.path());
            make_detached_worktree(env.layout().detached_dir(), "github.com/org/alpha");
            make_detached_worktree(env.layout().detached_dir(), "gitlab.com/team/beta");

            let values = completion_values(
                Some(&env),
                &["detach".to_owned(), "update".to_owned(), "al".to_owned()],
            )
            .expect("detach update prefix completions");
            assert_eq!(values, vec!["alpha"]);
        }

        #[test]
        fn detach_update_second_positional_returns_empty() {
            let dir = TempDir::new("update-second");
            let env = make_env(dir.path());
            make_detached_worktree(env.layout().detached_dir(), "github.com/org/alpha");

            let values = completion_values(
                Some(&env),
                &[
                    "detach".to_owned(),
                    "update".to_owned(),
                    "alpha".to_owned(),
                    String::new(),
                ],
            )
            .expect("detach update 2nd positional");
            assert!(values.is_empty());
        }

        #[test]
        fn detach_remove_dash_suggests_force() {
            let values = completion_values(
                None,
                &["detach".to_owned(), "remove".to_owned(), "-".to_owned()],
            )
            .expect("detach remove dash");
            assert!(values.contains(&"--force".to_owned()));
            assert!(values.contains(&"-h".to_owned()));
            assert!(values.contains(&"--help".to_owned()));
        }

        #[test]
        fn detach_remove_suggests_detached_repos() {
            let dir = TempDir::new("remove-suggests");
            let env = make_env(dir.path());
            make_detached_worktree(env.layout().detached_dir(), "github.com/org/gamma");

            let values = completion_values(
                Some(&env),
                &["detach".to_owned(), "remove".to_owned(), String::new()],
            )
            .expect("detach remove completions");
            assert_eq!(values, vec!["gamma"]);
        }

        #[test]
        fn detach_add_without_context_returns_empty() {
            let values = completion_values(
                None,
                &["detach".to_owned(), "add".to_owned(), String::new()],
            )
            .expect("detach add without context");
            assert!(values.is_empty());
        }

        #[test]
        fn detach_list_takes_no_args() {
            let values = completion_values(
                None,
                &["detach".to_owned(), "list".to_owned(), String::new()],
            )
            .expect("detach list completions");
            assert!(values.is_empty());
        }

        #[test]
        fn detach_double_dash_includes_help() {
            let values = completion_values(
                None,
                &["detach".to_owned(), "update".to_owned(), "--".to_owned()],
            )
            .expect("detach update double dash");
            assert!(values.contains(&"--help".to_owned()));
        }
    }
}
