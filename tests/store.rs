use grove::store::{NewTask, Signal, State, Store};

fn new_task(slug: &str) -> NewTask {
    NewTask {
        repo: "api".into(),
        root: "/r".into(),
        slug: slug.into(),
        worktree: format!("/r/.worktrees/api/{slug}"),
        team: false,
        branch: None,
    }
}

#[test]
fn new_tasks_get_lowest_free_number_and_port_block() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    let a = s.add_task(new_task("a"), 100).unwrap();
    let b = s.add_task(new_task("b"), 100).unwrap();
    assert_eq!((a.number, a.port_base), (1, 20000));
    assert_eq!((b.number, b.port_base), (2, 20010));
    assert_eq!(a.id, "api/a");
    assert_eq!(a.session, "api/a");
    assert_eq!(a.branch, "a");
    assert_eq!(
        grove::store::session_name(".dotfiles", "fix.login"),
        "dotfiles/fix_login"
    );
    assert_eq!(a.state, State::Idle);

    s.remove("api/a").unwrap();
    let c = s.add_task(new_task("c"), 100).unwrap();
    assert_eq!((c.number, c.port_base), (1, 20000));
}

#[test]
fn duplicate_slug_is_refused() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(new_task("a"), 100).unwrap();
    assert!(s.add_task(new_task("a"), 100).is_err());
}

#[test]
fn signals_move_state_and_report_needs_you_transitions() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(new_task("a"), 100).unwrap();

    let t = s.apply("api/a", Signal::Working, 110, false).unwrap().unwrap();
    assert_eq!(t.to, State::Working);
    assert!(!t.newly_needs_you);

    // same state again: no transition, nothing written
    assert!(s.apply("api/a", Signal::Working, 111, false).unwrap().is_none());

    let t = s.apply("api/a", Signal::Waiting, 120, false).unwrap().unwrap();
    assert!(t.newly_needs_you);

    let t = s
        .apply(
            "api/a",
            Signal::Done {
                info: Some("All tests pass".into()),
            },
            130,
            false,
        )
        .unwrap()
        .unwrap();
    assert!(t.newly_needs_you);
    let task = s.get("api/a").unwrap().unwrap();
    assert_eq!(task.state, State::Done);
    assert!(!task.seen);
    assert_eq!(task.info.as_deref(), Some("All tests pass"));
    assert_eq!(task.state_since, 130);
}

#[test]
fn done_while_viewed_is_born_seen() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(new_task("a"), 100).unwrap();
    let t = s
        .apply("api/a", Signal::Done { info: None }, 130, true)
        .unwrap()
        .unwrap();
    assert!(!t.newly_needs_you);
    assert!(s.get("api/a").unwrap().unwrap().seen);
}

#[test]
fn visiting_marks_done_seen() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(new_task("a"), 100).unwrap();
    s.apply("api/a", Signal::Done { info: None }, 130, false).unwrap();
    s.visit_session("api/a", 140).unwrap();
    let task = s.get("api/a").unwrap().unwrap();
    assert!(task.seen);
    assert!(!task.needs_you());
}

#[test]
fn rotation_is_waiting_then_unseen_done_oldest_first_skipping_current() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    for slug in ["a", "b", "c", "d", "e"] {
        s.add_task(new_task(slug), 100).unwrap();
    }
    s.apply("api/a", Signal::Done { info: None }, 150, false).unwrap();
    s.apply("api/b", Signal::Waiting, 170, false).unwrap();
    s.apply("api/c", Signal::Done { info: None }, 140, false).unwrap();
    s.apply("api/d", Signal::Waiting, 160, false).unwrap();
    s.apply("api/e", Signal::Working, 100, false).unwrap();

    let ids: Vec<_> = s.rotation(None).unwrap().into_iter().map(|t| t.id).collect();
    assert_eq!(ids, ["api/d", "api/b", "api/c", "api/a"]);

    let ids: Vec<_> = s
        .rotation(Some("api/d"))
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, ["api/b", "api/c", "api/a"]);

    let c = s.counts().unwrap();
    assert_eq!((c.waiting, c.done_unseen), (2, 2));
}

#[test]
fn task_branch_can_differ_from_slug() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    let mut t = new_task("a");
    t.branch = Some("grove/a".into());
    assert_eq!(s.add_task(t, 1).unwrap().branch, "grove/a");
}
