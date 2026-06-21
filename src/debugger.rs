//! Debugger frontend: parses GHCi's `Stopped in ...` output into a Frame and
//! renders a source/locals panel.
//!
//! Detection is a pure transform: callers pass raw GHCi output to [`observe`],
//! which returns a [`Frame`] on a breakpoint hit. The REPL owns the state.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::highlight;
use crate::style;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub function: String,
    pub file: PathBuf,
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub locals: Vec<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub ty: Option<String>,
    pub value: String, // "_" for an unforced thunk
}

/// Source-file cache used to render stopped frames. GHCi owns breakpoints
/// (`:break` passes straight through), so there's nothing else to track.
#[derive(Default)]
pub struct DebuggerState {
    source_cache: HashMap<PathBuf, (SystemTime, Vec<String>)>,
}

/// Commands that may resume out of the debugger. After one runs, if no fresh
/// `Stopped` frame appears, we clear `active`.
pub fn is_step_or_continue(cmd: &str) -> bool {
    let head = cmd.split_whitespace().next().unwrap_or("");
    matches!(
        head,
        ":continue" | ":c"
        | ":abandon"
        | ":step" | ":s"
        | ":steplocal"
        | ":stepmodule"
        | ":back"
        | ":forward"
    )
}

/// Scan `text` for a `Stopped in ...` block. Returns the parsed [`Frame`]
/// on the first match, or `None`.
pub fn observe(text: &str) -> Option<Frame> {
    let lines: Vec<&str> = text.lines().collect();
    let stop_idx = lines.iter().position(|l| l.trim_start().starts_with("Stopped in "))?;
    let header = lines[stop_idx].trim_start();
    let (function, file, line, col_start, col_end) = parse_stop_header(header)?;

    let mut locals = Vec::new();
    for raw in &lines[stop_idx + 1..] {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(local) = parse_local(trimmed) {
            locals.push(local);
        } else {
            break;
        }
    }

    Some(Frame {
        function,
        file: PathBuf::from(file),
        line,
        col_start,
        col_end,
        locals,
    })
}

/// Parse `Stopped in Main.doThing, <span>`. GHC spans come in two forms:
///   single-line:  `<path>:<line>:<col1>-<col2>`  or  `<path>:<line>:<col>`
///   multi-line:   `<path>:(<line1>,<col1>)-(<line2>,<col2>)`
/// Multi-line spans render the start line and treat its end column as end-of-line.
fn parse_stop_header(line: &str) -> Option<(String, String, usize, usize, usize)> {
    let rest = line.strip_prefix("Stopped in ")?;
    let comma = rest.find(", ")?;
    let function = rest[..comma].trim().to_string();
    let loc = rest[comma + 2..].trim();

    // Multi-line form: split at the last `:(` so paths containing `:` survive.
    if let Some(paren_idx) = loc.find(":(") {
        let path = loc[..paren_idx].to_string();
        let span = &loc[paren_idx + 1..]; // `(l1,c1)-(l2,c2)`
        let (line_num, col_start, col_end) = parse_paren_span(span)?;
        return Some((function, path, line_num, col_start, col_end));
    }

    // Single-line form: `<path>:<line>:<colspan>`
    let last_colon = loc.rfind(':')?;
    let second_last = loc[..last_colon].rfind(':')?;
    let path = loc[..second_last].to_string();
    let line_str = &loc[second_last + 1..last_colon];
    let col_str = &loc[last_colon + 1..];
    let line_num: usize = line_str.parse().ok()?;
    let (col_start, col_end) = parse_col_span(col_str)?;
    Some((function, path, line_num, col_start, col_end))
}

/// `(line1,col1)-(line2,col2)` -> (line1, col1, col2 if same line else usize::MAX)
fn parse_paren_span(s: &str) -> Option<(usize, usize, usize)> {
    let s = s.strip_prefix('(')?;
    let end_first = s.find(')')?;
    let first = &s[..end_first];
    let (l1, c1) = first.split_once(',')?;
    let line1: usize = l1.trim().parse().ok()?;
    let col1: usize = c1.trim().parse().ok()?;

    let after = &s[end_first + 1..];
    let after = after.strip_prefix("-(")?;
    let end_second = after.find(')')?;
    let second = &after[..end_second];
    let (l2, c2) = second.split_once(',')?;
    let line2: usize = l2.trim().parse().ok()?;
    let col2: usize = c2.trim().parse().ok()?;

    // Multi-line span: underline to end of line.
    let col_end = if line2 == line1 { col2 } else { usize::MAX };
    Some((line1, col1, col_end))
}

