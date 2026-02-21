use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::{Cli, Commands};
use crate::core::{
    Layout, ResolveResult, branch_from_worktree_path, build_task_rows, normalize_repo_key,
    parse_worktree_porcelain, repo_key_from_common_dir, resolve_repo_key, session_name_for,
};

pub fn run(cli: Cli) -> i32 {
    let layout = Layout::new(default_dev_root());
    let result = match cli.command {
        Commands::Bootstrap => cmd_bootstrap(&layout),
        Commands::Doctor => cmd_doctor(&layout),
        Commands::Clone { repo_url, repo_key } => cmd_clone(&layout, &repo_url, repo_key),
        Commands::Start {
            repo,
            branch,
            base_ref,
        } => cmd_start(&layout, &repo, &branch, base_ref.as_deref()),
        Commands::Open { repo, branch } => cmd_open(&layout, &repo, &branch),
        Commands::Park => cmd_park(&layout),
        Commands::Path { repo, branch } => cmd_path(&layout, &repo, &branch),
        Commands::List { repo } => cmd_list(&layout, repo.as_deref()),
        Commands::Worktrees { repo } => cmd_worktrees(&layout, repo.as_deref()),
        Commands::Clean {
            repo,
            branch,
            force,
        } => cmd_clean(&layout, &repo, &branch, force),
        Commands::Prune { repo } => cmd_prune(&layout, &repo),
        Commands::Done { worktree_path } => cmd_done(worktree_path.as_deref()),
    };

    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

fn default_dev_root() -> PathBuf {
    if let Ok(dev_root) = env::var("DEV_ROOT") {
        return PathBuf::from(dev_root);
    }

    let home = env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    PathBuf::from(home).join("dev")
}

fn cmd_bootstrap(layout: &Layout) -> Result<(), String> {
    ensure_layout(layout)?;
    log(&format!("Workspace root: {}", default_dev_root().display()));

    if command_exists("asdf") {
        let plugins = run_capture("asdf", &["plugin", "list"], None).unwrap_or_default();
        if !plugins.lines().any(|line| line.trim() == "nodejs") {
            log("Installing asdf nodejs plugin");
            run_status(
                "asdf",
                &[
                    "plugin",
                    "add",
                    "nodejs",
                    "https://github.com/asdf-vm/asdf-nodejs.git",
                ],
                None,
            )?;
        }

        let home = env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        let asdf_data_dir = env::var("ASDF_DATA_DIR").unwrap_or_else(|_| format!("{home}/.asdf"));
        let import_script = PathBuf::from(asdf_data_dir)
            .join("plugins")
            .join("nodejs")
            .join("bin")
            .join("import-release-team-keyring");
        if import_script.exists()
            && let Err(error) = run_status(import_script.as_os_str(), &[], None)
        {
            warn(&format!("Could not import nodejs release keyring: {error}"));
        }

        let tool_versions = PathBuf::from(home).join(".tool-versions");
        if tool_versions.exists() {
            log("Installing runtimes from ~/.tool-versions");
            run_status("asdf", &["install"], None)?;
        }
    } else {
        warn(
            "asdf not found. Install toolchain first (nix profile install path:~/flakes#toolchain).",
        );
    }

    if command_exists("node") && command_exists("corepack") {
        let _ = run_status("corepack", &["enable"], None);
    }

    log("Bootstrap complete");
    Ok(())
}

fn cmd_doctor(_layout: &Layout) -> Result<(), String> {
    let mut missing = false;

    println!("DEV_ROOT: {}", default_dev_root().display());
    for cmd in [
        "git", "tmux", "vim", "codium", "opencode", "nix", "direnv", "asdf",
    ] {
        if command_exists(cmd) {
            println!("[ok]      {cmd}");
        } else {
            println!("[missing] {cmd}");
            missing = true;
        }
    }

    let dev_root = default_dev_root();
    if dev_root.join("repos").is_dir() && dev_root.join("wt").is_dir() {
        println!("[ok]      {} layout", dev_root.display());
    } else {
        println!("[missing] {} layout", dev_root.display());
        missing = true;
    }

    if command_exists("opencode") {
        if run_status("opencode", &["auth", "list"], None).is_ok() {
            println!("[ok]      opencode auth storage reachable");
        } else {
            println!("[warn]    opencode auth storage not initialized yet");
        }
    }

    if missing {
        return Err("Doctor check found missing dependencies".to_string());
    }

    Ok(())
}

fn cmd_clone(layout: &Layout, repo_url: &str, repo_key: Option<String>) -> Result<(), String> {
    ensure_layout(layout)?;
    let repo_key = repo_key.unwrap_or_else(|| normalize_repo_key(repo_url));
    clone_bare_repo(layout, repo_url, &repo_key)?;
    log(&format!("Repo key: {repo_key}"));
    Ok(())
}

fn cmd_start(
    layout: &Layout,
    repo_arg: &str,
    branch: &str,
    base_ref: Option<&str>,
) -> Result<(), String> {
    ensure_layout(layout)?;
    let repo_key = resolve_repo_key_input(layout, repo_arg)?;
    ensure_repo_available(layout, repo_arg, &repo_key)?;

    let gitdir = layout.repo_gitdir_path(&repo_key);
    run_status(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "fetch",
            "--all",
            "--prune",
        ],
        None,
    )?;

    let base_ref = base_ref
        .map(|value| value.to_string())
        .unwrap_or_else(|| detect_default_base(&gitdir));

    let worktree = layout.worktree_path(&repo_key, branch);
    if worktree.exists() && !worktree.join(".git").exists() {
        return Err(format!(
            "Path exists but is not a git worktree: {}",
            worktree.display()
        ));
    }

    if worktree.join(".git").exists() {
        log(&format!(
            "Reusing existing worktree: {}",
            worktree.display()
        ));
        return launch_workspace(&repo_key, branch, &worktree);
    }

    if let Some(parent) = worktree.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if ref_exists(&gitdir, &format!("refs/heads/{branch}")) {
        run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "add",
                worktree.to_string_lossy().as_ref(),
                branch,
            ],
            None,
        )?;
    } else if ref_exists(&gitdir, &format!("refs/remotes/origin/{branch}")) {
        run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "add",
                "--track",
                "-b",
                branch,
                worktree.to_string_lossy().as_ref(),
                &format!("origin/{branch}"),
            ],
            None,
        )?;
    } else {
        if !rev_exists(&gitdir, &base_ref) {
            return Err(format!("Base ref not found: {base_ref}"));
        }
        run_status(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "add",
                "-b",
                branch,
                worktree.to_string_lossy().as_ref(),
                &base_ref,
            ],
            None,
        )?;
    }

    launch_workspace(&repo_key, branch, &worktree)
}

