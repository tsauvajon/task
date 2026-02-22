use std::collections::HashSet;

use crate::layout::Layout;

pub fn run(layout: &Layout, words: &[String]) -> Result<(), String> {
    let values = completion_values(layout, words)?;
    for value in values {
        println!("{value}");
    }
    Ok(())
}

fn completion_values(layout: &Layout, words: &[String]) -> Result<Vec<String>, String> {
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
                repo_candidates(layout)?
            } else {
                Vec::new()
            }
        }
        "open" => {
            if arg_count <= 1 {
                let mut values = task_candidates(layout, None)?;
                values.extend(repo_candidates(layout)?);
                values
            } else if arg_count == 2 {
                task_candidates(layout, Some(&args[0]))?
            } else {
                Vec::new()
            }
        }
        "path" | "finish" => {
            if arg_count <= 1 {
                if current.starts_with('-') && command == "finish" {
                    vec!["--force".to_string()]
                } else {
                    repo_candidates(layout)?
                }
            } else if arg_count == 2 {
                task_candidates(layout, Some(&args[0]))?
            } else if command == "finish" && arg_count == 3 && current.starts_with('-') {
                vec!["--force".to_string()]
            } else {
                Vec::new()
            }
        }
        "prune" | "list" | "ui" | "worktrees" => {
            if arg_count <= 1 {
                repo_candidates(layout)?
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
    vec![
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
        "completions",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn repo_candidates(layout: &Layout) -> Result<Vec<String>, String> {
    let mut values = HashSet::new();
    for key in super::available_repo_keys(layout)? {
        if let Some(short) = key.rsplit('/').next() {
            values.insert(short.to_string());
        }
    }
    let mut values: Vec<String> = values.into_iter().collect();
    values.sort();
    Ok(values)
}

fn filter_prefix(values: Vec<String>, current: &str) -> Vec<String> {
    if current.is_empty() {
        return values;
    }

    let current_lower = current.to_lowercase();
    values
        .into_iter()
        .filter(|value| value.to_lowercase().starts_with(&current_lower))
        .collect()
}

fn task_candidates(layout: &Layout, repo_hint: Option<&str>) -> Result<Vec<String>, String> {
    let keys = match repo_hint {
        Some(repo) => {
            let resolved = super::resolve_repo_key_input(layout, repo)?;
            vec![resolved]
        }
        None => super::available_repo_keys(layout)?,
    };

    let open_sessions = HashSet::new();
    let mut values = HashSet::new();
    for repo_key in keys {
        let gitdir = layout.repo_gitdir_path(&repo_key);
        for row in super::repo_task_rows(&repo_key, &gitdir, &open_sessions)? {
            values.insert(row.branch);
        }
    }

    let mut values: Vec<String> = values.into_iter().collect();
    values.sort();
    Ok(values)
}
