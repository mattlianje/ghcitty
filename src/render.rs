use std::fmt::Write;
use std::time::Duration;

use crate::config::Config;
use crate::highlight;
use crate::parse::{self, Diagnostic, EvalResult};
use crate::style;

const MAX_OUTPUT_CHARS: usize = 3000;

pub fn render(result: &EvalResult, config: &Config, elapsed: Option<Duration>) -> String {
    if !result.diagnostics.is_empty() {
        return render_diagnostics(&result.diagnostics, config);
    }

    if !result.value.is_empty() {
        return render_value_block(result, config, elapsed);
    }

    if parse::is_let_binding(&result.expr) {
        if let Some(ty) = &result.type_str {
            let name = parse::let_bound_name(&result.expr).unwrap_or_default();
            return format!(
                "{}{}\n",
                style::dim().paint(format!("{name} :: {ty}")),
                timing_suffix(config, elapsed),
            );
        }
    }

    String::new()
}

/// After interactive IO (output was already printed live), return diagnostics + type tail.
pub fn render_interactive_tail(
    result: &EvalResult,
    config: &Config,
    elapsed: Option<Duration>,
) -> String {
    if !result.diagnostics.is_empty() {
        return render_diagnostics(&result.diagnostics, config);
    }

    if parse::is_let_binding(&result.expr) {
        return result
            .type_str
            .as_deref()
            .map(|ty| {
                let name = parse::let_bound_name(&result.expr).unwrap_or_default();
                format!(
                    "{}{}\n",
                    style::dim().paint(format!("{name} :: {ty}")),
                    timing_suffix(config, elapsed),
                )
            })
            .unwrap_or_default();
    }

    match &result.type_str {
        Some(ty) if !result.value.is_empty() => {
            let prefix = if result.value.contains('\n') {
                "\n  "
            } else {
                "  "
            };
            format!(
                "{}{}\n",
                style::dim().paint(format!("{prefix}:: {ty}")),
                timing_suffix(config, elapsed),
            )
        }
        _ => String::new(),
    }
}

fn render_diagnostics(diags: &[Diagnostic], config: &Config) -> String {
    let mut out = String::new();
    for d in diags {
        let block = if config.pretty_errors {
            render_diagnostic_pretty(d)
        } else {
            render_diagnostic_raw(d)
        };
        out.push_str(&block);
    }
    out
}

fn render_value_block(result: &EvalResult, config: &Config, elapsed: Option<Duration>) -> String {
    let is_io = result
        .type_str
        .as_deref()
        .map(|t| t.starts_with("IO ") || t == "IO ()")
        .unwrap_or(true);
    let value = if !is_io && config.pretty_values {
        crate::pretty::pretty(&result.value)
    } else {
        result.value.clone()
    };
    let body = if is_io {
        render_plain_truncated(&value, config.max_output_lines)
    } else {
        render_value_truncated(&value, config.max_output_lines)
    };

    let type_tail = result
        .type_str
        .as_deref()
        .map(|ty| {
            let prefix = if value.contains('\n') { "\n  " } else { "  " };
            style::dim().paint(format!("{prefix}:: {ty}")).to_string()
        })
        .unwrap_or_default();

    format!("{body}{type_tail}{}\n", timing_suffix(config, elapsed))
}

fn timing_suffix(config: &Config, elapsed: Option<Duration>) -> String {
    if !config.show_timing {
        return String::new();
    }
    elapsed.map(format_timing).unwrap_or_default()
}

fn format_timing(d: Duration) -> String {
    let secs = d.as_secs_f64();
    let label = if secs < 0.001 {
        "(<1ms)".to_string()
    } else if secs < 1.0 {
        format!("({:.0}ms)", secs * 1000.0)
    } else {
        format!("({:.2}s)", secs)
    };
    format!("  {}", style::dim().paint(label))
}

fn char_truncate(value: &str) -> Option<(&str, usize)> {
    if value.len() <= MAX_OUTPUT_CHARS {
        return None;
    }
    let cut = value[..MAX_OUTPUT_CHARS]
        .rfind(|c: char| c == ',' || c == ' ')
        .unwrap_or(MAX_OUTPUT_CHARS);
    Some((&value[..cut], value.len() - cut))
}

fn truncated_tail(remaining: usize, unit: &str) -> String {
    style::dim()
        .paint(format!("... ({remaining} more {unit})"))
        .to_string()
}

