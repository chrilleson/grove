//! Task lifecycle: find the repo, make the worktree, run setup, open the session.

use crate::config::Config;
use crate::mux::{Mux, SessionSpec};
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
        let dir =
            std::fs::canonicalize(as_path).with_context(|| format!("no such directory: {name}"))?;
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

    let mut env = vec![("GROVE_TASK".to_string(), task.id.clone())];
    env.extend(rc.port_env(task.port_base));
    if team {
        env.push(("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".into(), "1".into()));
    }

    if let Some(failed) = setup::run(&rc, &repo.dir, &wt, &env)? {
        store.apply(&task.id, Signal::Idle, crate::now(), false)?;
        store.set_info(&task.id, &format!("setup failed: {failed}"))?;
        eprintln!("grove: setup command failed: {failed} (task kept; fix it in the shell pane)");
    }

    let mut agent_cmd = config.agent_command.clone();
    if team {
        agent_cmd.push_str(" --teammate-mode tmux");
    }
    let mut agent_env = vec![];
    if let Some(brief) = &opts.brief {
        // Passed through the environment so no shell (bash, zsh, fish) needs quoting rules.
        agent_cmd.push_str(" \"$GROVE_BRIEF\"");
        agent_env.push(("GROVE_BRIEF".to_string(), brief.clone()));
    }

    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    mux.create_task_session(&SessionSpec {
        name: session,
        cwd: wt,
        env,
        sidebar_cmd: format!("'{exe}' sidebar"),
        sidebar_width: config.sidebar_width,
        agent_cmd,
        agent_env,
    })?;
    Ok(store.get(&task.id)?.expect("exists"))
}