fn cmd_open(layout: &Layout, repo_arg: &str, branch: &str) -> Result<(), String> {
    let repo_key = resolve_repo_key_input(layout, repo_arg)?;
    let worktree = layout.worktree_path(&repo_key, branch);
    if !worktree.join(".git").exists() {
        return Err(format!("Worktree not found: {}", worktree.display()));
    }
    launch_workspace(&repo_key, branch, &worktree)
}

fn cmd_park(layout: &Layout) -> Result<(), String> {
    ensure_layout(layout)?;
    let (repo_key, branch, root) = current_task_info()?;

    if !command_exists("tmux") {
        return Err("tmux is not available. Run 'task list' to inspect tasks.".to_string());
    }

    let session = session_name_for(&repo_key, &branch);
    if tmux_has_session(&session) {
        run_status("tmux", &["kill-session", "-t", &session], None)?;
        log(&format!("Parked task: {repo_key} {branch}"));
    } else {
        log(&format!("Task already parked: {repo_key} {branch}"));
    }

    println!("{}", root.display());
    Ok(())
}

fn cmd_path(layout: &Layout, repo_arg: &str, branch: &str) -> Result<(), String> {
    let repo_key = resolve_repo_key_input(layout, repo_arg)?;
    println!("{}", layout.worktree_path(&repo_key, branch).display());
    Ok(())
}

fn cmd_list(layout: &Layout, repo_arg: Option<&str>) -> Result<(), String> {
    ensure_layout(layout)?;
    let open_sessions = tmux_sessions();
    println!("{:<7} {:<35} {:<28} PATH", "STATUS", "REPO", "BRANCH");

    let mut rows_printed = 0usize;
    if let Some(repo_arg) = repo_arg {
        let repo_key = resolve_repo_key_input(layout, repo_arg)?;
        let gitdir = layout.repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }

        rows_printed += print_repo_tasks(&repo_key, &gitdir, &open_sessions)?;
        if rows_printed == 0 {
            log(&format!("No tasks found for {repo_key}"));
        }
        return Ok(());
    }

    let repo_keys = available_repo_keys(layout)?;
    for repo_key in repo_keys {
        let gitdir = layout.repo_gitdir_path(&repo_key);
        rows_printed += print_repo_tasks(&repo_key, &gitdir, &open_sessions)?;
    }

    if rows_printed == 0 {
        log(&format!(
            "No tasks found under {}",
            default_dev_root().join("wt").display()
        ));
    }

    Ok(())
}

