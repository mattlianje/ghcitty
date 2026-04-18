use std::time::Duration;

use crate::config::Config;
use crate::highlight::{self, color::*};
use crate::parse::{self, Diagnostic, EvalResult};

const MAX_OUTPUT_LINES: usize = 20;
const MAX_OUTPUT_CHARS: usize = 3000;

pub fn render(result: &EvalResult, config: &Config, elapsed: Option<Duration>) -> String {
    let mut out = String::new();

    if !result.diagnostics.is_empty() {
        for d in &result.diagnostics {
            if config.pretty_errors {
                out.push_str(&render_diagnostic_pretty(d));
            } else {
                out.push_str(&render_diagnostic_raw(d));
            }
        }
        return out;
    }

    if !result.value.is_empty() {
        let is_io = result
            .type_str
            .as_ref()
            .map(|t| t.starts_with("IO ") || t == "IO ()")
            .unwrap_or(true);
        if is_io {
            out.push_str(&render_plain_truncated(&result.value));
        } else {
            out.push_str(&render_value_truncated(&result.value));
        }
        if let Some(ty) = &result.type_str {
            if result.value.contains('\n') {
                out.push_str(&format!("\n  {DIM}:: {ty}{RESET}"));
            } else {
                out.push_str(&format!("  {DIM}:: {ty}{RESET}"));
            }
        }
        if config.show_timing {
            if let Some(d) = elapsed {
                out.push_str(&format_timing(d));
            }
        }
        out.push('\n');
    } else if parse::is_let_binding(&result.expr) {
        if let Some(ty) = &result.type_str {
            let name = parse::let_bound_name(&result.expr).unwrap_or_default();
            out.push_str(&format!("{DIM}{name} :: {ty}{RESET}"));
            if config.show_timing {
                if let Some(d) = elapsed {
                    out.push_str(&format_timing(d));
                }
            }
            out.push('\n');
        }
    }

    out
}

/// After interactive IO (output was already printed live), show diagnostics + type.
pub fn render_interactive_tail(result: &EvalResult, config: &Config, elapsed: Option<Duration>) {
    if !result.diagnostics.is_empty() {
        for d in &result.diagnostics {
            if config.pretty_errors {
                eprint!("{}", render_diagnostic_pretty(d));
            } else {
                eprint!("{}", render_diagnostic_raw(d));
            }
        }
        return;
    }

    if parse::is_let_binding(&result.expr) {
        if let Some(ty) = &result.type_str {
            let name = parse::let_bound_name(&result.expr).unwrap_or_default();
            print!("{DIM}{name} :: {ty}{RESET}");
            if config.show_timing {
                if let Some(d) = elapsed {
                    print!("{}", format_timing(d));
                }
            }
            println!();
        }
    } else if let Some(ty) = &result.type_str {
        // For expressions, show type if there was visible output
        if !result.value.is_empty() {
            let prefix = if result.value.contains('\n') {
                "\n  "
            } else {
                "  "
            };
            print!("{DIM}{prefix}:: {ty}{RESET}");
            if config.show_timing {
                if let Some(d) = elapsed {
                    print!("{}", format_timing(d));
                }
            }
            println!();
        }
    }
}

fn format_timing(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 0.001 {
        format!("  {DIM}(<1ms){RESET}")
    } else if secs < 1.0 {
        format!("  {DIM}({:.0}ms){RESET}", secs * 1000.0)
    } else {
        format!("  {DIM}({:.2}s){RESET}", secs)
    }
}

fn char_truncate(value: &str) -> Option<(String, usize)> {
    if value.len() <= MAX_OUTPUT_CHARS {
        return None;
    }
    // Find a clean cut point: prefer a comma or space near the limit
    let cut = value[..MAX_OUTPUT_CHARS]
        .rfind(|c: char| c == ',' || c == ' ')
        .unwrap_or(MAX_OUTPUT_CHARS);
    let remaining = value.len() - cut;
    Some((value[..cut].to_string(), remaining))
}

fn render_value_truncated(value: &str) -> String {
    // Character truncation first (handles single long lines)
    if let Some((truncated, remaining)) = char_truncate(value) {
        let mut out = highlight::highlight_input(&truncated);
        out.push_str(&format!("\n{DIM}... ({remaining} more chars){RESET}"));
        return out;
    }
    let lines: Vec<&str> = value.lines().collect();
    if lines.len() <= MAX_OUTPUT_LINES {
        return highlight::highlight_input(value);
    }
    let head: String = lines[..MAX_OUTPUT_LINES]
        .iter()
        .map(|l| highlight::highlight_input(l) + "\n")
        .collect();
    let remaining = lines.len() - MAX_OUTPUT_LINES;
    format!("{head}{DIM}... ({remaining} more lines){RESET}")
}

