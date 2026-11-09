//! `grove doctor`: check that this machine is set up for grove, and say how
//! to fix what isn't. `gather` reads the machine; `diagnose` is pure, so the
//! advice can be tested without one.

use crate::config::Config;
use crate::theme;
use serde_json::Value;

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
}

#[derive(Debug, Clone, Default)]
pub struct HookStatus {
    /// Hook events with no grove entry.
    pub missing: Vec<String>,
    /// A grove entry calls a different grove binary (moved or reinstalled).
    pub stale_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Facts {
    pub os: Os,
    /// Terminal name as best we can tell: TERM_PROGRAM, or tmux's client_termname.
    pub terminal: Option<String>,
    pub tmux_version: Option<(u32, u32)>,
    pub inside_tmux: bool,
    pub tmux_conf_written: bool,
    /// Whether the running tmux server has sourced grove.tmux.conf; None when no server runs.
    pub tmux_conf_loaded: Option<bool>,
    /// Whether tmux sees true-colour support in this client; None outside tmux.
    pub client_rgb: Option<bool>,
    pub config_exists: bool,
    /// (root as written, exists, number of repos directly under it)
    pub roots: Vec<(String, bool, usize)>,
    pub hooks: HookStatus,
    pub missing_tools: Vec<String>,
    pub state_writable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub level: Level,
    pub what: String,
    pub fix: Option<String>,
}

fn ok(what: impl Into<String>) -> Finding {
    Finding {
        level: Level::Ok,
        what: what.into(),
        fix: None,
    }
}
fn warn(what: impl Into<String>, fix: impl Into<String>) -> Finding {
    Finding {
        level: Level::Warn,
        what: what.into(),
        fix: Some(fix.into()),
    }
}
fn fail(what: impl Into<String>, fix: impl Into<String>) -> Finding {
    Finding {
        level: Level::Fail,
        what: what.into(),
        fix: Some(fix.into()),
    }
}

fn tmux_conf_path() -> String {
    crate::paths::tilde(&crate::paths::config_dir().join("grove.tmux.conf"))
}

/// How to make ⌥ send Alt in a given terminal on macOS.
fn alt_hint(terminal: Option<&str>) -> String {
    let t = terminal.unwrap_or("").to_lowercase();
    let ghostty = "Ghostty: add `macos-option-as-alt = true` to ~/.config/ghostty/config";
    let iterm = "iTerm2: Settings → Profiles → Keys → Left Option key: Esc+";
    let kitty = "kitty: add `macos_option_as_alt yes` to kitty.conf";
    let wezterm = "WezTerm: set `config.send_composed_key_when_left_alt_is_pressed = false` in wezterm.lua";
    if t.contains("ghostty") {
        ghostty.into()
    } else if t.contains("iterm") {
        iterm.into()
    } else if t.contains("kitty") {
        kitty.into()
    } else if t.contains("wezterm") {
        wezterm.into()
    } else {
        format!("{ghostty}\n{iterm}\n{kitty}\n{wezterm}")
    }
}

pub fn diagnose(f: &Facts) -> Vec<Finding> {
    let mut out = vec![];

    out.push(match f.tmux_version {
        None => fail("tmux not found", "install tmux 3.3 or newer"),
        Some((a, b)) if (a, b) < (3, 3) => fail(
            format!("tmux {a}.{b} is too old"),
            "grove needs tmux 3.3 or newer",
        ),
        Some((a, b)) => ok(format!("tmux {a}.{b}")),
    });

    let conf = tmux_conf_path();
    out.push(match (f.tmux_conf_written, f.tmux_conf_loaded) {
        (false, _) => fail(format!("{conf} not written"), "run `grove init`"),
        (true, Some(false)) => fail(
            format!("{conf} is not sourced by the running tmux"),
            format!(
                "add `source-file {conf}` at the end of your tmux.conf, then run `tmux source-file` on it"
            ),
        ),
        (true, None) => warn(
            format!("{conf} written; no tmux server running to check it is sourced"),
            "run `grove doctor` again inside tmux",
        ),
        (true, Some(true)) => ok(format!("{conf} loaded")),
    });

    match f.client_rgb {
        Some(false) => out.push(warn(
            "tmux isn't passing true colour, so grove's amber and green look wrong",
            "add `set -as terminal-features ',xterm*:RGB'` to your tmux.conf",
        )),
        Some(true) => out.push(ok("true colour")),
        None => out.push(ok(
            "true colour: not checked (run inside an attached tmux client)",
        )),
    }

    out.push(match f.os {
        Os::Linux => ok("⌥ sends Alt (Linux default)"),
        Os::MacOs => warn(
            "⌥ must send Alt for grove's keys (can't be checked from here)",
            alt_hint(f.terminal.as_deref()),
        ),
    });

    let config = crate::paths::tilde(&Config::path());
    if !f.config_exists {
        out.push(fail(format!("no config at {config}"), "run `grove init`"));
    } else if f.roots.is_empty() {
        out.push(fail("no roots configured", format!("set `roots` in {config}")));
    }
    for (root, exists, repos) in &f.roots {
        out.push(match (exists, repos) {
            (false, _) => fail(
                format!("root {root} does not exist"),
                format!("fix `roots` in {config}"),
            ),
            (true, 0) => warn(
                format!("root {root} has no git repos directly under it"),
                "roots hold repos one level deep",
            ),
            (true, n) => ok(format!("root {root}: {n} repo{}", if *n == 1 { "" } else { "s" })),
        });
    }

    out.push(if let Some(old) = &f.hooks.stale_path {
        fail(
            format!("Claude Code hooks call {old}, not this grove"),
            "run `grove init` to point them here",
        )
    } else if !f.hooks.missing.is_empty() {
        fail(
            format!("Claude Code hooks missing: {}", f.hooks.missing.join(", ")),
            "run `grove init`",
        )
    } else {
        ok("Claude Code hooks installed")
    });

    for tool in &f.missing_tools {
        let why = match tool.as_str() {
            "lazygit" => "needed for ⌥c",
            "code" => "needed for ⌥v",
            "gh" => "needed for PR status (later)",
            "notify-send" | "osascript" => "needed for desktop notifications",
            _ => "needed for ⌥e (your `editor` in config.toml)",
        };
        out.push(if tool == "claude" {
            fail(
                "claude not found",
                "install Claude Code, or set `agent_command` in config.toml",
            )
        } else {
            warn(format!("{tool} not found"), why)
        });
    }
    if f.missing_tools.is_empty() {
        out.push(ok("all tools found"));
    }

    if !f.state_writable {
        out.push(fail(
            format!("can't write {}", crate::paths::tilde(&crate::paths::state_dir())),
            "check permissions",
        ));
    }
    out
}

fn on_path(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

fn tmux(args: &[&str]) -> Option<String> {
    let mut c = Command::new("tmux");
    c.arg("-u");
    if let Ok(s) = std::env::var("GROVE_TMUX_SOCKET") {
        c.args(["-L", &s]);
    }
    let out = c.args(args).stdin(std::process::Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// "tmux 3.4", "tmux 3.3a", "tmux next-3.5" → (3, 4) …
fn parse_tmux_version(s: &str) -> Option<(u32, u32)> {
    let v = s.split_whitespace().nth(1)?.trim_start_matches("next-");
    let (major, rest) = v.split_once('.')?;
    let minor: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((major.parse().ok()?, minor.parse().ok()?))
}

pub fn hook_status(settings: &Value, exe: &str) -> HookStatus {
    let mut status = HookStatus::default();
    for event in crate::init::HOOK_EVENTS {
        let want = format!("{exe} hook {event}");
        let entries = settings["hooks"][*event].as_array().cloned().unwrap_or_default();
        let grove: Vec<&Value> = entries
            .iter()
            .filter(|e| crate::init::is_grove_entry(e))
            .collect();
        let current = grove.iter().any(|e| {
            e["hooks"]
                .as_array()
                .is_some_and(|hs| hs.iter().any(|h| h["command"].as_str() == Some(&want)))
        });
        if current {
            continue;
        }
        match grove.first().and_then(|e| e["hooks"][0]["command"].as_str()) {
            Some(cmd) => status.stale_path = Some(cmd.split(" hook ").next().unwrap_or(cmd).to_string()),
            None => status.missing.push(event.to_string()),
        }
    }
    status
}

pub fn gather() -> Facts {
    let inside_tmux = std::env::var_os("TMUX").is_some();
    let config_path = Config::path();
    let config_exists = config_path.exists();
    let config = Config::load().unwrap_or_default();
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    let terminal = if inside_tmux {
        tmux(&["display-message", "-p", "#{client_termname}"])
    } else {
        std::env::var("TERM_PROGRAM")
            .or_else(|_| std::env::var("TERM"))
            .ok()
    };
    let roots = config
        .roots
        .iter()
        .map(|r| {
            let dir = crate::paths::expand(r);
            let repos = std::fs::read_dir(&dir)
                .map(|it| it.flatten().filter(|e| e.path().join(".git").exists()).count())
                .unwrap_or(0);
            (r.clone(), dir.is_dir(), repos)
        })
        .collect();

    let settings: Value = std::fs::read_to_string(crate::init::claude_settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);

    let mut wanted: Vec<String> = vec![
        config
            .agent_command
            .split_whitespace()
            .next()
            .unwrap_or("claude")
            .to_string(),
        "lazygit".into(),
        config
            .editor
            .split_whitespace()
            .next()
            .unwrap_or("nvim")
            .to_string(),
        "code".into(),
        "gh".into(),
        if cfg!(target_os = "macos") {
            "osascript"
        } else {
            "notify-send"
        }
        .into(),
    ];
    wanted.dedup();

    let state_writable = std::fs::create_dir_all(crate::paths::state_dir()).is_ok()
        && crate::store::Store::open(&crate::paths::state_db(), 20000, 10).is_ok();

    Facts {
        os: if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        },
        terminal,
        tmux_version: tmux(&["-V"]).as_deref().and_then(parse_tmux_version),
        inside_tmux,
        tmux_conf_written: crate::paths::config_dir().join("grove.tmux.conf").exists(),
        tmux_conf_loaded: tmux(&["show-options", "-gqv", "@grove-loaded"]).map(|v| v == "1"),
        client_rgb: if inside_tmux {
            tmux(&["display-message", "-p", "#{client_termfeatures}"])
                // No attached client means no answer, not "no colour".
                .filter(|f| !f.is_empty())
                .map(|f| f.split(',').any(|x| x == "RGB"))
        } else {
            None
        },
        config_exists,
        roots,
        hooks: hook_status(&settings, &exe),
        missing_tools: wanted.into_iter().filter(|t| !on_path(t)).collect(),
        state_writable,
    }
}

fn paint(hex: &str, s: &str) -> String {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    format!(
        "\x1b[38;2;{};{};{}m{s}\x1b[0m",
        v >> 16,
        (v >> 8) & 0xff,
        v & 0xff
    )
}

/// Print the findings; returns whether anything failed.
pub fn run() -> bool {
    let findings = diagnose(&gather());
    for f in &findings {
        let (mark, color) = match f.level {
            Level::Ok => ("✓", theme::DONE),
            Level::Warn => ("!", theme::WAIT),
            Level::Fail => ("✗", theme::ERR),
        };
        println!("{} {}", paint(color, mark), f.what);
        if let Some(fix) = f.fix.as_deref().filter(|x| !x.is_empty()) {
            for line in fix.lines() {
                println!("  {}", paint(theme::DIM, &format!("→ {line}")));
            }
        }
    }
    findings.iter().any(|f| f.level == Level::Fail)
}
