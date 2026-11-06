# 4. A sidebar pane, not status-line chips

Every task session has a fixed-width `grove sidebar` pane on the left that lists all tasks with state, age and diff size. The earlier design used a chip strip in the tmux status line; it could not fit repo/slug, age and `+/−` without truncating to noise. The status line keeps only a counter (`⏳2 ✓1`). The cost is about 30 columns per session; pane zoom gives them back.
