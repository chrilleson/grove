//! Task lifecycle: find the repo, make the worktree, run setup, open the session.

use crate::config::Config;
use crate::mux::{Mux, Role, SessionSpec};
use crate::setup::{self, RepoConfig};
use crate::store::{NewTask, Signal, Store, Task, session_name};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct NewOpts {
    pub repo: String,
    pub slug: String,
    pub brief: Option<String>,
    pub base: Option<String>,
    pub team: Option<bool>,
}

pub struct Repo {
    pub name: String,
    pub root: PathBuf,
    pub dir: PathBuf,
}

/// Find a repo by name one level under the roots, or by path.
pub fn find_repo(config: &Config, name: &str) -> Result<Repo> {
    let as_path = Path::new(name);
    if name.contains('/') || name == "." {
        let dir = std::fs::canonicalize(as_path).with_context(|| format!("no such directory: {name}"))?;
        if !dir.join(".git").exists() {
            bail!("{} is not a git repository root", dir.display());
        }
        return Ok(Repo {
            name: dir.file_name().unwrap().to_string_lossy().into_owned(),
            root: dir.parent().unwrap().to_path_buf(),
            dir,
        });
    }
    let roots = config.roots();
    for root in &roots {
        let dir = root.join(name);
        if dir.join(".git").exists() {
            return Ok(Repo {
                name: name.to_string(),
                root: root.clone(),
                dir,
            });
        }
    }
    bail!(
        "repo {name} not found under roots [{}] (set `roots` in {})",
        roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        Config::path().display()
    )
}

/// All repos under the roots, most recently used first.
pub fn list_repos(config: &Config) -> Vec<Repo> {
    let mut repos: Vec<(std::time::SystemTime, Repo)> = vec![];
    for root in config.roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let dir = e.path();
            if dir.join(".git").exists() {
                let used = dir
                    .join(".git")
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                let name = e.file_name().to_string_lossy().into_owned();
                repos.push((
                    used,
                    Repo {
                        name,
                        root: root.clone(),
                        dir,
                    },
                ));
            }
        }
    }
    repos.sort_by_key(|r| std::cmp::Reverse(r.0));
    repos.into_iter().map(|(_, r)| r).collect()
}

pub fn worktree_path(config: &Config, repo: &Repo, slug: &str) -> PathBuf {
    match &config.worktree_root {
        Some(wr) => crate::paths::expand(wr).join(&repo.name).join(slug),
        None => repo.root.join(".worktrees").join(&repo.name).join(slug),
    }
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with(['.', '-'])
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("running git")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn default_base(repo: &Path) -> String {
    git(repo, &["rev-parse", "--abbrev-ref", "origin/HEAD"]).unwrap_or_else(|_| "HEAD".into())
}

pub fn new_task(config: &Config, store: &Store, mux: &dyn Mux, opts: NewOpts) -> Result<Task> {
    if !valid_slug(&opts.slug) {
        bail!(
            "invalid slug {:?}: use letters, digits, '-', '_' or '.'",
            opts.slug
        );
    }
    let repo = find_repo(config, &opts.repo)?;
    let id = format!("{}/{}", repo.name, opts.slug);
    if store.get(&id)?.is_some() {
        bail!("task {id} already exists");
    }
    let session = session_name(&repo.name, &opts.slug);
    if mux.session_exists(&session) {
        bail!("tmux session {session} already exists");
    }
    let rc = RepoConfig::load(&repo.name, &repo.dir)?;
    let team = opts.team.or(rc.team).unwrap_or(config.default_team);

    let wt = worktree_path(config, &repo, &opts.slug);
    if wt.exists() {
        bail!("{} already exists", wt.display());
    }
    std::fs::create_dir_all(wt.parent().unwrap())?;
    let base = opts
        .base
        .clone()
        .or(rc.base.clone())
        .unwrap_or_else(|| default_base(&repo.dir));
    // --no-track: the task branch must not push to the base branch by accident.
    git(
        &repo.dir,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &opts.slug,
            &wt.to_string_lossy(),
            &base,
        ],
    )?;

    let task = store.add_task(
        NewTask {
            repo: repo.name.clone(),
            root: repo.root.to_string_lossy().into_owned(),
            slug: opts.slug.clone(),
            worktree: wt.to_string_lossy().into_owned(),
            team,
        },
        crate::now(),
    )?;

    if let Some(brief) = opts.brief.as_deref() {
        write_brief(&wt, brief)?;
    }

    let mut env = vec![("GROVE_TASK".to_string(), task.id.clone())];
    env.extend(rc.port_env(task.port_base));
    if team {
        env.push(("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".into(), "1".into()));
    }

    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let shell = std::env::var("SHELL").ok();
    let shell = shell
        .as_deref()
        .and_then(|s| s.rsplit('/').next())
        .unwrap_or("shell");
    mux.create_task_session(&SessionSpec {
        name: session,
        cwd: wt.clone(),
        env,
        sidebar_cmd: format!("'{exe}' sidebar"),
        sidebar_width: config.sidebar_width,
        shell_percent: config.shell_percent,
        agent_title: format!("claude · {}", task.id),
        shell_title: format!("{shell} · {}", crate::paths::tilde(&wt)),
        shell_first: Some(format!("'{exe}' setup '{}'", task.id)),
    })?;
    Ok(store.get(&task.id)?.expect("exists"))
}

