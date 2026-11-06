# grove (working name)

One command per task: a git worktree, its setup, a tmux session with a sidebar, Claude Code and a shell. Agents that need you are impossible to miss. Your commits, pushes and code reading stay yours.

Status: early, milestone 1. See [docs/spec.md](docs/spec.md).

## Quick start

```sh
cargo install --path .
grove init            # writes ~/.config/grove/{config.toml,grove.tmux.conf}, installs Claude Code hooks
# add to ~/.tmux.conf:  source-file ~/.config/grove/grove.tmux.conf
grove new myrepo fix-login --brief "Login fails when the session cookie expires"
```

Requires tmux ≥ 3.3, git, and Claude Code. macOS and Linux.
