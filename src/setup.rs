//! Per-repo setup: `~/.config/grove/repos/<repo>.toml` over the repo's
//! committed `.grove.toml`, merged key by key (user wins).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    pub base: Option<String>,
    pub team: Option<bool>,
    pub setup: Setup,
    /// Variable name → offset from `GROVE_PORT`.
    pub ports: BTreeMap<String, i64>,
    pub teardown: Teardown,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Setup {
    pub copy: Vec<String>,
    pub symlink: Vec<String>,
    pub run: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Teardown {
    pub run: Vec<String>,
}

impl RepoConfig {
    pub fn merged(committed: Option<&str>, user: Option<&str>) -> Result<RepoConfig> {
        let parse = |s: Option<&str>, what: &str| -> Result<toml::Table> {
            s.map(|s| {
                s.parse::<toml::Table>()
                    .with_context(|| format!("parsing {what}"))
            })
            .transpose()
            .map(Option::unwrap_or_default)
        };
        let mut merged = parse(committed, ".grove.toml")?;
        deep_merge(&mut merged, parse(user, "user repo config")?);
        Ok(toml::Value::Table(merged).try_into()?)
    }

    pub fn load(repo_name: &str, repo_dir: &Path) -> Result<RepoConfig> {
        let read = |p: &Path| std::fs::read_to_string(p).ok();
        let user_path = crate::paths::config_dir()
            .join("repos")
            .join(format!("{repo_name}.toml"));
        Self::merged(
            read(&repo_dir.join(".grove.toml")).as_deref(),
            read(&user_path).as_deref(),
        )
    }

    /// `GROVE_PORT` plus every mapped port variable.
    pub fn port_env(&self, port_base: i64) -> Vec<(String, String)> {
        let mut env = vec![("GROVE_PORT".to_string(), port_base.to_string())];
        env.extend(
            self.ports
                .iter()
                .map(|(k, off)| (k.clone(), (port_base + off).to_string())),
        );
        env
    }
}

fn deep_merge(base: &mut toml::Table, over: toml::Table) {
    for (k, v) in over {
        match (base.get_mut(&k), v) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => deep_merge(b, o),
            (_, v) => {
                base.insert(k, v);
            }
        }
    }
}

/// Copy/symlink files from the main checkout, then run commands in the
/// worktree. Returns the first failing command, if any.
/// Print a quiet `grove:` progress line, as in the design (image 07).
pub fn say(msg: &str) {
    println!("{}", dim(&format!("grove: {msg}")));
}

fn dim(s: &str) -> String {
    let v = u32::from_str_radix(crate::theme::DIM.trim_start_matches('#'), 16).unwrap_or(0x888888);
    format!(
        "\x1b[38;2;{};{};{}m{s}\x1b[0m",
        v >> 16,
        (v >> 8) & 0xff,
        v & 0xff
    )
}

/// Copy/symlink files from the main checkout, then run commands in the
/// worktree. Each command's output is shown only if it fails. Returns the
/// first failing command, if any.
pub fn run(
    cfg: &RepoConfig,
    main: &Path,
    worktree: &Path,
    env: &[(String, String)],
) -> Result<Option<String>> {
    let (mut copied, mut linked, mut missing) = (vec![], vec![], vec![]);
    for f in &cfg.setup.copy {
        let (src, dst) = (main.join(f), worktree.join(f));
        if !src.exists() {
            missing.push(f.as_str());
            continue;
        }
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        copy_recursive(&src, &dst).with_context(|| format!("copying {f}"))?;
        copied.push(f.as_str());
    }
    for f in &cfg.setup.symlink {
        let (src, dst) = (main.join(f), worktree.join(f));
        if !src.exists() {
            missing.push(f.as_str());
            continue;
        }
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        if dst.symlink_metadata().is_err() {
            std::os::unix::fs::symlink(&src, &dst).with_context(|| format!("linking {f}"))?;
        }
        linked.push(f.as_str());
    }
    let mut parts = vec![];
    if !copied.is_empty() {
        parts.push(format!("copied {}", copied.join(", ")));
    }
    if !linked.is_empty() {
        parts.push(format!("symlinked {}", linked.join(", ")));
    }
    if !parts.is_empty() {
        say(&parts.join(" · "));
    }
    if !missing.is_empty() {
        say(&format!(
            "not in the main checkout, skipped: {}",
            missing.join(", ")
        ));
    }

    for cmd in &cfg.setup.run {
        use std::io::Write;
        print!("{}", dim(&format!("grove: {cmd} …")));
        let _ = std::io::stdout().flush();
        let started = std::time::Instant::now();
        let out = Command::new("sh")
            .args(["-c", cmd])
            .current_dir(worktree)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::null())
            .output();
        let secs = started.elapsed().as_secs_f32();
        match out {
            Ok(o) if o.status.success() => println!("{}", dim(&format!(" done ({secs:.1}s)"))),
            Ok(o) => {
                println!(" failed");
                let _ = std::io::stdout().write_all(&o.stdout);
                let _ = std::io::stdout().write_all(&o.stderr);
                return Ok(Some(cmd.clone()));
            }
            Err(e) => {
                println!(" failed: {e}");
                return Ok(Some(cmd.clone()));
            }
        }
    }
    Ok(None)
}

fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}