const BRIEF: &str = ".grove/brief.md";

/// Write the brief into the worktree and hide `.grove/` from git via the
/// repo's local exclude file, so it never shows up as a change.
fn write_brief(wt: &Path, brief: &str) -> Result<()> {
    let path = wt.join(BRIEF);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, format!("{}\n", brief.trim_end()))?;
    let common = PathBuf::from(git(wt, &["rev-parse", "--git-common-dir"])?);
    let exclude = if common.is_absolute() {
        common
    } else {
        wt.join(common)
    }
    .join("info/exclude");
    let current = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !current.lines().any(|l| l.trim() == ".grove/") {
        std::fs::create_dir_all(exclude.parent().unwrap())?;
        let sep = if current.is_empty() || current.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        std::fs::write(&exclude, format!("{current}{sep}.grove/\n"))?;
    }
    Ok(())
}

/// `grove setup <task>`: runs in the shell pane of a new session. Streams
/// setup output, then starts the agent in the agent pane.
pub fn run_setup(config: &Config, store: &Store, mux: &dyn Mux, id: &str) -> Result<()> {
    let task = store.get(id)?.with_context(|| format!("no task {id}"))?;
    let wt = PathBuf::from(&task.worktree);
    let repo = PathBuf::from(&task.root).join(&task.repo);
    let rc = RepoConfig::load(&task.repo, &repo)?;
    let mut env = vec![("GROVE_TASK".to_string(), task.id.clone())];
    env.extend(rc.port_env(task.port_base));

    println!("grove: session {} · {}", task.id, crate::paths::tilde(&wt));
    println!("grove: branch {} · GROVE_PORT={}", task.branch, task.port_base);
    if let Some(failed) = setup::run(&rc, &repo, &wt, &env)? {
        store.apply(&task.id, Signal::Idle, crate::now(), false)?;
        store.set_info(&task.id, &format!("setup failed: {failed}"))?;
        println!("grove: setup failed: {failed}");
        println!("grove: fix it here, then run: grove setup {}", task.id);
        return Ok(());
    }

    let mut agent_cmd = config.agent_command.clone();
    if task.team {
        agent_cmd.push_str(" --teammate-mode tmux");
    }
    let has_brief = wt.join(BRIEF).exists();
    if has_brief {
        agent_cmd.push_str(" \"Read .grove/brief.md and start.\"");
    }
    mux.type_into(&task.session, Role::Agent, &agent_cmd)?;
    println!(
        "grove: started {}{}{}",
        config.agent_command.split_whitespace().next().unwrap_or("agent"),
        if has_brief { " on the brief" } else { "" },
        if task.team { " (agent team)" } else { "" }
    );
    Ok(())
}

/// Why a task's worktree can't be removed without losing work.
pub fn unsaved_work(worktree: &Path, branch: &str) -> Result<Option<String>> {
    if !worktree.exists() {
        return Ok(None);
    }
    let dirty = git(worktree, &["status", "--porcelain"])?;
    if !dirty.is_empty() {
        let n = dirty.lines().count();
        return Ok(Some(format!(
            "{n} uncommitted change{}",
            if n == 1 { "" } else { "s" }
        )));
    }
    // Commits reachable only from this task's branch: on no other branch and no remote.
    // --exclude patterns for --branches are written without the refs/heads/ prefix.
    let exclude = format!("--exclude={branch}");
    let only_here = git(
        worktree,
        &[
            "rev-list",
            "--count",
            "HEAD",
            "--not",
            &exclude,
            "--branches",
            "--remotes",
        ],
    )?;
    let n: usize = only_here.parse().unwrap_or(0);
    if n > 0 {
        return Ok(Some(format!(
            "{n} commit{} not pushed or merged anywhere",
            if n == 1 { "" } else { "s" }
        )));
    }
    Ok(None)
}

/// Teardown: refuses unsaved work unless `force`. The session is killed
/// last because `grove rm` is often run from inside it.
pub fn remove_task(store: &Store, mux: &dyn Mux, task: &Task, force: bool) -> Result<()> {
    let wt = PathBuf::from(&task.worktree);
    let repo = PathBuf::from(&task.root).join(&task.repo);
    if !force && let Some(why) = unsaved_work(&wt, &task.branch)? {
        bail!(
            "{} has {why}; commit and push it, or use --force to discard",
            task.id
        );
    }

    let rc = RepoConfig::load(&task.repo, &repo).unwrap_or_default();
    for cmd in &rc.teardown.run {
        eprintln!("grove: teardown: {cmd}");
        let _ = Command::new("sh")
            .args(["-c", cmd])
            .current_dir(&wt)
            .envs(rc.port_env(task.port_base))
            .status();
    }

    if wt.exists() {
        let wt_s = wt.to_string_lossy();
        let mut args = vec!["worktree", "remove", &*wt_s];
        if force {
            args.push("--force");
        }
        git(&repo, &args)?;
    }
    let _ = git(&repo, &["worktree", "prune"]);
    let delete = if force { "-D" } else { "-d" };
    if let Err(e) = git(&repo, &["branch", delete, &task.branch]) {
        eprintln!("grove: kept branch {}: {e}", task.branch);
    }
    store.remove(&task.id)?;
    mux.kill_session(&task.session)?;
    Ok(())
}
