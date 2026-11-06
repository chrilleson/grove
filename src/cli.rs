use crate::config::Config;
use crate::store::{Counts, Store};
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
    /// Status-line counter, e.g. `⏳2 ✓1`
    Status,
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Hook { event } => {
            crate::hook::run(&event);
            Ok(())
        }
        Cmd::Status => {
            println!("{}", format_counts(open_store(&Config::load()?)?.counts()?));
            Ok(())
        }
    }
}

pub fn open_store(config: &Config) -> Result<Store> {
    Store::open(
        &crate::paths::state_db(),
        config.ports.start,
        config.ports.block,
    )
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
