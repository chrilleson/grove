//! Desktop notifications. Best effort: a missing tool is silent.

use std::process::{Command, Stdio};

pub fn enabled(config_notify: bool) -> bool {
    config_notify
        && std::env::var("GROVE_NOTIFY")
            .map(|v| v != "0")
            .unwrap_or(true)
}

pub fn send(title: &str, body: &str) {
    let mut cmd = if cfg!(target_os = "macos") {
        let script = format!(
            "display notification {} with title {}",
            applescript_string(body),
            applescript_string(title)
        );
        let mut c = Command::new("osascript");
        c.args(["-e", &script]);
        c
    } else {
        let mut c = Command::new("notify-send");
        c.args(["--app-name=grove", title, body]);
        c
    };
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
