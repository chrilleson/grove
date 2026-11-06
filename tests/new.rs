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
    assert_eq!(git(&wt, &["branch", "--show-current"]), "fix-login");
    assert_eq!(
        std::fs::read_to_string(wt.join(".env")).unwrap(),
        "SECRET=1\n"
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("ports.txt"))
            .unwrap()
            .trim(),
        "20000 20001"
    );

    let store = Store::open(&w.state_db(), 20000, 10).unwrap();
    let t = store.get("api/fix-login").unwrap().expect("task stored");
    assert_eq!(
        (t.number, t.port_base, t.session.as_str()),
        (1, 20000, "api-fix-login")
    );

    let panes = w.tmux(&[
        "list-panes",
        "-t",
        "=api-fix-login",
        "-F",
        "#{pane_current_path}",
    ]);
    assert_eq!(panes.lines().count(), 3, "sidebar | agent | shell: {panes}");
    let env = w.tmux(&["show-environment", "-t", "=api-fix-login"]);
    assert!(env.contains("GROVE_TASK=api/fix-login"), "{env}");
    assert!(env.contains("GROVE_PORT=20000"), "{env}");
    assert!(env.contains("PORT=20000"), "{env}");

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

    w.grove().arg("ls").assert().success().stdout(
        predicates::str::contains("api/fix-login").and(predicates::str::contains("api/other")),
    );
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
