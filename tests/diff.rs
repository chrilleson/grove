mod common;
use common::git;
use grove::task::{commits_only_here, diff_stats};

fn commit(dir: &std::path::Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(
        dir,
        &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", msg],
    );
}

#[test]
fn diff_counts_commits_uncommitted_and_untracked_work_since_the_base() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    commit(repo, "base");
    git(repo, &["checkout", "-qb", "grove/x"]);

    // committed on the task branch: +1 −0
    std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    commit(repo, "work");
    // uncommitted: change "one" → +1 −1
    std::fs::write(repo.join("a.txt"), "ONE\ntwo\nthree\nfour\n").unwrap();
    // untracked new file: +2
    std::fs::write(repo.join("new.txt"), "x\ny\n").unwrap();

    assert_eq!(diff_stats(repo, Some("main")).unwrap(), (4, 1));
    assert_eq!(commits_only_here(repo, "grove/x").unwrap(), 1);

    // the base moving on doesn't count against the task
    let other = tempfile::tempdir().unwrap();
    let main_wt = other.path().join("main");
    git(
        repo,
        &["worktree", "add", "-q", main_wt.to_str().unwrap(), "main"],
    );
    std::fs::write(main_wt.join("other.txt"), "a\nb\nc\n").unwrap();
    commit(&main_wt, "main moves");
    assert_eq!(diff_stats(repo, Some("main")).unwrap(), (4, 1));
}
