use std::collections::HashSet;

use crate::{error::Result, runtime::environment::RuntimeEnvironment};

pub fn run(context: Option<&RuntimeEnvironment>, words: &[String]) -> Result<()> {
    for value in completion_values(context, words)? {
        println!("{value}");
    }
    Ok(())
}

fn completion_values(
    context: Option<&RuntimeEnvironment>,
    words: &[String],
) -> Result<Vec<String>> {
    if words.is_empty() {
        return Ok(top_level_commands());
    }

    let command = words[0].as_str();
    let args = &words[1..];
    let current = args.last().map(String::as_str).unwrap_or_default();
    let arg_count = args.len();

    let mut values = match command {
        "start" => {
            if arg_count <= 1 {
                repo_candidates(context)?
            } else {
                Vec::new()
            }
        }
        "open" => {
            if arg_count <= 1 {
                let mut values = task_candidates(context, None)?;
                values.extend(repo_candidates(context)?);
                values
            } else if arg_count == 2 {
                task_candidates(context, Some(&args[0]))?
            } else {
                Vec::new()
            }
        }
        "path" | "finish" => {
            if arg_count <= 1 {
                if current.starts_with('-') && command == "finish" {
                    vec!["--force".to_string()]
                } else {
                    repo_candidates(context)?
                }
            } else if arg_count == 2 {
                task_candidates(context, Some(&args[0]))?
            } else if command == "finish" && arg_count == 3 && current.starts_with('-') {
                vec!["--force".to_string()]
            } else {
                Vec::new()
            }
        }
        "rebase" => {
            if arg_count <= 1 {
                let mut values = task_candidates(context, None)?;
                values.extend(repo_candidates(context)?);
                values
            } else if arg_count == 2 {
                task_candidates(context, Some(&args[0]))?
            } else {
                Vec::new()
            }
        }
        "prune" | "list" | "ui" | "worktrees" => {
            if arg_count <= 1 {
                repo_candidates(context)?
            } else {
                Vec::new()
            }
        }
        "doctor" => {
            if arg_count <= 1 {
                vec!["--fix".to_string()]
            } else {
                Vec::new()
            }
        }
        "completions" => vec!["bash".to_string(), "fish".to_string(), "zsh".to_string()],
        _ => Vec::new(),
    };

    values.sort();
    values.dedup();
    Ok(filter_prefix(values, current))
}

fn top_level_commands() -> Vec<String> {
    [
        "bootstrap",
        "doctor",
        "clone",
        "start",
        "open",
        "park",
        "path",
        "list",
        "ui",
        "worktrees",
        "finish",
        "prune",
        "check",
        "rebase",
        "completions",
    ]
    .iter()
    .map(|&s| s.to_string())
    .collect()
}

