use crate::style;

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
    highlight_styled(input)
        .iter()
        .map(|(style, text)| style.paint(text.as_str()).to_string())
        .collect()
}

/// Return styled segments for reedline's StyledText.
/// Each segment is a (nu_ansi_term::Style, String) pair with plain text.
pub fn highlight_styled(input: &str) -> Vec<(nu_ansi_term::Style, String)> {
    // Handle :{ ... :} blocks: dim the delimiters, highlight body as Haskell
    let trimmed = input.trim();
    if trimmed.starts_with(":{") {
        return input
            .split('\n')
            .enumerate()
            .flat_map(|(i, line)| {
                let sep = (i > 0).then(|| (nu_ansi_term::Style::default(), "\n".to_string()));
                let body: Vec<_> = match line.trim() {
                    ":{" | ":}" => vec![(style::dim(), line.to_string())],
                    _ => style_line(line),
                };
                sep.into_iter().chain(body)
            })
            .collect();
    }
    style_line(input)
}

fn style_line(line: &str) -> Vec<(nu_ansi_term::Style, String)> {
    tokenize(line)
        .into_iter()
        .map(|span| {
            (
                token_style(span.kind),
                line[span.start..span.end].to_string(),
            )
        })
        .collect()
}

fn token_style(kind: Token) -> nu_ansi_term::Style {
    match kind {
        Token::Keyword => style::keyword(),
        Token::TypeCon => style::type_con(),
        Token::StringLit | Token::CharLit => style::string_lit(),
        Token::Number => style::number(),
        Token::Comment => style::dim(),
        Token::Operator => style::operator(),
        Token::GhciCmd => style::ghci_cmd(),
        Token::Paren | Token::Ident | Token::Whitespace => nu_ansi_term::Style::default(),
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

    fn styled_with(out: &str, style: nu_ansi_term::Style, text: &str) -> bool {
        out.contains(&style.paint(text).to_string())
    }

    #[test]
    fn test_keyword_highlighted() {
        let out = highlight_input("let x = 1");
        assert!(styled_with(&out, style::keyword(), "let"));
    }

    #[test]
    fn test_string_highlighted() {
        let out = highlight_input("putStrLn \"hello\"");
        assert!(styled_with(&out, style::string_lit(), "\"hello\""));
    }

    #[test]
    fn test_number_highlighted() {
        let out = highlight_input("42 + 3.14");
        assert!(styled_with(&out, style::number(), "42"));
        assert!(styled_with(&out, style::number(), "3.14"));
    }

    #[test]
    fn test_type_constructor() {
        let out = highlight_input("Just True");
        assert!(styled_with(&out, style::type_con(), "Just"));
        assert!(styled_with(&out, style::type_con(), "True"));
    }

    #[test]
    fn test_comment() {
        let out = highlight_input("1 + 1 -- add");
        assert!(styled_with(&out, style::dim(), "-- add"));
    }

    #[test]
    fn test_operator() {
        let out = highlight_input("x + y");
        assert!(styled_with(&out, style::operator(), "+"));
    }

    #[test]
    fn test_ghci_command() {
        let out = highlight_input(":type map");
        assert!(styled_with(&out, style::ghci_cmd(), ":type"));
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
        assert!(styled_with(&out, style::dim(), "{- comment -}"));
    }

    #[test]
    fn test_hex_number() {
        let out = highlight_input("0xFF");
        assert!(styled_with(&out, style::number(), "0xFF"));
        assert_eq!(strip_ansi(&out), "0xFF");
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
