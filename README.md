# task

`task` is a CLI for daily git-worktree workflow: start a task, switch tasks fast, run checks, and clean up finished worktrees.

## Quick start

```bash
nix develop -c cargo install --path .
task --help
```

## Core commands

```text
task start <name>   Create/open a task worktree
task list           Show open and parked tasks
task open <name>    Re-open a parked task
task park           Park current task
task check          Run project checks for current task
task finish <name>  Remove a finished task worktree
task ui             Open interactive dashboard
```

## Build and test

```bash
nix develop -c cargo build
nix develop -c cargo test
```

## Shell completions

```bash
mkdir -p ~/.config/fish/completions
task completions fish > ~/.config/fish/completions/task.fish
```
