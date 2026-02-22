use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use comfy_table::{Cell, Color, ContentArrangement, Table};
use dialoguer::{theme::ColorfulTheme, Select};
use owo_colors::OwoColorize;

use crate::layout::Layout;
use crate::repo_key::{normalize_repo_key, resolve_repo_key, ResolveResult};
use crate::session::session_name_for;
use crate::worktree::{
    branch_from_worktree_path, build_task_rows, parse_worktree_porcelain, repo_key_from_common_dir,
    TaskRow,
};

pub(super) fn default_dev_root() -> PathBuf {
    if let Ok(dev_root) = env::var("DEV_ROOT") {
        return PathBuf::from(dev_root);
    }

    let home = env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    PathBuf::from(home).join("dev")
}

pub(super) fn ensure_layout(layout: &Layout) -> Result<(), String> {
    let repos = layout.repo_gitdir_path("");
    let repos_dir = repos
        .parent()
        .ok_or_else(|| "Could not resolve repos dir".to_string())?;
    let wt_root = layout.worktree_path("", "");
    let wt_dir = wt_root
        .parent()
        .ok_or_else(|| "Could not resolve wt dir".to_string())?;

    fs::create_dir_all(repos_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(wt_dir).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn available_repo_keys(layout: &Layout) -> Result<Vec<String>, String> {
    let repos_dir = layout
        .repo_gitdir_path("")
        .parent()
        .ok_or_else(|| "Could not resolve repos dir".to_string())?
        .to_path_buf();

    let gitdirs = collect_gitdirs(&repos_dir)?;
    let mut keys = Vec::new();
    for gitdir in gitdirs {
        if let Ok(relative) = gitdir.strip_prefix(&repos_dir) {
            let mut key = relative.to_string_lossy().to_string();
            if key.ends_with(".git") {
                key.truncate(key.len() - 4);
            }
            keys.push(key);
        }
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

pub(super) fn collect_gitdirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut stack = vec![root.to_path_buf()];
    let mut gitdirs = Vec::new();

    while let Some(current) = stack.pop() {
        if !current.is_dir() {
            continue;
        }

        let entries = fs::read_dir(&current).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
            if name.ends_with(".git") {
                gitdirs.push(path);
            } else {
                stack.push(path);
            }
        }
    }

    gitdirs.sort();
    Ok(gitdirs)
}

pub(super) fn is_git_url(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("ssh://")
        || input.starts_with("git@")
}

pub(super) fn resolve_repo_key_input(layout: &Layout, repo_arg: &str) -> Result<String, String> {
    if is_git_url(repo_arg) {
        return Ok(normalize_repo_key(repo_arg));
    }

    let normalized = normalize_repo_key(repo_arg);
    if layout.repo_gitdir_path(&normalized).is_dir() {
        return Ok(normalized);
    }

    let keys = available_repo_keys(layout)?;
    match resolve_repo_key(&normalized, &keys) {
        ResolveResult::Resolved(value) => Ok(value),
        ResolveResult::Ambiguous(choices) => choose_repo_key_interactive(repo_arg, &choices),
    }
}

pub(super) fn choose_repo_key_interactive(
    query: &str,
    choices: &[String],
) -> Result<String, String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(format!(
            "Multiple repositories match '{query}': {}. Please use a full repo key.",
            choices.join(" ")
        ));
    }

    let prompt = format!("Multiple repositories match '{query}'. Choose one:");
    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(choices)
        .default(0)
        .interact_opt()
        .map_err(|error| error.to_string())?;

    if let Some(index) = index {
        return Ok(choices[index].clone());
    }

    Err("Selection cancelled.".to_string())
}

pub(super) fn clone_bare_repo(
    layout: &Layout,
    repo_url: &str,
    repo_key: &str,
) -> Result<(), String> {
    let gitdir = layout.repo_gitdir_path(repo_key);
    if gitdir.is_dir() {
        return Ok(());
    }

    log(&format!("Cloning bare repo: {repo_url}"));
    if let Some(parent) = gitdir.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    run_status(
        "git",
        &[
            "clone",
            "--bare",
            repo_url,
            gitdir.to_string_lossy().as_ref(),
        ],
        None,
    )
}