fn repo_candidates(context: Option<&RuntimeEnvironment>) -> Result<Vec<String>> {
    let Some(context) = context else {
        return Ok(Vec::new());
    };

    let mut short_names: Vec<String> = context
        .tasks()
        .available_repo_keys()?
        .into_iter()
        .filter_map(|key| key.rsplit('/').next().map(str::to_string))
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

    // Empty set — completions don't need real tmux session state.
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

#[cfg(test)]
mod tests {
    use super::{completion_values, filter_prefix, top_level_commands};

    mod filter_prefix {
        use super::*;

        #[test]
        fn returns_all_when_empty_prefix() {
            let values = vec!["alpha".to_string(), "beta".to_string()];
            assert_eq!(filter_prefix(values.clone(), ""), values);
        }

        #[test]
        fn filters_case_insensitive() {
            let values = vec!["alpha".to_string(), "beta".to_string(), "Aleph".to_string()];
            let filtered = filter_prefix(values, "al");
            assert_eq!(filtered, vec!["alpha", "Aleph"]);
        }

        #[test]
        fn returns_empty_when_nothing_matches() {
            let values = vec!["alpha".to_string(), "beta".to_string()];
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
                "bootstrap",
                "doctor",
                "clone",
                "start",
                "open",
                "park",
                "path",
                "list",
                "ui",
                "worktrees",
                "finish",
                "prune",
                "check",
                "rebase",
                "completions",
            ] {
                assert!(
                    cmds.contains(&expected.to_string()),
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
            assert!(values.contains(&"doctor".to_string()));
            assert!(values.contains(&"bootstrap".to_string()));
        }

        #[test]
        fn unknown_command_returns_empty() {
            let values = completion_values(None, &["nonexistent".to_string(), "".to_string()])
                .expect("unknown command completions");
            assert!(values.is_empty());
        }

        #[test]
        fn doctor_includes_fix_flag() {
            let values = completion_values(None, &["doctor".to_string(), "".to_string()])
                .expect("doctor completion values");
            assert_eq!(values, vec!["--fix".to_string()]);
        }

        #[test]
        fn doctor_returns_empty_after_fix_flag() {
            let values = completion_values(
                None,
                &["doctor".to_string(), "--fix".to_string(), "".to_string()],
            )
            .expect("doctor extra arg");
            assert!(values.is_empty());
        }

        #[test]
        fn doctor_with_fix_prefix_returns_fix_flag() {
            let values = completion_values(None, &["doctor".to_string(), "--f".to_string()])
                .expect("doctor --fix prefix");
            assert_eq!(values, vec!["--fix"]);
        }

        #[test]
        fn doctor_with_non_matching_prefix_returns_empty() {
            let values = completion_values(None, &["doctor".to_string(), "--xyz".to_string()])
                .expect("doctor non-matching prefix");
            assert!(values.is_empty());
        }

        #[test]
        fn start_returns_empty_without_context() {
            let values = completion_values(None, &["start".to_string(), "".to_string()])
                .expect("start completions");
            assert!(values.is_empty());
        }

        #[test]
        fn start_returns_empty_for_second_arg() {
            let values = completion_values(
                None,
                &["start".to_string(), "some-repo".to_string(), "".to_string()],
            )
            .expect("start completions 2nd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn open_returns_empty_without_context() {
            let values = completion_values(None, &["open".to_string(), "".to_string()])
                .expect("open completions");
            assert!(values.is_empty());
        }

        #[test]
        fn open_second_arg_returns_empty_without_context() {
            let values = completion_values(
                None,
                &["open".to_string(), "some-repo".to_string(), "".to_string()],
            )
            .expect("open 2nd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn open_third_arg_returns_empty() {
            let values = completion_values(
                None,
                &[
                    "open".to_string(),
                    "repo".to_string(),
                    "branch".to_string(),
                    "".to_string(),
                ],
            )
            .expect("open 3rd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn path_returns_empty_without_context() {
            let values = completion_values(None, &["path".to_string(), "".to_string()])
                .expect("path completions");
            assert!(values.is_empty());
        }

        #[test]
        fn finish_suggests_force_flag_on_dash_prefix() {
            let values = completion_values(None, &["finish".to_string(), "-".to_string()])
                .expect("finish flag completions");
            assert_eq!(values, vec!["--force"]);
        }

        #[test]
        fn finish_returns_empty_repos_without_context() {
            let values = completion_values(None, &["finish".to_string(), "".to_string()])
                .expect("finish completions");
            assert!(values.is_empty());
        }

        #[test]
        fn finish_suggests_force_on_third_arg_dash() {
            let values = completion_values(
                None,
                &[
                    "finish".to_string(),
                    "repo".to_string(),
                    "branch".to_string(),
                    "-".to_string(),
                ],
            )
            .expect("finish 3rd arg flag");
            assert_eq!(values, vec!["--force"]);
        }

        #[test]
        fn rebase_returns_empty_without_context() {
            let values = completion_values(None, &["rebase".to_string(), "".to_string()])
                .expect("rebase completions");
            assert!(values.is_empty());
        }

        #[test]
        fn rebase_third_arg_returns_empty() {
            let values = completion_values(
                None,
                &[
                    "rebase".to_string(),
                    "repo".to_string(),
                    "branch".to_string(),
                    "".to_string(),
                ],
            )
            .expect("rebase 3rd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn prune_returns_empty_without_context() {
            let values = completion_values(None, &["prune".to_string(), "".to_string()])
                .expect("prune completions");
            assert!(values.is_empty());
        }

        #[test]
        fn prune_second_arg_returns_empty() {
            let values = completion_values(
                None,
                &["prune".to_string(), "repo".to_string(), "".to_string()],
            )
            .expect("prune 2nd arg");
            assert!(values.is_empty());
        }

        #[test]
        fn list_returns_empty_without_context() {
            let values = completion_values(None, &["list".to_string(), "".to_string()])
                .expect("list completions");
            assert!(values.is_empty());
        }

        #[test]
        fn ui_returns_empty_without_context() {
            let values = completion_values(None, &["ui".to_string(), "".to_string()])
                .expect("ui completions");
            assert!(values.is_empty());
        }

        #[test]
        fn worktrees_returns_empty_without_context() {
            let values = completion_values(None, &["worktrees".to_string(), "".to_string()])
                .expect("worktrees completions");
            assert!(values.is_empty());
        }

        #[test]
        fn completions_suggests_all_shells() {
            let values =
                completion_values(None, &["completions".to_string()]).expect("shell completions");
            assert!(values.contains(&"bash".to_string()));
            assert!(values.contains(&"fish".to_string()));
            assert!(values.contains(&"zsh".to_string()));
        }

        #[test]
        fn completions_with_partial_shell_prefix_filters_correctly() {
            let values = completion_values(None, &["completions".to_string(), "b".to_string()])
                .expect("shell prefix completions");
            assert_eq!(values, vec!["bash"]);
        }

        #[test]
        fn completions_with_fish_prefix_returns_fish() {
            let values = completion_values(None, &["completions".to_string(), "f".to_string()])
                .expect("fish prefix completions");
            assert_eq!(values, vec!["fish"]);
        }
    }
}
