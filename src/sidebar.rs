//! `grove sidebar`: the left pane of every task session (ADR 4), and the
//! `⌥s` picker (`grove dash`), which is the same view in a popup.
//!
//! Two lines per task, as in the design simulator:
//!
//! ```text
//!  1 ⏳ portal/fix-login-redirect
//!     needs input · 3m · +42 −7
//! ```
//!
//! Waiting rows are amber, done-but-unseen rows green, everything else
//! quiet; the current task gets a subtle background bar.

use crate::config::Config;
use crate::mux::{Mux, Tmux};
use crate::store::{State, Task};
use crate::theme;
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const INDENT: &str = "    ";

pub fn icon(t: &Task) -> &'static str {
    match t.state {
        State::Waiting => "⏳",
        State::Working => "●",
        State::Done => "✓",
        State::Idle => "○",
    }
}

pub fn state_color(t: &Task) -> &'static str {
    match t.state {
        State::Waiting => theme::WAIT,
        State::Working => theme::WORK,
        State::Done => theme::DONE,
        State::Idle => theme::IDLE,
    }
}

/// How the second line describes the state.
pub fn state_words(t: &Task) -> &'static str {
    match t.state {
        State::Waiting => "needs input",
        State::Working => "working",
        State::Done if !t.seen => "done, not seen",
        State::Done => "done",
        State::Idle => "idle",
    }
}

/// The agent pane header's right-hand text, e.g. `⏳ waiting`, in tmux style tags.
pub fn pane_state(t: &Task) -> String {
    format!(
        "#[fg={}]{} {}#[default]",
        state_color(t),
        icon(t),
        t.state.as_str()
    )
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
pub fn truncate(s: &str, max: usize) -> String {
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

/// The two plain-text lines for one task, each at most `width` columns.
pub fn task_lines(t: &Task, now: i64, width: usize) -> [String; 2] {
    let top = truncate(&format!(" {} {} {}", t.number, icon(t), t.id), width);
    let mut parts = vec![state_words(t).to_string(), age(now - t.state_since)];
    match (t.diff_added, t.diff_removed) {
        (Some(a), Some(r)) if a + r > 0 => parts.push(format!("+{a} −{r}")),
        _ => {
            if let Some(info) = t.info.as_deref().filter(|i| !i.trim().is_empty()) {
                parts.push(info.to_string());
            }
        }
    }
    if t.team {
        parts.push("team".into());
    }
    let sub = truncate(&format!("{INDENT}{}", parts.join(" · ")), width);
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

struct View<'a> {
    tasks: &'a [Task],
    current: Option<&'a str>,
    selected: Option<usize>,
    now: i64,
    blink_after: i64,
}

fn pad(s: String, width: usize) -> String {
    let w = s.width();
    if w >= width { s } else { s + &" ".repeat(width - w) }
}

fn row(t: &Task, v: &View, i: usize, width: usize) -> Vec<Line<'static>> {
    let highlighted = v.current == Some(t.id.as_str()) || v.selected == Some(i);
    let [top, sub] = task_lines(t, v.now, width);
    let color = theme::color(state_color(t));

    let mut title = Style::default().fg(theme::color(theme::DIM));
    let mut icon_style = Style::default().fg(color);
    match t.state {
        State::Waiting => {
            title = Style::default().fg(color).add_modifier(Modifier::BOLD);
            if v.now - t.state_since >= v.blink_after {
                icon_style = icon_style.add_modifier(Modifier::SLOW_BLINK);
            }
        }
        State::Done if !t.seen => title = Style::default().fg(color).add_modifier(Modifier::BOLD),
        _ if highlighted => title = Style::default().fg(theme::color(theme::FG)),
        _ => {}
    }
    let mut small = Style::default().fg(theme::color(theme::FAINT));
    let mut num = Style::default().fg(theme::color(theme::FAINT));
    if highlighted {
        let bg = theme::color(theme::CURRENT_BG);
        title = title.bg(bg);
        icon_style = icon_style.bg(bg);
        small = small.bg(bg).fg(theme::color(theme::DIM));
        num = num.bg(bg);
    }

    // Split " 1 ⏳ repo/slug" so number, icon and id get their own styles.
    let ic = icon(t);
    let (before, after) = top.split_once(ic).unwrap_or((&top, ""));
    let used = top.width();
    let fill = " ".repeat(width.saturating_sub(used));
    vec![
        Line::from(vec![
            Span::styled(before.to_string(), num),
            Span::styled(ic.to_string(), icon_style),
            Span::styled(format!("{after}{fill}"), title),
        ]),
        Line::styled(pad(sub, width), small),
    ]
}

fn render(v: &View, width: usize) -> (Text<'static>, Line<'static>) {
    let faint = Style::default().fg(theme::color(theme::FAINT));
    let mut lines = vec![];
    for (i, t) in v.tasks.iter().enumerate() {
        lines.extend(row(t, v, i, width));
    }
    if v.tasks.is_empty() {
        lines.push(Line::styled(" no tasks · ⌥n starts one", faint));
    }
    let footer = if v.selected.is_some() {
        " j/k move · ⏎ open · q close"
    } else {
        " ⌥w next · ⌥s all · q hide"
    };
    (Text::from(lines), Line::styled(truncate(footer, width), faint))
}

/// The sidebar pane, or with `popup` the `⌥s` picker: j/k/Enter, digits,
/// q/Esc to close. In the sidebar, `q` closes the sidebar pane only; the
/// agent and shell keep running.
pub fn run(config: &Config, popup: bool) -> Result<()> {
    let store = crate::cli::open_store(config)?;
    let current = std::env::var("GROVE_TASK").ok();
    let blink_after = parse_duration(&config.attention.blink_after).unwrap_or(120);
    let mux = Tmux::from_env();
    // The count lives in the pane's own header row ("tasks · 7"), as in the design.
    let own_pane = std::env::var("TMUX_PANE").ok().filter(|_| !popup);
    let mut shown_count = None;
    let mut selected = 0usize;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            let tasks = store.list()?;
            selected = selected.min(tasks.len().saturating_sub(1));
            if let Some(pane) = &own_pane
                && shown_count != Some(tasks.len())
            {
                mux.set_pane_title(pane, &format!("tasks · {}", tasks.len()));
                shown_count = Some(tasks.len());
            }
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
