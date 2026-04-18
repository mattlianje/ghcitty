use serde::Serialize;

pub const SENTINEL: &str = "___GHCITTY_SENTINEL___";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Diagnostic {
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiagLocation {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EvalResult {
    pub expr: String,
    #[serde(rename = "type")]
    pub type_str: Option<String>,
    pub value: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn extract_between_sentinels(raw: &str) -> &str {
    let s = raw.trim();
    let s = s.strip_prefix(SENTINEL).unwrap_or(s).trim();
    s.strip_suffix(SENTINEL).unwrap_or(s).trim()
}

pub fn parse_type_output(raw: &str) -> Option<String> {
    let cleaned = extract_between_sentinels(raw);
    // GHCi prints: `expr :: SomeType`
    // Handle multiline types (they continue with leading whitespace)
    let full = cleaned
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ");

    let (_before, after) = full.split_once("::")?;
    Some(after.trim().to_string())
}

pub fn parse_eval_output(raw: &str) -> (String, Vec<Diagnostic>) {
    let cleaned = extract_between_sentinels(raw);
    let mut value_lines = Vec::new();
    let mut diag_blocks: Vec<String> = Vec::new();

    for line in cleaned.lines() {
        if line.starts_with("<interactive>:")
            || line.starts_with("<no location info>:")
            || line.starts_with("error:")
            || line.starts_with("warning:")
        {
            // Start a new diagnostic block
            diag_blocks.push(line.to_string());
        } else if !diag_blocks.is_empty()
            && (line.starts_with(' ') || line.starts_with('\t') || line.is_empty())
        {
            if let Some(last) = diag_blocks.last_mut() {
                last.push('\n');
                last.push_str(line);
            }
        } else {
            value_lines.push(line);
        }
    }

    let diagnostics = diag_blocks
        .into_iter()
        .map(|block| parse_diagnostic(&block))
        .collect();
    let value = value_lines.join("\n");
    (value, diagnostics)
}

fn parse_diagnostic(block: &str) -> Diagnostic {
    let mut severity = "error".to_string();
    let mut location = None;
    let mut code = None;
    let mut expected = None;
    let mut actual = None;
    let mut suggestion = None;

    let first_line = block.lines().next().unwrap_or("");

    // Parse location: <interactive>:LINE:COL:
    if first_line.starts_with("<interactive>:") {
        let parts: Vec<&str> = first_line.splitn(4, ':').collect();
        if parts.len() >= 3 {
            if let (Ok(line), Ok(col)) = (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                location = Some(DiagLocation { line, col });
            }
        }
    }

    // Parse severity and code: "error: [GHC-12345]" or "warning: [GHC-12345]"
    if let Some(idx) = first_line.find("warning:") {
        severity = "warning".to_string();
        code = extract_ghc_code(&first_line[idx..]);
    } else if let Some(idx) = first_line.find("error:") {
        severity = "error".to_string();
        code = extract_ghc_code(&first_line[idx..]);
    }

    // Parse body for expected/actual and suggestions
    for line in block.lines().skip(1) {
        let trimmed = line.trim();
        // Remove bullet character
        let content = trimmed
            .strip_prefix('\u{2022}')
            .or_else(|| trimmed.strip_prefix('•'))
            .unwrap_or(trimmed)
            .trim();

        if content.starts_with("Expected:") || content.starts_with("Expected type:") {
            expected = Some(
                content
                    .splitn(2, ':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            );
        } else if content.starts_with("Actual:") || content.starts_with("Actual type:") {
            actual = Some(
                content
                    .splitn(2, ':')
                    .nth(1)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            );
        } else if content.starts_with("Perhaps you meant")
            || content.starts_with("Did you mean")
            || content.starts_with("Suggested fix:")
        {
            suggestion = Some(content.to_string());
        }
    }

    Diagnostic {
        severity,
        message: block.to_string(),
        location,
        code,
        expected,
        actual,
        suggestion,
    }
}

fn extract_ghc_code(s: &str) -> Option<String> {
    // Look for [GHC-NNNNN]
    let start = s.find("[GHC-")?;
    let end = s[start..].find(']')? + start + 1;
    Some(s[start..end].to_string())
}

pub fn simple_diagnostic(severity: &str, message: String) -> Diagnostic {
    Diagnostic {
        severity: severity.to_string(),
        message,
        location: None,
        code: None,
        expected: None,
        actual: None,
        suggestion: None,
    }
}

/// Declaration detection: `let x = ...`, `f x = ...`, guarded defs.
/// Used to decide whether to query the type of the bound name after eval.
pub fn is_let_binding(input: &str) -> bool {
    let trimmed = input.trim();

    // Explicit "let x = ...", but not "let x = ... in ..." (which is an expression)
    if trimmed.starts_with("let ") {
        let without_let = &trimmed[4..];
        return !has_top_level_in(without_let);
    }

    // Bare function/value definition: `f x y = ...`, `x = ...`
    // Must start with a lowercase letter or underscore (not an operator, number, etc.)
    if let Some(first) = trimmed.chars().next() {
        if first.is_ascii_lowercase() || first == '_' {
            let first_line = trimmed.lines().next().unwrap_or("");

            // Check that the first line contains `=` that isn't `==`
            if let Some(eq_pos) = find_bare_equals(first_line) {
                let before_eq = first_line[..eq_pos].trim();
                if !before_eq.is_empty() {
                    return true;
                }
            }

            // Guarded definition: `name args\n  | guard = ...`
            // First line is `name args` (no =), subsequent lines have guards
            if trimmed.contains('\n') {
                let has_guard = trimmed.lines().skip(1).any(|l| l.trim().starts_with('|'));
                if has_guard {
                    return true;
                }
            }
        }
    }

    false
}

fn find_bare_equals(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut in_parens: i32 = 0;
    let mut prev = '\0';

    for (i, ch) in line.char_indices() {
        let next = line[i + ch.len_utf8()..].chars().next().unwrap_or('\0');

        if ch == '"' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match ch {
                '(' => in_parens += 1,
                ')' => in_parens -= 1,
                '=' if in_parens == 0 => {
                    if next != '='
                        && prev != '='
                        && prev != '/'
                        && prev != '<'
                        && prev != '>'
                        && prev != '!'
                    {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        prev = ch;
    }
    None
}

fn has_top_level_in(s: &str) -> bool {
    s.lines().any(|line| {
        let t = line.trim();
        t == "in" || t.starts_with("in ") || t.contains(" in ")
    })
}

pub fn let_bound_name(input: &str) -> Option<String> {
    let trimmed = input.trim();
    // Strip "let " prefix if present
    let start = trimmed.strip_prefix("let ").unwrap_or(trimmed).trim_start();
    // First token is the name (could be an operator in parens)
    let name: String = if start.starts_with('(') {
        // Operator binding like `let (+++) a b = ...`
        start[1..].split(')').next()?.to_string()
    } else {
        start
            .split(|c: char| c.is_whitespace() || c == '=')
            .next()?
            .to_string()
    };
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_between_sentinels_clean() {
        let raw = format!("{SENTINEL}\nhello world\n{SENTINEL}");
        assert_eq!(extract_between_sentinels(&raw), "hello world");
    }

    #[test]
    fn test_extract_between_sentinels_no_leading() {
        let raw = format!("hello world\n{SENTINEL}");
        assert_eq!(extract_between_sentinels(&raw), "hello world");
    }

    #[test]
    fn test_extract_between_sentinels_bare() {
        assert_eq!(extract_between_sentinels("just text"), "just text");
    }

    #[test]
    fn test_extract_empty() {
        let raw = format!("{SENTINEL}\n{SENTINEL}");
        assert_eq!(extract_between_sentinels(&raw), "");
    }

    #[test]
    fn test_parse_type_simple() {
        let raw = format!("{SENTINEL}\nit :: Integer\n{SENTINEL}");
        assert_eq!(parse_type_output(&raw), Some("Integer".to_string()));
    }

    #[test]
    fn test_parse_type_polymorphic() {
        let raw = format!("{SENTINEL}\nit :: Num a => a\n{SENTINEL}");
        assert_eq!(parse_type_output(&raw), Some("Num a => a".to_string()));
    }

    #[test]
    fn test_parse_type_multiline() {
        let raw = format!("{SENTINEL}\nit :: (Num a,\n      Show a)\n   => a\n{SENTINEL}");
        assert_eq!(
            parse_type_output(&raw),
            Some("(Num a, Show a) => a".to_string())
        );
    }

    #[test]
    fn test_parse_type_function() {
        let raw = format!("{SENTINEL}\nmap :: (a -> b) -> [a] -> [b]\n{SENTINEL}");
        assert_eq!(
            parse_type_output(&raw),
            Some("(a -> b) -> [a] -> [b]".to_string())
        );
    }

    #[test]
    fn test_parse_eval_simple() {
        let raw = format!("{SENTINEL}\n42\n{SENTINEL}");
        let (val, diags) = parse_eval_output(&raw);
        assert_eq!(val, "42");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_eval_multiline_value() {
        let raw = format!("{SENTINEL}\n[1\n,2\n,3]\n{SENTINEL}");
        let (val, diags) = parse_eval_output(&raw);
        assert_eq!(val, "[1\n,2\n,3]");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_parse_eval_error() {
        let raw = format!(
            "{SENTINEL}\n<interactive>:1:1: error:\n    Variable not in scope: foo\n{SENTINEL}"
        );
        let (val, diags) = parse_eval_output(&raw);
        assert_eq!(val, "");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, "error");
        assert!(diags[0].message.contains("Variable not in scope"));
    }

    #[test]
    fn test_eval_result_json() {
        let r = EvalResult {
            expr: "1+1".to_string(),
            type_str: Some("Integer".to_string()),
            value: "2".to_string(),
            diagnostics: vec![],
        };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["expr"], "1+1");
        assert_eq!(j["type"], "Integer");
        assert_eq!(j["value"], "2");
        assert!(j["diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_is_let_binding_simple() {
        assert!(is_let_binding("let x = 1"));
    }

    #[test]
    fn test_is_let_binding_function() {
        assert!(is_let_binding("let f x y = x + y"));
    }

    #[test]
    fn test_is_let_binding_multiline() {
        assert!(is_let_binding("let f x y =\n    x + y"));
    }

    #[test]
    fn test_is_not_let_in_expression() {
        assert!(!is_let_binding("let x = 1 in x + 1"));
    }

    #[test]
    fn test_is_not_let_in_multiline() {
        assert!(!is_let_binding("let x = 1\nin x + 1"));
    }

    #[test]
    fn test_is_not_let_other() {
        assert!(!is_let_binding("map (+1) [1,2,3]"));
    }

    #[test]
    fn test_let_bound_name_simple() {
        assert_eq!(let_bound_name("let x = 1"), Some("x".into()));
    }

    #[test]
    fn test_let_bound_name_function() {
        assert_eq!(let_bound_name("let f x y = x + y"), Some("f".into()));
    }

    #[test]
    fn test_let_bound_name_multiline() {
        assert_eq!(
            let_bound_name("let greet name =\n    \"hello \" ++ name"),
            Some("greet".into())
        );
    }

    #[test]
    fn test_bare_definition_simple() {
        assert!(is_let_binding("x = 1"));
    }

    #[test]
    fn test_bare_definition_function() {
        assert!(is_let_binding("f x y = x + y"));
    }

    #[test]
    fn test_bare_definition_multiline() {
        assert!(is_let_binding("f x y =\n    x + y + 22"));
    }

    #[test]
    fn test_bare_definition_name() {
        assert_eq!(let_bound_name("f x y = x + y"), Some("f".into()));
    }

    #[test]
    fn test_bare_definition_name_simple() {
        assert_eq!(let_bound_name("x = 1"), Some("x".into()));
    }

    #[test]
    fn test_not_bare_definition_expression() {
        // Regular expressions should NOT be detected as definitions
        assert!(!is_let_binding("map (+1) [1,2,3]"));
        assert!(!is_let_binding("x + y"));
        assert!(!is_let_binding("putStrLn \"hello\""));
    }

    #[test]
    fn test_not_bare_definition_comparison() {
        // x == 1 is an expression, not a definition
        assert!(!is_let_binding("x == 1"));
    }

    #[test]
    fn test_guarded_definition() {
        let input = "fizzbuzz n\n  | n `mod` 15 == 0 = \"FizzBuzz\"\n  | otherwise = show n";
        assert!(is_let_binding(input));
        assert_eq!(let_bound_name(input), Some("fizzbuzz".into()));
    }
}
