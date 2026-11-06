//! User config: `$XDG_CONFIG_HOME/grove/config.toml`.

use crate::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub roots: Vec<String>,
    pub worktree_root: Option<String>,
    pub default_team: bool,
    pub agent_command: String,
    pub editor: String,
    pub notify: bool,
    pub sidebar_width: u16,
    pub ports: Ports,
    pub attention: Attention,
    pub keys: Keys,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Ports {
    pub start: i64,
    pub block: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Attention {
    pub blink_after: String,
    pub renotify_after: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Keys {
    pub next: String,
    pub dashboard: String,
    pub last: String,
    pub lazygit: String,
    pub editor: String,
    pub vscode: String,
    pub new: String,
    pub leader: String,
    /// Bind `M-1`..`M-9` to tasks by number.
    pub numbers: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            roots: vec![],
            worktree_root: None,
            default_team: false,
            agent_command: "claude".into(),
            editor: "nvim".into(),
            notify: true,
            sidebar_width: 32,
            ports: Ports::default(),
            attention: Attention::default(),
            keys: Keys::default(),
        }
    }
}

impl Default for Ports {
    fn default() -> Self {
        Ports {
            start: 20000,
            block: 10,
        }
    }
}

impl Default for Attention {
    fn default() -> Self {
        Attention {
            blink_after: "2m".into(),
            renotify_after: "10m".into(),
        }
    }
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            next: "M-w".into(),
            dashboard: "M-s".into(),
            last: "M-Tab".into(),
            lazygit: "M-c".into(),
            editor: "M-e".into(),
            vscode: "M-v".into(),
            new: "M-n".into(),
            leader: "M-g".into(),
            numbers: true,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        paths::config_dir().join("config.toml")
    }

    /// Missing file = defaults. A broken file is an error, so typos don't go unnoticed.
    pub fn load() -> Result<Config> {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|r| paths::expand(r)).collect()
    }
}