/// Accepts `7` or `7-19`.
fn parse_col_span(s: &str) -> Option<(usize, usize)> {
    if let Some(dash) = s.find('-') {
        let start: usize = s[..dash].parse().ok()?;
        let end: usize = s[dash + 1..].parse().ok()?;
        Some((start, end))
    } else {
        let v: usize = s.parse().ok()?;
        Some((v, v))
    }
}

/// Parse `name :: Type = value` or `name :: Type = _`.
fn parse_local(line: &str) -> Option<Local> {
    let sep = line.find(" :: ")?;
    let name = line[..sep].trim().to_string();
    if name.is_empty() || name.contains(' ') {
        return None;
    }
    let rest = &line[sep + 4..];
    let (ty, value) = if let Some(eq) = find_top_level_eq(rest) {
        (Some(rest[..eq].trim().to_string()), rest[eq + 1..].trim().to_string())
    } else {
        (Some(rest.trim().to_string()), String::from("_"))
    };
    Some(Local { name, ty, value })
}

/// Find a `=` not inside parens/brackets. Type signatures don't contain raw
/// `=` at depth 0, so the first one separates the type from the value.
fn find_top_level_eq(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '=' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// After a `Stopped` block, GHCi echoes the location as `[Foo.hs:42:7-19]`.
/// The panel header already shows it, so drop those lines.
pub fn strip_location_echo(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if is_location_echo(trimmed) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn is_location_echo(line: &str) -> bool {
    // `[path:line:col]` or `[path:line:col-col]` or `[path:(l,c)-(l,c)]`
    let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return false;
    };
    // Must contain at least one ':' and a digit somewhere after the path.
    inner.contains(':') && inner.chars().any(|c| c.is_ascii_digit())
}

/// Render the frame panel: source snippet + locals + footer hint.
pub fn render_frame(frame: &Frame, dbg: &mut DebuggerState) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&render_header(frame));
    out.push('\n');
    out.push_str(&render_source(frame, dbg));
    out.push('\n');
    out.push_str(&render_locals(frame));
    out.push_str(&render_footer());
    out
}

fn render_header(frame: &Frame) -> String {
    let file_disp = short_path(&frame.file);
    format!(
        "{} {} {} {} {} {}\n",
        style::err().paint("●"),
        style::bold().paint("break"),
        style::dim().paint("·"),
        style::hint().paint(format!(
            "{}:{}:{}",
            file_disp,
            frame.line,
            col_span(frame.col_start, frame.col_end)
        )),
        style::dim().paint("in"),
        style::bold().paint(&frame.function),
    )
}

/// Format a column span like GHCi: `10` for one column, `10-25` for a range.
/// A multi-line span (`col_end == usize::MAX`) shows just the start column.
fn col_span(col_start: usize, col_end: usize) -> String {
    if col_end == usize::MAX || col_end <= col_start {
        col_start.to_string()
    } else {
        format!("{col_start}-{col_end}")
    }
}

fn render_source(frame: &Frame, dbg: &mut DebuggerState) -> String {
    let lines = match load_source(&frame.file, dbg) {
        Some(v) => v,
        None => {
            return format!(
                "  {}\n",
                style::dim().paint(format!("(source unavailable: {})", frame.file.display()))
            );
        }
    };

    const CONTEXT: usize = 3;
    let target = frame.line.saturating_sub(1); // 0-indexed
    let start = target.saturating_sub(CONTEXT);
    let end = (target + CONTEXT + 1).min(lines.len());
    if start >= lines.len() {
        return format!(
            "  {}\n",
            style::dim().paint(format!("(line {} past end of file)", frame.line))
        );
    }

    let max_line_no = end; // for width
    let width = max_line_no.to_string().len();

    let mut out = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let line_no = start + i + 1;
        let is_current = line_no == frame.line;
        let gutter = if is_current {
            style::err().paint(">").to_string()
        } else {
            " ".to_string()
        };
        let lineno_text = format!("{line_no:>width$}", width = width);
        let lineno_styled = if is_current {
            style::bold().paint(lineno_text).to_string()
        } else {
            style::dim().paint(lineno_text).to_string()
        };
        let body = if is_current {
            highlight_with_caret(line, frame.col_start, frame.col_end)
        } else {
            highlight::highlight_input(line)
        };
        out.push_str(&format!(
            " {} {} {} {}\n",
            gutter,
            lineno_styled,
            style::dim().paint("│"),
            body,
        ));
    }
    out
}

