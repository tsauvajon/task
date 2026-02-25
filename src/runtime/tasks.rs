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
    error::{Error, Result},
    runtime::{
        paths::WorkspacePaths,
        process,
        task_rows::{TaskRow, TaskStatus, build_task_rows},
    },
    tools::{
        asdf, direnv,
        git::{
            context::{current_root, git_common_dir, repo_key_from_common_dir},
            refs::current_branch,
            repo::{
                ResolveResult, clone_bare_repo, is_valid_bare_repo, parse_repo_input,
                resolve_repo_query,
            },
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
    types::{BranchName, RepoKey},
};

#[derive(Debug, Clone)]
pub struct TaskResolver {
    layout: WorkspacePaths,
    codium_trusted_roots: Vec<PathBuf>,
}

impl TaskResolver {
    pub fn new(layout: WorkspacePaths, codium_trusted_roots: Vec<PathBuf>) -> Self {
        Self { layout, codium_trusted_roots }
    }

    pub fn layout(&self) -> &WorkspacePaths {
        &self.layout
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.layout.repos_dir())?;
        fs::create_dir_all(self.layout.wt_dir())?;
        Ok(())
    }

    pub fn available_repo_keys(&self) -> Result<Vec<RepoKey>> {
        let repos_dir = self.layout.repos_dir().to_path_buf();
        let gitdirs = collect_gitdirs(&repos_dir)?;
        let mut keys: Vec<RepoKey> = gitdirs
            .into_iter()
            .filter_map(|gitdir| {
                let relative = gitdir.strip_prefix(&repos_dir).ok()?;
                let key = relative.to_string_lossy();
                let key = key.strip_suffix(".git").unwrap_or(&key);
                Some(RepoKey::new(key))
            })
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    pub fn resolve_repo_key_input(&self, repo_arg: &str) -> Result<RepoKey> {
        let parsed = parse_repo_input(repo_arg);
        let normalized = RepoKey::new(parsed.repo_key);

        if parsed.clone_url.is_some() || self.layout.repo_gitdir_path(&normalized).is_dir() {
            return Ok(normalized);
        }

        let keys = self.available_repo_keys()?;
        let key_strs: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        match resolve_repo_query(&normalized, &key_strs) {
            ResolveResult::Resolved(key) => Ok(RepoKey::new(key)),
            ResolveResult::Ambiguous(choices) => {
                choose_repo_key_interactive(repo_arg, &choices).map(RepoKey::new)
            }
        }
    }

    pub fn clone_bare_repo(&self, repo_url: &str, repo_key: &RepoKey) -> Result<()> {
        let gitdir = self.layout.repo_gitdir_path(repo_key);
        if gitdir.is_dir() && is_valid_bare_repo(&gitdir) {
            return Ok(());
        }

        if gitdir.is_dir() {
            process::warn(&format!(
                "Removing invalid bare repo at {}; will re-clone",
                gitdir.display()
            ));
        }

        process::log(&format!("Cloning bare repo: {repo_url}"));
        clone_bare_repo(repo_url, &gitdir)
    }

    pub fn ensure_repo_available(&self, repo_arg: &str, repo_key: &RepoKey) -> Result<()> {
        let gitdir = self.layout.repo_gitdir_path(repo_key);
        if gitdir.is_dir() {
            return Ok(());
        }
        let parsed = parse_repo_input(repo_arg);
        if let Some(clone_url) = parsed.clone_url {
            return self.clone_bare_repo(&clone_url, repo_key);
        }
        Err(Error::not_found(format!(
            "Bare repo not found at {}. Use 'task repo clone <repo-url> {repo_key}'.",
            gitdir.display()
        )))
    }

    pub fn launch_workspace(&self, repo_key: &RepoKey, branch: &BranchName, path: &Path) -> Result<()> {
        if path.join(".envrc").exists() && direnv::is_available() {
            let _ = direnv::allow(path);
        }

        if asdf::is_available() {
            let installed = asdf::install_from_workspace_tool_versions(path)?;
            if installed && nodejs::runtime::corepack_available() {
                let _ = nodejs::runtime::enable_corepack();
            }
        }

        if open_task_session(repo_key, branch, path, &self.codium_trusted_roots)?
            == OpenResult::Attached
        {
            return Ok(());
        }

        println!("{}", path.display());
        Ok(())
    }

    pub fn repo_task_rows(
        &self,
        repo_key: &RepoKey,
        gitdir: &Path,
        open_sessions: &HashSet<String>,
    ) -> Result<Vec<TaskRow>> {
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

    pub fn resolve_worktree_path(&self, repo_key: &RepoKey, branch: &BranchName) -> PathBuf {
        let fallback = self.layout.worktree_path(repo_key, branch);
        let gitdir = self.layout.repo_gitdir_path(repo_key);
        if !gitdir.is_dir() {
            return fallback;
        }

        let open_sessions = HashSet::new();
        if let Ok(rows) = self.repo_task_rows(repo_key, &gitdir, &open_sessions)
            && let Some(row) = rows.into_iter().find(|row| row.branch == *branch)
        {
            return row.path;
        }

        fallback
    }

    pub fn resolve_task_from_args(
        &self,
        args: &[String],
        usage: &str,
    ) -> Result<(RepoKey, BranchName)> {
        match args {
            [] => {
                let (repo_key, branch, _) = self.current_task_info()?;
                Ok((repo_key, branch))
            }
            [query] => self.resolve_task_from_query(query),
            [repo_arg, branch] => {
                let repo_key = self.resolve_repo_key_input(repo_arg)?;
                Ok((repo_key, BranchName::new(branch)))
            }
            _ => Err(Error::failed(usage)),
        }
    }

    pub fn resolve_task_from_query(&self, query: &str) -> Result<(RepoKey, BranchName)> {
        let tasks = self.all_tasks()?;

        // Sort key helper: "repo/branch"
        let sort_key = |row: &&TaskRow| format!("{}/{}", row.repo, row.branch);

        let mut matches: Vec<&TaskRow> = tasks.iter().filter(|r| *r.branch == *query).collect();
        matches.sort_by_key(sort_key);
        if matches.len() == 1 {
            return Ok((matches[0].repo.clone(), matches[0].branch.clone()));
        }
        if !matches.is_empty() {
            return choose_task_interactive(query, &matches);
        }

        let mut matches: Vec<&TaskRow> =
            tasks.iter().filter(|r| r.branch.contains(query)).collect();
        matches.sort_by_key(sort_key);
        if matches.len() == 1 {
            return Ok((matches[0].repo.clone(), matches[0].branch.clone()));
        }
        if !matches.is_empty() {
            return choose_task_interactive(query, &matches);
        }

        let mut matches: Vec<&TaskRow> = tasks.iter().filter(|r| r.repo.contains(query)).collect();
        matches.sort_by_key(sort_key);

        if matches.is_empty() {
            return Err(Error::not_found(format!("No task matched '{query}'.")));
        }
        if matches.len() == 1 {
            return Ok((matches[0].repo.clone(), matches[0].branch.clone()));
        }

        choose_task_interactive(query, &matches)
    }

    pub fn print_task_rows_table(&self, rows: &[TaskRow]) {
        let mut table = Table::new();
        table
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["STATUS", "REPO", "BRANCH", "PATH"]);

        for row in rows {
            let status_cell = match row.status {
                TaskStatus::Open => Cell::new(row.status).fg(Color::Green),
                TaskStatus::Parked => Cell::new(row.status).fg(Color::Yellow),
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
        list_sessions()
    }

    pub fn current_task_info(&self) -> Result<(RepoKey, BranchName, PathBuf)> {
        let root = current_root()?;
        let common_dir = git_common_dir(&root)?;
        let repos_dir = self.layout.repos_dir().to_path_buf();
        let repo_key = repo_key_from_common_dir(&common_dir, &repos_dir)?.ok_or_else(|| {
            Error::failed(
                "Current repository is not managed by task. Run 'task list' to see parkable tasks.",
            )
        })?;

        let branch = current_branch(&root)
            .or_else(|| branch_from_worktree_path(self.layout.wt_dir(), &repo_key, &root))
            .or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .ok_or_else(|| {
                Error::failed(
                    "Could not determine current task branch. Run 'task list' to inspect tasks.",
                )
            })?;

        Ok((repo_key, BranchName::new(branch), root))
    }

    pub fn current_repo_key(&self) -> Option<RepoKey> {
        self.current_task_info()
            .ok()
            .map(|(repo_key, _, _)| repo_key)
    }

    pub fn resolve_repo_branch_inputs(
        &self,
        repo_arg: Option<&str>,
        branch_arg: Option<&str>,
    ) -> Result<(RepoKey, BranchName)> {
        if let (Some(repo_arg), Some(branch_arg)) = (repo_arg, branch_arg) {
            return Ok((RepoKey::new(repo_arg), BranchName::new(branch_arg)));
        }

        if let (Some(query), None) = (repo_arg, branch_arg) {
            return self.resolve_task_from_query(query);
        }

        let (current_repo, current_branch, _) = self.current_task_info()?;
        let repo = repo_arg.map(RepoKey::new).unwrap_or(current_repo);
        let branch = branch_arg.map(BranchName::new).unwrap_or(current_branch);
        Ok((repo, branch))
    }

    pub fn resolve_repo_input(&self, repo_arg: Option<&str>) -> Result<RepoKey> {
        if let Some(repo_arg) = repo_arg {
            return Ok(RepoKey::new(repo_arg));
        }

        self.current_repo_key().ok_or_else(|| {
            Error::failed(
                "Repository not specified and current directory is not a task worktree.",
            )
        })
    }

    fn all_tasks(&self) -> Result<Vec<TaskRow>> {
        let open_sessions = self.tmux_sessions();
        let mut rows = Vec::new();
        for repo_key in self.available_repo_keys()? {
            let gitdir = self.layout.repo_gitdir_path(&repo_key);
            if gitdir.is_dir() {
                rows.extend(self.repo_task_rows(&repo_key, &gitdir, &open_sessions)?);
            }
        }
        Ok(rows)
    }

}

fn collect_gitdirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut gitdirs = Vec::new();

    while let Some(current) = stack.pop() {
        if !current.is_dir() {
            continue;
        }

        for entry in fs::read_dir(&current)? {
            let path = entry?.path();
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

fn choose_repo_key_interactive(query: &str, choices: &[String]) -> Result<String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::failed(format!(
            "Multiple repositories match '{query}': {}. Please use a full repo key.",
            choices.join(" ")
        )));
    }

    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Multiple repositories match '{query}'. Choose one:"))
        .items(choices)
        .default(0)
        .interact_opt()?;

    index.map(|i| choices[i].clone()).ok_or(Error::Cancelled)
}

fn choose_task_interactive(query: &str, choices: &[&TaskRow]) -> Result<(RepoKey, BranchName)> {
    let items: Vec<String> = choices
        .iter()
        .map(|row| format!("{}/{}", row.repo, row.branch))
        .collect();

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::failed(format!(
            "Multiple tasks match '{query}': {}. Please specify repo and branch.",
            items.join(" ")
        )));
    }

    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Multiple tasks match '{query}'. Choose one:"))
        .items(&items)
        .default(0)
        .interact_opt()?;

    let Some(i) = index else {
        return Err(Error::Cancelled);
    };
    let row = choices[i];
    Ok((row.repo.clone(), row.branch.clone()))
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
