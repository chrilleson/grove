#![allow(dead_code)]
use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process::Command as Std;

/// An isolated grove world: temp HOME/XDG dirs, a root with one git repo,
/// and a private tmux server that is killed on drop.
pub struct World {
    pub dir: tempfile::TempDir,
    pub socket: String,
}

impl World {
    pub fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let socket = format!("grove-test-{}", dir.path().file_name().unwrap().to_string_lossy());
        let w = World { dir, socket };
        std::fs::create_dir_all(w.root()).unwrap();
        std::fs::create_dir_all(w.config_dir()).unwrap();
        w
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }
    pub fn root(&self) -> PathBuf {
        self.path().join("root")
    }
    pub fn config_dir(&self) -> PathBuf {
        self.path().join("config/grove")
    }
    pub fn state_db(&self) -> PathBuf {
        self.path().join("state/grove/state.db")
    }

    pub fn write_config(&self, extra: &str) {
        let cfg = format!(
            "roots = [\"{}\"]\nagent_command = \"echo agent\"\nnotify = false\n{extra}",
            self.root().display()
        );
        std::fs::write(self.config_dir().join("config.toml"), cfg).unwrap();
    }

    pub fn git_repo(&self, name: &str) -> PathBuf {
        let repo = self.root().join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("README.md"), "hi\n").unwrap();
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        );
        repo
    }

    pub fn grove(&self) -> Command {
        let mut c = Command::cargo_bin("grove").unwrap();
        c.env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join("config"))
            .env("XDG_STATE_HOME", self.path().join("state"))
            .env("GROVE_TMUX_SOCKET", &self.socket)
            .env("GROVE_NOTIFY", "0")
            .env_remove("TMUX")
            .env_remove("GROVE_TASK");
        c
    }

    pub fn tmux(&self, args: &[&str]) -> String {
        let out = Std::new("tmux")
            .args(["-u", "-L", &self.socket])
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = Std::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
    }
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

pub fn has_tmux() -> bool {
    Std::new("tmux").arg("-V").output().is_ok()
}

/// Poll for up to 3 s; tmux panes start asynchronously.
pub fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..30 {
        if let Some(v) = f() {
            return Some(v);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}
