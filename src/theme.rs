//! The grove palette, taken from the design simulator. Used by the sidebar
//! (ratatui colours) and by everything grove writes into tmux (hex strings).

use ratatui::style::Color;

pub const WAIT: &str = "#f0b03f";
pub const WORK: &str = "#6fb3ff";
pub const DONE: &str = "#7fd492";
pub const IDLE: &str = "#5d6877";
pub const FG: &str = "#d5dde7";
pub const DIM: &str = "#7b889a";
pub const FAINT: &str = "#4d5a6b";
pub const LINE: &str = "#273241";
pub const LINE_HI: &str = "#3a4a5e";
pub const ERR: &str = "#ff7b72";
pub const POPUP_BG: &str = "#10161f";
pub const CURRENT_BG: &str = "#1f2a38";

pub fn color(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
