//! XDG locations, honoured on macOS too (spec: Platform).

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("grove")
}

pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("grove")
}

pub fn state_db() -> PathBuf {
    state_dir().join("state.db")
}

pub fn hook_log() -> PathBuf {
    state_dir().join("hooks.jsonl")
}

/// Expand a leading `~/`.
pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}
