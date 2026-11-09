use grove::doctor::{Facts, HookStatus, Level, Os, diagnose};
use predicates::prelude::*;

/// A machine where everything is set up.
fn healthy() -> Facts {
    Facts {
        os: Os::Linux,
        terminal: Some("ghostty".into()),
        tmux_version: Some((3, 4)),
        inside_tmux: true,
        tmux_conf_written: true,
        tmux_conf_loaded: Some(true),
        client_rgb: Some(true),
        config_exists: true,
        roots: vec![("~/dev".into(), true, 3)],
        hooks: HookStatus {
            missing: vec![],
            stale_path: None,
        },
        missing_tools: vec![],
        state_writable: true,
    }
}

fn find<'a>(f: &'a [grove::doctor::Finding], what: &str) -> &'a grove::doctor::Finding {
    f.iter()
        .find(|x| x.what.contains(what))
        .unwrap_or_else(|| panic!("no finding about {what:?}: {f:#?}"))
}

#[test]
fn healthy_machine_has_no_problems() {
    let f = diagnose(&healthy());
    assert!(f.iter().all(|x| x.level == Level::Ok), "{f:#?}");
}

#[test]
fn old_or_missing_tmux_fails() {
    let mut facts = healthy();
    facts.tmux_version = Some((3, 2));
    assert_eq!(find(&diagnose(&facts), "tmux").level, Level::Fail);
    facts.tmux_version = None;
    assert_eq!(find(&diagnose(&facts), "tmux").level, Level::Fail);
}

#[test]
fn conf_written_but_not_sourced_says_which_line_to_add() {
    let mut facts = healthy();
    facts.tmux_conf_loaded = Some(false);
    let f = diagnose(&facts);
    let x = find(&f, "grove.tmux.conf");
    assert_eq!(x.level, Level::Fail);
    assert!(x.fix.as_deref().unwrap().contains("source-file"), "{x:?}");
}

#[test]
fn no_true_colour_warns_with_the_terminal_features_line() {
    let mut facts = healthy();
    facts.client_rgb = Some(false);
    let x = diagnose(&facts)
        .into_iter()
        .find(|x| x.what.contains("colour"))
        .unwrap();
    assert_eq!(x.level, Level::Warn);
    assert!(x.fix.unwrap().contains("terminal-features"));
}

#[test]
fn alt_key_hint_depends_on_terminal_and_os() {
    let mut facts = healthy();
    facts.os = Os::MacOs;
    let hint = |facts: &Facts| find(&diagnose(facts), "⌥").fix.clone().unwrap_or_default();
    assert!(hint(&facts).contains("macos-option-as-alt = true"));
    facts.terminal = Some("iTerm.app".into());
    assert!(hint(&facts).contains("Esc+"));
    facts.terminal = Some("kitty".into());
    assert!(hint(&facts).contains("macos_option_as_alt"));
    facts.terminal = Some("WezTerm".into());
    assert!(hint(&facts).contains("send_composed_key_when_left_alt_is_pressed"));
    // Linux terminals send Alt by default
    facts.os = Os::Linux;
    assert_eq!(find(&diagnose(&facts), "⌥").level, Level::Ok);
}

#[test]
fn hooks_missing_or_pointing_at_an_old_binary() {
    let mut facts = healthy();
    facts.hooks.missing = vec!["Stop".into(), "PreToolUse".into()];
    let x = find(&diagnose(&facts), "hooks").clone();
    assert_eq!(x.level, Level::Fail);
    assert!(x.what.contains("Stop") && x.fix.unwrap().contains("grove init"));

    let mut facts = healthy();
    facts.hooks.stale_path = Some("/old/grove".into());
    let x = find(&diagnose(&facts), "hooks").clone();
    assert_eq!(x.level, Level::Fail);
    assert!(x.what.contains("/old/grove"));
}

#[test]
fn missing_roots_and_tools() {
    let mut facts = healthy();
    facts.roots = vec![("~/gone".into(), false, 0), ("~/dev".into(), true, 0)];
    facts.missing_tools = vec!["lazygit".into(), "gh".into()];
    let f = diagnose(&facts);
    assert_eq!(find(&f, "~/gone").level, Level::Fail);
    assert_eq!(find(&f, "~/dev").level, Level::Warn, "a root with no repos");
    assert_eq!(find(&f, "lazygit").level, Level::Warn);
}

#[test]
fn doctor_binary_reports_and_fails_on_a_blank_machine() {
    let home = tempfile::tempdir().unwrap();
    assert_cmd::Command::cargo_bin("grove")
        .unwrap()
        .arg("doctor")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("GROVE_TMUX_SOCKET", "grove-doctor-test-none")
        .env_remove("TMUX")
        .env_remove("CLAUDE_CONFIG_DIR")
        .assert()
        .failure()
        .stdout(predicate::str::contains("grove init").and(predicate::str::contains("config")));
}

#[test]
fn hook_status_reads_what_grove_init_writes() {
    use grove::doctor::hook_status;
    use grove::init::{HOOK_EVENTS, merge_claude_hooks};
    let installed = merge_claude_hooks(serde_json::json!({}), "/bin/grove");

    let s = hook_status(&installed, "/bin/grove");
    assert!(s.missing.is_empty() && s.stale_path.is_none(), "{s:?}");

    let s = hook_status(&installed, "/new/grove");
    assert_eq!(s.stale_path.as_deref(), Some("/bin/grove"));

    let s = hook_status(
        &serde_json::json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "say hi" }] }] } }),
        "/bin/grove",
    );
    assert_eq!(
        s.missing.len(),
        HOOK_EVENTS.len(),
        "a user's own hooks don't count as grove's"
    );
}
