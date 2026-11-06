mod common;
use common::*;
use grove::config::Config;
use grove::init::{merge_claude_hooks, tmux_conf};
use serde_json::json;

const EXE: &str = "/opt/bin/grove";

#[test]
fn tmux_conf_binds_alt_keys_and_hooks() {
    let conf = tmux_conf(&Config::default(), EXE);
    for needle in [
        "bind-key -n M-w run-shell -b '/opt/bin/grove next'",
        "bind-key -n M-Tab switch-client -l",
        "bind-key -n M-1 run-shell -b '/opt/bin/grove go 1'",
        "bind-key -n M-9 run-shell -b '/opt/bin/grove go 9'",
        "set-hook -ga client-session-changed",
    ] {
        assert!(conf.contains(needle), "missing {needle:?} in\n{conf}");
    }
    assert!(
        !conf.contains("M-h") && !conf.contains("M-l"),
        "window nav keys stay the user's"
    );
}

#[test]
fn number_binds_can_be_turned_off() {
    let mut c = Config::default();
    c.keys.numbers = false;
    assert!(!tmux_conf(&c, EXE).contains("M-1"));
}

#[test]
fn tmux_conf_is_valid_tmux_syntax() {
    if !has_tmux() {
        return;
    }
    let w = World::new();
    let path = w.path().join("grove.tmux.conf");
    std::fs::write(&path, tmux_conf(&Config::default(), EXE)).unwrap();
    w.tmux(&["-f", "/dev/null", "new-session", "-d", "-s", "x"]);
    let out = std::process::Command::new("tmux")
        .args(["-L", &w.socket, "source-file"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(w.tmux(&["list-keys", "-T", "root"]).contains("M-w"));
    // sourcing twice must not duplicate the status counter
    std::process::Command::new("tmux")
        .args(["-L", &w.socket, "source-file"])
        .arg(&path)
        .output()
        .unwrap();
    let right = w.tmux(&["show-options", "-gv", "status-right"]);
    assert_eq!(right.matches("grove status").count(), 1, "{right}");
}

#[test]
fn claude_hooks_merge_keeps_user_hooks_and_is_idempotent() {
    let user = json!({
        "model": "opus",
        "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }],
            "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-guard" }] }]
        }
    });
    let once = merge_claude_hooks(user.clone(), EXE);
    let twice = merge_claude_hooks(once.clone(), EXE);
    assert_eq!(once, twice);
    assert_eq!(once["model"], "opus");

    let stop = once["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0]["hooks"][0]["command"], "say done");
    assert_eq!(stop[1]["hooks"][0]["command"], "/opt/bin/grove hook Stop");

    let pre = once["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[1]["matcher"], "*");

    // a moved binary replaces the old grove entries rather than adding more
    let moved = merge_claude_hooks(once, "/new/grove");
    let stop = moved["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[1]["hooks"][0]["command"], "/new/grove hook Stop");
}

#[test]
fn init_dry_run_writes_nothing() {
    let w = World::new();
    w.grove()
        .args(["init", "--dry-run", "--root", "~/dev"])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("grove.tmux.conf")
                .and(predicates::str::contains("settings.json")),
        );
    assert!(!w.config_dir().join("config.toml").exists());
    assert!(!w.path().join(".claude/settings.json").exists());
}

#[test]
fn init_writes_config_tmux_conf_and_claude_settings() {
    let w = World::new();
    w.grove()
        .args(["init", "--root", "~/dev"])
        .assert()
        .success();
    let cfg = std::fs::read_to_string(w.config_dir().join("config.toml")).unwrap();
    assert!(cfg.contains("~/dev"), "{cfg}");
    assert!(w.config_dir().join("grove.tmux.conf").exists());
    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(w.path().join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["hooks"]["Stop"].is_array());
}

use predicates::prelude::*;
