use grove::dash::{row_cells, sorted};
use grove::newtask::{filter_repos, slug_from_title};
use grove::store::{NewTask, Signal, Store};

fn store_with(tasks: &[(&str, Signal, i64)]) -> Store {
    let s = Store::open_in_memory(20000, 10).unwrap();
    for (slug, sig, at) in tasks {
        s.add_task(
            NewTask {
                repo: "api".into(),
                root: "/r".into(),
                slug: (*slug).into(),
                worktree: "/w".into(),
                team: false,
                branch: None,
                base: None,
            },
            0,
        )
        .unwrap();
        // tasks start idle, so an idle one must have been busy first to get its own timestamp
        if *sig == Signal::Idle {
            s.apply(&format!("api/{slug}"), Signal::Working, at - 1, false)
                .unwrap();
        }
        s.apply(&format!("api/{slug}"), sig.clone(), *at, false).unwrap();
    }
    s
}

#[test]
fn dashboard_sorts_waiting_oldest_first_then_done_working_idle_newest_first() {
    let s = store_with(&[
        ("idle-old", Signal::Idle, 10),
        ("work", Signal::Working, 50),
        ("wait-new", Signal::Waiting, 90),
        ("done-old", Signal::Done { info: None }, 20),
        ("wait-old", Signal::Waiting, 30),
        ("done-new", Signal::Done { info: None }, 80),
        ("idle-new", Signal::Idle, 70),
    ]);
    let ids: Vec<_> = sorted(s.list().unwrap()).into_iter().map(|t| t.slug).collect();
    assert_eq!(
        ids,
        [
            "wait-old", "wait-new", "done-new", "done-old", "work", "idle-new", "idle-old"
        ]
    );
}

#[test]
fn dashboard_row_cells() {
    let s = store_with(&[("fix-login", Signal::Waiting, 100)]);
    s.set_diff("api/fix-login", 42, 7).unwrap();
    let t = s.get("api/fix-login").unwrap().unwrap();
    assert_eq!(
        row_cells(&t, 280, 0),
        ["⏳", "api", "fix-login", "+42 −7", "—", "3m"]
    );
    assert_eq!(row_cells(&t, 280, 2)[4], "⇡2");
}

#[test]
fn slug_comes_from_the_title() {
    assert_eq!(
        slug_from_title("Retry failed Fortnox syncs"),
        "retry-failed-fortnox-syncs"
    );
    assert_eq!(
        slug_from_title("  Fix: login → redirect loop!! "),
        "fix-login-redirect-loop"
    );
    assert_eq!(slug_from_title("Ünïcode & émojis 🎉 ok"), "n-code-mojis-ok");
    // at most 32 characters, never ending in a dash
    assert_eq!(
        slug_from_title("a very long title that keeps going and going"),
        "a-very-long-title-that-keeps"
    );
    assert_eq!(slug_from_title("!!!"), "task");
}

#[test]
fn repo_filter_keeps_recent_order_and_matches_substrings() {
    let repos = ["portal", "api", "integrations", "infra"];
    assert_eq!(filter_repos(&repos, "in"), ["integrations", "infra"]);
    assert_eq!(filter_repos(&repos, "INTEG"), ["integrations"]);
    assert_eq!(filter_repos(&repos, ""), repos);
}