pub(super) fn ensure_repo_available(
    layout: &Layout,
    repo_arg: &str,
    repo_key: &str,
) -> Result<(), String> {
    let gitdir = layout.repo_gitdir_path(repo_key);
    if gitdir.is_dir() {
        return Ok(());
    }
    if is_git_url(repo_arg) {
        return clone_bare_repo(layout, repo_arg, repo_key);
    }
    Err(format!(
        "Bare repo not found at {}. Use 'task clone <repo-url> {repo_key}'.",
        gitdir.display()
    ))
}

pub(super) fn detect_default_base(gitdir: &Path) -> String {
    if let Ok(output) = run_capture(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "ls-remote",
            "--symref",
            "origin",
            "HEAD",
        ],
        None,
    ) {
        for line in output.lines() {
            if let Some(target) = line.strip_prefix("ref: ") {
                let target = target.trim();
                if let Some(target) = target.strip_suffix(" HEAD")
                    && let Some(branch) = target.strip_prefix("refs/heads/")
                {
                    let remote_branch = format!("origin/{branch}");
                    if rev_exists(gitdir, &remote_branch) {
                        return remote_branch;
                    }
                    if rev_exists(gitdir, branch) {
                        return branch.to_string();
                    }
                }
            }
        }
    }

    let gitdir_text = gitdir.to_string_lossy();
    if run_status(
        "git",
        &[
            "--git-dir",
            gitdir_text.as_ref(),
            "show-ref",
            "--verify",
            "--quiet",
            "refs/remotes/origin/master",
        ],
        None,
    )
    .is_ok()
    {
        return "origin/master".to_string();
    }

    if let Ok(head) = run_capture(
        "git",
        &[
            "--git-dir",
            gitdir_text.as_ref(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
        None,
    ) {
        let head = head.trim();
        if !head.is_empty() {
            return head.to_string();
        }
    }

    "HEAD".to_string()
}

pub(super) fn fetch_origin_refs(gitdir: &Path) -> Result<(), String> {
    run_status(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "fetch",
            "origin",
            "--prune",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        None,
    )
}

pub(super) fn ref_exists(gitdir: &Path, reference: &str) -> bool {
    run_status(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "show-ref",
            "--verify",
            "--quiet",
            reference,
        ],
        None,
    )
    .is_ok()
}

pub(super) fn rev_exists(gitdir: &Path, revision: &str) -> bool {
    let value = format!("{revision}^{{commit}}");
    run_status(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "rev-parse",
            "--verify",
            "--quiet",
            &value,
        ],
        None,
    )
    .is_ok()
}

pub(super) fn launch_workspace(repo_key: &str, branch: &str, path: &Path) -> Result<(), String> {
    if path.join(".envrc").exists() && command_exists("direnv") {
        let _ = run_status("direnv", &["allow"], Some(path));
    }

    if path.join(".tool-versions").exists() && command_exists("asdf") {
        run_status("asdf", &["install"], Some(path))?;
        if command_exists("corepack") {
            let _ = run_status("corepack", &["enable"], None);
        }
    }

    if command_exists("codium") {
        let _ = Command::new("codium")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    if command_exists("tmux") {
        let session = session_name_for(repo_key, branch);
        if !tmux_has_session(&session) {
            run_status(
                "tmux",
                &[
                    "new-session",
                    "-d",
                    "-s",
                    &session,
                    "-c",
                    path.to_string_lossy().as_ref(),
                ],
                None,
            )?;
        }

        if env::var("TMUX")
            .ok()
            .filter(|value| !value.is_empty())
            .is_some()
        {
            run_status("tmux", &["switch-client", "-t", &session], None)?;
        } else {
            run_status("tmux", &["attach-session", "-t", &session], None)?;
        }
        return Ok(());
    }

    println!("{}", path.display());
    Ok(())
}

pub(super) fn repo_task_rows(
    repo_key: &str,
    gitdir: &Path,
    open_sessions: &HashSet<String>,
) -> Result<Vec<TaskRow>, String> {
    let output = run_capture(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "list",
            "--porcelain",
        ],
        None,
    )?;

    let entries = parse_worktree_porcelain(&output);
    let open_session_list: Vec<String> = open_sessions.iter().cloned().collect();
    Ok(build_task_rows(repo_key, &entries, &open_session_list))
}

