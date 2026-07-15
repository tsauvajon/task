# task

`task` is a CLI for daily git-worktree workflow: start a task, switch tasks fast, and clean up finished worktrees.

## Quick start

```bash
nix develop -c cargo install --path .
task --help
```

## Core commands

```text
task                Open interactive TUI

task repo clone     Clone a bare repo into repos dir
task start <name>   Create/open a task worktree
task coverage       Run Rust test coverage (cargo-llvm-cov)

task park           Park current task
task list           Show open and parked tasks
task open <name>    Re-open a parked task
task finish <query> [<query> ...]
                    Remove finished task worktrees
```

## Editor

By default, `task start` opens VSCodium in a separate window and runs
opencode + a shell inside a [Zellij](https://zellij.dev) session. To use
[Helix](https://helix-editor.com) inside a Zellij pane instead, set the
top-level `editor` key in `~/.config/task/config.toml`:

```toml
editor = "helix"
```

The Helix layout splits the Zellij session into a fixed-width `task ui`
status pane on the left, an opencode + shell stack in the middle, and
an `hx .` pane on the right. `task park` and `task finish` still tear
the session down the same way.

Note: both commands kill the Zellij session, which terminates `hx` and
discards any unsaved buffers. Save with `:w` inside Helix before parking
or finishing.

## OpenCode executable

By default, `task start` launches the `opencode` executable. To use another
executable name or path, configure it in `~/.config/task/config.toml`:

```toml
[opencode]
command = "opencode-shared"
```

The value must be either a PATH-resolvable executable name or an absolute Unix path.
Relative paths such as `bin/opencode` are rejected. It is passed directly as
one executable, is not parsed as a shell command, and cannot include separate
command-line arguments.

A custom command is expected to be an OpenCode-compatible wrapper that uses
the standard OpenCode session and data behavior. It should `exec` stock
OpenCode so task's existing live-process and status detection remains accurate.

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
