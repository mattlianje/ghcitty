use std::fmt::Display;

use nu_ansi_term::{Color, Style};

pub fn dim() -> Style {
    Style::new().dimmed()
}
pub fn bold() -> Style {
    Style::new().bold()
}

pub fn keyword() -> Style {
    Style::new().bold().fg(Color::Magenta)
}
pub fn type_con() -> Style {
    Style::new().fg(Color::Cyan)
}
pub fn type_sig() -> Style {
    Style::new().fg(Color::Cyan)
}
pub fn string_lit() -> Style {
    Style::new().fg(Color::Green)
}
pub fn number() -> Style {
    Style::new().fg(Color::Yellow)
}
pub fn operator() -> Style {
    Style::new().fg(Color::Blue)
}
pub fn ghci_cmd() -> Style {
    Style::new().bold().fg(Color::Cyan)
}

pub fn err() -> Style {
    Style::new().fg(Color::Red)
}
pub fn warn() -> Style {
    Style::new().fg(Color::Yellow)
}
pub fn ok() -> Style {
    Style::new().fg(Color::Green)
}
pub fn hint() -> Style {
    Style::new().fg(Color::Cyan)
}

pub fn severity(sev: &str) -> Style {
    match sev {
        "warning" => warn(),
        _ => err(),
    }
}

/// OSC 8 hyperlink
pub fn hyperlink(url: &str, text: impl Display) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}
