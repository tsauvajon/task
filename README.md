# task

Rust rewrite of the `task` workflow CLI for git worktrees.

## Build

```bash
cargo build
```

Or with Nix:

```bash
nix build
```

## Test

```bash
cargo test
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
worktrees   Show raw git worktree list output
clean       Remove a task worktree
prune       Prune stale worktree metadata
done        Run project checks for current task
```
