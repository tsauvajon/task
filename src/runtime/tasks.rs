use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use comfy_table::{Cell, Color, ContentArrangement, Table};
use dialoguer::{Select, theme::ColorfulTheme};

use crate::{
    runtime::{
        paths::WorkspacePaths,
        process::ProcessRunner,
        task_rows::{TaskRow, TaskStatus, build_task_rows},
    },
    tools::{
        asdf, direnv,
        git::{
            context::{current_root, git_common_dir, repo_key_from_common_dir},
            refs::current_branch,
            repo::{ResolveResult, clone_bare_repo, parse_repo_input, resolve_repo_query},
            worktrees::{
                branch_from_worktree_path, parse_worktree_porcelain, worktree_list_porcelain,
            },
        },
        nodejs,
        tmux::{
            sessions::list_sessions,
            workflow::{OpenResult, open_task_session},
        },
    },
};

#[derive(Debug, Clone)]
pub struct TaskResolver {
    layout: WorkspacePaths,
    process: ProcessRunner,
    codium_trusted_roots: Vec<PathBuf>,
}

impl TaskResolver {
    pub fn new(
        layout: WorkspacePaths,
        process: ProcessRunner,
        codium_trusted_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            layout,
            process,
            codium_trusted_roots,
        }
    }

    pub fn layout(&self) -> &WorkspacePaths {
        &self.layout
    }

    pub fn ensure_layout(&self) -> Result<(), String> {
        fs::create_dir_all(self.layout.repos_dir()).map_err(|e| e.to_string())?;
        fs::create_dir_all(self.layout.wt_dir()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn available_repo_keys(&self) -> Result<Vec<String>, String> {
        let repos_dir = self
            .layout
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

    pub fn resolve_repo_key_input(&self, repo_arg: &str) -> Result<String, String> {
        let parsed = parse_repo_input(repo_arg);
        let has_clone_url = parsed.clone_url.is_some();
        let normalized = parsed.repo_key;

        if has_clone_url || self.layout.repo_gitdir_path(&normalized).is_dir() {
            return Ok(normalized);
        }

        let keys = self.available_repo_keys()?;
        match resolve_repo_query(&normalized, &keys) {
            ResolveResult::Resolved(value) => Ok(value),
            ResolveResult::Ambiguous(choices) => choose_repo_key_interactive(repo_arg, &choices),
        }
    }

    pub fn clone_bare_repo(&self, repo_url: &str, repo_key: &str) -> Result<(), String> {
        let gitdir = self.layout.repo_gitdir_path(repo_key);
        if gitdir.is_dir() {
            return Ok(());
        }

        self.process.log(&format!("Cloning bare repo: {repo_url}"));
        clone_bare_repo(repo_url, &gitdir)
    }

    pub fn ensure_repo_available(&self, repo_arg: &str, repo_key: &str) -> Result<(), String> {
        let gitdir = self.layout.repo_gitdir_path(repo_key);
        if gitdir.is_dir() {
            return Ok(());
        }
        let parsed = parse_repo_input(repo_arg);
        if let Some(clone_url) = parsed.clone_url {
            return self.clone_bare_repo(&clone_url, repo_key);
        }
        Err(format!(
            "Bare repo not found at {}. Use 'task repo clone <repo-url> {repo_key}'.",
            gitdir.display()
        ))
    }

    pub fn launch_workspace(
        &self,
        repo_key: &str,
        branch: &str,
        path: &Path,
    ) -> Result<(), String> {
        if path.join(".envrc").exists() && direnv::is_available(self.process) {
            let _ = direnv::allow(self.process, path);
        }

        if asdf::is_available(self.process) {
            let installed = asdf::install_from_workspace_tool_versions(self.process, path)?;
            if installed && nodejs::runtime::corepack_available(self.process) {
                let _ = nodejs::runtime::enable_corepack(self.process);
            }
        }

        if open_task_session(
            self.process,
            repo_key,
            branch,
            path,
            &self.codium_trusted_roots,
        )? == OpenResult::Attached
        {
            return Ok(());
        }

        println!("{}", path.display());
        Ok(())
    }

    pub fn repo_task_rows(
        &self,
        repo_key: &str,
        gitdir: &Path,
        open_sessions: &HashSet<String>,
    ) -> Result<Vec<TaskRow>, String> {
        let output = worktree_list_porcelain(gitdir)?;

        let entries = parse_worktree_porcelain(&output);
        let open_session_list: Vec<String> = open_sessions.iter().cloned().collect();
        Ok(build_task_rows(
            repo_key,
            self.layout.wt_dir(),
            &entries,
            &open_session_list,
        ))
    }

    pub fn resolve_worktree_path(&self, repo_key: &str, branch: &str) -> PathBuf {
        let fallback = self.layout.worktree_path(repo_key, branch);
        let gitdir = self.layout.repo_gitdir_path(repo_key);
        if !gitdir.is_dir() {
            return fallback;
        }

        let open_sessions = HashSet::new();
        if let Ok(rows) = self.repo_task_rows(repo_key, &gitdir, &open_sessions)
            && let Some(row) = rows.into_iter().find(|row| row.branch == branch)
        {
            return row.path;
        }

        fallback
    }

    pub fn resolve_task_from_args(
        &self,
        args: &[String],
        usage: &str,
    ) -> Result<(String, String), String> {
        match args {
            [] => {
                let (repo_key, branch, _) = self.current_task_info()?;
                Ok((repo_key, branch))
            }
            [query] => self.resolve_task_from_query(query),
            [repo_arg, branch] => {
                let repo_key = self.resolve_repo_key_input(repo_arg)?;
                Ok((repo_key, branch.to_string()))
            }
            _ => Err(usage.to_string()),
        }
    }

    pub fn resolve_task_from_query(&self, query: &str) -> Result<(String, String), String> {
        let tasks = self.all_tasks()?;

        let mut branch_exact: Vec<&TaskRow> =
            tasks.iter().filter(|row| row.branch == query).collect();
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

    pub fn print_task_rows_table(&self, rows: &[TaskRow]) {
        let mut table = Table::new();
        table
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["STATUS", "REPO", "BRANCH", "PATH"]);

        for row in rows {
            let status_cell = match row.status {
                TaskStatus::Open => Cell::new("open").fg(Color::Green),
                TaskStatus::Parked => Cell::new("parked").fg(Color::Yellow),
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

    pub fn tmux_sessions(&self) -> HashSet<String> {
        list_sessions(self.process)
    }

    pub fn current_task_info(&self) -> Result<(String, String, PathBuf), String> {
        let root = current_root()?;
        let common_dir = git_common_dir(&root)?;
        let repos_dir = self
            .layout
            .repo_gitdir_path("")
            .parent()
            .ok_or_else(|| "Could not resolve repos dir".to_string())?
            .to_path_buf();
        let repo_key = repo_key_from_common_dir(&common_dir, &repos_dir)?.ok_or_else(|| {
            "Current repository is not managed by task. Run 'task list' to see parkable tasks."
                .to_string()
        })?;

        let branch = current_branch(&root)
            .or_else(|| branch_from_worktree_path(self.layout.wt_dir(), &repo_key, &root))
            .or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .ok_or_else(|| {
                "Could not determine current task branch. Run 'task list' to inspect tasks."
                    .to_string()
            })?;

        Ok((repo_key, branch, root))
    }

    pub fn current_repo_key(&self) -> Option<String> {
        self.current_task_info()
            .ok()
            .map(|(repo_key, _, _)| repo_key)
    }

    pub fn resolve_repo_branch_inputs(
        &self,
        repo_arg: Option<&str>,
        branch_arg: Option<&str>,
    ) -> Result<(String, String), String> {
        if let (Some(repo_arg), Some(branch_arg)) = (repo_arg, branch_arg) {
            return Ok((repo_arg.to_string(), branch_arg.to_string()));
        }

        if let (Some(query), None) = (repo_arg, branch_arg) {
            return self.resolve_task_from_query(query);
        }

        let (current_repo, current_branch, _) = self.current_task_info()?;
        let repo = repo_arg.unwrap_or(&current_repo).to_string();
        let branch = branch_arg.unwrap_or(&current_branch).to_string();
        Ok((repo, branch))
    }

    pub fn resolve_repo_input(&self, repo_arg: Option<&str>) -> Result<String, String> {
        if let Some(repo_arg) = repo_arg {
            return Ok(repo_arg.to_string());
        }

        self.current_repo_key().ok_or_else(|| {
            "Repository not specified and current directory is not a task worktree.".to_string()
        })
    }

    fn all_tasks(&self) -> Result<Vec<TaskRow>, String> {
        let mut rows = Vec::new();
        let open_sessions = self.tmux_sessions();
        let repo_keys = self.available_repo_keys()?;

        for repo_key in repo_keys {
            let gitdir = self.layout.repo_gitdir_path(&repo_key);
            if !gitdir.is_dir() {
                continue;
            }
            rows.extend(self.repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
        }

        Ok(rows)
    }
}

fn collect_gitdirs(root: &Path) -> Result<Vec<PathBuf>, String> {
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

fn choose_repo_key_interactive(query: &str, choices: &[String]) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::collect_gitdirs;

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
