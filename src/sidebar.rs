//! `grove sidebar`: the left pane of every task session (ADR 4), and the
//! `⌥s` picker (`grove dash`), which is the same view in a popup.
//!
//! Each task is a small card:
//!
//! ```text
//! ▌1 ✓ fix-login              1m
//!      .dotfiles · +42 −7
//!      I couldn't find any login
//!      or session-cookie code…
//! ```
//!
//! Icons are all single-width so columns line up in every terminal, and
//! colours are the terminal's own ANSI palette so the sidebar follows the
//! user's theme.

use crate::config::Config;
use crate::mux::{Mux, Tmux};
use crate::store::{State, Task};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Columns before the slug: marker, number, space, icon, space.
const INDENT: usize = 5;
const INFO_LINES: usize = 2;

pub fn icon(t: &Task) -> (&'static str, Color) {
    match t.state {
        State::Waiting => ("●", Color::Yellow),
        State::Working => ("◐", Color::Blue),
        State::Done if !t.seen => ("✓", Color::Green),
        State::Done => ("✓", Color::DarkGray),
        State::Idle => ("○", Color::DarkGray),
    }
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

/// Cut `s` to at most `max` display columns, ending in `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Greedy word wrap to `width` columns; words longer than a line are cut.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if candidate.width() <= width {
            cur = candidate;
        } else {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            cur = truncate(word, width);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// At most `INFO_LINES` wrapped lines, the last ending in `…` if text was dropped.
fn info_lines(info: &str, width: usize) -> Vec<String> {
    let all = wrap(info, width);
    if all.len() <= INFO_LINES {
        return all;
    }
    let mut out: Vec<String> = all[..INFO_LINES - 1].to_vec();
    let rest = all[INFO_LINES - 1..].join(" ");
    let mut last = wrap(&rest, width.saturating_sub(1))
        .into_iter()
        .next()
        .unwrap_or_default();
    last.push('…');
    out.push(last);
    out
}

/// The plain-text lines of one task card, each at most `width` columns.
pub fn task_lines(t: &Task, now: i64, width: usize, current: bool) -> Vec<String> {
    let (ic, _) = icon(t);
    let marker = if current { "▌" } else { " " };
    let age = age(now - t.state_since);
    let slug_w = width.saturating_sub(INDENT + 1 + age.width());
    let slug = truncate(&t.slug, slug_w);
    let pad = " ".repeat(slug_w - slug.width());
    let mut lines = vec![format!("{marker}{} {ic} {slug}{pad} {age}", t.number)];

    let indent = " ".repeat(INDENT);
    let body_w = width.saturating_sub(INDENT);
    let mut repo = t.repo.clone();
    if let (Some(a), Some(r)) = (t.diff_added, t.diff_removed)
        && a + r > 0
    {
        repo = format!("{repo} · +{a} −{r}");
    }
    lines.push(format!("{indent}{}", truncate(&repo, body_w)));
    if let Some(info) = t.info.as_deref().filter(|i| !i.trim().is_empty()) {
        lines.extend(
            info_lines(info, body_w)
                .into_iter()
                .map(|l| format!("{indent}{l}")),
        );
    }
    lines
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

struct View<'a> {
    tasks: &'a [Task],
    current: Option<&'a str>,
    selected: Option<usize>,
    now: i64,
    blink_after: i64,
}

fn card(t: &Task, v: &View, i: usize, width: usize) -> Vec<Line<'static>> {
    let is_current = v.current == Some(t.id.as_str());
    let is_selected = v.selected == Some(i);
    let lines = task_lines(t, v.now, width, is_current || is_selected);
    let (ic, color) = icon(t);
    let needs_you = t.needs_you();
    let dim = Style::default().fg(Color::DarkGray);

    let mut out = vec![];
    // Title line, split so the icon and marker get their own colours.
    let title = &lines[0];
    let (marker, rest) = title.split_at(title.chars().next().map_or(0, char::len_utf8));
    let after_icon = rest.split_once(ic).map_or("", |(_, r)| r);
    let mut icon_style = Style::default().fg(color);
    if t.state == State::Waiting && v.now - t.state_since >= v.blink_after {
        icon_style = icon_style.add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD);
    }
    let mut title_style = Style::default();
    if needs_you || is_current || is_selected {
        title_style = title_style.add_modifier(Modifier::BOLD);
    }
    if !needs_you && !is_current && !is_selected {
        title_style = title_style.fg(Color::Gray);
    }
    out.push(Line::from(vec![
        Span::styled(marker.to_string(), Style::default().fg(Color::Cyan)),
        Span::styled(format!("{} ", t.number), dim),
        Span::styled(ic.to_string(), icon_style),
        Span::styled(after_icon.to_string(), title_style),
    ]));
    out.extend(lines[1..].iter().map(|l| Line::styled(l.clone(), dim)));
    out
}