fn render_plain_truncated(value: &str) -> String {
    // Character truncation first
    if let Some((truncated, remaining)) = char_truncate(value) {
        return format!("{truncated}\n{DIM}... ({remaining} more chars){RESET}");
    }
    let lines: Vec<&str> = value.lines().collect();
    if lines.len() <= MAX_OUTPUT_LINES {
        return value.to_string();
    }
    let head: String = lines[..MAX_OUTPUT_LINES]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect();
    let remaining = lines.len() - MAX_OUTPUT_LINES;
    format!("{head}{DIM}... ({remaining} more lines){RESET}")
}

fn render_diagnostic_raw(d: &Diagnostic) -> String {
    let color = match d.severity.as_str() {
        "warning" => YELLOW,
        _ => RED,
    };
    format!("{color}{}{RESET}\n", d.message)
}

fn render_diagnostic_pretty(d: &Diagnostic) -> String {
    let mut out = String::new();
    let color = match d.severity.as_str() {
        "warning" => YELLOW,
        _ => RED,
    };

    out.push_str(color);
    out.push_str(BOLD);
    out.push_str(&d.severity);
    out.push_str(RESET);

    if let Some(loc) = &d.location {
        out.push_str(&format!(" {DIM}at line {}:{}{RESET}", loc.line, loc.col));
    }
    out.push('\n');

    // Expected vs actual in greeen / red
    // TODO: tweak the alignment so its not just the `:`
    if d.expected.is_some() || d.actual.is_some() {
        if let Some(expected) = &d.expected {
            out.push_str(&format!(
                "  {GREEN}expected:{RESET} {GREEN}{expected}{RESET}\n"
            ));
        }
        if let Some(actual) = &d.actual {
            out.push_str(&format!("  {RED}  actual:{RESET} {RED}{actual}{RESET}\n"));
        }
    }

    // Render the rest of the body, dropping anything already shown above.
    let has_exp_act = d.expected.is_some() || d.actual.is_some();
    let has_suggestion = d.suggestion.is_some();
    for line in d.message.lines().skip(1) {
        let trimmed = line.trim();
        if has_exp_act
            && (trimmed.starts_with("Expected:")
                || trimmed.starts_with("Expected type:")
                || trimmed.starts_with("Actual:")
                || trimmed.starts_with("Actual type:"))
        {
            continue;
        }
        if has_suggestion
            && (trimmed.starts_with("Perhaps you meant")
                || trimmed.starts_with("Did you mean")
                || trimmed.starts_with("Suggested fix:"))
        {
            continue;
        }
        let cleaned = trimmed.replace('\u{2018}', "'").replace('\u{2019}', "'");
        if cleaned.is_empty() {
            continue;
        }
        out.push_str(&format!("  {DIM}{cleaned}{RESET}\n"));
    }

    // Suggestion
    if let Some(suggestion) = &d.suggestion {
        out.push_str(&format!("  {CYAN}{suggestion}{RESET}\n"));
    }

    if let Some(hint) = import_hint(d) {
        out.push_str(&format!("  {CYAN}{hint}{RESET}\n"));
    }

    // Error code URL at the bottom
    if let Some(code) = &d.code {
        if let Some(num) = code.strip_prefix("[GHC-").and_then(|s| s.strip_suffix(']')) {
            let url = format!("https://errors.haskell.org/messages/GHC-{num}");
            out.push_str(&format!(
                "  \x1b]8;;{url}\x1b\\\x1b[4m{DIM}{code}{RESET}\x1b]8;;\x1b\\ {DIM}{url}{RESET}\n"
            ));
        }
    }

    out
}

fn import_hint(d: &Diagnostic) -> Option<String> {
    if !d.message.contains("not in scope") && !d.message.contains("Not in scope") {
        return None;
    }
    // Extract the identifier name from the error
    let name = extract_not_in_scope_name(&d.message)?;
    IMPORT_MAP.iter().find_map(|(func, module)| {
        if *func == name {
            Some(format!("try: :m + {module}"))
        } else {
            None
        }
    })
}