/// Highlight the line and underline the breakpoint span. GHCi columns are
/// 1-indexed; end_col == start_col is a single point. `usize::MAX` (from
/// [`parse_paren_span`] on a cross-line span) underlines to end of line.
fn highlight_with_caret(line: &str, col_start: usize, col_end: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let lo = col_start.saturating_sub(1).min(chars.len());
    let hi = col_end.min(chars.len());
    if lo >= hi {
        return highlight::highlight_input(line);
    }
    let before: String = chars[..lo].iter().collect();
    let span: String = chars[lo..hi].iter().collect();
    let after: String = chars[hi..].iter().collect();

    let mut out = String::new();
    out.push_str(&highlight::highlight_input(&before));
    out.push_str(
        &nu_ansi_term::Style::new()
            .underline()
            .bold()
            .paint(span)
            .to_string(),
    );
    out.push_str(&highlight::highlight_input(&after));
    out
}

fn render_locals(frame: &Frame) -> String {
    if frame.locals.is_empty() {
        return format!(
            "  {}\n\n",
            style::dim().paint("(no locals in scope)")
        );
    }
    let name_w = frame.locals.iter().map(|l| l.name.len()).max().unwrap_or(0);
    let type_w = frame
        .locals
        .iter()
        .map(|l| l.ty.as_deref().map(str::len).unwrap_or(0))
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    out.push_str(&format!("  {}\n", style::dim().paint("locals")));
    for local in &frame.locals {
        let name = format!("{:<name_w$}", local.name, name_w = name_w);
        let ty = local
            .ty
            .as_deref()
            .map(|t| format!(":: {:<type_w$}", t, type_w = type_w))
            .unwrap_or_default();
        let value = if local.value == "_" {
            format!(
                "= {}  {}",
                style::dim().paint("_"),
                style::dim().paint("(thunk)"),
            )
        } else {
            format!("= {}", local.value)
        };
        out.push_str(&format!(
            "    {} {} {}\n",
            style::bold().paint(name),
            style::dim().paint(ty),
            value,
        ));
    }
    out.push('\n');
    out
}

fn render_footer() -> String {
    format!(
        "  {}\n  {}\n",
        style::dim().paint(
            "s step   n next   c cont   b back   p <name> print   l list   h history   q quit"
        ),
        style::dim().paint("(press Enter alone to step)"),
    )
}

fn load_source<'a>(path: &Path, dbg: &'a mut DebuggerState) -> Option<&'a Vec<String>> {
    let mtime = fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let needs_reload = dbg
        .source_cache
        .get(path)
        .map(|(t, _)| *t != mtime)
        .unwrap_or(true);
    if needs_reload {
        let content = fs::read_to_string(path).ok()?;
        let lines = content.lines().map(|l| l.to_string()).collect();
        dbg.source_cache.insert(path.to_path_buf(), (mtime, lines));
    }
    dbg.source_cache.get(path).map(|(_, v)| v)
}

