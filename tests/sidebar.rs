use grove::sidebar::{age, task_lines};
use grove::store::{NewTask, Signal, Store, Task};
use unicode_width::UnicodeWidthStr;

fn task(repo: &str, slug: &str) -> (Store, String) {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(
        NewTask {
            repo: repo.into(),
            root: "/r".into(),
            slug: slug.into(),
            worktree: "/w".into(),
            team: false,
            branch: None,
            base: None,
        },
        0,
    )
    .unwrap();
    (s, format!("{repo}/{slug}"))
}

fn get(s: &Store, id: &str) -> Task {
    s.get(id).unwrap().unwrap()
}

fn assert_fits(lines: &[String], width: usize) {
    for l in lines {
        assert!(l.width() <= width, "{l:?} is wider than {width}");
    }
}

#[test]
fn ages_are_compact() {
    assert_eq!(age(5), "5s");
    assert_eq!(age(185), "3m");
    assert_eq!(age(2 * 3600 + 10), "2h");
    assert_eq!(age(3 * 86400), "3d");
}

#[test]
fn row_is_id_line_then_state_age_and_info() {
    let (s, id) = task(".dotfiles", "fix-login");
    s.apply(
        &id,
        Signal::Done {
            info: Some("I couldn't find any login code".into()),
        },
        100,
        false,
    )
    .unwrap();
    let lines = task_lines(&get(&s, &id), 160, 40);
    assert_eq!(
        lines,
        [
            " 1 ✓ .dotfiles/fix-login",
            "    done, not seen · 1m · I couldn't fi…"
        ]
    );
    assert_fits(&lines, 40);
}

#[test]
fn waiting_row_says_needs_input_and_shows_diff_instead_of_info() {
    let (s, id) = task("api", "a-really-long-task-name-here");
    s.apply(&id, Signal::Waiting, 10, false).unwrap();
    s.set_info(&id, "ignored when there is a diff").unwrap();
    s.set_diff(&id, 42, 7).unwrap();
    let lines = task_lines(&get(&s, &id), 15, 30);
    assert_eq!(
        lines,
        [" 1 ⏳ api/a-really-long-task-…", "    needs input · 5s · +42 −7"]
    );
    assert_fits(&lines, 30);
}

#[test]
fn working_and_idle_rows() {
    let (s, id) = task("web", "nav");
    s.apply(&id, Signal::Working, 0, false).unwrap();
    assert_eq!(
        task_lines(&get(&s, &id), 120, 30),
        [" 1 ● web/nav", "    working · 2m"]
    );
    s.apply(&id, Signal::Idle, 0, false).unwrap();
    assert_eq!(
        task_lines(&get(&s, &id), 7200, 30),
        [" 1 ○ web/nav", "    idle · 2h"]
    );
}
