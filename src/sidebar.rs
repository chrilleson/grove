//! `grove sidebar`: the left pane of every task session (ADR 4).

use crate::config::Config;
use crate::mux::{Mux, Tmux};
use crate::store::{State, Task};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;
use std::time::Duration;

pub fn icon(t: &Task) -> &'static str {
    match t.state {
        State::Waiting => "⏳",
        State::Working => "⚙",
        State::Done if !t.seen => "✓",
        State::Done => "·",
        State::Idle => "○",
    }
}

/// Display width of an icon; `⏳` is double-width in every terminal we target.
fn icon_width(icon: &str) -> usize {
    if icon == "⏳" { 2 } else { 1 }
}

pub fn age(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// The two text lines for one task, each exactly `width` columns wide (top)
/// or at most `width` (bottom).
pub fn row_lines(t: &Task, now: i64, width: usize) -> [String; 2] {
    let ic = icon(t);
    let num = format!("{} ", t.number);
    let age = age(now - t.state_since);
    // "<num><icon> <id> <age>": id gets whatever is left
    let fixed = num.chars().count() + icon_width(ic) + 1 + 1 + age.chars().count();
    let id_w = width.saturating_sub(fixed);
    let id = truncate(&t.id, id_w);
    let pad = id_w - id.chars().count();
    let top = format!("{num}{ic} {id}{} {age}", " ".repeat(pad));

    let detail = match (t.diff_added, t.diff_removed) {
        (Some(a), Some(r)) if a + r > 0 => format!("+{a} −{r}"),
        _ => t.info.clone().unwrap_or_default(),
    };
    let sub = if detail.is_empty() {
        String::new()
    } else {
        format!("    {}", truncate(&detail, width.saturating_sub(4)))
    };
    [top, sub]
}

pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = n.parse().ok()?;
    Some(match unit {
        "" | "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => return None,
    })
}

fn render(
    tasks: &[Task],
    current: Option<&str>,
    now: i64,
    width: usize,
    blink_after: i64,
) -> Text<'static> {
    let mut lines = vec![];
    for t in tasks {
        let mut style = Style::default();
        if t.state == State::Done && !t.seen {
            style = style.add_modifier(Modifier::BOLD);
        }
        if t.state == State::Waiting && now - t.state_since >= blink_after {
            style = style.add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD);
        }
        if Some(t.id.as_str()) == current {
            style = style.add_modifier(Modifier::REVERSED);
        }
        let [top, sub] = row_lines(t, now, width);
        lines.push(Line::styled(top, style));
        if !sub.is_empty() {
            lines.push(Line::styled(
                sub,
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
    }
    if tasks.is_empty() {
        lines.push(Line::raw("no tasks"));
    }
    Text::from(lines)
}

pub fn run(config: &Config) -> Result<()> {
    let store = crate::cli::open_store(config)?;
    let current = std::env::var("GROVE_TASK").ok();
    let blink_after = parse_duration(&config.attention.blink_after).unwrap_or(120);
    let mux = Tmux::from_env();
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            let tasks = store.list()?;
            let now = crate::now();
            terminal.draw(|f| {
                let area = f.area();
                f.render_widget(
                    Paragraph::new(render(
                        &tasks,
                        current.as_deref(),
                        now,
                        area.width as usize,
                        blink_after,
                    )),
                    area,
                );
            })?;
            if event::poll(Duration::from_millis(500))?
                && let Event::Key(k) = event::read()?
            {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                match k.code {
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char(d @ '1'..='9') => {
                        if let Some(t) = store.by_number(d as i64 - '0' as i64)? {
                            let _ = mux.switch_to(&t.session);
                        }
                    }
                    _ => {}
                }
            }
        }
    })();
    ratatui::restore();
    result
}
