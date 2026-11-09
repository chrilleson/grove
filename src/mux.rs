//! The pane engine. tmux is the only implementation; the trait keeps tmux
//! details in one place and lets other muxes slot in later.

use crate::theme;
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
    /// Header text above the agent and shell panes.
    pub agent_title: String,
    pub shell_title: String,
    /// Initial right-hand text of the agent header; the hook keeps it current.
    pub agent_state: String,
    /// Runs in the shell pane before handing it to the user's shell (setup).
    pub shell_first: Option<String>,
}

/// What a pane in a task session is for; stored on the pane as `@grove_role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Sidebar,
    Agent,
    Shell,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Sidebar => "sidebar",
            Role::Agent => "agent",
            Role::Shell => "shell",
        }
    }
}

pub trait Mux {
    fn session_exists(&self, name: &str) -> bool;
    fn create_task_session(&self, spec: &SessionSpec) -> Result<()>;
    fn kill_session(&self, name: &str) -> Result<()>;
    /// Type a command into a pane's shell and press Enter.
    fn type_into(&self, session: &str, role: Role, command: &str) -> Result<()>;
    /// Focus a pane by role.
    fn focus(&self, session: &str, role: Role) -> Result<()>;
    /// Sessions that some client is currently looking at.
    fn viewed_sessions(&self) -> Vec<String>;
    /// The session of the client that invoked us, when run inside tmux.
    fn current_session(&self) -> Option<String>;
    /// Switch the current client, or attach when not inside tmux.
    fn switch_to(&self, name: &str) -> Result<()>;
    fn message(&self, text: &str);
    /// The program running in a pane (e.g. `zsh`, `nvim`).
    fn pane_command(&self, session: &str, role: Role) -> Result<String>;
    /// A window in the session running `cmd`.
    fn new_window(&self, session: &str, cwd: &str, cmd: &str) -> Result<()>;
    /// A bordered popup over the current client running `cmd`.
    fn popup(&self, cwd: &str, title: &str, cmd: &str) -> Result<()>;
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
        // -u: values like "·" and "⏳" must survive even without a UTF-8 locale.
        c.arg("-u");
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

    fn pane(&self, session: &str, role: Role) -> Result<String> {
        let out = self.run(&[
            "list-panes",
            "-s",
            "-t",
            &exact(session),
            "-F",
            "#{pane_id} #{@grove_role}",
        ])?;
        out.lines()
            .find_map(|l| {
                l.split_once(' ')
                    .filter(|(_, r)| *r == role.as_str())
                    .map(|(id, _)| id.to_string())
            })
            .with_context(|| format!("no {} pane in {session}", role.as_str()))
    }

    /// Set a pane's header text (the sidebar keeps its `tasks · N` current).
    pub fn set_pane_title(&self, pane: &str, title: &str) {
        let _ = self.run(&["set-option", "-p", "-t", pane, "@grove_title", title]);
    }

    /// Show a task's state in its agent pane header. Called only on a state change.
    pub fn set_pane_state(&self, pane: &str, styled: &str) {
        let _ = self.run(&["set-option", "-p", "-t", pane, "@grove_state", styled]);
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

        let shell_size = format!("{}%", s.shell_percent.clamp(10, 90));
        let mut args = vec![
            "split-window",
            "-h",
            "-d",
            "-t",
            &agent,
            "-l",
            &shell_size,
            "-c",
            &cwd,
        ];
        args.extend(["-P", "-F", "#{pane_id}"]);
        let shell = self.run(&args)?;

        for (pane, role, title) in [
            (&sidebar, Role::Sidebar, "tasks"),
            (&agent, Role::Agent, s.agent_title.as_str()),
            (&shell, Role::Shell, s.shell_title.as_str()),
        ] {
            self.run(&["set-option", "-p", "-t", pane, "@grove_role", role.as_str()])?;
            self.run(&["set-option", "-p", "-t", pane, "@grove_title", title])?;
        }
        self.run(&["set-option", "-p", "-t", &agent, "@grove_state", &s.agent_state])?;

        // Setup output streams in the shell pane, then it becomes the user's
        // shell. Started only now, so it can find the other panes by role.
        if let Some(cmd) = &s.shell_first {
            let cmd = format!("{cmd}; exec \"$SHELL\"");
            self.run(&["respawn-pane", "-k", "-t", &shell, "-c", &cwd, &cmd])?;
        }

        let win = format!("{}:", exact(&s.name));
        let header = format!(
            "#{{?pane_active,#[fg={dim}],#[fg={faint}]}} #{{E:@grove_title}} #[align=right]#{{@grove_state}} ",
            dim = theme::DIM,
            faint = theme::FAINT
        );
        for (opt, val) in [
            ("pane-border-status", "top".to_string()),
            ("pane-border-lines", "single".to_string()),
            ("pane-border-style", format!("fg={}", theme::LINE)),
            ("pane-active-border-style", format!("fg={}", theme::WORK)),
            ("pane-border-format", header),
        ] {
            self.run(&["set-option", "-w", "-t", &win, opt, &val])?;
        }

        let width = s.sidebar_width.to_string();
        self.run(&["resize-pane", "-t", &sidebar, "-x", &width])?;
        // Keep the sidebar fixed when the terminal or another client resizes the window.
        let keep = format!("resize-pane -t {sidebar} -x {width}");
        self.run(&["set-hook", "-w", "-t", &win, "window-resized", &keep])?;
        self.run(&["select-pane", "-t", &shell])?;
        Ok(())
    }

    fn type_into(&self, session: &str, role: Role, command: &str) -> Result<()> {
        let pane = self.pane(session, role)?;
        self.run(&["send-keys", "-t", &pane, "-l", command])?;
        self.run(&["send-keys", "-t", &pane, "Enter"])?;
        Ok(())
    }

    fn pane_command(&self, session: &str, role: Role) -> Result<String> {
        let pane = self.pane(session, role)?;
        self.run(&["display-message", "-p", "-t", &pane, "#{pane_current_command}"])
    }

    fn new_window(&self, session: &str, cwd: &str, cmd: &str) -> Result<()> {
        self.run(&[
            "new-window",
            "-t",
            &format!("{}:", exact(session)),
            "-c",
            cwd,
            cmd,
        ])
        .map(|_| ())
    }

    fn popup(&self, cwd: &str, title: &str, cmd: &str) -> Result<()> {
        let border = format!("fg={}", theme::LINE_HI);
        let args = [
            "display-popup",
            "-E",
            "-w",
            "90%",
            "-h",
            "90%",
            "-b",
            "rounded",
            "-S",
            &border,
        ];
        let mut args = args.to_vec();
        args.extend(["-T", title, "-d", cwd, cmd]);
        self.run(&args).map(|_| ())
    }

    fn focus(&self, session: &str, role: Role) -> Result<()> {
        let pane = self.pane(session, role)?;
        self.run(&["select-pane", "-t", &pane]).map(|_| ())
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
