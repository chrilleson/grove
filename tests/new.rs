mod common;
use common::*;
use grove::store::Store;
use predicates::prelude::*;

#[test]
fn new_creates_worktree_setup_and_session() {
    if !has_tmux() {
        eprintln!("skipping: no tmux");
        return;
    }
    let w = World::new();
    w.write_config("");
    let repo = w.git_repo("api");
    std::fs::write(repo.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(
        repo.join(".grove.toml"),
        "[setup]\ncopy = [\".env\", \"missing.txt\"]\nrun = [\"echo $PORT $API_PORT > ports.txt\"]\n\
         [ports]\nPORT = 0\nAPI_PORT = 1\n",
    )
    .unwrap();

    w.grove()
        .args(["new", "api", "fix-login", "--brief", "Fix it", "--detach"])
        .assert()
        .success();

    let wt = w.root().join(".worktrees/api/fix-login");
    assert_eq!(git(&wt, &["branch", "--show-current"]), "grove/fix-login");
    // setup runs in the shell pane, so the session exists before it finishes
    let ports = wait_for(|| {
        std::fs::read_to_string(wt.join("ports.txt"))
            .ok()
            .filter(|s| !s.is_empty())
    });
    assert_eq!(ports.as_deref().map(str::trim), Some("20000 20001"));
    assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "SECRET=1\n");
    // the brief is a file in the worktree, hidden from git
    assert_eq!(
        std::fs::read_to_string(wt.join(".grove/brief.md")).unwrap(),
        "Fix it\n"
    );
    assert!(!git(&wt, &["status", "--porcelain"]).contains(".grove"));

    let store = Store::open(&w.state_db(), 20000, 10).unwrap();
    let t = store.get("api/fix-login").unwrap().expect("task stored");
    assert_eq!(
        (t.number, t.port_base, t.session.as_str()),
        (1, 20000, "api/fix-login")
    );

    let panes = w.tmux(&["list-panes", "-t", "=api/fix-login", "-F", "#{pane_current_path}"]);
    assert_eq!(panes.lines().count(), 3, "sidebar | agent | shell: {panes}");
    let env = w.tmux(&["show-environment", "-t", "=api/fix-login"]);
    assert!(env.contains("GROVE_TASK=api/fix-login"), "{env}");
    assert!(env.contains("GROVE_PORT=20000"), "{env}");
    assert!(env.contains("PORT=20000"), "{env}");

    // after setup, the agent is started on the brief
    let agent = wait_for(|| {
        let out = w.tmux(&["capture-pane", "-J", "-p", "-t", "=api/fix-login:0.1"]);
        out.contains("agent Read .grove/brief.md and start.")
            .then_some(out)
    });
    assert!(agent.is_some(), "agent never started on the brief");

    // panes carry a role and a header title
    let panes = w.tmux(&[
        "list-panes",
        "-t",
        "=api/fix-login",
        "-F",
        "#{@grove_role}|#{@grove_title}",
    ]);
    let lines: Vec<_> = panes.lines().collect();
    assert_eq!(lines[0], "sidebar|tasks");
    assert_eq!(lines[1], "agent|claude · api/fix-login");
    assert!(lines[2].starts_with("shell|"), "{panes}");
    assert_eq!(
        w.tmux(&[
            "show-options",
            "-wv",
            "-t",
            "=api/fix-login:",
            "pane-border-status"
        ]),
        "top"
    );

    // sidebar is the configured width and stays so when the window resizes
    let widths = || w.tmux(&["list-panes", "-t", "=api/fix-login", "-F", "#{pane_width}"]);
    assert_eq!(widths().lines().next(), Some("32"), "{}", widths());
    w.tmux(&["resize-window", "-t", "=api/fix-login", "-x", "150", "-y", "40"]);
    assert!(
        wait_for(|| (widths().lines().next() == Some("32")).then_some(())).is_some(),
        "sidebar drifted after resize: {}",
        widths()
    );

    // second task gets the next number and port block; duplicate refused
    w.grove()
        .args(["new", "api", "other", "--detach"])
        .assert()
        .success();
    assert_eq!(store.get("api/other").unwrap().unwrap().port_base, 20010);
    w.grove()
        .args(["new", "api", "other", "--detach"])
        .assert()
        .failure();

    w.grove()
        .arg("ls")
        .assert()
        .success()
        .stdout(predicates::str::contains("api/fix-login").and(predicates::str::contains("api/other")));
}

#[test]
fn unknown_repo_is_an_error() {
    let w = World::new();
    w.write_config("");
    w.grove()
        .args(["new", "nope", "x", "--detach"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("nope"));
}

#[test]
fn rm_removes_clean_task_and_refuses_dirty_one_without_force() {
    if !has_tmux() {
        return;
    }
    let w = World::new();
    w.write_config("");
    let repo = w.git_repo("api");
    w.grove()
        .args(["new", "api", "clean", "--detach"])
        .assert()
        .success();
    w.grove()
        .args(["new", "api", "dirty", "--detach"])
        .assert()
        .success();
    let dirty_wt = w.root().join(".worktrees/api/dirty");
    std::fs::write(dirty_wt.join("wip.txt"), "unsaved").unwrap();

    w.grove().args(["rm", "api/clean"]).assert().success();
    assert!(!w.root().join(".worktrees/api/clean").exists());
    assert!(!git(&repo, &["branch", "--list", "clean"]).contains("clean"));
    assert!(
        !w.tmux(&["list-sessions", "-F", "#{session_name}"])
            .contains("api/clean")
    );

    w.grove()
        .args(["rm", "api/dirty"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("uncommitted"));
    assert!(dirty_wt.exists());

    w.grove().args(["rm", "api/dirty", "--force"]).assert().success();
    assert!(!dirty_wt.exists());
    w.grove().arg("ls").assert().success().stdout("");
}

#[test]
fn rm_refuses_commits_that_exist_only_on_the_task_branch() {
    if !has_tmux() {
        return;
    }
    let w = World::new();
    w.write_config("");
    w.git_repo("api");
    w.grove()
        .args(["new", "api", "work", "--detach"])
        .assert()
        .success();
    let wt = w.root().join(".worktrees/api/work");
    std::fs::write(wt.join("new.txt"), "x").unwrap();
    git(&wt, &["add", "."]);
    git(
        &wt,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "work",
        ],
    );

    w.grove()
        .args(["rm", "api/work"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("1 commit"));
    w.grove().args(["rm", "api/work", "--force"]).assert().success();
}
