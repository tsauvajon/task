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
                .map(|row| row.branch)
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    branches.sort();
    Ok(branches)
}

#[cfg(test)]
mod tests {
    use super::completion_values;

    #[test]
    fn doctor_completion_includes_fix_flag() {
        let values = completion_values(None, &["doctor".to_string(), "".to_string()])
            .expect("doctor completion values");
        assert_eq!(values, vec!["--fix".to_string()]);
    }

    #[test]
    fn top_level_completion_available_without_configured_context() {
        let values = completion_values(None, &[]).expect("top-level completion values");
        assert!(values.contains(&"doctor".to_string()));
        assert!(values.contains(&"bootstrap".to_string()));
    }
}