/// Trim the cwd prefix for display.
fn short_path(p: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(stripped) = p.strip_prefix(&cwd) {
            return stripped.display().to_string();
        }
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_stop() {
        let out = "Stopped in Main.doThing, /tmp/Foo.hs:42:7-19\n_result :: Int = _\nn :: Int = 5\n";
        let f = observe(out).expect("frame");
        assert_eq!(f.function, "Main.doThing");
        assert_eq!(f.file, PathBuf::from("/tmp/Foo.hs"));
        assert_eq!(f.line, 42);
        assert_eq!(f.col_start, 7);
        assert_eq!(f.col_end, 19);
        assert_eq!(f.locals.len(), 2);
        assert_eq!(f.locals[0].name, "_result");
        assert_eq!(f.locals[0].value, "_");
        assert_eq!(f.locals[1].name, "n");
        assert_eq!(f.locals[1].value, "5");
    }

    #[test]
    fn parses_multiline_paren_span() {
        let out = "Stopped in Main.doThing, /tmp/Foo.hs:(42,7)-(43,9)\n";
        let f = observe(out).expect("frame");
        assert_eq!(f.line, 42);
        assert_eq!(f.col_start, 7);
        assert_eq!(f.col_end, usize::MAX);
    }

    #[test]
    fn parses_single_line_paren_span() {
        let out = "Stopped in Main.doThing, /tmp/Foo.hs:(42,7)-(42,19)\n";
        let f = observe(out).expect("frame");
        assert_eq!(f.line, 42);
        assert_eq!(f.col_start, 7);
        assert_eq!(f.col_end, 19);
    }

    #[test]
    fn parses_point_span() {
        let out = "Stopped in Main.go, /a/b/C.hs:1:1\n";
        let f = observe(out).expect("frame");
        assert_eq!(f.line, 1);
        assert_eq!(f.col_start, 1);
        assert_eq!(f.col_end, 1);
    }

    #[test]
    fn no_stop_returns_none() {
        assert!(observe("hello\nworld\n").is_none());
    }

    #[test]
    fn col_span_formats() {
        assert_eq!(col_span(10, 25), "10-25");
        assert_eq!(col_span(7, 7), "7"); // single point
        assert_eq!(col_span(7, usize::MAX), "7"); // multi-line span
        assert_eq!(col_span(10, 5), "10"); // degenerate, never below start
    }

    #[test]
    fn parses_local_with_function_type() {
        let l = parse_local("f :: Int -> Int = <fun>").unwrap();
        assert_eq!(l.name, "f");
        assert_eq!(l.ty.as_deref(), Some("Int -> Int"));
        assert_eq!(l.value, "<fun>");
    }

    #[test]
    fn parses_thunk_local() {
        let l = parse_local("xs :: [Int] = _").unwrap();
        assert_eq!(l.name, "xs");
        assert_eq!(l.value, "_");
    }

    /// Visual sanity check. Run with:
    ///     cargo test --bin ghcitty debugger::tests::render_demo -- --ignored --nocapture
    #[test]
    #[ignore]
    fn render_demo() {
        let fixture_path = "/tmp/ghcitty-dbg-smoke/Fact.hs";
        // Skip if the fixture isn't set up.
        if std::fs::metadata(fixture_path).is_err() {
            eprintln!("skipping: write a Haskell file at {fixture_path} first");
            return;
        }
        let raw = format!(
            "Stopped in Fact.fact, {fixture_path}:5:1-26\n\
             _result :: Int = _\n\
             n :: Int = 3\n"
        );
        let frame = observe(&raw).expect("frame");
        let mut dbg = DebuggerState::default();
        let out = render_frame(&frame, &mut dbg);
        println!("{out}");
        assert!(out.contains("break"));
        assert!(out.contains("locals"));
    }

    #[test]
    fn strips_location_echo() {
        assert!(is_location_echo("[Foo.hs:42:7-19]"));
        assert!(is_location_echo("[/tmp/Bar.hs:1:1]"));
        assert!(is_location_echo("[Demo.hs:(3,5)-(4,7)]"));
        assert!(!is_location_echo("[1, 2, 3]"));
        assert!(!is_location_echo("[]"));
        assert!(!is_location_echo("plain text"));

        let cleaned = strip_location_echo("first\n[Foo.hs:1:2-3]\nsecond\n");
        assert_eq!(cleaned, "first\nsecond\n");
    }

    #[test]
    fn step_or_continue_detection() {
        assert!(is_step_or_continue(":c"));
        assert!(is_step_or_continue(":continue"));
        assert!(is_step_or_continue(":step"));
        assert!(is_step_or_continue(":steplocal"));
        assert!(is_step_or_continue(":abandon"));
        assert!(!is_step_or_continue(":print xs"));
        assert!(!is_step_or_continue("n + 1"));
    }
}
