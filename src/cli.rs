use crate::config::Config;
use crate::mux::{Mux, Role, Tmux};
use crate::store::{Counts, Store};
use crate::task::NewOpts;
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "grove",
    version,
    about = "One worktree, one tmux session, one agent per task"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Claude Code hook entry point (reads the hook JSON on stdin)
    Hook { event: String },
    /// Status-line counter, e.g. `⏳2 ●3 ✓1`
    Status {
        /// Colour it with tmux style tags
        #[arg(long)]
        tmux: bool,
    },
    /// Create a task: worktree, setup, tmux session, agent
    New {
        /// Repo name under a root, or a path to a repo
        repo: String,
        /// Task name; also the branch name
        slug: String,
        /// First prompt for the agent
        #[arg(long)]
        brief: Option<String>,
        /// Ref to branch from (default: repo config, then origin HEAD, then HEAD)
        #[arg(long)]
        base: Option<String>,
        /// Run as an agent team
        #[arg(long, overrides_with = "no_team")]
        team: bool,
        #[arg(long)]
        no_team: bool,
        /// Don't switch to the new session
        #[arg(short, long)]
        detach: bool,
    },
    /// List tasks
    Ls,
    /// List repos under the roots, most recently used first
    Repos,
    /// Switch to a task by number or id
    Go { task: String },
    /// Switch to the next task that needs you
    Next,
    /// Mark the task in a session seen (called by the tmux session hook)
    Visit { session: Option<String> },
    /// Remove a task: session, worktree and branch. Refuses unsaved work unless --force
    Rm {
        /// Number or id; defaults to the task you're in
        task: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Run a task's setup and start its agent (runs in the shell pane of a new session)
    #[command(hide = true)]
    Setup { task: String },
    /// Sidebar pane (runs inside task sessions)
    Sidebar,
    /// Task picker popup
    Dash,
    /// Write config, tmux conf and Claude Code hooks
    Init {
        /// Print every change, write nothing
        #[arg(long)]
        dry_run: bool,
        /// Root directory holding repos (repeatable); asked for if omitted
        #[arg(long = "root")]
        roots: Vec<String>,
    },
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Hook { event } => {
            crate::hook::run(&event);
            Ok(())
        }
        Cmd::Status { tmux } => {
            let c = open_store(&Config::load()?)?.counts()?;
            println!(
                "{}",
                if tmux {
                    format_counts_tmux(c)
                } else {
                    format_counts(c)
                }
            );
            Ok(())
        }
        Cmd::Init { dry_run, roots } => crate::init::run(dry_run, roots),
        cmd => {
            let config = Config::load()?;
            let store = open_store(&config)?;
            let mux = Tmux::from_env();
            run(cmd, &config, &store, &mux)
        }
    }
}

fn run(cmd: Cmd, config: &Config, store: &Store, mux: &Tmux) -> Result<()> {
    match cmd {
        Cmd::Hook { .. } | Cmd::Status { .. } | Cmd::Init { .. } => unreachable!(),
        Cmd::New {
            repo,
            slug,
            brief,
            base,
            team,
            no_team,
            detach,
        } => {
            let brief = brief.filter(|b| !b.trim().is_empty());
            let team = if team {
                Some(true)
            } else if no_team {
                Some(false)
            } else {
                None
            };
            let task = crate::task::new_task(
                config,
                store,
                mux,
                NewOpts {
                    repo,
                    slug,
                    brief,
                    base,
                    team,
                },
            )?;
            eprintln!("grove: created {} in {}", task.id, task.worktree);
            if !detach {
                mux.switch_to(&task.session)?;
            }
            Ok(())
        }
        Cmd::Ls => {
            let now = crate::now();
            for t in store.list()? {
                println!(
                    "{:>2} {:<8} {:<5} {:<40} {}",
                    t.number,
                    state_label(&t),
                    crate::sidebar::age(now - t.state_since),
                    t.id,
                    t.info.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
        Cmd::Repos => {
            for r in crate::task::list_repos(config) {
                println!("{}\t{}", r.name, r.dir.display());
            }
            Ok(())
        }
        Cmd::Rm { task, force } => {
            let t = match task {
                Some(task) => find_task(store, &task)?,
                None => match std::env::var("GROVE_TASK").ok() {
                    Some(id) => store.get(&id)?,
                    None => match mux.current_session() {
                        Some(s) => store.by_session(&s)?,
                        None => None,
                    },
                },
            };
            let Some(t) = t else {
                anyhow::bail!("no such task (give a number or id)")
            };
            crate::task::remove_task(store, mux, &t, force)?;
            eprintln!("grove: removed {}", t.id);
            Ok(())
        }
        Cmd::Go { task } => match find_task(store, &task)? {
            Some(t) => mux.switch_to(&t.session),
            None => {
                mux.message(&format!("grove: no task {task}"));
                Ok(())
            }
        },
        Cmd::Next => {
            let current = mux.current_session();
            match store.rotation(current.as_deref())?.into_iter().next() {
                Some(t) => go_to(mux, &t),
                None => {
                    mux.message("grove: nothing needs you");
                    Ok(())
                }
            }
        }
        Cmd::Visit { session } => {
            if let Some(s) = session.or_else(|| mux.current_session()) {
                store.visit_session(&s, crate::now())?;
            }
            Ok(())
        }
        Cmd::Setup { task } => crate::task::run_setup(config, store, mux, &task),
        Cmd::Sidebar => crate::sidebar::run(config, false),
        Cmd::Dash => crate::sidebar::run(config, true),
    }
}

/// Switch to a task, landing in the agent pane if it is waiting for you
/// and in the shell otherwise.
fn go_to(mux: &Tmux, t: &crate::store::Task) -> Result<()> {
    let role = if t.state == crate::store::State::Waiting {
        Role::Agent
    } else {
        Role::Shell
    };
    let _ = mux.focus(&t.session, role);
    mux.switch_to(&t.session)
}

fn find_task(store: &Store, task: &str) -> Result<Option<crate::store::Task>> {
    match task.parse::<i64>() {
        Ok(n) => store.by_number(n),
        Err(_) => store.get(task),
    }
}

fn state_label(t: &crate::store::Task) -> &'static str {
    match t.state {
        crate::store::State::Done if !t.seen => "done*",
        s => s.as_str(),
    }
}

pub fn open_store(config: &Config) -> Result<Store> {
    Store::open(&crate::paths::state_db(), config.ports.start, config.ports.block)
}

pub fn format_counts(c: Counts) -> String {
    let mut parts = vec![];
    if c.waiting > 0 {
        parts.push(format!("⏳{}", c.waiting));
    }
    if c.done_unseen > 0 {
        parts.push(format!("✓{}", c.done_unseen));
    }
    parts.join(" ")
}

/// The counter with tmux style tags, in the simulator's colours.
pub fn format_counts_tmux(c: Counts) -> String {
    let mut parts = vec![];
    if c.waiting > 0 {
        parts.push(format!(
            "#[fg={},bold]⏳{}#[default]",
            crate::theme::WAIT,
            c.waiting
        ));
    }
    if c.working > 0 {
        parts.push(format!("#[fg={}]●{}#[default]", crate::theme::WORK, c.working));
    }
    if c.done_unseen > 0 {
        parts.push(format!(
            "#[fg={},bold]✓{}#[default]",
            crate::theme::DONE,
            c.done_unseen
        ));
    }
    parts.join(" ")
}
