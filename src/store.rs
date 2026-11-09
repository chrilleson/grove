//! Task state, persisted in SQLite. All writers (hooks, CLI) and readers
//! (sidebar, status line) go through this module. See ADR 3.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Waiting,
    Working,
    Done,
    Idle,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Waiting => "waiting",
            State::Working => "working",
            State::Done => "done",
            State::Idle => "idle",
        }
    }

    fn parse(s: &str) -> State {
        match s {
            "waiting" => State::Waiting,
            "working" => State::Working,
            "done" => State::Done,
            _ => State::Idle,
        }
    }
}

/// What a hook tells us about the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Working,
    Waiting,
    Done { info: Option<String> },
    Idle,
}

#[derive(Debug, Clone)]
pub struct NewTask {
    pub repo: String,
    pub root: String,
    pub slug: String,
    pub worktree: String,
    pub team: bool,
    /// Defaults to the slug.
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub repo: String,
    pub root: String,
    pub slug: String,
    pub branch: String,
    pub worktree: String,
    pub session: String,
    pub number: i64,
    pub state: State,
    pub seen: bool,
    pub state_since: i64,
    pub team: bool,
    pub port_base: i64,
    pub info: Option<String>,
    pub diff_added: Option<i64>,
    pub diff_removed: Option<i64>,
    pub created_at: i64,
}

impl Task {
    pub fn needs_you(&self) -> bool {
        self.state == State::Waiting || (self.state == State::Done && !self.seen)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub task: Task,
    pub from: State,
    pub to: State,
    /// The task just entered a needs-you state (waiting, or done-unseen): the
    /// moment to notify. Waiting → done counts; it is new information.
    pub newly_needs_you: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub waiting: usize,
    pub working: usize,
    pub done_unseen: usize,
}

pub struct Store {
    conn: Connection,
    port_start: i64,
    port_block: i64,
}

/// The task id as a tmux session name: `repo/slug`. tmux treats `.` and `:`
/// specially in targets; a leading dot (dotfiles repos) is dropped rather
/// than shown as `_`.
pub fn session_name(repo: &str, slug: &str) -> String {
    format!("{}/{slug}", repo.trim_start_matches('.')).replace(['.', ':'], "_")
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    repo TEXT NOT NULL,
    root TEXT NOT NULL,
    slug TEXT NOT NULL,
    branch TEXT NOT NULL,
    worktree TEXT NOT NULL,
    session TEXT NOT NULL UNIQUE,
    number INTEGER NOT NULL UNIQUE,
    state TEXT NOT NULL,
    seen INTEGER NOT NULL,
    state_since INTEGER NOT NULL,
    team INTEGER NOT NULL,
    port_base INTEGER NOT NULL UNIQUE,
    info TEXT,
    diff_added INTEGER,
    diff_removed INTEGER,
    created_at INTEGER NOT NULL
);
";

const COLUMNS: &str = "id, repo, root, slug, branch, worktree, session, number, state, seen, \
    state_since, team, port_base, info, diff_added, diff_removed, created_at";

fn row_to_task(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        repo: r.get(1)?,
        root: r.get(2)?,
        slug: r.get(3)?,
        branch: r.get(4)?,
        worktree: r.get(5)?,
        session: r.get(6)?,
        number: r.get(7)?,
        state: State::parse(&r.get::<_, String>(8)?),
        seen: r.get(9)?,
        state_since: r.get(10)?,
        team: r.get(11)?,
        port_base: r.get(12)?,
        info: r.get(13)?,
        diff_added: r.get(14)?,
        diff_removed: r.get(15)?,
        created_at: r.get(16)?,
    })
}

