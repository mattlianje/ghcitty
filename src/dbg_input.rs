//! Raw-key input loop for the `(dbg)` prompt.
//!
//! At a breakpoint reedline is bypassed: shortcut keys (`s`/`n`/`c`/`b`/`q`/
//! `l`/`h`) fire on press, Enter alone steps, and any other key drops into a
//! small buffered editor.

use std::io::{self, Write};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// Outcome of one dbg-prompt read.
pub enum DbgInput {
    Command(String),
    Cancel,
    Quit,
}

/// Draw the `(dbg)` prompt and block on a single keypress / buffered line.
pub fn read_dbg_input() -> io::Result<DbgInput> {
    print_prompt("");
    enable_raw_mode()?;
    let guard = RawGuard;

    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        // With the kitty protocol crossterm emits Press, Repeat, and Release.
        // Ignore Release; Press and Repeat both count (so held keys repeat).
        if matches!(key.kind, KeyEventKind::Release) {
            continue;
        }

        // Ctrl modifiers short-circuit everything.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => {
                    drop(guard);
                    println!();
                    return Ok(DbgInput::Cancel);
                }
                KeyCode::Char('d') => {
                    drop(guard);
                    println!();
                    return Ok(DbgInput::Quit);
                }
                _ => continue,
            }
        }

        match key.code {
            KeyCode::Enter => {
                drop(guard);
                println!();
                return Ok(DbgInput::Command(":step".into()));
            }
            KeyCode::Char(c) => {
                if let Some(cmd) = shortcut_for(c) {
                    // Echo what fired, then newline.
                    print!("{c}");
                    io::stdout().flush().ok();
                    drop(guard);
                    println!();
                    return Ok(DbgInput::Command(cmd.into()));
                }
                // Not a shortcut: start buffered mode with `c` already in.
                let mut buf = String::new();
                push_char(&mut buf, c);
                let result = buffered_read(&mut buf, key)?;
                drop(guard);
                println!();
                return Ok(result);
            }
            _ => continue,
        }
    }
}

/// Read into `buf` until Enter, handling backspace and Ctrl-C.
fn buffered_read(buf: &mut String, _first_key: KeyEvent) -> io::Result<DbgInput> {
    loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if matches!(key.kind, KeyEventKind::Release) {
            continue;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return Ok(DbgInput::Cancel),
                KeyCode::Char('d') => return Ok(DbgInput::Quit),
                _ => continue,
            }
        }

        match key.code {
            KeyCode::Enter => return Ok(DbgInput::Command(buf.clone())),
            KeyCode::Backspace => {
                if buf.pop().is_some() {
                    // Erase one column: back up, overwrite with space, back up.
                    print!("\x08 \x08");
                    io::stdout().flush().ok();
                }
            }
            KeyCode::Char(c) => {
                push_char(buf, c);
            }
            _ => continue,
        }
    }
}

fn push_char(buf: &mut String, c: char) {
    buf.push(c);
    print!("{c}");
    io::stdout().flush().ok();
}

/// Map a keypress to its GHCi command, or `None` if it's not a shortcut.
fn shortcut_for(c: char) -> Option<&'static str> {
    match c {
        's' => Some(":step"),
        'n' => Some(":steplocal"),
        'c' => Some(":continue"),
        'b' => Some(":back"),
        'q' => Some(":abandon"),
        'l' => Some(":list"),
        'h' => Some(":history"),
        _ => None,
    }
}

fn print_prompt(initial_buffer: &str) {
    // Red `(dbg) ` plus the current buffer.
    print!("\r\x1b[2K\x1b[31m(dbg)\x1b[0m {initial_buffer}");
    io::stdout().flush().ok();
}

/// RAII guard so a panic in the loop doesn't leave the terminal in raw mode.
struct RawGuard;
impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_table() {
        assert_eq!(shortcut_for('s'), Some(":step"));
        assert_eq!(shortcut_for('n'), Some(":steplocal"));
        assert_eq!(shortcut_for('c'), Some(":continue"));
        assert_eq!(shortcut_for('q'), Some(":abandon"));
        assert_eq!(shortcut_for('p'), None);
        assert_eq!(shortcut_for('x'), None);
        assert_eq!(shortcut_for(':'), None);
    }
}
