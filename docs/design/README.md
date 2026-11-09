# Design references

Screenshots of the interactive simulator built while designing grove. They show the intended **feel**: layout, attention, and how your own tools are reached. They are not pixel specs.

**`docs/spec.md` wins** wherever it and these images disagree. The spec now follows the images for the sidebar, pane headers, status counter, dashboard and new-task dialog. Remaining differences:

- The simulator draws Claude, lazygit and Neovim as mock panes and popups. The real ones are the actual programs in tmux panes and popups, so their contents look like those programs, not like the mock.
- The simulator's agent pane has a header input box ("Type to queue a message"); that is Claude Code's own UI, not grove's.
- Desktop notifications are the OS's own (`notify-send`, `osascript`), not the in-window toasts in image 01.
- PR status in the dashboard (image 05, `PR #88 ✓`) comes with the forge adapter (M3); until then the column shows `⇡N` for commits that exist only on the task branch.
- Repos under `~/dev/repositories/work` and `…/personal` are example roots, not defaults.

Open `simulator.html` in a browser to try it. `⌥` is Alt (Option on macOS). It starts with the sidebar; the two chip styles were rejected in favour of it (ADR 0004).

| Image | Shows |
| --- | --- |
| [01-sidebar-needs-you](01-sidebar-needs-you.png) | Working in a quiet task while others change state. Waiting tasks are amber, done-unseen green. There are notifications and the status counter `⏳3 ●0 ✓2`. |
| [02-waiting-question](02-waiting-question.png) | After `⌥w`: the oldest waiting task is in front, and Claude's question is in the agent pane. |
| [03-review-in-editor](03-review-in-editor.png) | A done task opened with `⌥e`: the editor on the worktree's changed files, Claude's summary beside it. |
| [04-lazygit-popup](04-lazygit-popup.png) | `⌥c`: lazygit in a popup on the worktree. The user stages, commits and pushes; grove never does. |
| [05-dashboard](05-dashboard.png) | `⌥s`: all tasks sorted waiting, done, working, idle, with diff and PR status. |
| [06-new-task-team](06-new-task-team.png) | `⌥n`: pick a repo from the roots, give a title, toggle team mode. |
| [07-new-task-setup](07-new-task-setup.png) | Right after creation: setup output in the shell pane, the new task at the bottom of the sidebar marked `team`. |

Layout in every task session: sidebar (left, fixed width) · Claude (middle) · shell in the worktree (right).
