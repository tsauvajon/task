use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use comfy_table::{Cell, Color, ContentArrangement, Table};
use dialoguer::{Select, theme::ColorfulTheme};
use rayon::prelude::*;

use crate::{
    error::{Error, Result},
    runtime::{
        BranchName, RepoKey,
        config::is_interactive_terminal,
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
            worktrees::{branch_from_worktree_path, list_porcelain, parse_worktree_porcelain},
        },
        nodejs,
        tmux::{
            sessions::list_sessions,
            workflow::{OpenResult, open_session},
        },
    },
};

#[derive(Debug, Clone)]
pub struct TaskResolver {
    layout: WorkspacePaths,
    codium_trusted_roots: Vec<PathBuf>,
    interactive: bool,
}

impl TaskResolver {
    pub fn new(
        layout: WorkspacePaths,
        codium_trusted_roots: Vec<PathBuf>,
        interactive: bool,
    ) -> Self {
        Self {
            layout,
            codium_trusted_roots,
            interactive,
        }
    }

    pub fn layout(&self) -> &WorkspacePaths {
        &self.layout
    }

    #[cfg(test)]
    pub fn codium_trusted_roots(&self) -> &[PathBuf] {
        &self.codium_trusted_roots
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.layout.repos_dir())?;
        fs::create_dir_all(self.layout.wt_dir())?;
        fs::create_dir_all(self.layout.detached_dir())?;
        Ok(())
    }

    /// Returns all repos as `(key, gitdir)` pairs, sorted by key.
    /// Prefer this over `available_repo_keys` when you need both pieces,
    /// so the gitdir path is never recomputed from the key.
    pub fn available_repos(&self) -> Result<Vec<(RepoKey, PathBuf)>> {
        let repos_dir = self.layout.repos_dir().to_path_buf();
        let mut repos: Vec<(RepoKey, PathBuf)> = collect_gitdirs(&repos_dir)?
            .into_iter()
            .filter_map(|gitdir| {
                let relative = gitdir.strip_prefix(&repos_dir).ok()?;
                let key = relative.to_string_lossy();
                let key = key.strip_suffix(".git").unwrap_or(&key);
                Some((RepoKey::new(key), gitdir))
            })
            .collect();
        repos.sort_by(|(a, _), (b, _)| a.cmp(b));
        repos.dedup_by(|(a, _), (b, _)| a == b);
        Ok(repos)
    }

    pub fn available_repo_keys(&self) -> Result<Vec<RepoKey>> {
        Ok(self
            .available_repos()?
            .into_iter()
            .map(|(key, _)| key)
            .collect())
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
                choose_repo_key_interactive(repo_arg, &choices, self.interactive).map(RepoKey::new)
            }
        }
    }

    /// Like `resolve_repo_key_input` but refuses to accept clone URLs or
    /// unknown inputs — the repo **must** already exist in the repos directory.
    /// Returns an error with a helpful message when the repo is not found.
    pub fn resolve_existing_repo_key(&self, repo_arg: &str) -> Result<RepoKey> {
        // Reject clone URLs up-front — detach only works on already-cloned repos.
        let parsed = parse_repo_input(repo_arg);
        if parsed.clone_url.is_some() {
            return Err(Error::not_found(format!(
                "'{repo_arg}' looks like a clone URL. \
                 Clone the repository first with 'task repo clone {repo_arg}', \
                 then use 'task detach add <repo>'."
            )));
        }

        let normalized = RepoKey::new(parsed.repo_key);
        if self.layout.repo_gitdir_path(&normalized).is_dir() {
            return Ok(normalized);
        }

        // Try partial/suffix matching against already-cloned repos.
        let keys = self.available_repo_keys()?;
        let key_strs: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        match resolve_repo_query(&normalized, &key_strs) {
            ResolveResult::Resolved(key) => {
                let resolved = RepoKey::new(&key);
                // resolve_repo_query returns the query itself when no match is found,
                // so check the resolved key actually exists.
                if self.layout.repo_gitdir_path(&resolved).is_dir() {
                    Ok(resolved)
                } else {
                    Err(Error::not_found(format!(
                        "Repository '{repo_arg}' not found. \
                         Clone it first with 'task repo clone <url>', \
                         then use 'task detach add <repo>'."
                    )))
                }
            }
            ResolveResult::Ambiguous(choices) => {
                choose_repo_key_interactive(repo_arg, &choices, self.interactive).map(RepoKey::new)
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

    pub fn launch_workspace(
        &self,
        repo_key: &RepoKey,
        branch: &BranchName,
        path: &Path,
    ) -> Result<()> {
        self.launch_workspace_impl(repo_key, branch, path, is_interactive_terminal(), false)
    }

    pub fn launch_workspace_no_open(
        &self,
        repo_key: &RepoKey,
        branch: &BranchName,
        path: &Path,
        no_open: bool,
    ) -> Result<()> {
        self.launch_workspace_impl(repo_key, branch, path, is_interactive_terminal(), no_open)
    }

    fn launch_workspace_impl(
        &self,
        repo_key: &RepoKey,
        branch: &BranchName,
        path: &Path,
        interactive: bool,
        no_open: bool,
    ) -> Result<()> {
        if !interactive || no_open {
            println!("{}", path.display());
            return Ok(());
        }

        if path.join(".envrc").exists() && direnv::is_available() {
            let _ = direnv::allow(path);
        }

        if asdf::is_available() {
            let installed = asdf::install_from_workspace_tool_versions(path)?;
            if installed && nodejs::runtime::corepack_available() {
                let _ = nodejs::runtime::enable_corepack();
            }
        }

        if open_session(repo_key, branch, path, &self.codium_trusted_roots)? == OpenResult::Attached
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
        let output = list_porcelain(gitdir)?;
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
            return choose_task_interactive(query, &matches, self.interactive);
        }

        let mut matches: Vec<&TaskRow> =
            tasks.iter().filter(|r| r.branch.contains(query)).collect();
        matches.sort_by_key(sort_key);
        if matches.len() == 1 {
            return Ok((matches[0].repo.clone(), matches[0].branch.clone()));
        }
        if !matches.is_empty() {
            return choose_task_interactive(query, &matches, self.interactive);
        }

        let mut matches: Vec<&TaskRow> = tasks.iter().filter(|r| r.repo.contains(query)).collect();
        matches.sort_by_key(sort_key);

        if matches.is_empty() {
            return Err(Error::not_found(format!("No task matched '{query}'.")));
        }
        if matches.len() == 1 {
            return Ok((matches[0].repo.clone(), matches[0].branch.clone()));
        }

        choose_task_interactive(query, &matches, self.interactive)
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
            Error::failed("Repository not specified and current directory is not a task worktree.")
        })
    }

    fn all_tasks(&self) -> Result<Vec<TaskRow>> {
        let open_sessions = self.tmux_sessions();
        // Resolve the nix store path for git before entering the parallel
        // section: the OnceLock inside NixRunner would otherwise block every
        // rayon thread on the first caller while the rest stall idle.
        crate::tools::git::warmup();
        self.available_repos()?
            .into_par_iter()
            .map(|(repo_key, gitdir)| self.repo_task_rows(&repo_key, &gitdir, &open_sessions))
            .try_reduce(Vec::new, |mut acc, mut v| {
                acc.append(&mut v);
                Ok(acc)
            })
    }
}

