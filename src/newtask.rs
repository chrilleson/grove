//! `grove new` without arguments (`⌥n`): pick a repo, give a title, toggle
//! team mode (design image 06). The slug is derived from the title and the
//! title becomes the brief.

use crate::config::Config;
use crate::mux::{Mux, Role, Tmux};
use crate::setup::RepoConfig;
use crate::store::Store;
use crate::task::{NewOpts, Repo, list_repos};
use crate::{theme, ui};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position, Rect};

use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::Duration;

const SLUG_MAX: usize = 32;

/// "Retry failed Fortnox syncs" → "retry-failed-fortnox-syncs": ASCII
/// letters and digits, dashes between, at most 32 characters, cut at a dash.
pub fn slug_from_title(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_end_matches('-').to_string();
    if slug.len() > SLUG_MAX {
        slug.truncate(SLUG_MAX);
        if let Some(cut) = slug.rfind('-') {
            slug.truncate(cut);
        }
    }
    if slug.is_empty() { "task".into() } else { slug }
}

/// Names containing `query` (case-insensitive), keeping the given order.
pub fn filter_repos<'a>(names: &[&'a str], query: &str) -> Vec<&'a str> {
    let q = query.to_lowercase();
    names
        .iter()
        .copied()
        .filter(|n| n.to_lowercase().contains(&q))
        .collect()
}

/// Repos under the roots, the ones you started tasks in most recently first.
fn repos_by_recent_use(config: &Config, store: &Store) -> Vec<Repo> {
    let last_task: std::collections::HashMap<String, i64> = store
        .list()
        .unwrap_or_default()
        .into_iter()
        .fold(Default::default(), |mut m, t| {
            let e = m.entry(t.repo).or_insert(0);
            *e = (*e).max(t.created_at);
            m
        });
    let mut repos = list_repos(config);
    let mtime = |r: &Repo| {
        r.dir
            .join(".git")
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64)
    };
    repos.sort_by_key(|r| std::cmp::Reverse(last_task.get(&r.name).copied().unwrap_or(0).max(mtime(r))));
    repos
}

#[derive(PartialEq, Clone, Copy)]
enum Field {
    Repo,
    Title,
    Team,
}

struct Form {
    query: String,
    selected: usize,
    title: String,
    team: Option<bool>,
    field: Field,
    error: Option<String>,
}

fn input(f: &mut ratatui::Frame, area: Rect, text: &str, focused: bool) {
    let border = if focused { theme::WORK } else { theme::LINE };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(ui::fg(border));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Line::styled(format!(" {text}"), ui::fg(theme::FG))),
        inner,
    );
    if focused {
        let x = inner.x + 1 + unicode_width::UnicodeWidthStr::width(text) as u16;
        f.set_cursor_position(Position::new(x.min(inner.right().saturating_sub(1)), inner.y));
    }
}

