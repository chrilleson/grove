//! `grove dash` (`⌥s`): every task in one table, sorted by what needs you
//! (design image 05).

use crate::cli::{Tool, go_to};
use crate::config::Config;
use crate::mux::Tmux;
use crate::sidebar::{age, icon, state_color, truncate};
use crate::store::{State, Store, Task};
use crate::{theme, ui};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Constraint;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Waiting oldest first, then done, working and idle, newest first.
pub fn sorted(mut tasks: Vec<Task>) -> Vec<Task> {
    let rank = |t: &Task| match t.state {
        State::Waiting => 0,
        State::Done => 1,
        State::Working => 2,
        State::Idle => 3,
    };
    tasks.sort_by(|a, b| {
        rank(a).cmp(&rank(b)).then_with(|| {
            if a.state == State::Waiting {
                a.state_since.cmp(&b.state_since)
            } else {
                b.state_since.cmp(&a.state_since)
            }
        })
    });
    tasks
}

/// icon, repo, slug, diff, PR/push state, age. `ahead` is the number of
/// commits that exist only on the task branch.
pub fn row_cells(t: &Task, now: i64, ahead: usize) -> [String; 6] {
    let diff = format!("+{} −{}", t.diff_added.unwrap_or(0), t.diff_removed.unwrap_or(0));
    let pr = if ahead > 0 {
        format!("⇡{ahead}")
    } else {
        "—".into()
    };
    [
        icon(t).into(),
        t.repo.clone(),
        t.slug.clone(),
        diff,
        pr,
        age(now - t.state_since),
    ]
}

enum Footer {
    Keys,
    Confirm(Box<Task>),
    Message(String),
}

pub fn run(config: &Config, store: &Store) -> Result<()> {
    let mux = Tmux::from_env();
    let mut selected = 0usize;
    let mut footer = Footer::Keys;
    let mut ahead: HashMap<String, usize> = HashMap::new();
    let mut ahead_at = Instant::now() - Duration::from_secs(60);
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            let tasks = sorted(store.list()?);
            selected = selected.min(tasks.len().saturating_sub(1));
            if ahead_at.elapsed() > Duration::from_secs(5) {
                ahead = tasks
                    .iter()
                    .map(|t| {
                        let n = crate::task::commits_only_here(std::path::Path::new(&t.worktree), &t.branch);
                        (t.id.clone(), n.unwrap_or(0))
                    })
                    .collect();
                ahead_at = Instant::now();
            }
            let now = crate::now();
            terminal.draw(|f| {
                let foot = match &footer {
                    Footer::Keys => ui::hints(&[
                        ("⏎", "jump"),
                        ("j/k", "move"),
                        ("c", "lazygit"),
                        ("e", config.editor.split_whitespace().next().unwrap_or("editor")),
                        ("v", "code"),
                        ("n", "new"),
                        ("x", "discard"),
                        ("esc", "close"),
                    ]),
                    Footer::Confirm(t) => Line::from(vec![
                        Span::styled(
                            format!(" Discard {}? Removes worktree and branch. ", t.id),
                            ui::fg(theme::WAIT),
                        ),
                        Span::styled("y", ui::fg(theme::FG)),
                        Span::styled(" yes  ", ui::fg(theme::FAINT)),
                        Span::styled("n", ui::fg(theme::FG)),
                        Span::styled(" no", ui::fg(theme::FAINT)),
                    ]),
                    Footer::Message(m) => Line::styled(format!(" {m}"), ui::fg(theme::WAIT)),
                };
                let body = ui::frame(
                    f,
                    &ui::title("grove", &format!(" · {} tasks", tasks.len())),
                    "sorted: waiting, done, working, idle ",
                    foot,
                );
                let rows = tasks.iter().enumerate().map(|(i, t)| {
                    let [ic, repo, slug, diff, pr, age] = row_cells(t, now, *ahead.get(&t.id).unwrap_or(&0));
                    let (add, del) = diff.split_once(' ').unwrap_or((&diff, ""));
                    let mut row = Row::new(vec![
                        Cell::from(Span::styled(ic, ui::fg(state_color(t)))),
                        Cell::from(Span::styled(truncate(&repo, 10), ui::fg(theme::DIM))),
                        Cell::from(Span::styled(slug, ui::fg(theme::FG))),
                        Cell::from(Line::from(vec![
                            Span::styled(add.to_string(), ui::fg(theme::DONE)),
                            Span::raw(" "),
                            Span::styled(del.to_string(), ui::fg(theme::ERR)),
                        ])),
                        Cell::from(Span::styled(pr, ui::fg(theme::DIM))),
                        Cell::from(Span::styled(age, ui::fg(theme::FAINT))),
                    ]);
                    if i == selected {
                        row = row.style(Style::default().bg(theme::color(theme::CURRENT_BG)));
                    }
                    row
                });
                let widths = [
                    Constraint::Length(3),
                    Constraint::Length(10),
                    Constraint::Fill(1),
                    Constraint::Length(12),
                    Constraint::Length(10),
                    Constraint::Length(4),
                ];
                f.render_widget(Table::new(rows, widths).column_spacing(1), body);
            })?;

            if !event::poll(Duration::from_millis(1000))? {
                continue;
            }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            if let Footer::Confirm(t) = &footer {
                let t = t.clone();
                footer = Footer::Keys;
                if k.code == KeyCode::Char('y')
                    && let Err(e) = crate::task::remove_task(store, &mux, &t, false)
                {
                    footer = Footer::Message(format!("{e:#}"));
                }
                continue;
            }
            footer = Footer::Keys;
            let current = tasks.get(selected).cloned();
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => selected += 1,
                KeyCode::Char('k') | KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Enter => {
                    if let Some(t) = current {
                        let _ = go_to(&mux, &t);
                        return Ok(());
                    }
                }
                KeyCode::Char(c @ ('c' | 'e' | 'v')) => {
                    if let Some(t) = current {
                        let tool = match c {
                            'c' => Tool::Lazygit,
                            'e' => Tool::Editor,
                            _ => Tool::Code,
                        };
                        let name = match tool {
                            Tool::Lazygit => "lazygit",
                            Tool::Editor => "editor",
                            Tool::Code => "code",
                        };
                        ui::after_popup(&["open", name, &t.id]);
                        return Ok(());
                    }
                }
                KeyCode::Char('n') => {
                    ui::after_popup(&["new", "--popup"]);
                    return Ok(());
                }
                KeyCode::Char('x') => {
                    if let Some(t) = current {
                        footer = Footer::Confirm(Box::new(t));
                    }
                }
                _ => {}
            }
        }
    })();
    ratatui::restore();
    result
}
