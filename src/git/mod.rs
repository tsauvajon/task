mod context;
mod refs;
mod repo;
mod runner;
mod worktrees;

pub use context::{current_root, git_common_dir, repo_key_from_common_dir};
pub use refs::{current_branch, detect_default_base, fetch_origin_refs, ref_exists, rev_exists};
pub use repo::{
    RepoInput, ResolveResult, clone_bare_repo, normalize_repo_key, parse_repo_input,
    resolve_repo_query,
};
pub use worktrees::{
    WorktreeEntry, branch_from_ref, branch_from_worktree_path, parse_worktree_porcelain, rebase,
    status_porcelain, worktree_add_existing_branch, worktree_add_from_base,
    worktree_add_tracking_remote_branch, worktree_list, worktree_list_porcelain, worktree_prune,
    worktree_remove,
};
