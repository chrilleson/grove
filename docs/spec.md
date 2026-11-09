# grove — spec

Working name. See `GLOSSARY.md` for terms and `docs/adr/` for the decisions behind this spec.

## Problem

You run several Claude Code agents in parallel, each in its own git worktree, inside tmux. Today that means hand-making worktrees, copying `.env` files, starting sessions, and polling panes to find the agent that is stuck on a permission prompt. grove makes this one command per task, makes agents that need you impossible to miss, and otherwise stays out of the way: you keep full control of commits, pushes and code reading, using your own tools.

grove is generic. Nothing in it is specific to one user, employer or machine.

## Non-goals

- grove never commits, pushes or edits code. It hands you a shell and your tools.
- No daemon (ADR 3).
- No Windows support in v1 (WSL may work, unofficially).
- One agent in v1: Claude Code.

## Platform

- Rust, single static binary (ADR 1). Ratatui for the TUIs.
- tmux ≥ 3.3 is the pane engine, behind a `Mux` trait.
- macOS and Linux.
- State: SQLite at `$XDG_STATE_HOME/grove/state.db` (default `~/.local/state/grove/`), WAL mode.
- Config: `$XDG_CONFIG_HOME/grove/` (default `~/.config/grove/`) on both OSes.

## Task model

A task is one worktree + one branch + one tmux session + one Claude Code session (ADR 2).

| Field | Notes |
| --- | --- |
| `id` | `<repo>/<slug>`, unique |
| `repo`, `root`, `slug`, `branch`, `base` | branch is `branch_prefix` + slug (default `grove/`); base is the ref it forked from (origin HEAD, else the current branch) |
| `worktree` | absolute path |
| `session` | tmux session name: the id `<repo>/<slug>`, leading dots dropped, `.`/`:` replaced |
| `number` | small integer, lowest free, used by `⌥1–9` |
| `state` | `waiting` · `working` · `done` · `idle` |
| `seen` | bool, only meaningful when `done` |
| `state_since` | timestamp of last state change |
| `team` | bool |
| `port_base` | first port of the block |
| `info` | short line from the agent's last message |
| `pr` | cached PR number/state, optional |
| `created_at`, `last_visited_at` | |

### Team tasks

