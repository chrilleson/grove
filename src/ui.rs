//! Shared chrome for grove's popups (design images 05 and 06): a header with
//! a bold title and a quiet note on the right, a rule, and a footer of key
//! hints where the keys are bold.

use crate::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn fg(hex: &str) -> Style {
    Style::default().fg(theme::color(hex))
}

/// Draws header and footer, returns the body area.
pub fn frame(f: &mut Frame, title: &[Span<'static>], note: &str, footer: Line<'static>) -> Rect {
    let area = f.area();
    f.render_widget(
        Block::default().style(Style::default().bg(theme::color(theme::POPUP_BG))),
        area,
    );
    let [head, body, foot] =
        Layout::vertical([Constraint::Length(2), Constraint::Fill(1), Constraint::Length(2)]).areas(area);
    let rule = Style::default().fg(theme::color(theme::LINE));

    let head_block = Block::default().borders(Borders::BOTTOM).border_style(rule);
    let inner = head_block.inner(head);
    f.render_widget(head_block, head);
    let [l, r] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(note.chars().count() as u16 + 1),
    ])
    .areas(inner);
    let mut spans = vec![Span::raw(" ")];
    spans.extend(title.iter().cloned());
    f.render_widget(Paragraph::new(Line::from(spans)), l);
    f.render_widget(Paragraph::new(Line::styled(note.to_string(), fg(theme::DIM))), r);

    let foot_block = Block::default().borders(Borders::TOP).border_style(rule);
    let inner = foot_block.inner(foot);
    f.render_widget(foot_block, foot);
    f.render_widget(Paragraph::new(footer), inner);
    body
}

/// `[("⏎", "jump"), ("j/k", "move")]` → " ⏎ jump  j/k move", keys bold.
pub fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (k, what)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            k.to_string(),
            fg(theme::FG).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {what}"), fg(theme::FAINT)));
    }
    Line::from(spans)
}

pub fn title(name: &str, rest: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(name.to_string(), fg(theme::FG).add_modifier(Modifier::BOLD)),
        Span::styled(rest.to_string(), fg(theme::DIM)),
    ]
}

/// Run a grove command once the current popup has closed (tmux can't stack popups).
pub fn after_popup(args: &[&str]) {
    let Ok(exe) = std::env::current_exe() else { return };
    let quoted: Vec<String> = std::iter::once(exe.to_string_lossy().into_owned())
        .chain(args.iter().map(|a| a.to_string()))
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect();
    let _ = std::process::Command::new("sh")
        .args(["-c", &format!("sleep 0.15; {}", quoted.join(" "))])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