pub fn run(config: &Config, store: &Store) -> Result<()> {
    let mux = Tmux::from_env();
    let repos = repos_by_recent_use(config, store);
    let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
    let mut form = Form {
        query: String::new(),
        selected: 0,
        title: String::new(),
        team: None,
        field: Field::Repo,
        error: None,
    };
    let mut terminal = ratatui::init();

    let chosen = (|| -> Result<Option<(Repo, String, bool)>> {
        loop {
            let matches = filter_repos(&names, &form.query);
            form.selected = form.selected.min(matches.len().saturating_sub(1));
            let repo = matches
                .get(form.selected)
                .and_then(|n| repos.iter().find(|r| r.name == *n));
            // Team defaults to the repo's own setting, then the global one, until toggled.
            let team = form.team.unwrap_or_else(|| {
                repo.and_then(|r| RepoConfig::load(&r.name, &r.dir).ok()?.team)
                    .unwrap_or(config.default_team)
            });

            terminal.draw(|f| {
                let foot = match &form.error {
                    Some(e) => Line::styled(format!(" {e}"), ui::fg(theme::WAIT)),
                    None => ui::hints(&[
                        ("↑↓", "repo"),
                        ("tab", "next field"),
                        ("⌥t", "team"),
                        ("⏎", "create"),
                        ("esc", "cancel"),
                    ]),
                };
                let body = ui::frame(
                    f,
                    &ui::title("grove new", ""),
                    "worktree, setup, session and agent in one go ",
                    foot,
                );
                let list_h = matches.len().clamp(1, 6) as u16;
                let [repo_row, list, _, title_row, team_row] = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(list_h),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ])
                .areas(body);
                let label_w = 9;
                let label = |f: &mut ratatui::Frame, area: Rect, text: &str| {
                    let [l, rest] =
                        Layout::horizontal([Constraint::Length(label_w), Constraint::Fill(1)]).areas(area);
                    let [_, mid, _] = Layout::vertical([Constraint::Length(1); 3]).areas(l);
                    f.render_widget(
                        Paragraph::new(Line::styled(format!(" {text}"), ui::fg(theme::DIM))),
                        mid,
                    );
                    rest
                };

                let r = label(f, repo_row, "repo");
                input(f, r, &form.query, form.field == Field::Repo);

                let [_, list_area] =
                    Layout::horizontal([Constraint::Length(label_w), Constraint::Fill(1)]).areas(list);
                let lines: Vec<Line> = if matches.is_empty() {
                    vec![Line::styled("  no repo matches", ui::fg(theme::FAINT))]
                } else {
                    matches
                        .iter()
                        .take(6)
                        .enumerate()
                        .map(|(i, n)| {
                            let path = repos
                                .iter()
                                .find(|r| r.name == *n)
                                .map(|r| crate::paths::tilde(&r.dir))
                                .unwrap_or_default();
                            if i == form.selected {
                                Line::from(vec![
                                    Span::styled("❯ ", ui::fg(theme::WORK)),
                                    Span::styled(path, ui::fg(theme::FG)),
                                ])
                            } else {
                                Line::styled(format!("  {path}"), ui::fg(theme::DIM))
                            }
                        })
                        .collect()
                };
                f.render_widget(Paragraph::new(lines), list_area);

                let r = label(f, title_row, "title");
                input(f, r, &form.title, form.field == Field::Title);

                let r = label(f, team_row, "team");
                let [toggle, explain] =
                    Layout::horizontal([Constraint::Length(7), Constraint::Fill(1)]).areas(r);
                let border = if form.field == Field::Team {
                    theme::WORK
                } else {
                    theme::LINE_HI
                };
                f.render_widget(
                    Paragraph::new(Line::styled(if team { " on" } else { " off" }, ui::fg(theme::FG))).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(ui::fg(border)),
                    ),
                    toggle,
                );
                let [_, mid, _] = Layout::vertical([Constraint::Length(1); 3]).areas(explain);
                let why = if team {
                    " lead plus teammates, each in its own pane"
                } else {
                    " one agent"
                };
                f.render_widget(Paragraph::new(Line::styled(why, ui::fg(theme::FAINT))), mid);
            })?;

            if !event::poll(Duration::from_millis(500))? {
                continue;
            }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            form.error = None;
            let alt = k.modifiers.contains(KeyModifiers::ALT);
            match k.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(None),
                KeyCode::Char('t') if alt => form.team = Some(!team),
                KeyCode::Tab => {
                    form.field = match form.field {
                        Field::Repo => Field::Title,
                        Field::Title => Field::Team,
                        Field::Team => Field::Repo,
                    }
                }
                KeyCode::BackTab => {
                    form.field = match form.field {
                        Field::Repo => Field::Team,
                        Field::Title => Field::Repo,
                        Field::Team => Field::Title,
                    }
                }
                KeyCode::Up => form.selected = form.selected.saturating_sub(1),
                KeyCode::Down => form.selected += 1,
                KeyCode::Char(' ') if form.field == Field::Team => form.team = Some(!team),
                KeyCode::Enter => {
                    let Some(repo) = repo else {
                        form.error = Some("pick a repo".into());
                        continue;
                    };
                    if form.title.trim().is_empty() {
                        form.error = Some("give the task a title".into());
                        form.field = Field::Title;
                        continue;
                    }
                    return Ok(Some((
                        Repo {
                            name: repo.name.clone(),
                            root: repo.root.clone(),
                            dir: repo.dir.clone(),
                        },
                        form.title.trim().to_string(),
                        team,
                    )));
                }
                KeyCode::Backspace => {
                    match form.field {
                        Field::Repo => drop(form.query.pop()),
                        Field::Title => drop(form.title.pop()),
                        Field::Team => {}
                    };
                }
                KeyCode::Char(c) if !alt => match form.field {
                    Field::Repo => {
                        form.query.push(c);
                        form.selected = 0;
                    }
                    Field::Title => form.title.push(c),
                    Field::Team => {}
                },
                _ => {}
            }
        }
    })();
    ratatui::restore();

    let Some((repo, title, team)) = chosen? else {
        return Ok(());
    };
    let opts = NewOpts {
        repo: repo.dir.to_string_lossy().into_owned(),
        slug: slug_from_title(&title),
        brief: Some(format!("# {title}")),
        base: None,
        team: Some(team),
    };
    match crate::task::new_task(config, store, &mux, opts) {
        Ok(t) => {
            let _ = mux.focus(&t.session, Role::Shell);
            mux.switch_to(&t.session)
        }
        Err(e) => {
            // Inside a popup the message would vanish with it, so wait for a key.
            eprintln!("grove: {e:#}\n\npress enter to close");
            let _ = std::io::stdin().read_line(&mut String::new());
            Ok(())
        }
    }
}
