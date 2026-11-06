# 5. Claude Code hooks are the status source

Task state comes from Claude Code hooks that call `grove hook <event>`, with the task identified by `GROVE_TASK` set on the tmux session. Scraping pane contents was the alternative; it is fragile across Claude Code versions and costs CPU on every poll. The price is that v1 supports Claude Code only; other agents need their own adapter later.
