# Glossary

**Task**
One unit of work: one worktree, one branch, one tmux session and one Claude Code session. A task may run as a team; it is still one task.
_Avoid_: job, workspace, session (a session is only the tmux part of a task)

**Team task**
A task whose Claude Code session runs as an agent team. Its state is the most urgent state of any member.

**Member**
The lead or a teammate inside a team task.

**Lead**
The Claude Code session the task was started with. Its turn ending means the task is done.

**Root**
A directory that holds repos one level deep, e.g. `~/dev/repositories/work`.

**Repo**
A git repository found directly under a root.

**Slug**
The short, filesystem-safe name of a task, unique within its repo. Also the branch name.

**State**
What a task is doing now: `waiting`, `working`, `done` or `idle`.

**Waiting**
The agent is blocked on the user (permission prompt, question, idle prompt).

**Working**
The agent is busy and does not need the user.

**Done**
The lead's turn has ended. A done task is either **seen** or **unseen**.

**Idle**
No agent is running in the task.

**Seen**
The user has visited the task's session since it last became done.

**Needs you**
A task that is waiting, or done and unseen.
_Avoid_: attention, urgent, pending

**Rotation**
The order `⌥w` walks through tasks that need you: waiting first, then done-unseen, oldest first within each group.

**Port block**
The ten consecutive ports reserved for a task. `GROVE_PORT` is the first.

**Setup**
The per-repo steps that turn a fresh worktree into a runnable one: copy/symlink files, run install commands, map ports.

**Teardown**
Removing a task: its tmux session, worktree and (if merged) branch.

**Forge**
The host of a repo's remote (GitHub, Azure DevOps, GitLab). Reached through a forge adapter.

**Tracker**
Where issues live (GitHub Issues, Linear, a local vault). Reached through a tracker adapter.

**Adapter**
A swappable implementation of one outside system behind a small interface: mux, forge, tracker.

**Sidebar**
The narrow pane on the left of every task session that lists all tasks.
