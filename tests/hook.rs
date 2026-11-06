use assert_cmd::Command;
use grove::hook::signal_for;
use grove::store::{NewTask, Signal, State, Store};
use serde_json::json;
use std::io::Write;

#[test]
fn events_map_to_signals() {
    assert_eq!(
        signal_for("UserPromptSubmit", &json!({})),
        Some(Signal::Working)
    );
    assert_eq!(
        signal_for("PreToolUse", &json!({"tool_name": "Bash"})),
        Some(Signal::Working)
    );
    assert_eq!(
        signal_for(
            "Notification",
            &json!({"notification_type": "permission_prompt"})
        ),
        Some(Signal::Waiting)
    );
    assert_eq!(
        signal_for("Notification", &json!({"notification_type": "idle_prompt"})),
        Some(Signal::Waiting)
    );
    assert_eq!(
        signal_for(
            "Notification",
            &json!({"notification_type": "auth_success"})
        ),
        None
    );
    assert_eq!(
        signal_for("PermissionRequest", &json!({})),
        Some(Signal::Waiting)
    );
    assert_eq!(signal_for("SessionEnd", &json!({})), Some(Signal::Idle));
    assert_eq!(signal_for("SubagentStart", &json!({"agent_id": "x"})), None);
    assert_eq!(signal_for("SubagentStop", &json!({"agent_id": "x"})), None);
    assert_eq!(signal_for("TeammateIdle", &json!({})), None);
}

#[test]
fn stop_takes_first_line_of_last_assistant_message() {
    let sig = signal_for(
        "Stop",
        &json!({"last_assistant_message": "Fixed the login bug.\n\nDetails…"}),
    );
    assert_eq!(
        sig,
        Some(Signal::Done {
            info: Some("Fixed the login bug.".into())
        })
    );
}

#[test]
fn stop_falls_back_to_transcript() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        f,
        r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#
    )
    .unwrap();
    writeln!(f, r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"First answer"}}]}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash"}},{{"type":"text","text":"Updated README"}}]}}}}"#).unwrap();
    let sig = signal_for("Stop", &json!({"transcript_path": f.path()}));
    assert_eq!(
        sig,
        Some(Signal::Done {
            info: Some("Updated README".into())
        })
    );
}

#[test]
fn hook_binary_updates_state_and_logs_payload() {
    let state = tempfile::tempdir().unwrap();
    let db = state.path().join("grove/state.db");
    let store = Store::open(&db, 20000, 10).unwrap();
    store
        .add_task(
            NewTask {
                repo: "api".into(),
                root: "/r".into(),
                slug: "a".into(),
                worktree: "/w".into(),
                team: false,
            },
            1,
        )
        .unwrap();

    let run = |event: &str, payload: serde_json::Value| {
        Command::cargo_bin("grove")
            .unwrap()
            .args(["hook", event])
            .env("XDG_STATE_HOME", state.path())
            .env("XDG_CONFIG_HOME", state.path().join("cfg"))
            .env("GROVE_TASK", "api/a")
            .env("GROVE_NOTIFY", "0")
            .env_remove("TMUX")
            .write_stdin(payload.to_string())
            .assert()
            .success()
            .stdout("");
    };

    run(
        "PermissionRequest",
        json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash"}),
    );
    assert_eq!(store.get("api/a").unwrap().unwrap().state, State::Waiting);

    Command::cargo_bin("grove")
        .unwrap()
        .arg("status")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_CONFIG_HOME", state.path().join("cfg"))
        .assert()
        .success()
        .stdout("⏳1\n");

    run(
        "Stop",
        json!({"hook_event_name": "Stop", "last_assistant_message": "Done."}),
    );
    let t = store.get("api/a").unwrap().unwrap();
    assert_eq!(
        (t.state, t.seen, t.info.as_deref()),
        (State::Done, false, Some("Done."))
    );

    let log = std::fs::read_to_string(state.path().join("grove/hooks.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 2);
    assert!(log.contains("\"tool_name\":\"Bash\""));
}

#[test]
fn hook_never_fails_claude() {
    let state = tempfile::tempdir().unwrap();
    for task in [None, Some("nope/x")] {
        let mut cmd = Command::cargo_bin("grove").unwrap();
        cmd.args(["hook", "Stop"])
            .env("XDG_STATE_HOME", state.path())
            .write_stdin("not json");
        match task {
            Some(t) => cmd.env("GROVE_TASK", t),
            None => cmd.env_remove("GROVE_TASK"),
        };
        cmd.assert().success().stdout("");
    }
}