fn cmd_worktrees(layout: &Layout, repo_arg: Option<&str>) -> Result<(), String> {
    ensure_layout(layout)?;

    if let Some(repo_arg) = repo_arg {
        let repo_key = resolve_repo_key_input(layout, repo_arg)?;
        let gitdir = layout.repo_gitdir_path(&repo_key);
        if !gitdir.is_dir() {
            return Err(format!("Repo not found: {repo_key}"));
        }
        let output = run_capture(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "list",
            ],
            None,
        )?;
        print!("{output}");
        return Ok(());
    }

    let repo_keys = available_repo_keys(layout)?;
    if repo_keys.is_empty() {
        log(&format!(
            "No repositories found in {}",
            layout
                .repo_gitdir_path("")
                .parent()
                .unwrap_or(Path::new("/"))
                .display()
        ));
        return Ok(());
    }

    for repo_key in repo_keys {
        println!();
        println!("[{repo_key}]");
        let gitdir = layout.repo_gitdir_path(&repo_key);
        let output = run_capture(
            "git",
            &[
                "--git-dir",
                gitdir.to_string_lossy().as_ref(),
                "worktree",
                "list",
            ],
            None,
        )?;
        print!("{output}");
    }

    Ok(())
}

fn cmd_clean(layout: &Layout, repo_arg: &str, branch: &str, force: bool) -> Result<(), String> {
    let repo_key = resolve_repo_key_input(layout, repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    let worktree = layout.worktree_path(&repo_key, branch);

    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }
    if !worktree.join(".git").exists() {
        return Err(format!("Worktree not found: {}", worktree.display()));
    }

    if !force {
        let status = run_capture(
            "git",
            &[
                "-C",
                worktree.to_string_lossy().as_ref(),
                "status",
                "--porcelain",
            ],
            None,
        )?;
        if !status.trim().is_empty() {
            return Err(
                "Worktree has uncommitted changes. Use --force if you really want to remove it."
                    .to_string(),
            );
        }
    }

    let mut args = vec![
        "--git-dir".to_string(),
        gitdir.to_string_lossy().to_string(),
        "worktree".to_string(),
        "remove".to_string(),
    ];
    if force {
        args.push("--force".to_string());
    }
    args.push(worktree.to_string_lossy().to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_status("git", &arg_refs, None)?;

    if let Some(parent) = worktree.parent() {
        let _ = fs::remove_dir(parent);
    }

    Ok(())
}

fn cmd_prune(layout: &Layout, repo_arg: &str) -> Result<(), String> {
    let repo_key = resolve_repo_key_input(layout, repo_arg)?;
    let gitdir = layout.repo_gitdir_path(&repo_key);
    if !gitdir.is_dir() {
        return Err(format!("Repo not found: {repo_key}"));
    }
    run_status(
        "git",
        &[
            "--git-dir",
            gitdir.to_string_lossy().as_ref(),
            "worktree",
            "prune",
            "--verbose",
        ],
        None,
    )
}

fn cmd_done(worktree_path: Option<&str>) -> Result<(), String> {
    let path = worktree_path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !path.is_dir() {
        return Err(format!("Path not found: {}", path.display()));
    }

    let mut checked = false;
    if path.join("Cargo.toml").exists() {
        checked = true;
        log("Running Rust checks");
        run_status("cargo", &["fmt", "--all", "--check"], Some(&path))?;
        run_status(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
            Some(&path),
        )?;
        run_status(
            "cargo",
            &["test", "--workspace", "--all-features"],
            Some(&path),
        )?;
    }

    if path.join("package.json").exists() {
        checked = true;
        log("Running JS checks");
        if command_exists("corepack") {
            let _ = run_status("corepack", &["enable"], None);
        }

        let (tool, install_args): (&str, Vec<&str>) = if command_exists("pnpm") {
            ("pnpm", vec!["install", "--frozen-lockfile"])
        } else if command_exists("corepack") {
            ("corepack", vec!["pnpm", "install", "--frozen-lockfile"])
        } else {
            warn("pnpm/corepack not found. Skipping JS checks.");
            ("", Vec::new())
        };

        if !tool.is_empty() {
            if run_status(tool, &install_args, Some(&path)).is_err() {
                let fallback = if tool == "pnpm" {
                    vec!["install"]
                } else {
                    vec!["pnpm", "install"]
                };
                run_status(tool, &fallback, Some(&path))?;
            }

            let commands = if tool == "pnpm" {
                vec![
                    vec!["run", "lint", "--if-present"],
                    vec!["run", "check", "--if-present"],
                    vec!["run", "test", "--if-present"],
                    vec!["run", "build", "--if-present"],
                ]
            } else {
                vec![
                    vec!["pnpm", "run", "lint", "--if-present"],
                    vec!["pnpm", "run", "check", "--if-present"],
                    vec!["pnpm", "run", "test", "--if-present"],
                    vec!["pnpm", "run", "build", "--if-present"],
                ]
            };

            for args in commands {
                run_status(tool, &args, Some(&path))?;
            }
        }
    }

    if !checked {
        warn("No Cargo.toml or package.json found. Nothing to run.");
    }

    Ok(())
}

fn ensure_layout(layout: &Layout) -> Result<(), String> {
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

fn available_repo_keys(layout: &Layout) -> Result<Vec<String>, String> {
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

fn is_git_url(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("ssh://")
        || input.starts_with("git@")
}

fn resolve_repo_key_input(layout: &Layout, repo_arg: &str) -> Result<String, String> {
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

fn choose_repo_key_interactive(query: &str, choices: &[String]) -> Result<String, String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(format!(
            "Multiple repositories match '{query}': {}. Please use a full repo key.",
            choices.join(" ")
        ));
    }

    if !command_exists("fzf") {
        return Err(format!(
            "Multiple repositories match '{query}': {}. Install fzf or use a full repo key.",
            choices.join(" ")
        ));
    }

    let mut child = Command::new("fzf")
        .args([
            "--prompt=repo> ",
            "--height=40%",
            "--reverse",
            "--border",
            &format!("--header=Multiple repos match '{query}' - choose one"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(choices.join("\n").as_bytes())
            .map_err(|e| e.to_string())?;
    }

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Selection cancelled.".to_string());
    }

    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        return Err("Selection cancelled.".to_string());
    }
    Ok(selected)
}

fn clone_bare_repo(layout: &Layout, repo_url: &str, repo_key: &str) -> Result<(), String> {
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

fn ensure_repo_available(layout: &Layout, repo_arg: &str, repo_key: &str) -> Result<(), String> {
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

fn detect_default_base(gitdir: &Path) -> String {
    let gitdir_text = gitdir.to_string_lossy();
    if run_status(
        "git",
        &[
            "--git-dir",
            gitdir_text.as_ref(),
            "show-ref",
            "--verify",
            "--quiet",
            "refs/remotes/origin/main",
        ],
        None,
    )
    .is_ok()
    {
        return "origin/main".to_string();
    }

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

fn ref_exists(gitdir: &Path, reference: &str) -> bool {
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

fn rev_exists(gitdir: &Path, revision: &str) -> bool {
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

fn launch_workspace(repo_key: &str, branch: &str, path: &Path) -> Result<(), String> {
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

fn print_repo_tasks(
    repo_key: &str,
    gitdir: &Path,
    open_sessions: &HashSet<String>,
) -> Result<usize, String> {
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
    let rows = build_task_rows(repo_key, &entries, &open_session_list);
    for row in &rows {
        println!(
            "{:<7} {:<35} {:<28} {}",
            row.status,
            row.repo,
            row.branch,
            row.path.display()
        );
    }
    Ok(rows.len())
}

fn tmux_sessions() -> HashSet<String> {
    if !command_exists("tmux") {
        return HashSet::new();
    }

    let output = match run_capture("tmux", &["ls"], None) {
        Ok(output) => output,
        Err(_) => return HashSet::new(),
    };

    output
        .lines()
        .filter_map(|line| line.split(':').next())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn tmux_has_session(session: &str) -> bool {
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

fn current_task_info() -> Result<(String, String, PathBuf), String> {
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

fn command_exists(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).exists();
    }

    let path_var = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path_var).any(|dir| dir.join(name).exists())
}

fn run_capture(
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

fn run_status(program: impl AsRef<OsStr>, args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
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

fn log(message: &str) {
    println!("==> {message}");
}

fn warn(message: &str) {
    eprintln!("warning: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_sessions_parses_names() {
        let text = "task_a: 1 windows\nmain: 2 windows\n";
        let sessions: HashSet<String> = text
            .lines()
            .filter_map(|line| line.split(':').next())
            .map(|name| name.trim().to_string())
            .collect();
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
