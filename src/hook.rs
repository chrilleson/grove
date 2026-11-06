//! `grove hook <Event>`: Claude Code hook → task state. Hot path (runs on
//! every tool call), so: one DB read, a write only on change, tmux and
//! notifications only on a transition. Never fails Claude. See ADR 5.

use crate::store::Signal;
use serde_json::Value;
use std::io::{BufRead, BufReader};

const INFO_MAX: usize = 80;

/// Map a hook event to what it says about the task. `None` = no change.
pub fn signal_for(event: &str, payload: &Value) -> Option<Signal> {
    match event {
        "UserPromptSubmit" | "PreToolUse" => Some(Signal::Working),
        "PermissionRequest" => Some(Signal::Waiting),
        "Notification" => match payload.get("notification_type").and_then(Value::as_str) {
            // Older Claude Code versions send no type; every notification was a prompt.
            None | Some("permission_prompt" | "idle_prompt" | "elicitation_dialog") => Some(Signal::Waiting),
            Some(_) => None,
        },
        "Stop" => Some(Signal::Done {
            info: last_message(payload).as_deref().and_then(one_line),
        }),
        "SessionEnd" => Some(Signal::Idle),
        // SubagentStart/Stop must never mark the task done; team events are
        // only logged until their payloads are confirmed.
        _ => None,
    }
}

fn last_message(payload: &Value) -> Option<String> {
    if let Some(m) = payload.get("last_assistant_message").and_then(Value::as_str) {
        return Some(m.to_string());
    }
    let path = payload.get("transcript_path")?.as_str()?;
    let file = std::fs::File::open(path).ok()?;
    let mut last = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array)
            && let Some(text) = blocks
                .iter()
                .rev()
                .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .and_then(|b| b.get("text"))
                .and_then(Value::as_str)
        {
            last = Some(text.to_string());
        }
    }
    last
}

fn one_line(s: &str) -> Option<String> {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(if line.chars().count() > INFO_MAX {
        let cut: String = line.chars().take(INFO_MAX - 1).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    })
}

/// Entry point. Swallows every error: a broken grove must never break Claude.
pub fn run(event: &str) {
    if let Err(e) = try_run(event) {
        let _ = log_line(&serde_json::json!({ "ts": crate::now(), "event": event, "error": e.to_string() }));
    }
}

fn try_run(event: &str) -> anyhow::Result<()> {
    use crate::config::Config;
    use crate::mux::{Mux, Tmux};
    use crate::store::Store;
    use std::io::Read;

    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let Ok(task_id) = std::env::var("GROVE_TASK") else {
        return Ok(());
    };
    let payload: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);

    if std::env::var("GROVE_LOG_HOOKS").map(|v| v != "0").unwrap_or(true) {
        log_line(&serde_json::json!({
            "ts": crate::now(),
            "task": task_id,
            "event": event,
            "payload": if payload.is_null() { Value::String(raw) } else { payload.clone() },
        }))?;
    }

    let Some(signal) = signal_for(event, &payload) else {
        return Ok(());
    };
    let config = Config::load().unwrap_or_default();
    let store = Store::open(&crate::paths::state_db(), config.ports.start, config.ports.block)?;

    // Only ask tmux when the answer matters (rare events), never on PreToolUse.
    let attention = matches!(signal, Signal::Waiting | Signal::Done { .. });
    let tmux = Tmux::from_env();
    let viewed = attention && Tmux::reachable_from_env() && {
        let session = store.get(&task_id)?.map(|t| t.session).unwrap_or_default();
        tmux.viewed_sessions().contains(&session)
    };

    if let Some(t) = store.apply(&task_id, signal, crate::now(), viewed)?
        && t.newly_needs_you
        && !viewed
        && crate::notify::enabled(config.notify)
    {
        let what = if t.to == crate::store::State::Waiting {
            "needs you"
        } else {
            "is done"
        };
        crate::notify::send(
            &format!("{} {what}", t.task.id),
            t.task.info.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

fn log_line(v: &Value) -> anyhow::Result<()> {
    use std::io::Write;
    let path = crate::paths::hook_log();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{v}")?;
    Ok(())
}