fn render(v: &View, width: usize) -> (Text<'static>, Line<'static>) {
    let dim = Style::default().fg(Color::DarkGray);
    let waiting = v.tasks.iter().filter(|t| t.state == State::Waiting).count();
    let done = v
        .tasks
        .iter()
        .filter(|t| t.state == State::Done && !t.seen)
        .count();

    let mut header = vec![Span::styled(
        " grove",
        Style::default().add_modifier(Modifier::BOLD),
    )];
    if waiting > 0 {
        header.push(Span::styled(
            format!("  ● {waiting}"),
            Style::default().fg(Color::Yellow),
        ));
    }
    if done > 0 {
        header.push(Span::styled(
            format!("  ✓ {done}"),
            Style::default().fg(Color::Green),
        ));
    }
    let mut lines = vec![Line::from(header), Line::raw("")];

    for (i, t) in v.tasks.iter().enumerate() {
        lines.extend(card(t, v, i, width));
        lines.push(Line::raw(""));
    }
    if v.tasks.is_empty() {
        lines.push(Line::styled(" no tasks — ⌥n to start one", dim));
    }
    let footer = if v.selected.is_some() {
        " j/k move · ⏎ open · q close"
    } else {
        " ⌥w next · ⌥s tasks · q hide"
    };
    (Text::from(lines), Line::styled(truncate(footer, width), dim))
}

/// The sidebar pane, or with `popup` the `⌥s` picker: j/k/Enter, digits,
/// q/Esc to close. In the sidebar, `q` closes the sidebar pane only; the
/// agent and shell keep running.
pub fn run(config: &Config, popup: bool) -> Result<()> {
    let store = crate::cli::open_store(config)?;
    let current = std::env::var("GROVE_TASK").ok();
    let blink_after = parse_duration(&config.attention.blink_after).unwrap_or(120);
    let mux = Tmux::from_env();
    let mut selected = 0usize;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            let tasks = store.list()?;
            selected = selected.min(tasks.len().saturating_sub(1));
            let view = View {
                tasks: &tasks,
                current: current.as_deref(),
                selected: popup.then_some(selected),
                now: crate::now(),
                blink_after,
            };
            terminal.draw(|f| {
                let [body, footer] =
                    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(f.area());
                let (text, foot) = render(&view, body.width as usize);
                f.render_widget(Paragraph::new(text), body);
                f.render_widget(Paragraph::new(foot), footer);
            })?;
            if !event::poll(Duration::from_millis(500))? {
                continue;
            }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let target = match k.code {
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => {
                    selected += 1;
                    None
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    None
                }
                KeyCode::Enter if popup => tasks.get(selected).cloned(),
                KeyCode::Char(d @ '1'..='9') => store.by_number(d as i64 - '0' as i64)?,
                _ => None,
            };
            if let Some(t) = target {
                let _ = mux.switch_to(&t.session);
                if popup {
                    return Ok(());
                }
            }
        }
    })();
    ratatui::restore();
    result
}