fn collect_gitdirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut gitdirs = Vec::new();

    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)?.filter_map(|e| e.ok()) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
            if name.ends_with(".git") && name != ".git" {
                gitdirs.push(path);
            } else {
                stack.push(path);
            }
        }
    }

    gitdirs.sort();
    Ok(gitdirs)
}

fn choose_repo_key_interactive(
    query: &str,
    choices: &[String],
    interactive: bool,
) -> Result<String> {
    if !interactive {
        return Err(Error::failed(format!(
            "Multiple repositories match '{query}': {}. Please use a full repo key.",
            choices.join(" ")
        )));
    }

    let index = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Multiple repositories match '{query}'. Choose one:"
        ))
        .items(choices)
        .default(0)
        .interact_opt()?;

    index.map(|i| choices[i].clone()).ok_or(Error::Cancelled)
}

fn choose_task_interactive(
    query: &str,
    choices: &[&TaskRow],
    interactive: bool,
) -> Result<(RepoKey, BranchName)> {
    let items: Vec<String> = choices
        .iter()
        .map(|row| format!("{}/{}", row.repo, row.branch))
        .collect();

    if !interactive {
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
    use std::{env, fs, path::PathBuf};

    use super::{TaskResolver, collect_gitdirs};
    use crate::runtime::{BranchName, RepoKey, paths::WorkspacePaths};

    /// RAII guard that removes its directory on drop, including on test failure.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("task-rs-tasks-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Create a minimal bare git repo at the given path using `git init --bare`.
    fn init_bare_repo(path: &std::path::Path) {
        fs::create_dir_all(path).expect("create bare repo dir");
        let status = std::process::Command::new("git")
            .args(["init", "--bare"])
            .arg(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", std::env::temp_dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git init --bare");
        assert!(status.success(), "git init --bare failed");
    }

    /// Build a `TaskResolver` from temp repos, wt, and detached dirs.
    fn resolver_for(repos_dir: &std::path::Path, wt_dir: &std::path::Path) -> TaskResolver {
        let layout = WorkspacePaths::new(repos_dir, wt_dir, std::path::Path::new("/tmp/detached"));
        TaskResolver::new(layout, Vec::new(), false)
    }

    mod collect_gitdirs {
        use super::*;

        #[test]
        fn finds_nested_bare_repos() {
            let dir = TempDir::new("nested");
            fs::create_dir_all(dir.path().join("repos/github.com/me/app.git"))
                .expect("create nested gitdir");

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            assert_eq!(results.len(), 1);
            assert!(results[0].ends_with("app.git"));
        }

        #[test]
        fn returns_all_repos_across_orgs() {
            let dir = TempDir::new("multi-org");
            fs::create_dir_all(dir.path().join("github.com/org-a/alpha.git")).unwrap();
            fs::create_dir_all(dir.path().join("github.com/org-a/beta.git")).unwrap();
            fs::create_dir_all(dir.path().join("github.com/org-b/gamma.git")).unwrap();
            fs::create_dir_all(dir.path().join("gitlab.com/org-c/delta.git")).unwrap();

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            assert_eq!(results.len(), 4);
        }

        #[test]
        fn output_is_sorted() {
            let dir = TempDir::new("sorted");
            // Create in reverse alphabetical order to confirm sort is applied.
            fs::create_dir_all(dir.path().join("github.com/z/zzz.git")).unwrap();
            fs::create_dir_all(dir.path().join("github.com/a/aaa.git")).unwrap();
            fs::create_dir_all(dir.path().join("github.com/m/mmm.git")).unwrap();

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            let names: Vec<String> = results
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted, "output must be sorted");
        }

        #[test]
        fn ignores_non_git_directories_and_files() {
            let dir = TempDir::new("noise");
            fs::create_dir_all(dir.path().join("github.com/org/real.git")).unwrap();
            // A plain directory that should be recursed but not collected.
            fs::create_dir_all(dir.path().join("github.com/org/not-a-repo")).unwrap();
            // A file at the root level - should be silently skipped.
            fs::write(dir.path().join("README.txt"), b"hello").unwrap();

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            assert_eq!(results.len(), 1);
            assert!(results[0].ends_with("real.git"));
        }

        #[test]
        fn returns_empty_for_empty_directory() {
            let dir = TempDir::new("empty");

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            assert!(results.is_empty());
        }

        #[test]
        fn handles_deeply_nested_tree() {
            let dir = TempDir::new("deep");
            // host / org / repo.git  — 3 levels
            fs::create_dir_all(dir.path().join("github.com/org/deep.git")).unwrap();

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            assert_eq!(results.len(), 1);
            assert!(results[0].ends_with("deep.git"));
        }

        #[test]
        fn ignores_dot_git_directories() {
            let dir = TempDir::new("dot-git");
            // A real bare repo that should be collected.
            fs::create_dir_all(dir.path().join("github.com/me/app.git")).unwrap();
            // A .git metadata directory (e.g. created by opencode) at a namespace
            // level — must NOT be treated as a bare repo.
            fs::create_dir_all(dir.path().join("github.com/me/.git")).unwrap();

            let results = collect_gitdirs(dir.path()).expect("collect gitdirs");
            assert_eq!(results.len(), 1);
            assert!(results[0].ends_with("app.git"));
        }
    }

    mod ensure_layout {
        use super::*;

        #[test]
        fn creates_repos_and_wt_dirs() {
            let dir = TempDir::new("ensure-layout");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            assert!(!repos_dir.is_dir());
            assert!(!wt_dir.is_dir());

            resolver.ensure_layout().expect("ensure_layout");

            assert!(repos_dir.is_dir());
            assert!(wt_dir.is_dir());
        }

        #[test]
        fn idempotent_when_dirs_already_exist() {
            let dir = TempDir::new("ensure-layout-idempotent");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            fs::create_dir_all(&wt_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            resolver.ensure_layout().expect("ensure_layout idempotent");

            assert!(repos_dir.is_dir());
            assert!(wt_dir.is_dir());
        }
    }

    mod available_repos {
        use super::*;

        #[test]
        fn discovers_bare_repos() {
            let dir = TempDir::new("avail-repos");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            init_bare_repo(&repos_dir.join("github.com/me/lib.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repos = resolver.available_repos().expect("available_repos");
            let keys: Vec<String> = repos.iter().map(|(k, _)| k.to_string()).collect();
            assert_eq!(keys.len(), 2);
            assert!(keys.contains(&"github.com/me/app".to_string()));
            assert!(keys.contains(&"github.com/me/lib".to_string()));
        }

        #[test]
        fn returns_sorted_and_deduped() {
            let dir = TempDir::new("avail-repos-sorted");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/z/zzz.git"));
            init_bare_repo(&repos_dir.join("github.com/a/aaa.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repos = resolver.available_repos().expect("available_repos");
            let keys: Vec<String> = repos.iter().map(|(k, _)| k.to_string()).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted);
        }

        #[test]
        fn returns_empty_when_no_repos() {
            let dir = TempDir::new("avail-repos-empty");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repos = resolver.available_repos().expect("available_repos");
            assert!(repos.is_empty());
        }
    }

    mod available_repo_keys {
        use super::*;

        #[test]
        fn returns_keys_only() {
            let dir = TempDir::new("avail-keys");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let keys = resolver.available_repo_keys().expect("available_repo_keys");
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].to_string(), "github.com/me/app");
        }
    }

    mod resolve_repo_key_input {
        use super::*;

        #[test]
        fn resolves_exact_full_key() {
            let dir = TempDir::new("resolve-exact");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let key = resolver
                .resolve_repo_key_input("github.com/me/app")
                .expect("exact key");
            assert_eq!(key.to_string(), "github.com/me/app");
        }

        #[test]
        fn resolves_short_name_when_unique() {
            let dir = TempDir::new("resolve-short");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/unique-app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let key = resolver
                .resolve_repo_key_input("unique-app")
                .expect("short name");
            assert_eq!(key.to_string(), "github.com/me/unique-app");
        }

        #[test]
        fn accepts_clone_url_without_existing_repo() {
            let dir = TempDir::new("resolve-clone-url");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let key = resolver
                .resolve_repo_key_input("https://github.com/me/new-app.git")
                .expect("clone url");
            assert_eq!(key.to_string(), "github.com/me/new-app");
        }

        #[test]
        fn falls_through_to_normalized_key_when_no_repos_exist() {
            let dir = TempDir::new("resolve-unknown");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            // When no repos exist, resolve_repo_query passes through the normalized key
            let key = resolver
                .resolve_repo_key_input("nonexistent")
                .expect("passthrough key");
            assert_eq!(key.to_string(), "nonexistent");
        }
    }

    mod resolve_existing_repo_key {
        use super::*;

        #[test]
        fn resolves_exact_full_key_when_cloned() {
            let dir = TempDir::new("rerk-exact");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let key = resolver
                .resolve_existing_repo_key("github.com/me/app")
                .expect("exact key");
            assert_eq!(key.to_string(), "github.com/me/app");
        }

        #[test]
        fn resolves_short_name_when_unique_and_cloned() {
            let dir = TempDir::new("rerk-short");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/unique-app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let key = resolver
                .resolve_existing_repo_key("unique-app")
                .expect("short name");
            assert_eq!(key.to_string(), "github.com/me/unique-app");
        }

        #[test]
        fn errors_on_clone_url() {
            let dir = TempDir::new("rerk-clone-url");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver
                .resolve_existing_repo_key("https://github.com/me/new-app.git")
                .unwrap_err();
            assert!(
                err.to_string().contains("Clone the repository first")
                    || err.to_string().contains("clone URL"),
                "error should suggest cloning first: {err}"
            );
        }

        #[test]
        fn errors_when_repo_not_cloned() {
            let dir = TempDir::new("rerk-not-cloned");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver
                .resolve_existing_repo_key("nonexistent-repo")
                .unwrap_err();
            assert!(
                err.to_string().contains("not found") || err.to_string().contains("Clone it first"),
                "error should mention repo not found: {err}"
            );
        }

        #[test]
        fn errors_with_helpful_message_when_no_repos_exist() {
            let dir = TempDir::new("rerk-no-repos");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver.resolve_existing_repo_key("any-repo").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("not found") || msg.contains("Clone"),
                "error should be helpful: {msg}"
            );
        }
    }

    mod ensure_repo_available {
        use super::*;

        #[test]
        fn returns_ok_when_repo_exists() {
            let dir = TempDir::new("ensure-avail");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let repo_key = RepoKey::new("github.com/me/app");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            resolver
                .ensure_repo_available("github.com/me/app", &repo_key)
                .expect("repo available");
        }

        #[test]
        fn errors_when_repo_missing_and_no_clone_url() {
            let dir = TempDir::new("ensure-avail-missing");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let repo_key = RepoKey::new("github.com/me/missing");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver
                .ensure_repo_available("missing", &repo_key)
                .unwrap_err();
            assert!(err.to_string().contains("not found"));
        }
    }

    mod resolve_worktree_path {
        use super::*;

        #[test]
        fn returns_fallback_when_no_gitdir() {
            let dir = TempDir::new("resolve-wt-path-no-gitdir");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repo_key = RepoKey::new("github.com/me/app");
            let branch = BranchName::new("feature-x");
            let path = resolver.resolve_worktree_path(&repo_key, &branch);
            assert_eq!(path, wt_dir.join("github.com/me/app/feature-x"));
        }

        #[test]
        fn returns_fallback_when_gitdir_exists_but_no_worktree() {
            let dir = TempDir::new("resolve-wt-path-no-wt");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let repo_key = RepoKey::new("github.com/me/app");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let branch = BranchName::new("feature-y");
            let path = resolver.resolve_worktree_path(&repo_key, &branch);
            // Bare repo has no worktrees, so falls back to computed path
            assert_eq!(path, wt_dir.join("github.com/me/app/feature-y"));
        }

        #[test]
        fn returns_actual_worktree_path_when_branch_exists() {
            let dir = TempDir::new("resolve-wt-path-found");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let repo_slug = "github.com/me/app";
            let gitdir = repos_dir.join(format!("{repo_slug}.git"));
            init_bare_repo(&gitdir);

            // Create a real worktree at the expected path
            let wt_path = wt_dir.join(repo_slug).join("feat-login");
            fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
            let status = std::process::Command::new("git")
                .args([
                    "--git-dir",
                    gitdir.to_str().unwrap(),
                    "worktree",
                    "add",
                    "--orphan",
                    "-b",
                    "feat-login",
                    wt_path.to_str().unwrap(),
                ])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", std::env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git worktree add");
            assert!(status.success(), "git worktree add failed");

            let resolver = resolver_for(&repos_dir, &wt_dir);
            let repo_key = RepoKey::new(repo_slug);
            let branch = BranchName::new("feat-login");
            let path = resolver.resolve_worktree_path(&repo_key, &branch);
            // Compare canonicalized paths to handle macOS /var → /private/var symlinks
            assert_eq!(
                path.canonicalize().unwrap(),
                wt_path.canonicalize().unwrap()
            );
        }
    }

    mod resolve_repo_branch_inputs {
        use super::*;

        #[test]
        fn returns_direct_pair_when_both_provided() {
            let dir = TempDir::new("resolve-rb-both");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let (repo, branch) = resolver
                .resolve_repo_branch_inputs(Some("github.com/me/app"), Some("feat"))
                .expect("both provided");
            assert_eq!(repo.to_string(), "github.com/me/app");
            assert_eq!(branch.to_string(), "feat");
        }
    }

    mod resolve_repo_input {
        use super::*;

        #[test]
        fn returns_provided_repo_arg() {
            let dir = TempDir::new("resolve-repo-input-arg");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let key = resolver
                .resolve_repo_input(Some("github.com/me/app"))
                .expect("provided arg");
            assert_eq!(key.to_string(), "github.com/me/app");
        }

        #[test]
        fn errors_without_arg_and_outside_worktree() {
            let dir = TempDir::new("resolve-repo-input-none");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver.resolve_repo_input(None).unwrap_err();
            assert!(err.to_string().contains("not specified"));
        }
    }

    mod resolve_task_from_args {
        use super::*;

        #[test]
        fn resolves_repo_and_branch_from_two_args() {
            let dir = TempDir::new("resolve-task-from-args-two");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let args = vec!["github.com/me/app".to_string(), "feature-x".to_string()];
            let (repo, branch) = resolver
                .resolve_task_from_args(&args, "usage: task rebase <repo> <branch>")
                .expect("two args");
            assert_eq!(repo.to_string(), "github.com/me/app");
            assert_eq!(branch.to_string(), "feature-x");
        }

        #[test]
        fn errors_on_too_many_args() {
            let dir = TempDir::new("resolve-task-from-args-many");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let args = vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
            ];
            let err = resolver
                .resolve_task_from_args(&args, "too many")
                .unwrap_err();
            assert_eq!(err.to_string(), "too many");
        }
    }

    mod resolve_task_from_query {
        use super::*;

        /// Creates a bare repo with a single worktree at the expected wt path.
        pub(super) fn setup_worktree(
            repos_dir: &std::path::Path,
            wt_dir: &std::path::Path,
            repo_slug: &str,
            branch: &str,
        ) {
            let gitdir = repos_dir.join(format!("{repo_slug}.git"));
            init_bare_repo(&gitdir);
            let wt_path = wt_dir.join(repo_slug).join(branch);
            fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
            let status = std::process::Command::new("git")
                .args([
                    "--git-dir",
                    gitdir.to_str().unwrap(),
                    "worktree",
                    "add",
                    "--orphan",
                    "-b",
                    branch,
                    wt_path.to_str().unwrap(),
                ])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", std::env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git worktree add");
            assert!(status.success(), "git worktree add --orphan failed");
        }

        #[test]
        fn errors_when_no_tasks_exist() {
            let dir = TempDir::new("resolve-query-empty");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            fs::create_dir_all(&repos_dir).unwrap();
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver.resolve_task_from_query("anything").unwrap_err();
            assert!(
                err.to_string().contains("No task matched"),
                "unexpected error: {err}"
            );
        }

        #[test]
        fn resolves_exact_branch_name_match() {
            let dir = TempDir::new("resolve-query-exact");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            setup_worktree(&repos_dir, &wt_dir, "github.com/me/app", "feat-login");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let (repo, branch) = resolver
                .resolve_task_from_query("feat-login")
                .expect("exact match");
            assert_eq!(repo.to_string(), "github.com/me/app");
            assert_eq!(branch.to_string(), "feat-login");
        }

        #[test]
        fn resolves_partial_branch_name_match() {
            let dir = TempDir::new("resolve-query-partial");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            setup_worktree(
                &repos_dir,
                &wt_dir,
                "github.com/me/app",
                "feature-pagination",
            );
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let (repo, branch) = resolver
                .resolve_task_from_query("pagination")
                .expect("partial branch match");
            assert_eq!(branch.to_string(), "feature-pagination");
            assert_eq!(repo.to_string(), "github.com/me/app");
        }

        #[test]
        fn resolves_by_repo_when_branch_query_has_no_match() {
            let dir = TempDir::new("resolve-query-repo");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            setup_worktree(&repos_dir, &wt_dir, "github.com/me/myservice", "main");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let (repo, _branch) = resolver
                .resolve_task_from_query("myservice")
                .expect("repo match");
            assert_eq!(repo.to_string(), "github.com/me/myservice");
        }

        #[test]
        fn errors_when_nothing_matches() {
            let dir = TempDir::new("resolve-query-nomatch");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            setup_worktree(&repos_dir, &wt_dir, "github.com/me/app", "main");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver
                .resolve_task_from_query("completely-unknown-xyz-999")
                .unwrap_err();
            assert!(
                err.to_string().contains("No task matched"),
                "unexpected error: {err}"
            );
        }
    }

    mod repo_task_rows {
        use std::collections::HashSet;

        use super::*;

        #[test]
        fn returns_empty_for_bare_repo_with_no_worktrees() {
            let dir = TempDir::new("task-rows-empty");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let repo_key = RepoKey::new("github.com/me/app");
            let gitdir = repos_dir.join("github.com/me/app.git");
            init_bare_repo(&gitdir);
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let rows = resolver
                .repo_task_rows(&repo_key, &gitdir, &HashSet::new())
                .expect("task rows");
            assert!(rows.is_empty());
        }

        #[test]
        fn discovers_worktree_entries() {
            let dir = TempDir::new("task-rows-wt");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let repo_key = RepoKey::new("github.com/me/app");
            let gitdir = repos_dir.join("github.com/me/app.git");
            init_bare_repo(&gitdir);

            // Create an initial commit so we can create worktrees from it
            let wt_path = wt_dir.join("github.com/me/app/main");
            fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
            // Use git worktree add from the bare repo
            let status = std::process::Command::new("git")
                .args([
                    "--git-dir",
                    gitdir.to_str().unwrap(),
                    "worktree",
                    "add",
                    "--orphan",
                    "-b",
                    "main",
                    wt_path.to_str().unwrap(),
                ])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", std::env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("git worktree add");
            assert!(status.success(), "git worktree add --orphan failed");

            let resolver = resolver_for(&repos_dir, &wt_dir);
            let rows = resolver
                .repo_task_rows(&repo_key, &gitdir, &HashSet::new())
                .expect("task rows");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].branch.to_string(), "main");
        }
    }

    mod launch_workspace_impl {
        use super::*;

        #[test]
        fn non_interactive_prints_path_and_returns_ok() {
            let dir = TempDir::new("launch-non-interactive");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repo_key = RepoKey::new("github.com/me/app");
            let branch = BranchName::new("feat-x");
            let path = dir.path().join("worktree");

            // interactive=false, no_open=false → non-interactive path; must succeed
            let result = resolver.launch_workspace_impl(&repo_key, &branch, &path, false, false);
            assert!(
                result.is_ok(),
                "non-interactive launch_workspace should succeed"
            );
        }

        #[test]
        fn non_interactive_does_not_require_worktree_tools() {
            // Even when the path does not exist (no .envrc, no tmux available),
            // the non-interactive path must succeed — it only prints the path.
            let dir = TempDir::new("launch-non-interactive-missing-path");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repo_key = RepoKey::new("github.com/me/app");
            let branch = BranchName::new("feat-y");
            // Intentionally point at a path that does not exist on disk.
            let path = dir.path().join("no-such-worktree");

            let result = resolver.launch_workspace_impl(&repo_key, &branch, &path, false, false);
            assert!(
                result.is_ok(),
                "non-interactive launch_workspace should succeed even with missing path"
            );
        }

        #[test]
        fn no_open_flag_skips_tools_in_interactive_terminal() {
            let dir = TempDir::new("launch-no-open-flag");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let repo_key = RepoKey::new("github.com/me/app");
            let branch = BranchName::new("feat-z");
            let path = dir.path().join("worktree");

            // interactive=true but no_open=true → must also skip tools and succeed
            let result = resolver.launch_workspace_impl(&repo_key, &branch, &path, true, true);
            assert!(
                result.is_ok(),
                "no_open=true should skip tools and succeed even in an interactive terminal"
            );
        }
    }

    mod ambiguous_repo_resolution {
        use super::*;

        #[test]
        fn resolve_repo_key_input_errors_on_ambiguous_match() {
            // In a non-terminal environment (test harness), ambiguous matches
            // produce an error listing the choices.
            let dir = TempDir::new("resolve-ambiguous-input");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            // Two repos with the same short name under different orgs
            init_bare_repo(&repos_dir.join("github.com/org-a/tool.git"));
            init_bare_repo(&repos_dir.join("github.com/org-b/tool.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver.resolve_repo_key_input("tool").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("org-a/tool") && msg.contains("org-b/tool"),
                "error should list both choices: {msg}"
            );
        }

        #[test]
        fn resolve_existing_repo_key_errors_on_ambiguous_match() {
            let dir = TempDir::new("rerk-ambiguous");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            init_bare_repo(&repos_dir.join("github.com/org-a/service.git"));
            init_bare_repo(&repos_dir.join("github.com/org-b/service.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            let err = resolver.resolve_existing_repo_key("service").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("org-a/service") && msg.contains("org-b/service"),
                "error should list both choices: {msg}"
            );
        }
    }

    mod resolve_task_from_query_multi_match {
        use super::{resolve_task_from_query::setup_worktree, *};

        #[test]
        fn errors_with_choices_on_partial_branch_match() {
            let dir = TempDir::new("resolve-query-multi-partial");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            setup_worktree(&repos_dir, &wt_dir, "github.com/me/app", "feat-alpha");
            setup_worktree(&repos_dir, &wt_dir, "github.com/me/lib", "feat-beta");
            let resolver = resolver_for(&repos_dir, &wt_dir);

            // "feat" partially matches both branches
            let err = resolver.resolve_task_from_query("feat").unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("feat-alpha") && msg.contains("feat-beta"),
                "error should list both matching tasks: {msg}"
            );
        }
    }

    mod clone_bare_repo_method {
        use super::*;

        #[test]
        fn returns_ok_when_valid_bare_repo_already_exists() {
            let dir = TempDir::new("clone-idempotent");
            let repos_dir = dir.path().join("repos");
            let wt_dir = dir.path().join("wt");
            let repo_key = RepoKey::new("github.com/me/app");
            init_bare_repo(&repos_dir.join("github.com/me/app.git"));
            let resolver = resolver_for(&repos_dir, &wt_dir);

            // Should return Ok without doing anything (no network needed)
            let result = resolver.clone_bare_repo("https://example.com/fake.git", &repo_key);
            assert!(
                result.is_ok(),
                "should skip clone when valid bare repo exists"
            );
        }
    }
}
