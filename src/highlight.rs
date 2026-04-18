pub mod color {
    pub const RESET: &str = "\x1b[0m";
    pub const DIM: &str = "\x1b[2m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
}

use color::*;

const KEYWORDS: &[&str] = &[
    "case",
    "class",
    "data",
    "default",
    "deriving",
    "do",
    "else",
    "forall",
    "foreign",
    "if",
    "import",
    "in",
    "infix",
    "infixl",
    "infixr",
    "instance",
    "let",
    "module",
    "newtype",
    "of",
    "qualified",
    "then",
    "type",
    "where",
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Token {
    Keyword,
    TypeCon, // Capitalized identifier (type constructor / module)
    Ident,   // lowercase identifier
    StringLit,
    CharLit,
    Number,
    Comment,
    Operator,
    Paren, // () [] {}
    Whitespace,
    GhciCmd, // :type, :info, etc.
}

struct Span {
    start: usize,
    end: usize,
    kind: Token,
}

pub fn highlight_input(input: &str) -> String {
    let spans = tokenize(input);
    let mut out = String::with_capacity(input.len() * 2);

    for span in &spans {
        let text = &input[span.start..span.end];
        match span.kind {
            Token::Keyword => {
                out.push_str(MAGENTA);
                out.push_str(BOLD);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::TypeCon => {
                out.push_str(CYAN);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::StringLit => {
                out.push_str(GREEN);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::CharLit => {
                out.push_str(GREEN);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::Number => {
                out.push_str(YELLOW);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::Comment => {
                out.push_str(DIM);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::Operator => {
                out.push_str(BLUE);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::GhciCmd => {
                out.push_str(CYAN);
                out.push_str(BOLD);
                out.push_str(text);
                out.push_str(RESET);
            }
            Token::Paren | Token::Ident | Token::Whitespace => {
                out.push_str(text);
            }
        }
    }

    out
}

/// Return styled segments for reedline's StyledText.
/// Each segment is a (nu_ansi_term::Style, String) pair with plain text.
pub fn highlight_styled(input: &str) -> Vec<(nu_ansi_term::Style, String)> {
    // Handle :{ ... :} blocks: dim the delimiters, highlight body as Haskell
    let trimmed = input.trim();
    if trimmed.starts_with(":{") {
        let mut segments = Vec::new();
        let dim = nu_ansi_term::Style::new().dimmed();
        for (i, line) in input.split('\n').enumerate() {
            if i > 0 {
                segments.push((nu_ansi_term::Style::default(), "\n".to_string()));
            }
            let lt = line.trim();
            if lt == ":{" || lt == ":}" {
                segments.push((dim, line.to_string()));
            } else {
                let spans = tokenize(line);
                for span in &spans {
                    let text = line[span.start..span.end].to_string();
                    let style = token_style(span.kind);
                    segments.push((style, text));
                }
            }
        }
        return segments;
    }

    let spans = tokenize(input);
    let mut segments = Vec::with_capacity(spans.len());

    for span in &spans {
        let text = input[span.start..span.end].to_string();
        let style = token_style(span.kind);
        segments.push((style, text));
    }

    segments
}

fn token_style(kind: Token) -> nu_ansi_term::Style {
    use nu_ansi_term::{Color as C, Style as S};
    match kind {
        Token::Keyword => S::new().bold().fg(C::Magenta),
        Token::TypeCon => S::new().fg(C::Cyan),
        Token::StringLit | Token::CharLit => S::new().fg(C::Green),
        Token::Number => S::new().fg(C::Yellow),
        Token::Comment => S::new().dimmed(),
        Token::Operator => S::new().fg(C::Blue),
        Token::GhciCmd => S::new().bold().fg(C::Cyan),
        Token::Paren | Token::Ident | Token::Whitespace => S::default(),
    }
}

fn tokenize(input: &str) -> Vec<Span> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut spans = Vec::new();
    let mut i = 0;

    while i < len {
        let ch = bytes[i];

        // Whitespace
        if ch.is_ascii_whitespace() {
            let start = i;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: Token::Whitespace,
            });
            continue;
        }

        // Line comment: --
        if ch == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            let start = i;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: Token::Comment,
            });
            continue;
        }

        // Block comment: {- ... -}
        if ch == b'{' && i + 1 < len && bytes[i + 1] == b'-' {
            let start = i;
            let mut depth = 1;
            i += 2;
            while i < len && depth > 0 {
                if bytes[i] == b'{' && i + 1 < len && bytes[i + 1] == b'-' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'-' && i + 1 < len && bytes[i + 1] == b'}' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            spans.push(Span {
                start,
                end: i,
                kind: Token::Comment,
            });
            continue;
        }

        // String literal
        if ch == b'"' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len {
                i += 1;
            } // closing quote
            spans.push(Span {
                start,
                end: i,
                kind: Token::StringLit,
            });
            continue;
        }

        // Char literal
        if ch == b'\'' && i + 1 < len && bytes[i + 1] != b' ' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'\\' && i + 1 < len {
                i += 2;
            } else if i < len {
                i += 1;
            }
            if i < len && bytes[i] == b'\'' {
                i += 1;
                spans.push(Span {
                    start,
                    end: i,
                    kind: Token::CharLit,
                });
                continue;
            }
            // TODO: revisit invalid char literals and Ctrl+ c support
            i = start;
        }

        // GHCi command: starts with : at beginning of input
        if ch == b':' && spans.iter().all(|s| s.kind == Token::Whitespace) {
            let start = i;
            while i < len && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: Token::GhciCmd,
            });
            continue;
        }

        // Number
        if ch.is_ascii_digit() {
            let start = i;
            // Hex: 0x...
            if ch == b'0' && i + 1 < len && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
                i += 2;
                while i < len && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
                // Decimal part
                if i < len && bytes[i] == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                // Exponent
                if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                    i += 1;
                    if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            spans.push(Span {
                start,
                end: i,
                kind: Token::Number,
            });
            continue;
        }

        // Identifier or keyword (starts with letter or underscore)
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let start = i;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'\'')
            {
                i += 1;
            }
            let word = &input[start..i];
            let kind = if KEYWORDS.contains(&word) {
                Token::Keyword
            } else if ch.is_ascii_uppercase() {
                Token::TypeCon
            } else {
                Token::Ident
            };
            spans.push(Span {
                start,
                end: i,
                kind,
            });
            continue;
        }

        // Parens/brackets/braces
        if ch == b'(' || ch == b')' || ch == b'[' || ch == b']' || ch == b'{' || ch == b'}' {
            spans.push(Span {
                start: i,
                end: i + 1,
                kind: Token::Paren,
            });
            i += 1;
            continue;
        }

        // Operators (sequences of symbolic characters)
        if is_operator_char(ch) {
            let start = i;
            while i < len && is_operator_char(bytes[i]) {
                i += 1;
            }
            spans.push(Span {
                start,
                end: i,
                kind: Token::Operator,
            });
            continue;
        }

        // TODO: also revisit UTF-8 + multi-bytes
        let start = i;
        // Advance past the full UTF-8 character (per above)
        i += 1;
        while i < len && (bytes[i] & 0xC0) == 0x80 {
            i += 1;
        }
        spans.push(Span {
            start,
            end: i,
            kind: Token::Ident,
        });
    }

    spans
}

