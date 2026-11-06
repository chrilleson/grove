use grove::sidebar::{age, task_lines};
use grove::store::{NewTask, Signal, Store, Task};

fn task(repo: &str, slug: &str) -> (Store, String) {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(
        NewTask {
            repo: repo.into(),
            root: "/r".into(),
            slug: slug.into(),
            worktree: "/w".into(),
            team: false,
        },
        0,
    )
    .unwrap();
    (s, format!("{repo}/{slug}"))
}

fn get(s: &Store, id: &str) -> Task {
    s.get(id).unwrap().unwrap()
}

#[test]
fn ages_are_compact() {
    assert_eq!(age(5), "5s");
    assert_eq!(age(185), "3m");
    assert_eq!(age(2 * 3600 + 10), "2h");
    assert_eq!(age(3 * 86400), "3d");
}

#[test]
fn card_has_slug_line_repo_line_and_wrapped_info() {
    let (s, id) = task(".dotfiles", "fix-login");
    s.apply(
        &id,
        Signal::Done {
            info: Some("I couldn't find any login or session-cookie code in this worktree at all".into()),
        },
        100,
        false,
    )
    .unwrap();
    let lines = task_lines(&get(&s, &id), 100 + 60, 30, true);
    assert_eq!(
        lines,
        [
            "▌1 ✓ fix-login              1m",
            "     .dotfiles",
            "     I couldn't find any login",
            "     or session-cookie code…",
        ]
    );
    assert_eq!(unicode_width::UnicodeWidthStr::width(lines[0].as_str()), 30);
    for l in &lines {
        assert!(unicode_width::UnicodeWidthStr::width(l.as_str()) <= 30, "{l:?}");
    }
}

#[test]
fn diff_goes_on_repo_line_and_long_slugs_truncate() {
    let (s, id) = task("api", "a-really-long-task-name-here");
    s.apply(&id, Signal::Waiting, 10, false).unwrap();
    s.set_diff(&id, 42, 7).unwrap();
    let lines = task_lines(&get(&s, &id), 15, 30, false);
    assert_eq!(lines, [" 1 ● a-really-long-task-na… 5s", "     api · +42 −7"]);
}