fn render_value_truncated(value: &str, max_lines: usize) -> String {
    if let Some((head, remaining)) = char_truncate(value) {
        return format!(
            "{}\n{}",
            highlight::highlight_input(head),
            truncated_tail(remaining, "chars")
        );
    }
    let lines: Vec<&str> = value.lines().collect();
    if lines.len() <= max_lines {
        return highlight::highlight_input(value);
    }
    let head: String = lines[..max_lines]
        .iter()
        .map(|l| highlight::highlight_input(l) + "\n")
        .collect();
    format!("{head}{}", truncated_tail(lines.len() - max_lines, "lines"))
}

fn render_plain_truncated(value: &str, max_lines: usize) -> String {
    if let Some((head, remaining)) = char_truncate(value) {
        return format!("{head}\n{}", truncated_tail(remaining, "chars"));
    }
    let lines: Vec<&str> = value.lines().collect();
    if lines.len() <= max_lines {
        return value.to_string();
    }
    let head: String = lines[..max_lines]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect();
    format!("{head}{}", truncated_tail(lines.len() - max_lines, "lines"))
}

fn render_diagnostic_raw(d: &Diagnostic) -> String {
    format!("{}\n", style::severity(&d.severity).paint(&d.message))
}

fn render_diagnostic_pretty(d: &Diagnostic) -> String {
    let sev_style = style::severity(&d.severity).bold();
    let mut out = sev_style.paint(&d.severity).to_string();

    if let Some(loc) = &d.location {
        let _ = write!(
            out,
            " {}",
            style::dim().paint(format!("at line {}:{}", loc.line, loc.col))
        );
    }
    out.push('\n');

    if let Some(expected) = &d.expected {
        let _ = writeln!(
            out,
            "  {} {}",
            style::ok().paint("expected:"),
            style::ok().paint(expected.as_str()),
        );
    }
    if let Some(actual) = &d.actual {
        let _ = writeln!(
            out,
            "  {}   {}",
            style::err().paint("actual:"),
            style::err().paint(actual.as_str()),
        );
    }

    let has_exp_act = d.expected.is_some() || d.actual.is_some();
    let has_suggestion = d.suggestion.is_some();
    let first_line = d.message.lines().next().unwrap_or("");
    let inline_body = first_line_body(first_line);
    for line in inline_body.into_iter().chain(d.message.lines().skip(1)) {
        let trimmed = line.trim();
        let drop_exp_act = has_exp_act
            && (trimmed.starts_with("Expected:")
                || trimmed.starts_with("Expected type:")
                || trimmed.starts_with("Actual:")
                || trimmed.starts_with("Actual type:"));
        let drop_sugg = has_suggestion
            && (trimmed.starts_with("Perhaps you meant")
                || trimmed.starts_with("Did you mean")
                || trimmed.starts_with("Suggested fix:"));
        if drop_exp_act || drop_sugg {
            continue;
        }
        let cleaned = trimmed.replace('\u{2018}', "'").replace('\u{2019}', "'");
        if cleaned.is_empty() {
            continue;
        }
        let _ = writeln!(out, "  {}", style::dim().paint(cleaned));
    }

    if let Some(suggestion) = &d.suggestion {
        let _ = writeln!(out, "  {}", style::hint().paint(suggestion.as_str()));
    }

    if let Some(hint) = import_hint(d) {
        let _ = writeln!(out, "  {}", style::hint().paint(hint));
    }

    if let Some(code) = &d.code {
        if let Some(num) = code.strip_prefix("[GHC-").and_then(|s| s.strip_suffix(']')) {
            let url = format!("https://errors.haskell.org/messages/GHC-{num}");
            let label = style::dim().underline().paint(code.as_str()).to_string();
            let _ = writeln!(
                out,
                "  {} {}",
                style::hyperlink(&url, label),
                style::dim().paint(url.as_str()),
            );
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

/// GHC 9.6+ inlines simple diagnostics, e.g.
/// `<interactive>:1:1: error: [GHC-88464] Variable not in scope: x`.
/// Extract the body that trails `error:`/`warning:` and an optional `[GHC-NNNNN]`.
fn first_line_body(line: &str) -> Option<&str> {
    let after_sev = line
        .find("error:")
        .map(|i| &line[i + "error:".len()..])
        .or_else(|| line.find("warning:").map(|i| &line[i + "warning:".len()..]))?;
    let rest = after_sev.trim_start();
    let rest = if let Some(after_bracket) = rest.strip_prefix('[') {
        match after_bracket.find(']') {
            Some(close) => after_bracket[close + 1..].trim_start(),
            None => rest,
        }
    } else {
        rest
    };
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
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
    let empty_msg = || match pattern {
        Some(pat) => format!(
            "{}\n",
            style::dim().paint(format!("(no bindings matching '{pat}')"))
        ),
        None => format!("{}\n", style::dim().paint("(no bindings)")),
    };

    if output.trim().is_empty() {
        return empty_msg();
    }

    let pattern_lower = pattern.map(|p| p.trim().to_lowercase());
    let out: String = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            pattern_lower
                .as_deref()
                .map_or(true, |pat| fuzzy_match(&l.to_lowercase(), pat))
        })
        .map(|line| match line.find("::") {
            Some(sep) => {
                let name = line[..sep].trim();
                let ty = line[sep + 2..].trim();
                format!(
                    "  {} {}\n",
                    style::bold().paint(name),
                    style::dim().paint(format!(":: {ty}"))
                )
            }
            None => format!("  {}\n", highlight::highlight_input(line)),
        })
        .collect();

    if out.is_empty() {
        empty_msg()
    } else {
        out
    }
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

/// Render output from `:l`/`:r`/`:i`/etc. GHC mixes Haskell-shaped output
/// (`:browse`, `:info`) with compiler chatter (errors, "[1 of 2] Compiling..."),
/// so we classify line-by-line and only apply Haskell highlighting to the bits
/// that actually are Haskell.
pub fn render_passthrough(output: &str) -> String {
    let mut out = String::new();
    let mut in_diag: Option<&'static str> = None;

    for line in output.lines() {
        if let Some(sev) = diag_header_severity(line) {
            in_diag = Some(sev);
            out.push_str(&style::severity(sev).paint(line).to_string());
            out.push('\n');
            continue;
        }

        if in_diag.is_some() && (line.is_empty() || line.starts_with(' ') || line.starts_with('\t'))
        {
            out.push_str(&style::dim().paint(line).to_string());
            out.push('\n');
            continue;
        }
        in_diag = None;

        if is_compile_progress(line) {
            out.push_str(&style::dim().paint(line).to_string());
            out.push('\n');
            continue;
        }
        if line.starts_with("Failed") {
            out.push_str(&style::err().paint(line).to_string());
            out.push('\n');
            continue;
        }
        if line.starts_with("Ok,") || line.starts_with("Ok ") {
            out.push_str(&style::ok().paint(line).to_string());
            out.push('\n');
            continue;
        }

        out.push_str(&highlight::highlight_input(line));
        out.push('\n');
    }
    out
}

/// True if `line` looks like a GHC diagnostic header, e.g.
/// `Foo.hs:5:22: error: [GHC-83865]` or `<interactive>:1:1: warning:`.
fn diag_header_severity(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    for sev in ["error", "warning"] {
        let needle = format!("{sev}:");
        if let Some(idx) = trimmed.find(&needle) {
            // Header form: starts with severity, OR has "...: " before it
            // (the ":" rules out "Real type errors" type prose in body lines).
            let prefix_ok = idx == 0 || trimmed[..idx].trim_end().ends_with(':');
            if prefix_ok {
                return Some(if sev == "error" { "error" } else { "warning" });
            }
        }
    }
    None
}

fn is_compile_progress(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('[')
        && trimmed.contains(']')
        && (trimmed.contains("Compiling")
            || trimmed.contains("Loading")
            || trimmed.contains("Linking"))
}

pub fn render_hoogle_results(results: &[crate::hoogle::HoogleResult]) -> String {
    if results.is_empty() {
        return format!("{}\n", style::dim().paint("(no results)"));
    }
    results
        .iter()
        .map(|r| {
            let sig = (!r.signature.is_empty())
                .then(|| {
                    format!(
                        " {} {}",
                        style::dim().paint("::"),
                        style::type_sig().paint(r.signature.as_str()),
                    )
                })
                .unwrap_or_default();
            let module = (!r.module.is_empty())
                .then(|| format!("  {}", style::dim().paint(r.module.as_str())))
                .unwrap_or_default();
            format!("  {}{sig}{module}\n", style::bold().paint(r.name.as_str()))
        })
        .collect()
}

pub fn render_hoogle_doc(result: &crate::hoogle::HoogleResult) -> String {
    let mut out = style::bold().paint(result.name.as_str()).to_string();
    if !result.signature.is_empty() {
        let _ = write!(
            out,
            " {} {}",
            style::dim().paint("::"),
            style::type_sig().paint(result.signature.as_str())
        );
    }
    out.push('\n');

    if !result.module.is_empty() {
        let _ = writeln!(out, "{}", style::dim().paint(result.module.as_str()));
    }

    if !result.doc.is_empty() {
        out.push('\n');
        let mut prev_blank = false;
        for line in result.doc.lines() {
            let is_blank = line.trim().is_empty();
            if is_blank && prev_blank {
                continue;
            }
            if is_blank {
                out.push('\n');
            } else {
                let _ = writeln!(out, "  {}", style::dim().paint(line));
            }
            prev_blank = is_blank;
        }
    }

    if !result.url.is_empty() {
        let _ = writeln!(out, "\n  {}", style::dim().paint(result.url.as_str()));
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
            pretty_values: false,
            show_timing: false,
            max_output_lines: 20,
        }
    }
    fn timing_cfg() -> Config {
        Config {
            pretty_errors: true,
            pretty_values: true,
            show_timing: true,
            max_output_lines: 20,
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
    fn test_render_error_inline_body_ghc96() {
        // GHC 9.6+ collapses simple diagnostics onto a single line.
        let d = Diagnostic {
            severity: "error".into(),
            message: "<interactive>:1:1: error: [GHC-88464] Variable not in scope: x".into(),
            location: Some(parse::DiagLocation { line: 1, col: 1 }),
            code: Some("[GHC-88464]".into()),
            expected: None,
            actual: None,
            suggestion: None,
        };
        let r = EvalResult {
            expr: "x".into(),
            type_str: None,
            value: "".into(),
            diagnostics: vec![d],
        };
        let out = render(&r, &default_cfg(), None);
        let plain = strip_ansi(&out);
        assert!(
            plain.contains("Variable not in scope: x"),
            "missing inline body in: {plain}"
        );
    }

    #[test]
    fn test_first_line_body_extracts_inline() {
        assert_eq!(
            first_line_body("<interactive>:1:1: error: [GHC-88464] Variable not in scope: x"),
            Some("Variable not in scope: x")
        );
        assert_eq!(
            first_line_body("Foo.hs:9:22: warning: [GHC-12345] Unused binding"),
            Some("Unused binding")
        );
        assert_eq!(
            first_line_body("<interactive>:1:1: error: parse error on input 'x'"),
            Some("parse error on input 'x'")
        );
        // Multi-line form: nothing trailing on the header line.
        assert_eq!(
            first_line_body("<interactive>:1:1: error: [GHC-88464]"),
            None
        );
        assert_eq!(first_line_body("<interactive>:1:1: error:"), None);
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
        assert!(out.contains(&style::warn().bold().prefix().to_string()));
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
        assert!(out.contains(&style::dim().prefix().to_string()));
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
        assert!(out.contains(&style::hint().prefix().to_string()));
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

    #[test]
    fn passthrough_does_not_haskell_highlight_errors() {
        // GHC compile error from `:l Foo.hs`. Body mentions `type` and `Char`
        // which the Haskell highlighter would otherwise paint as keyword/type...
        let raw = "[1 of 1] Compiling Main             ( Foo.hs, interpreted )\n\
                   Foo.hs:9:22: error: [GHC-83865]\n    \
                   \u{2022} Couldn't match type \u{2018}[Char]\u{2019} with \u{2018}Int\u{2019}\n      \
                   Expected: Int\n        \
                   Actual: [Char]\nFailed, no modules loaded.\n";
        let out = render_passthrough(raw);

        // Keyword `type` should NOT be wearing the keyword (magenta-bold) prefix
        let kw_prefix = style::keyword().prefix().to_string();
        assert!(
            !out.contains(&kw_prefix),
            "passthrough error output should not be keyword-highlighted: {out:?}"
        );
        // Type-constructor color (cyan) should not appear on `Char`/`Int` either
        let type_prefix = style::type_con().prefix().to_string();
        assert!(
            !out.contains(&type_prefix),
            "passthrough error output should not be type-highlighted: {out:?}"
        );

        let plain = strip_ansi(&out);
        assert!(plain.contains("Couldn't match type"));
        assert!(plain.contains("[1 of 1] Compiling Main"));
        assert!(plain.contains("Failed, no modules loaded."));
    }

    #[test]
    fn passthrough_still_highlights_haskell_lines() {
        let raw = "data Maybe a = Nothing | Just a\n";
        let out = render_passthrough(raw);
        let kw_prefix = style::keyword().prefix().to_string();
        assert!(
            out.contains(&kw_prefix),
            "passthrough should highlight real Haskell: {out:?}"
        );
    }

    #[test]
    fn diag_header_detects_file_location() {
        assert_eq!(
            diag_header_severity("Foo.hs:9:22: error: [GHC-83865]"),
            Some("error")
        );
        assert_eq!(
            diag_header_severity("<interactive>:1:1: warning:"),
            Some("warning")
        );
        assert_eq!(diag_header_severity("error: something"), Some("error"));
        assert_eq!(diag_header_severity("    Real error: in body"), None);
        assert_eq!(
            diag_header_severity("data Maybe a = Nothing | Just a"),
            None
        );
    }
}
