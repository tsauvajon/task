# task

Rust rewrite of the `task` workflow CLI for git worktrees.

## Build

```bash
nix develop -c cargo build
```

Or with Nix:

```bash
nix build
```

## Test

```bash
nix develop -c cargo test
```

## Toolchain

Rust tooling comes from the Nix dev shell. Run formatting and linting with:

```bash
nix develop -c cargo fmt
nix develop -c cargo clippy --workspace --all-targets --all-features -- -D warnings -D rust-2024-compatibility -A deprecated
```

## Commands

```text
bootstrap   Prepare workspace and asdf Node plugin
doctor      Check toolchain and workspace health
clone       Clone bare repo into ~/dev/repos
start       Create/open a task worktree
open        Re-open a parked task
park        Park current task (stop tmux session)
path        Print worktree path for a task
list        List tasks with open/parked status
ui          Interactive task dashboard
worktrees   Show raw git worktree list output
clean       Remove a task worktree
prune       Prune stale worktree metadata
done        Run project checks for current task
completions Generate shell completion scripts
```

Generate completions:

```bash
task completions bash > task.bash-completion
task completions fish > task.fish
```

## Terminal UI

- `comfy-table` renders status tables for `task list`
- `dialoguer` provides arrow-key interactive selection for ambiguous repo names
- `owo-colors` styles status and warning output
- `ratatui` + `crossterm` power `task ui` for a full-screen interactive view

`task` with no command opens the same interactive dashboard as `task ui`.

`task ui` keybindings:

- `j/k` or arrows to move
- `Enter` to open selected task
- `p` to park selected task
- `/` to filter, `Ctrl-U` to clear in filter mode
- `r` to refresh
- `?` to show help
- `q` to quit
