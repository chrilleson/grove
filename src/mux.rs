//! The pane engine. tmux is the only implementation; the trait keeps tmux
//! details in one place and lets other muxes slot in later.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct SessionSpec {
    pub name: String,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// Runs as the left pane's command.
    pub sidebar_cmd: String,
    pub sidebar_width: u16,
    /// Typed into the middle pane's shell, so the pane survives the agent exiting.
    pub agent_cmd: String,
}

pub trait Mux {
    fn session_exists(&self, name: &str) -> bool;
    fn create_task_session(&self, spec: &SessionSpec) -> Result<()>;
    fn kill_session(&self, name: &str) -> Result<()>;
    /// Sessions that some client is currently looking at.
    fn viewed_sessions(&self) -> Vec<String>;
    /// The session of the client that invoked us, when run inside tmux.
    fn current_session(&self) -> Option<String>;
    /// Switch the current client, or attach when not inside tmux.
    fn switch_to(&self, name: &str) -> Result<()>;
    fn message(&self, text: &str);
}

pub struct Tmux {
    socket: Option<String>,
}

impl Tmux {
    /// `GROVE_TMUX_SOCKET` selects a private server (`tmux -L`); tests use it.
    pub fn from_env() -> Tmux {
        Tmux {
            socket: std::env::var("GROVE_TMUX_SOCKET")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    /// Whether there is a tmux server we are expected to talk to.
    pub fn reachable_from_env() -> bool {
        std::env::var_os("TMUX").is_some() || std::env::var_os("GROVE_TMUX_SOCKET").is_some()
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new("tmux");
        if let Some(s) = &self.socket {
            c.args(["-L", s]);
        }
        c
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let out = self
            .cmd()
            .args(args)
            .stdin(Stdio::null())
            .output()
            .context("running tmux")?;
        if !out.status.success() {
            bail!(
                "tmux {}: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }

    fn inside_tmux(&self) -> bool {
        std::env::var_os("TMUX").is_some()
    }
}

fn exact(name: &str) -> String {
    format!("={name}")
}

impl Mux for Tmux {
    fn session_exists(&self, name: &str) -> bool {
        self.run(&["has-session", "-t", &exact(name)]).is_ok()
    }

    fn create_task_session(&self, s: &SessionSpec) -> Result<()> {
        let cwd = s.cwd.to_string_lossy();
        let env: Vec<String> = s.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

        let mut args: Vec<&str> = vec![
            "new-session",
            "-d",
            "-s",
            &s.name,
            "-c",
            &cwd,
            "-x",
            "240",
            "-y",
            "60",
        ];
        for e in &env {
            args.extend(["-e", e]);
        }
        args.extend(["-P", "-F", "#{pane_id}", &s.sidebar_cmd]);
        let sidebar = self.run(&args)?;

        let agent = self.run(&[
            "split-window",
            "-h",
            "-t",
            &sidebar,
            "-c",
            &cwd,
            "-P",
            "-F",
            "#{pane_id}",
        ])?;
        self.run(&[
            "split-window",
            "-h",
            "-t",
            &agent,
            "-c",
            &cwd,
            "-P",
            "-F",
            "#{pane_id}",
        ])?;
        self.run(&[
            "resize-pane",
            "-t",
            &sidebar,
            "-x",
            &s.sidebar_width.to_string(),
        ])?;
        self.run(&["send-keys", "-t", &agent, "-l", &s.agent_cmd])?;
        self.run(&["send-keys", "-t", &agent, "Enter"])?;
        self.run(&["select-pane", "-t", &agent])?;
        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        self.run(&["kill-session", "-t", &exact(name)]).map(|_| ())
    }

    fn viewed_sessions(&self) -> Vec<String> {
        self.run(&["list-clients", "-F", "#{client_session}"])
            .map(|s| s.lines().map(String::from).collect())
            .unwrap_or_default()
    }

    fn current_session(&self) -> Option<String> {
        if !self.inside_tmux() {
            return None;
        }
        self.run(&["display-message", "-p", "#{client_session}"])
            .ok()
            .filter(|s| !s.is_empty())
    }

    fn switch_to(&self, name: &str) -> Result<()> {
        if self.inside_tmux() {
            self.run(&["switch-client", "-t", &exact(name)]).map(|_| ())
        } else {
            let status = self
                .cmd()
                .args(["attach-session", "-t", &exact(name)])
                .status()?;
            if !status.success() {
                bail!("tmux attach-session failed");
            }
            Ok(())
        }
    }

    fn message(&self, text: &str) {
        if self.inside_tmux() {
            let _ = self.run(&["display-message", text]);
        } else {
            println!("{text}");
        }
    }
}
