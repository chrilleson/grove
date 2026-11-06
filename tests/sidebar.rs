use grove::sidebar::{age, row_lines};
use grove::store::{NewTask, Signal, Store};

#[test]
fn ages_are_compact() {
    assert_eq!(age(5), "5s");
    assert_eq!(age(185), "3m");
    assert_eq!(age(2 * 3600 + 10), "2h");
    assert_eq!(age(3 * 86400), "3d");
}

#[test]
fn row_shows_number_icon_id_age_and_info() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(
        NewTask {
            repo: "api".into(),
            root: "/r".into(),
            slug: "fix-login".into(),
            worktree: "/w".into(),
            team: false,
        },
        0,
    )
    .unwrap();
    s.apply(
        "api/fix-login",
        Signal::Done {
            info: Some("Updated README with the new flags".into()),
        },
        100,
        false,
    )
    .unwrap();
    let t = s.get("api/fix-login").unwrap().unwrap();

    let [top, sub] = row_lines(&t, 100 + 180, 32);
    assert_eq!(top, "1 ✓ api/fix-login             3m");
    assert_eq!(sub, "    Updated README with the new…");
    assert_eq!(top.chars().count(), 32);
}

#[test]
fn diff_stats_replace_info_line() {
    let s = Store::open_in_memory(20000, 10).unwrap();
    s.add_task(
        NewTask {
            repo: "a-really-long-repo-name".into(),
            root: "/r".into(),
            slug: "slug".into(),
            worktree: "/w".into(),
            team: false,
        },
        0,
    )
    .unwrap();
    s.apply("a-really-long-repo-name/slug", Signal::Waiting, 10, false)
        .unwrap();
    s.set_diff("a-really-long-repo-name/slug", 42, 7).unwrap();
    let t = s.get("a-really-long-repo-name/slug").unwrap().unwrap();
    let [top, sub] = row_lines(&t, 15, 32);
    assert_eq!(top, "1 ⏳ a-really-long-repo-name… 5s");
    assert_eq!(sub, "    +42 −7");
}