pub(super) fn resolve_task_from_args(
    layout: &Layout,
    args: &[String],
    usage: &str,
) -> Result<(String, String), String> {
    match args {
        [] => {
            let (repo_key, branch, _) = current_task_info()?;
            Ok((repo_key, branch))
        }
        [query] => resolve_task_from_query(layout, query),
        [repo_arg, branch] => {
            let repo_key = resolve_repo_key_input(layout, repo_arg)?;
            Ok((repo_key, branch.to_string()))
        }
        _ => Err(usage.to_string()),
    }
}

pub(super) fn resolve_task_from_query(
    layout: &Layout,
    query: &str,
) -> Result<(String, String), String> {
    let tasks = all_tasks(layout)?;

    let mut branch_exact: Vec<&TaskRow> = tasks.iter().filter(|row| row.branch == query).collect();
    branch_exact.sort_by(|a, b| {
        let a_key = format!("{}/{}", a.repo, a.branch);
        let b_key = format!("{}/{}", b.repo, b.branch);
        a_key.cmp(&b_key)
    });
    if branch_exact.len() == 1 {
        let row = branch_exact[0];
        return Ok((row.repo.clone(), row.branch.clone()));
    }
    if !branch_exact.is_empty() {
        return choose_task_interactive(query, &branch_exact);
    }

    let mut branch_partial: Vec<&TaskRow> = tasks
        .iter()
        .filter(|row| row.branch.contains(query))
        .collect();
    branch_partial.sort_by(|a, b| {
        let a_key = format!("{}/{}", a.repo, a.branch);
        let b_key = format!("{}/{}", b.repo, b.branch);
        a_key.cmp(&b_key)
    });
    if branch_partial.len() == 1 {
        let row = branch_partial[0];
        return Ok((row.repo.clone(), row.branch.clone()));
    }
    if !branch_partial.is_empty() {
        return choose_task_interactive(query, &branch_partial);
    }

    let mut repo_matches: Vec<&TaskRow> = tasks
        .iter()
        .filter(|row| row.repo.contains(query))
        .collect();
    repo_matches.sort_by(|a, b| {
        let a_key = format!("{}/{}", a.repo, a.branch);
        let b_key = format!("{}/{}", b.repo, b.branch);
        a_key.cmp(&b_key)
    });

    if repo_matches.is_empty() {
        return Err(format!("No task matched '{query}'."));
    }
    if repo_matches.len() == 1 {
        let row = repo_matches[0];
        return Ok((row.repo.clone(), row.branch.clone()));
    }

    choose_task_interactive(query, &repo_matches)
}

fn all_tasks(layout: &Layout) -> Result<Vec<TaskRow>, String> {
    let mut rows = Vec::new();
    let open_sessions = tmux_sessions();
    let repo_keys = available_repo_keys(layout)?;

    for repo_key in repo_keys {
        let gitdir = layout.repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            continue;
        }
        rows.extend(repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
    }

    Ok(rows)
}

fn choose_task_interactive(query: &str, choices: &[&TaskRow]) -> Result<(String, String), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        let list = choices
            .iter()
            .map(|row| format!("{}/{}", row.repo, row.branch))
            .collect::<Vec<String>>()
            .join(" ");
        return Err(format!(
            "Multiple tasks match '{query}': {list}. Please specify repo and branch."
        ));
    }

    let items = choices
        .iter()
        .map(|row| format!("{}/{}", row.repo, row.branch))
        .collect::<Vec<String>>();

    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Multiple tasks match '{query}'. Choose one:"))
        .items(&items)
        .default(0)
        .interact_opt()
        .map_err(|error| error.to_string())?;

    if let Some(index) = index {
        let row = choices[index];
        return Ok((row.repo.clone(), row.branch.clone()));
    }

    Err("Selection cancelled.".to_string())
}

pub(super) fn print_task_rows_table(rows: &[TaskRow]) {
    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["STATUS", "REPO", "BRANCH", "PATH"]);

    for row in rows {
        let status_cell = match row.status.as_str() {
            "open" => Cell::new("open").fg(Color::Green),
            "parked" => Cell::new("parked").fg(Color::Yellow),
            _ => Cell::new(&row.status),
        };

        table.add_row(vec![
            status_cell,
            Cell::new(&row.repo),
            Cell::new(&row.branch),
            Cell::new(row.path.display().to_string()),
        ]);
    }

    println!("{table}");
}

fn tmux_sessions_from_output(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| line.split(':').next())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

