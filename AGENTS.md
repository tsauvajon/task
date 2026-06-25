# AGENTS.md

Quick orientation for coding agents working on `task`.

## What `task` is

- `task` is a Rust CLI for bare-repo + worktree workflows.
- It manages:
  - cloned bare repos (`repos/`),
  - active task worktrees (`wt/`),
  - detached default-branch worktrees (`detached/`).
- Entry points:
  - CLI wiring: `src/commands/mod.rs`
  - Runtime/context: `src/runtime/`
  - Git helpers: `src/tools/git/`
  - TUI: `src/ui/`

## Core mental model

- A repository is identified by `RepoKey` (for example `github.com/org/repo`).
- Bare repos live at `<repos_dir>/<repo_key>.git`.
- Task worktrees live at `<wt_dir>/<repo_key>/<branch>`.
- Detached worktrees live at `<detached_dir>/<repo_key>`.
- `RuntimeEnvironment` builds the workspace layout from config and exposes `TaskResolver`.

## Command map (where to edit)

- Top-level command definitions and dispatch: `src/commands/mod.rs`
- Repo clone/list: `src/commands/repo.rs`, `src/commands/clone.rs`
- Task lifecycle:
  - `start`: `src/commands/start.rs`
  - `open`: `src/commands/open.rs`
  - `park`: `src/commands/park.rs`
  - `finish`: `src/commands/finish.rs`
  - `list/path`: corresponding files in `src/commands/`
- Quality commands:
  - `coverage`: `src/commands/coverage.rs`
- Detached workflows: `src/commands/detach.rs`

## Repo resolution behavior

- Use `TaskResolver` in `src/runtime/tasks.rs`.
- `resolve_repo_key_input` can accept clone URLs / pass-through keys (used by flows that can clone).
- `resolve_existing_repo_key` is strict:
  - rejects clone URLs,
  - resolves by partial/suffix matching against cloned repos,
  - errors if repo is not already cloned.
- For detached commands, use strict resolution (`resolve_existing_repo_key`).

## Detached worktree policy (important)

- Detached repos are for read-only access to a repo's default branch (or a pinned branch).
- Detached repos are **not** intended for active development.
- Agents should treat detached worktrees as read-only operational copies.
- Do not implement flows that encourage editing in detached worktrees.
- If UX/docs are touched, keep this intent explicit.

## TUI notes

- State models: `src/ui/state.rs`
- Data loading/refresh: `src/ui/tasks.rs`, `src/ui/effects.rs`
- Intent/key mapping: `src/ui/intent.rs`
- Rendering/help text: `src/ui/render.rs`
- App loop and intent handling: `src/ui/mod.rs`
- Repos view currently supports detach toggle keybind `d`.

## Testing and validation expectations

- Prefer colocated unit tests in each module under `#[cfg(test)]`.
- Keep tests focused on behavior, not internals.
- Validate changes with the relevant Cargo/Nix commands for the code path you changed.

## Practical coding guidance for this repo

- Reuse existing helpers in `runtime/tasks.rs` and `tools/git/*` before adding new abstractions.
- Keep command modules small and explicit.
- Preserve current error-message style: actionable and user-oriented.
- When a feature touches both CLI and TUI, update both behavior and help/hints.
