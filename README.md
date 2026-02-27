# task

`task` is a CLI for daily git-worktree workflow: start a task, switch tasks fast, run checks, and clean up finished worktrees.

## Quick start

```bash
nix develop -c cargo install --path .
task --help
```

On the first interactive workspace command, `task` offers to run full setup
(config creation, workspace layout, and toolchain bootstrap). You can also run
setup explicitly with `task bootstrap` or `task doctor --fix`.

## Core commands

```text
task                Open interactive TUI

task repo clone     Clone a bare repo into repos dir
task start <name>   Create/open a task worktree
task check          Run project checks for current task
task coverage       Run Rust test coverage (cargo-llvm-cov)

task park           Park current task
task list           Show open and parked tasks
task open <name>    Re-open a parked task
task finish <name>  Remove a finished task worktree
```

## Build and test

```bash
nix develop -c cargo build
nix develop -c cargo test
nix develop -c cargo llvm-cov --workspace --all-features --summary-only
```

## Shell completions

```bash
mkdir -p ~/.config/fish/completions
task completions fish > ~/.config/fish/completions/task.fish
```