impl Store {
    pub fn open(path: &Path, port_start: i64, port_block: i64) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        // WAL lets the sidebar read while a hook writes; busy_timeout absorbs
        // the rare overlapping writers.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        Self::init(conn, port_start, port_block)
    }

    pub fn open_in_memory(port_start: i64, port_block: i64) -> Result<Store> {
        Self::init(Connection::open_in_memory()?, port_start, port_block)
    }

    fn init(conn: Connection, port_start: i64, port_block: i64) -> Result<Store> {
        conn.execute_batch(SCHEMA)?;
        Ok(Store {
            conn,
            port_start,
            port_block,
        })
    }

    pub fn add_task(&self, t: NewTask, now: i64) -> Result<Task> {
        let id = format!("{}/{}", t.repo, t.slug);
        if self.get(&id)?.is_some() {
            bail!("task {id} already exists");
        }
        let number = self.lowest_free("number", 1, 1)?;
        let port_base = self.lowest_free("port_base", self.port_start, self.port_block)?;
        self.conn.execute(
            "INSERT INTO tasks (id, repo, root, slug, branch, worktree, session, number, state, seen,
                state_since, team, port_base, created_at)
             VALUES (?1, ?2, ?3, ?4, ?11, ?5, ?6, ?7, 'idle', 1, ?8, ?9, ?10, ?8)",
            params![
                id,
                t.repo,
                t.root,
                t.slug,
                t.worktree,
                session_name(&t.repo, &t.slug),
                number,
                now,
                t.team,
                port_base,
                t.branch.as_deref().unwrap_or(&t.slug)
            ],
        )?;
        Ok(self.get(&id)?.expect("just inserted"))
    }

    /// Lowest value `start + k*step` (k ≥ 0) not used in `column`.
    fn lowest_free(&self, column: &str, start: i64, step: i64) -> Result<i64> {
        let mut stmt = self.conn.prepare(&format!("SELECT {column} FROM tasks"))?;
        let used: std::collections::HashSet<i64> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok((0..)
            .map(|k| start + k * step)
            .find(|v| !used.contains(v))
            .unwrap())
    }

    pub fn get(&self, id: &str) -> Result<Option<Task>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM tasks WHERE id = ?1"),
                [id],
                row_to_task,
            )
            .optional()?)
    }

    pub fn by_session(&self, session: &str) -> Result<Option<Task>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM tasks WHERE session = ?1"),
                [session],
                row_to_task,
            )
            .optional()?)
    }

    pub fn by_number(&self, number: i64) -> Result<Option<Task>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM tasks WHERE number = ?1"),
                [number],
                row_to_task,
            )
            .optional()?)
    }

    pub fn list(&self) -> Result<Vec<Task>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM tasks ORDER BY number"))?;
        Ok(stmt
            .query_map([], row_to_task)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Apply a signal from the agent. `viewed` is whether a client is looking at
    /// the task's session right now. Returns `None` when nothing changed, so hot
    /// hooks (PreToolUse) cost one read and no write.
    pub fn apply(&self, id: &str, signal: Signal, now: i64, viewed: bool) -> Result<Option<Transition>> {
        let Some(before) = self.get(id)? else {
            return Ok(None);
        };
        let (to, info) = match signal {
            Signal::Working => (State::Working, None),
            Signal::Waiting => (State::Waiting, None),
            Signal::Done { info } => (State::Done, info),
            Signal::Idle => (State::Idle, None),
        };
        if before.state == to && info.is_none() {
            return Ok(None);
        }
        let seen = to != State::Done || viewed;
        self.conn.execute(
            "UPDATE tasks SET state = ?2, seen = ?3, state_since = ?4, info = COALESCE(?5, info)
             WHERE id = ?1",
            params![id, to.as_str(), seen, now, info],
        )?;
        let task = self.get(id)?.expect("exists");
        Ok(Some(Transition {
            newly_needs_you: task.needs_you() && (task.state != before.state || !before.needs_you()),
            from: before.state,
            to,
            task,
        }))
    }

    pub fn set_info(&self, id: &str, info: &str) -> Result<()> {
        self.conn
            .execute("UPDATE tasks SET info = ?2 WHERE id = ?1", params![id, info])?;
        Ok(())
    }

    pub fn set_diff(&self, id: &str, added: i64, removed: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET diff_added = ?2, diff_removed = ?3 WHERE id = ?1",
            params![id, added, removed],
        )?;
        Ok(())
    }

    /// A client switched to `session`: a done task there is now seen.
    pub fn visit_session(&self, session: &str, _now: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET seen = 1 WHERE session = ?1 AND state = 'done'",
            [session],
        )?;
        Ok(())
    }

    /// Tasks that need you in `⌥w` order, excluding the one in `current_session`.
    pub fn rotation(&self, current_session: Option<&str>) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM tasks
             WHERE (state = 'waiting' OR (state = 'done' AND seen = 0)) AND session IS NOT ?1
             ORDER BY state = 'done', state_since, number"
        ))?;
        Ok(stmt
            .query_map([current_session], row_to_task)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn counts(&self) -> Result<Counts> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(state = 'waiting'), 0), COALESCE(SUM(state = 'working'), 0),
                    COALESCE(SUM(state = 'done' AND seen = 0), 0) FROM tasks",
            [],
            |r| {
                Ok(Counts {
                    waiting: r.get::<_, i64>(0)? as usize,
                    working: r.get::<_, i64>(1)? as usize,
                    done_unseen: r.get::<_, i64>(2)? as usize,
                })
            },
        )?)
    }
}