A team task stores a state per member (`lead` plus each teammate's `agent_id`). The task's state is the most urgent member state, ordered `waiting > working > done > idle`.

## Roots and worktrees

```toml
# ~/.config/grove/config.toml
roots = ["~/dev/repositories/personal", "~/dev/repositories/work"]
# worktree_root = "~/worktrees"   # optional override
default_team = false
editor = "nvim"

[attention]
blink_after = "2m"
renotify_after = "10m"

[ports]
start = 20000
block = 10

[keys]           # all remappable; values are tmux key names
next = "M-w"
dashboard = "M-s"
last = "M-Tab"
lazygit = "M-c"
editor = "M-e"
vscode = "M-v"
new = "M-n"
leader = "M-g"
```

- Repos are directories with `.git` directly under a root, sorted by most recent use (last task created there, then mtime).
- Worktrees live in `<root>/.worktrees/<repo>/<slug>`, or `<worktree_root>/<repo>/<slug>` when set.

## Setup

Per-repo setup is read from `~/.config/grove/repos/<repo>.toml`, then from an optional committed `.grove.toml` at the repo root. Keys from the user's file win.

```toml
base = "origin/main"           # branch to fork from; default: origin HEAD
team = true                    # per-repo override of default_team

[setup]
copy    = [".env", ".env.local"]
symlink = ["node_modules/.cache"]
run     = ["pnpm install --frozen-lockfile"]

[ports]
PORT     = 0                   # GROVE_PORT + 0
API_PORT = 1                   # GROVE_PORT + 1

[teardown]
run = ["docker compose down"]
```

Order: create worktree → copy/symlink from the main checkout → write port vars → run commands in the worktree with the port vars set. A failing command marks the task `idle` with the error in `info` and leaves the session open so you can fix it.

## Ports

Each task gets the lowest free block of 10 ports starting at `ports.start`. `GROVE_PORT` and any mapped variables are set on the tmux session with `new-session -e`, so every pane inherits them.

## Session layout

The design reference is the grove simulator (claude.ai artifact "grove simulator", sidebar variant). Colours come from its palette (`src/theme.rs`): waiting `#f0b03f`, working `#6fb3ff`, done `#7fd492`, idle `#5d6877`.

```
┌─ grove ───────┬─ claude · portal/fix-login · ⏳ needs input ─┬─ zsh · ~/…/fix-login ─┐
│ sidebar       │ agent                                        │ shell (worktree)      │
│ (32 cols)     │                                              │                       │
└───────────────┴──────────────────────────────────────────────┴───────────────────────┘
```

- Agent and shell split the space after the sidebar 50/50 (`shell_percent`).
- Every pane has a header (`pane-border-status top`). Panes carry `@grove_role` and `@grove_title`; the agent header also shows `@grove_state`, which the hook updates on each state change. The focused pane's border is blue.
- The session is created at the real client size and a `window-resized` hook keeps the sidebar at its width.
- Environment on the session: `GROVE_TASK=<id>`, `GROVE_PORT`, mapped port vars, and for team tasks `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`.
- The session opens immediately. The shell pane first runs `grove setup <id>`, which prints `grove:` progress lines and the setup output, then starts the agent in the agent pane and becomes the user's shell.
- A brief is written to `.grove/brief.md` in the worktree (hidden via the repo's `info/exclude`) and the agent starts as `claude "Read .grove/brief.md and start."` (team: `claude --teammate-mode tmux …`).
- `⌥w`, `⌥1–9` and the sidebar land in the agent pane when the task is waiting, otherwise in the shell.

## Sidebar

`grove sidebar` runs in the left pane of every task session. It polls the database every 500 ms and renders two lines per task:

```
 tasks · 3
 1 ⏳ portal/fix-login-redirect
    needs input · 3m · +42 −7
 2 ● api/rate-limit-webhooks
    working · 40m · +118 −3
 3 ✓ dotfiles/zsh-linux
    done, not seen · 1h · Added zsh config…
```

- Icons: `⏳` waiting, `●` working, `✓` done, `○` idle, in the state colour.
- Waiting rows are amber and bold, done-unseen rows green and bold, the rest dim; the current task gets a background bar.
- The second line is state words, age, then `+/−` if there is a diff, else the agent's last message, then `team` for team tasks.
- Waiting icons blink after `blink_after`.
- Footer: `⌥w next · ⌥s all · q hide`. `q` closes only the sidebar pane.

## Status line

`grove status` prints a compact counter for the tmux status line, e.g. `⏳2 ●3 ✓1` (`--tmux` adds the palette colours), from the database in under 5 ms. At most every 2 minutes it also spawns a detached `grove refresh` that updates diff stats and PR status.

## States and attention

Hook → state mapping (ADR 5):

| Hook | Effect |
| --- | --- |
| `UserPromptSubmit`, `PreToolUse` | `working` |
| `Notification` (`permission_prompt`, `idle_prompt`), `PermissionRequest` | `waiting` |
| `Stop` | `done`, `seen = false`; store last assistant message as `info` |
| `SessionEnd` | `idle` |
| `SubagentStart`, `SubagentStop` | no state change (never mark done) |
| `TeammateIdle`, `TaskCreated`, `TaskCompleted` | log raw payload only in v1 |

- Every hook payload is appended to `$XDG_STATE_HOME/grove/hooks.jsonl` when `GROVE_LOG_HOOKS=1` (and always during M1 while schemas are confirmed).
- If `GROVE_TASK` is unset the hook exits 0 immediately. Hooks never block or fail Claude Code.
- **Needs you** = `waiting`, or `done` and not seen.
- Visiting a task's session marks it seen (tmux `client-session-changed` hook → `grove visit`).
- Rotation (`⌥w`, `grove next`): waiting tasks first, then done-unseen, oldest `state_since` first within each group; the current task is skipped.
- Notifications fire only on a transition into a needs-you state, never for the task you are looking at. Linux: `notify-send`; macOS: `osascript -e 'display notification …'`.
- A task still waiting after `renotify_after` gets one more notification (checked by `grove status`).

## Keys

grove binds only `M-…` keys in tmux's root table. Making ⌥ send Alt is the terminal's job; `grove doctor` explains it for Ghostty (`macos-option-as-alt = true`), iTerm2, kitty and WezTerm.

| Key | Action | Command |
| --- | --- | --- |
| `⌥w` | Next task that needs you | `grove next` |
| `⌥s` | Dashboard popup | `grove dash` |
| `⌥Tab` | Last task | `tmux switch-client -l` |
| `⌥1–9` | Task by number | `grove go <n>` |
| `⌥c` | lazygit in a bordered popup titled `lazygit · repo → branch` (image 04) | `grove open lazygit` |
| `⌥e` | Editor on the worktree, in the shell pane when it is at a prompt, else a new window (image 03) | `grove open editor` |
| `⌥v` | VS Code on the worktree | `grove open code` |
| `⌥n` | New task dialog (image 06) | `grove new` in a popup |
| `⌥g b/p/x/r` | brief / PR / discard / rename | `grove brief`, `grove pr`, `grove rm`, `grove rename` |

## CLI

| Command | Purpose |
| --- | --- |
| `grove new` | New-task dialog: repo filter (most recently used first), title, team toggle. The slug comes from the title, the title becomes the brief |
| `grove new <repo> [slug] [--title T] [--brief TEXT] [--team/--no-team] [--base REF]` | Create a task end to end; slug defaults to one derived from `--title` |
| `grove open lazygit\|editor\|code [task]` | Your tools on a task (defaults to the current one) |
| `grove ls` | List tasks |
| `grove go <n\|id>` | Switch to task |
| `grove next` | Switch to next task that needs you |
| `grove visit` | Mark the current session's task seen (called by tmux hook) |
| `grove hook <event>` | Claude Code hook entry point; reads JSON on stdin |
| `grove sidebar` | Sidebar TUI |
| `grove status` | Status-line counter |
| `grove refresh` | Update diff stats (started in the background by `grove status`, at most every 10 s) |
| `grove dash` | Dashboard (image 05): icon, repo, slug, `+a −r`, push state (`⇡N` commits only on the branch; PR status later), age; sorted waiting (oldest first), done, working, idle (newest first). Keys `⏎ j/k c e v n x esc` |
| `grove rm [id] [--force]` | Teardown (`⌥g x`); refuses uncommitted changes or commits on no other branch/remote unless `--force` |
| `grove setup <id>` | Internal: runs setup in the shell pane, then starts the agent |
| `grove init [--dry-run]` | Interactive setup |
| `grove doctor` | Check tmux, terminal Alt handling, hooks, gh |

## `grove init`

Interactive. Asks for roots, writes `~/.config/grove/config.toml`, writes `~/.config/grove/grove.tmux.conf` (key bindings, status counter, session hook) and tells you the one `source-file` line to add. Adds hook entries to `~/.claude/settings.json`; grove's entries are recognised by their command (`<grove path> hook <Event>`), so re-running replaces only those and leaves the user's own hooks alone. (No custom marker key, so Claude Code's settings validation never sees an unknown field.) Never rewrites your own files beyond that marked block. `--dry-run` prints every change.

## Forges

Adapter chosen from the `origin` host. v1: GitHub via `gh pr view --json`. PR status is refreshed by `grove refresh`.

## Teardown

- PR merged and worktree clean (no uncommitted changes, no unpushed commits): remove automatically, notify once.
- Otherwise mark `merged` and wait.
- `grove rm` runs repo `teardown.run`, kills the session, removes the worktree, deletes the branch if merged. Refuses dirty or unpushed work unless `--force`.

## Trackers

Optional adapters: GitHub Issues, Linear, local vault. v1 is read-only plus one optional write: the `⌥n` dialog lists issues and prefills slug, title and brief; grove can move an issue to in progress.

## Milestones

- **M1 — tracer bullet.** `grove new` → worktree → setup → session with sidebar, Claude and shell → hooks write state → sidebar and status counter update → `⌥w` jumps. Plus `grove init --dry-run`-able tmux conf and hook install, `grove ls`, `grove go`, `grove visit`, notifications.
- **M2 — daily driver.** Dashboard, `⌥n` dialog, teardown and `grove rm`, diff stats, blink/renotify, `grove doctor`, team tasks.
- **M3 — forge and tracker.** GitHub PR status, auto-teardown on merge, GitHub Issues adapter, `⌥g` leader actions.
- **Later.** Name, decision capture and tracker write-back, sidebar collapse, shell-command completion, other agents, remote, macOS menu-bar app.