pub(super) fn tmux_sessions() -> HashSet<String> {
    if !command_exists("tmux") {
        return HashSet::new();
    }

    let output = match run_capture("tmux", &["ls"], None) {
        Ok(output) => output,
        Err(_) => return HashSet::new(),
    };

    tmux_sessions_from_output(&output)
}

pub(super) fn tmux_has_session(session: &str) -> bool {
    match Command::new("tmux")
        .args(["has-session", "-t", session])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

pub(super) fn current_task_info() -> Result<(String, String, PathBuf), String> {
    let root = run_capture("git", &["rev-parse", "--show-toplevel"], None)?;
    let root = PathBuf::from(root.trim());

    let common_dir_raw = run_capture(
        "git",
        &[
            "-C",
            root.to_string_lossy().as_ref(),
            "rev-parse",
            "--git-common-dir",
        ],
        None,
    )?;
    let mut common_dir = PathBuf::from(common_dir_raw.trim());
    if common_dir.is_relative() {
        common_dir = root.join(common_dir);
    }

    let common_dir = fs::canonicalize(common_dir).map_err(|e| e.to_string())?;
    let common_text = common_dir.to_string_lossy().to_string();
    let repo_key = repo_key_from_common_dir(&common_text).ok_or_else(|| {
        "Current repository is not managed by task. Run 'task list' to see parkable tasks."
            .to_string()
    })?;

    let branch = run_capture(
        "git",
        &[
            "-C",
            root.to_string_lossy().as_ref(),
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ],
        None,
    )
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
    .or_else(|| branch_from_worktree_path(&repo_key, &root.to_string_lossy()))
    .or_else(|| {
        root.file_name()
            .map(|name| name.to_string_lossy().to_string())
    })
    .ok_or_else(|| {
        "Could not determine current task branch. Run 'task list' to inspect tasks.".to_string()
    })?;

    Ok((repo_key, branch, root))
}

pub(super) fn current_repo_key() -> Option<String> {
    current_task_info().ok().map(|(repo_key, _, _)| repo_key)
}

pub(super) fn resolve_repo_branch_inputs(
    layout: &Layout,
    repo_arg: Option<&str>,
    branch_arg: Option<&str>,
) -> Result<(String, String), String> {
    if let (Some(repo_arg), Some(branch_arg)) = (repo_arg, branch_arg) {
        return Ok((repo_arg.to_string(), branch_arg.to_string()));
    }

    if let (Some(query), None) = (repo_arg, branch_arg) {
        return resolve_task_from_query(layout, query);
    }

    let (current_repo, current_branch, _) = current_task_info()?;
    let repo = repo_arg.unwrap_or(&current_repo).to_string();
    let branch = branch_arg.unwrap_or(&current_branch).to_string();
    Ok((repo, branch))
}

pub(super) fn resolve_repo_input(repo_arg: Option<&str>) -> Result<String, String> {
    if let Some(repo_arg) = repo_arg {
        return Ok(repo_arg.to_string());
    }

    current_repo_key().ok_or_else(|| {
        "Repository not specified and current directory is not a task worktree.".to_string()
    })
}

pub(super) fn command_exists(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).exists();
    }

    let path_var = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path_var).any(|dir| dir.join(name).exists())
}

pub(super) fn run_capture(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            return Err(stderr);
        }
        return Err(format!("command failed with status {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(super) fn run_status(
    program: impl AsRef<OsStr>,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let status = command.status().map_err(|e| e.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(format!("command failed with status {status}"))
}

pub(super) fn log(message: &str) {
    println!("{} {}", "==>".bright_blue().bold(), message);
}

pub(super) fn warn(message: &str) {
    eprintln!("{} {}", "warning:".yellow().bold(), message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_sessions_parses_names() {
        let text = "task_a: 1 windows\nmain: 2 windows\n";
        let sessions = tmux_sessions_from_output(text);
        assert!(sessions.contains("task_a"));
        assert!(sessions.contains("main"));
    }

    #[test]
    fn collect_gitdirs_finds_nested_bare_repos() {
        let base = env::temp_dir().join("task-rs-collect-gitdirs");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("repos/github.com/me/app.git")).expect("create nested gitdir");

        let results = collect_gitdirs(&base).expect("collect gitdirs");
        assert_eq!(results.len(), 1);
        assert!(results[0].ends_with("app.git"));

        let _ = fs::remove_dir_all(base);
    }
}