// TODO: double check
fn is_operator_char(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'*'
            | b'+'
            | b'.'
            | b'/'
            | b'<'
            | b'='
            | b'>'
            | b'?'
            | b'@'
            | b'\\'
            | b'^'
            | b'|'
            | b'-'
            | b'~'
            | b':'
            | b','
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_highlighted() {
        let out = highlight_input("let x = 1");
        assert!(out.contains(MAGENTA));
        assert!(out.contains("let"));
    }

    #[test]
    fn test_string_highlighted() {
        let out = highlight_input("putStrLn \"hello\"");
        assert!(out.contains(GREEN));
        assert!(out.contains("\"hello\""));
    }

    #[test]
    fn test_number_highlighted() {
        let out = highlight_input("42 + 3.14");
        assert!(out.contains(YELLOW));
        assert!(out.contains("42"));
        assert!(out.contains("3.14"));
    }

    #[test]
    fn test_type_constructor() {
        let out = highlight_input("Just True");
        assert!(out.contains(CYAN));
        assert!(out.contains("Just"));
        assert!(out.contains("True"));
    }

    #[test]
    fn test_comment() {
        let out = highlight_input("1 + 1 -- add");
        assert!(out.contains(DIM));
        assert!(out.contains("-- add"));
    }

    #[test]
    fn test_operator() {
        let out = highlight_input("x + y");
        assert!(out.contains(BLUE));
        assert!(out.contains("+"));
    }

    #[test]
    fn test_ghci_command() {
        let out = highlight_input(":type map");
        assert!(out.contains(CYAN));
        assert!(out.contains(":type"));
    }

    #[test]
    fn test_roundtrip_no_content_lost() {
        let input = "let f x = case x of { Just y -> y; Nothing -> 0 }";
        let out = highlight_input(input);
        // Strip ANSI codes and check content is preserved
        let stripped = strip_ansi(&out);
        assert_eq!(stripped, input);
    }

    #[test]
    fn test_block_comment_highlight() {
        let out = highlight_input("{- comment -} 1");
        assert!(out.contains(DIM));
        assert!(out.contains("{- comment -}"));
    }

    #[test]
    fn test_hex_number() {
        let out = highlight_input("0xFF");
        assert!(out.contains(YELLOW));
        let stripped = strip_ansi(&out);
        assert_eq!(stripped, "0xFF");
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for ch in s.chars() {
            if ch == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if ch == 'm' {
                    in_escape = false;
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn test_non_ascii_no_panic() {
        // Should not panic on non-ASCII characters
        let out = highlight_input("putStrLn \"\u{03bb} hello\"");
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("\u{03bb}"));
    }

    #[test]
    fn test_backtick_infix() {
        let out = highlight_input("n `mod` 15");
        let stripped = strip_ansi(&out);
        assert_eq!(stripped, "n `mod` 15");
    }
}
