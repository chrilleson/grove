# grove

Rust CLI/TUI that runs one Claude Code agent per git worktree, each in its own tmux session, and surfaces the agents that need you.

## Read first

- `docs/spec.md` — what grove does. The spec wins over code comments and old design docs.
- `GLOSSARY.md` — use these terms in code, tests and issues (task, needs you, seen, root, …).
- `docs/adr/` — decisions not to relitigate without a new ADR.

## Tracker

GitHub Issues on `chrilleson/grove`. Milestone work is labelled `M1`, `M2`, `M3`. Each issue is a vertical slice with acceptance criteria.

## Rules

- grove never commits, pushes or edits user code. It only creates/removes worktrees and sessions.
- Nothing user- or employer-specific in code, defaults or docs.
- `grove hook` and `grove status` are hot paths: no network, no tmux calls unless a state actually changed, exit 0 on any error.
- Build test-first. Tests go through public seams: the `Store` API, pure render/plan functions, and the `grove` binary (with `XDG_STATE_HOME`, `XDG_CONFIG_HOME` and `GROVE_TMUX_SOCKET` pointed at temp dirs).

## Commands

- `cargo test` — all tests (tmux integration tests skip if `tmux` is missing).
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt`
