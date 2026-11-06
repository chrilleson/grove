# 3. No daemon

All state lives in SQLite at `~/.local/state/grove/state.db`. Writers are short-lived processes: hooks, CLI commands and a detached refresh spawned by `grove status` at most every two minutes. Readers (sidebar, status line) poll the database. A daemon would push updates faster but adds install, supervision and upgrade problems on two OSes. SQLite in WAL mode handles the concurrency.