fn extract_not_in_scope_name(msg: &str) -> Option<String> {
    // Patterns: "Variable not in scope: sort", "not in scope: `sort'"
    // "Not in scope: 'sort'", "not in scope: sort :: ..."
    for line in msg.lines() {
        let trimmed = line.trim();
        if let Some(after) = trimmed
            .split("not in scope:")
            .nth(1)
            .or_else(|| trimmed.split("Not in scope:").nth(1))
        {
            let cleaned = after
                .trim()
                .trim_start_matches('`')
                .trim_start_matches('\u{2018}')
                .trim_end_matches('\'')
                .trim_end_matches('\u{2019}')
                .trim();
            // Take just the identifier (before any :: or whitespace)
            let name = cleaned
                .split(|c: char| c.is_whitespace() || c == ':')
                .next()
                .unwrap_or("")
                .trim_start_matches('(')
                .trim_end_matches(')');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

const IMPORT_MAP: &[(&str, &str)] = &[
    // Data.List
    ("sort", "Data.List"),
    ("sortBy", "Data.List"),
    ("sortOn", "Data.List"),
    ("group", "Data.List"),
    ("groupBy", "Data.List"),
    ("nub", "Data.List"),
    ("nubBy", "Data.List"),
    ("partition", "Data.List"),
    ("intercalate", "Data.List"),
    ("intersperse", "Data.List"),
    ("transpose", "Data.List"),
    ("subsequences", "Data.List"),
    ("permutations", "Data.List"),
    ("isPrefixOf", "Data.List"),
    ("isSuffixOf", "Data.List"),
    ("isInfixOf", "Data.List"),
    ("find", "Data.List"),
    ("genericLength", "Data.List"),
    // Data.Map
    ("fromList", "Data.Map"),
    ("toList", "Data.Map"),
    ("singleton", "Data.Map"),
    ("insert", "Data.Map"),
    ("lookup", "Data.Map"),
    ("union", "Data.Map"),
    ("intersection", "Data.Map"),
    ("difference", "Data.Map"),
    // Data.Set
    ("member", "Data.Set"),
    ("empty", "Data.Set"),
    // Data.Char
    ("isAlpha", "Data.Char"),
    ("isDigit", "Data.Char"),
    ("isUpper", "Data.Char"),
    ("isLower", "Data.Char"),
    ("isSpace", "Data.Char"),
    ("toUpper", "Data.Char"),
    ("toLower", "Data.Char"),
    ("digitToInt", "Data.Char"),
    ("intToDigit", "Data.Char"),
    ("ord", "Data.Char"),
    ("chr", "Data.Char"),
    // Data.Maybe
    ("fromMaybe", "Data.Maybe"),
    ("isJust", "Data.Maybe"),
    ("isNothing", "Data.Maybe"),
    ("catMaybes", "Data.Maybe"),
    ("mapMaybe", "Data.Maybe"),
    ("fromJust", "Data.Maybe"),
    // Data.Either
    ("isLeft", "Data.Either"),
    ("isRight", "Data.Either"),
    ("fromLeft", "Data.Either"),
    ("fromRight", "Data.Either"),
    ("partitionEithers", "Data.Either"),
    // Data.IORef
    ("newIORef", "Data.IORef"),
    ("readIORef", "Data.IORef"),
    ("writeIORef", "Data.IORef"),
    ("modifyIORef", "Data.IORef"),
    // System.IO
    ("hFlush", "System.IO"),
    ("hSetBuffering", "System.IO"),
    ("openFile", "System.IO"),
    ("hClose", "System.IO"),
    ("hGetContents", "System.IO"),
    ("hPutStrLn", "System.IO"),
    // Data.Typeable
    ("typeOf", "Data.Typeable"),
    // Control.Monad
    ("when", "Control.Monad"),
    ("unless", "Control.Monad"),
    ("void", "Control.Monad"),
    ("guard", "Control.Monad"),
    ("join", "Control.Monad"),
    ("forM", "Control.Monad"),
    ("forM_", "Control.Monad"),
    ("mapM", "Control.Monad"),
    ("mapM_", "Control.Monad"),
    ("forever", "Control.Monad"),
    // Data.Text
    ("pack", "Data.Text"),
    ("unpack", "Data.Text"),
    // Debug.Trace
    ("trace", "Debug.Trace"),
    ("traceShow", "Debug.Trace"),
    ("traceShowId", "Debug.Trace"),
];

pub fn render_bindings(output: &str, pattern: Option<&str>) -> String {
    if output.trim().is_empty() {
        return format!("{DIM}(no bindings){RESET}\n");
    }

    let mut out = String::new();
    let pattern_lower = pattern.map(|p| p.trim().to_lowercase());

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Filter by pattern (fuzzy substring match)
        if let Some(pat) = &pattern_lower {
            if !fuzzy_match(&trimmed.to_lowercase(), pat) {
                continue;
            }
        }

        // Highlight: name in bold, type dimmed
        if let Some(sep) = trimmed.find("::") {
            let name = &trimmed[..sep].trim();
            let ty = &trimmed[sep + 2..].trim();
            out.push_str(&format!("  {BOLD}{name}{RESET} {DIM}:: {ty}{RESET}\n"));
        } else {
            out.push_str(&format!(
                "  {}{}\n",
                highlight::highlight_input(trimmed),
                RESET
            ));
        }
    }

    if out.is_empty() {
        if let Some(pat) = pattern {
            return format!("{DIM}(no bindings matching '{pat}'){RESET}\n");
        }
        return format!("{DIM}(no bindings){RESET}\n");
    }

    out
}

fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let mut hay_chars = haystack.chars();
    for ch in needle.chars() {
        loop {
            match hay_chars.next() {
                Some(h) if h == ch => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

pub fn render_passthrough(output: &str) -> String {
    output
        .lines()
        .map(|line| highlight::highlight_input(line) + "\n")
        .collect()
}

pub fn render_hoogle_results(results: &[crate::hoogle::HoogleResult]) -> String {
    if results.is_empty() {
        return format!("{DIM}(no results){RESET}\n");
    }
    results
        .iter()
        .map(|r| {
            let sig = if r.signature.is_empty() {
                String::new()
            } else {
                format!(" {DIM}::{RESET} {CYAN}{}{RESET}", r.signature)
            };
            let module = if r.module.is_empty() {
                String::new()
            } else {
                format!("  {DIM}{}{RESET}", r.module)
            };
            format!("  {BOLD}{}{RESET}{sig}{module}\n", r.name)
        })
        .collect()
}

pub fn render_hoogle_doc(result: &crate::hoogle::HoogleResult) -> String {
    let mut out = String::new();

    // Header: name :: signature
    out.push_str(&format!("{BOLD}{}{RESET}", result.name));
    if !result.signature.is_empty() {
        out.push_str(&format!(
            " {DIM}::{RESET} {CYAN}{}{RESET}",
            result.signature
        ));
    }
    out.push('\n');

    // Module
    if !result.module.is_empty() {
        out.push_str(&format!("{DIM}{}{RESET}\n", result.module));
    }

    if !result.doc.is_empty() {
        out.push('\n');
        let mut prev_blank = false;
        // TODO: better way of collapsing blank lines into one
        // or aybe straight from Hoogle...a bit dubious
        for line in result.doc.lines() {
            let is_blank = line.trim().is_empty();
            if is_blank && prev_blank {
                continue;
            }
            if is_blank {
                out.push('\n');
            } else {
                out.push_str(&format!("  {DIM}{line}{RESET}\n"));
            }
            prev_blank = is_blank;
        }
    }

    // URL
    if !result.url.is_empty() {
        out.push_str(&format!("\n  {DIM}{}{RESET}\n", result.url));
    }

    out
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '\x1b' && i + 1 < len {
            match chars[i + 1] {
                '[' => {
                    // CSI: skip until letter
                    i += 2;
                    while i < len && !chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    if i < len {
                        i += 1;
                    } // skip the final letter
                }
                ']' => {
                    // OSC: skip until ST (\x1b\\ or \x07)
                    i += 2;
                    while i < len {
                        if chars[i] == '\x07' {
                            i += 1;
                            break;
                        }
                        if chars[i] == '\x1b' && i + 1 < len && chars[i + 1] == '\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    i += 2;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_diag(severity: &str, message: &str) -> Diagnostic {
        parse::simple_diagnostic(severity, message.to_string())
    }

    fn default_cfg() -> Config {
        Config::default()
    }
    fn raw_cfg() -> Config {
        Config {
            pretty_errors: false,
            show_timing: false,
        }
    }
    fn timing_cfg() -> Config {
        Config {
            pretty_errors: true,
            show_timing: true,
        }
    }

    #[test]
    fn test_render_value_with_type() {
        let r = EvalResult {
            expr: "1+1".into(),
            type_str: Some("Integer".into()),
            value: "2".into(),
            diagnostics: vec![],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("2"));
        assert!(plain.contains(":: Integer"));
    }

    #[test]
    fn test_render_value_without_type() {
        let r = EvalResult {
            expr: "putStrLn \"hi\"".into(),
            type_str: None,
            value: "hi".into(),
            diagnostics: vec![],
        };
        let out = render(&r, &default_cfg(), None);
        assert!(out.contains("hi"));
        assert!(!out.contains("::"));
    }

    #[test]
    fn test_render_error_pretty() {
        let r = EvalResult {
            expr: "foo".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![make_diag("error", "Variable not in scope: foo")],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("error"));
        // No emoji
        assert!(!plain.contains('\u{2718}'));
        assert!(!plain.contains('\u{26a0}'));
    }

    #[test]
    fn test_render_error_raw() {
        let r = EvalResult {
            expr: "foo".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![make_diag("error", "Variable not in scope: foo")],
        };
        let out = render(&r, &raw_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("Variable not in scope: foo"));
    }

    #[test]
    fn test_render_error_with_expected_actual() {
        let d = Diagnostic {
            severity: "error".into(),
            message: "<interactive>:3:29: error: [GHC-83865]\n    Couldn't match\n      Expected: String\n        Actual: Int".into(),
            location: Some(parse::DiagLocation { line: 3, col: 29 }),
            code: Some("[GHC-83865]".into()),
            expected: Some("String".into()),
            actual: Some("Int".into()),
            suggestion: None,
        };
        let r = EvalResult {
            expr: "bad".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![d],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("expected:"));
        assert!(plain.contains("String"));
        assert!(plain.contains("actual:"));
        assert!(plain.contains("Int"));
        assert!(plain.contains("line 3:29"));
    }

    #[test]
    fn test_render_warning() {
        let r = EvalResult {
            expr: "x".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![make_diag("warning", "some warning")],
        };
        let out = render(&r, &default_cfg(), None);
        assert!(out.contains(YELLOW));
    }

    #[test]
    fn test_render_empty_value_no_output() {
        let r = EvalResult {
            expr: "putStrLn \"hi\"".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![],
        };
        let out = render(&r, &default_cfg(), None);
        assert_eq!(out, "");
    }

    #[test]
    fn test_render_let_binding_shows_type() {
        let r = EvalResult {
            expr: "let f x y = x + y".into(),
            type_str: Some("Num a => a -> a -> a".into()),
            value: "".into(),
            diagnostics: vec![],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("f :: Num a => a -> a -> a"));
        assert!(out.contains(DIM));
    }

    #[test]
    fn test_render_let_binding_no_type() {
        let r = EvalResult {
            expr: "let x = 1".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![],
        };
        let out = render(&r, &default_cfg(), None);
        assert_eq!(out, "");
    }

    #[test]
    fn test_render_suggestion() {
        let d = Diagnostic {
            severity: "error".into(),
            message: "<interactive>:1:1: error:\n    Not in scope".into(),
            location: None,
            code: None,
            expected: None,
            actual: None,
            suggestion: Some("Perhaps you meant 'fmap'".into()),
        };
        let r = EvalResult {
            expr: "fma".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![d],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("Perhaps you meant"));
        assert!(out.contains(CYAN));
    }

    #[test]
    fn test_render_timing() {
        let r = EvalResult {
            expr: "1+1".into(),
            type_str: Some("Integer".into()),
            value: "2".into(),
            diagnostics: vec![],
        };
        let d = Duration::from_millis(42);
        let out = render(&r, &timing_cfg(), Some(d));
        let plain = strip_ansi(&out);
        assert!(plain.contains("42ms"));
    }

    #[test]
    fn test_render_timing_slow() {
        let r = EvalResult {
            expr: "1+1".into(),
            type_str: Some("Integer".into()),
            value: "2".into(),
            diagnostics: vec![],
        };
        let d = Duration::from_secs_f64(1.5);
        let out = render(&r, &timing_cfg(), Some(d));
        let plain = strip_ansi(&out);
        assert!(plain.contains("1.50s"));
    }

    #[test]
    fn test_render_timing_off_by_default() {
        let r = EvalResult {
            expr: "1+1".into(),
            type_str: Some("Integer".into()),
            value: "2".into(),
            diagnostics: vec![],
        };
        let d = Duration::from_millis(42);
        let out = render(&r, &default_cfg(), Some(d));
        let plain = strip_ansi(&out);
        assert!(!plain.contains("ms"));
    }

    #[test]
    fn test_render_truncation() {
        let long_value = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let r = EvalResult {
            expr: "big".into(),
            type_str: None,
            value: long_value,
            diagnostics: vec![],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("... (30 more lines)"));
        assert!(plain.contains("line 0"));
        assert!(plain.contains("line 19"));
        assert!(!plain.contains("line 20"));
    }

    #[test]
    fn test_import_hint_sort() {
        let d = Diagnostic {
            severity: "error".into(),
            message: "Variable not in scope: sort".into(),
            location: None,
            code: None,
            expected: None,
            actual: None,
            suggestion: None,
        };
        let r = EvalResult {
            expr: "sort [3,1,2]".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![d],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(plain.contains("try: :m + Data.List"));
    }

    #[test]
    fn test_no_import_hint_for_known() {
        let d = make_diag("error", "Variable not in scope: myCustomFunc");
        let hint = import_hint(&d);
        assert!(hint.is_none());
    }
}
