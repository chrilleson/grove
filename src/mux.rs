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
    pub shell_percent: u8,
    /// Typed into the middle pane's shell, so the pane survives the agent exiting.
    pub agent_cmd: String,
    /// Extra environment for the agent pane only (e.g. `GROVE_BRIEF`).
    pub agent_env: Vec<(String, String)>,
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
            socket: std::env::var("GROVE_TMUX_SOCKET").ok().filter(|s| !s.is_empty()),
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

    /// Size of the client we're called from, else this terminal, else a big default.
    fn client_size(&self) -> (u16, u16) {
        if self.inside_tmux()
            && let Ok(out) = self.run(&["display-message", "-p", "#{client_width} #{client_height}"])
            && let Some((w, h)) = out.split_once(' ')
            && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
        {
            return (w, h);
        }
        ratatui::crossterm::terminal::size().unwrap_or((240, 60))
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
        // Build the layout at the size it will be seen at, so tmux doesn't
        // rescale the panes proportionally on attach.
        let (w, h) = self.client_size();
        let (w, h) = (w.to_string(), h.to_string());

        let mut args: Vec<&str> = vec!["new-session", "-d", "-s", &s.name, "-c", &cwd, "-x", &w, "-y", &h];
        for e in &env {
            args.extend(["-e", e]);
        }
        args.extend(["-P", "-F", "#{pane_id}", &s.sidebar_cmd]);
        let sidebar = self.run(&args)?;

        let agent_env: Vec<String> = s.agent_env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let mut args: Vec<&str> = vec![
            "split-window",
            "-h",
            "-t",
            &sidebar,
            "-c",
            &cwd,
            "-P",
            "-F",
            "#{pane_id}",
        ];
        for e in &agent_env {
            args.extend(["-e", e]);
        }
        let agent = self.run(&args)?;
        let shell_size = format!("{}%", s.shell_percent.clamp(10, 90));
        self.run(&["split-window", "-h", "-t", &agent, "-l", &shell_size, "-c", &cwd])?;

        let width = s.sidebar_width.to_string();
        self.run(&["resize-pane", "-t", &sidebar, "-x", &width])?;
        // Keep the sidebar fixed when the terminal or another client resizes the window.
        let keep = format!("resize-pane -t {sidebar} -x {width}");
        self.run(&[
            "set-hook",
            "-w",
            "-t",
            &format!("{}:", exact(&s.name)),
            "window-resized",
            &keep,
        ])?;

        self.run(&["send-keys", "-t", &agent, "-l", &s.agent_cmd])?;
        self.run(&["send-keys", "-t", &agent, "Enter"])?;
        self.run(&["select-pane", "-t", &agent])?;
        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        if !self.session_exists(name) {
            return Ok(());
        }
        // Move clients on this session to another one instead of detaching them.
        let _ = self.run(&["set-option", "-t", &exact(name), "detach-on-destroy", "off"]);
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
            let status = self.cmd().args(["attach-session", "-t", &exact(name)]).status()?;
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
