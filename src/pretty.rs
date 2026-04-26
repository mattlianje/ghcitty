//! Pretty-printer for Haskell `Show` output.
//!
//! Parses bracketed structures (records, lists, tuples, parens) into a tree
//! and re-emits with indentation when the inline form would exceed the width
//! budget. Strings are treated as opaque atoms so quoted brackets/commas don't
//! confuse the parser.

const TARGET_WIDTH: usize = 60;

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Atom(String),
    Group {
        open: char,
        close: char,
        items: Vec<Vec<Node>>,
    },
}

pub fn pretty(input: &str) -> String {
    let nodes = parse(input);
    render_seq(&nodes, 0)
}

#[derive(Default)]
struct Frame {
    open: char,
    items: Vec<Vec<Node>>,
    current: Vec<Node>,
    atom: String,
}

impl Frame {
    fn new(open: char) -> Self {
        Frame {
            open,
            ..Frame::default()
        }
    }

    fn flush_atom(&mut self) {
        let trimmed = self.atom.trim();
        if !trimmed.is_empty() {
            self.current.push(Node::Atom(trimmed.to_string()));
        }
        self.atom.clear();
    }

    fn finish_item(&mut self) {
        self.flush_atom();
        if !self.current.is_empty() || !self.items.is_empty() {
            let item = std::mem::take(&mut self.current);
            self.items.push(item);
        }
    }
}

fn parse(input: &str) -> Vec<Node> {
    let mut frames = vec![Frame::new('\0')];
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        let frame = frames.last_mut().unwrap();
        match ch {
            '"' => {
                frame.atom.push('"');
                while let Some(c) = chars.next() {
                    frame.atom.push(c);
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            frame.atom.push(n);
                        }
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '(' | '[' | '{' => {
                frame.flush_atom();
                frames.push(Frame::new(ch));
            }
            ')' | ']' | '}' => {
                let mut closed = frames.pop().unwrap();
                closed.finish_item();
                let group = Node::Group {
                    open: closed.open,
                    close: ch,
                    items: closed.items,
                };
                frames.last_mut().unwrap().current.push(group);
            }
            ',' => {
                frame.flush_atom();
                let item = std::mem::take(&mut frame.current);
                frame.items.push(item);
            }
            _ => {
                frame.atom.push(ch);
            }
        }
    }

    let mut root = frames.pop().unwrap();
    root.finish_item();
    root.items.into_iter().flatten().collect()
}

fn is_flat(items: &[Vec<Node>]) -> bool {
    items
        .iter()
        .all(|item| item.iter().all(|n| matches!(n, Node::Atom(_))))
}

fn inline(node: &Node) -> String {
    match node {
        Node::Atom(s) => s.clone(),
        Node::Group { open, close, items } => {
            if items.is_empty() {
                return format!("{open}{close}");
            }
            let parts: Vec<String> = items
                .iter()
                .map(|item| item.iter().map(inline).collect::<Vec<_>>().join(" "))
                .collect();
            format!("{open}{}{close}", parts.join(", "))
        }
    }
}

fn render_node(node: &Node, col: usize) -> String {
    let il = inline(node);
    if col + il.len() <= TARGET_WIDTH {
        return il;
    }
    match node {
        Node::Atom(s) => s.clone(),
        Node::Group { open, close, items } => {
            if items.is_empty() {
                return format!("{open}{close}");
            }
            // Flat lists/tuples (no nested groups) stay inline; truncation
            // handles the long ones. Records ({}) still expand.
            if matches!(open, '[' | '(') && is_flat(items) {
                return il;
            }
            if items.len() == 1 {
                // Single-item group like `Just (Just (Just 5))`: recurse into
                // the contents but keep the brackets tight.
                let inner = render_seq(&items[0], col + 1);
                return format!("{open}{inner}{close}");
            }
            // Expanded: open follows current column, comma+content lines align
            // under the open, close aligns under the open too.
            //
            //   User { name = "Alice"
            //        , age = 30
            //        }
            let comma_pad = " ".repeat(col);
            let item_col = col + 2;
            let mut out = String::new();
            out.push(*open);
            out.push(' ');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    out.push_str(&comma_pad);
                    out.push_str(", ");
                }
                out.push_str(&render_seq(item, item_col));
            }
            out.push('\n');
            out.push_str(&comma_pad);
            out.push(*close);
            out
        }
    }
}

/// Render a sequence of sibling nodes (an "item"), space-separated, tracking
/// the current column so groups can decide whether they fit.
fn render_seq(nodes: &[Node], col: usize) -> String {
    let mut out = String::new();
    let mut cur = col;
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
            cur += 1;
        }
        let rendered = render_node(n, cur);
        if let Some(nl) = rendered.rfind('\n') {
            cur = rendered.len() - nl - 1;
        } else {
            cur += rendered.len();
        }
        out.push_str(&rendered);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_atom() {
        assert_eq!(pretty("42"), "42");
        assert_eq!(pretty("\"hello\""), "\"hello\"");
        assert_eq!(pretty("Just 5"), "Just 5");
    }

    #[test]
    fn small_record_stays_inline() {
        let input = "User {name = \"Alice\", age = 30}";
        assert_eq!(pretty(input), input);
    }

    #[test]
    fn small_list_stays_inline() {
        assert_eq!(pretty("[1, 2, 3]"), "[1, 2, 3]");
    }

    #[test]
    fn long_record_expands() {
        let input =
            "User {name = \"Alice\", age = 30, city = \"NYC\", country = \"USA\", active = True}";
        let expected = "\
User { name = \"Alice\"
     , age = 30
     , city = \"NYC\"
     , country = \"USA\"
     , active = True
     }";
        assert_eq!(pretty(input), expected);
    }

    #[test]
    fn nested_record_expands_both_levels() {
        let input = "User {name = \"Alice\", contact = Contact {email = \"alice@example.com\", phone = \"+1-555-0100\"}}";
        let out = pretty(input);
        assert!(out.contains("User { name = \"Alice\""), "got: {out}");
        assert!(out.contains("contact = Contact { email"), "got: {out}");
        // Inner close should be indented further than outer close.
        let inner_close = out.find("\n     , contact").unwrap();
        let outer_close = out.rfind("}").unwrap();
        assert!(outer_close > inner_close);
    }

    #[test]
    fn long_list_of_records_expands() {
        let input =
            "[User {name = \"Alice\", age = 30}, User {name = \"Bob\", age = 25}, User {name = \"Carol\", age = 28}]";
        let out = pretty(input);
        assert!(out.starts_with("[ User"), "got: {out}");
        assert!(out.contains("\n, User"), "got: {out}");
        assert!(out.ends_with("\n]"), "got: {out}");
    }

    #[test]
    fn brackets_inside_strings_are_ignored() {
        let input = "Note {body = \"hello {world}, how are [you]?\"}";
        assert_eq!(pretty(input), input);
    }

    #[test]
    fn escaped_quote_in_string() {
        let input = "Msg {text = \"say \\\"hi\\\"\"}";
        assert_eq!(pretty(input), input);
    }

    #[test]
    fn empty_collections() {
        assert_eq!(pretty("[]"), "[]");
        assert_eq!(pretty("()"), "()");
        assert_eq!(pretty("Map.fromList []"), "Map.fromList []");
    }

    #[test]
    fn tuple_inline() {
        assert_eq!(pretty("(1, 2, 3)"), "(1, 2, 3)");
    }

    #[test]
    fn long_flat_list_stays_inline() {
        let xs: Vec<String> = (1..=50).map(|n| n.to_string()).collect();
        let input = format!("[{}]", xs.join(", "));
        assert_eq!(pretty(&input), input);
    }

    #[test]
    fn long_flat_tuple_stays_inline() {
        let xs: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
        let input = format!("({})", xs.join(", "));
        assert_eq!(pretty(&input), input);
    }

    #[test]
    fn list_of_records_still_expands() {
        let input =
            "[User {name = \"Alice\", age = 30}, User {name = \"Bob\", age = 25}, User {name = \"Carol\", age = 28}]";
        let out = pretty(input);
        assert!(out.starts_with("[ User"), "got: {out}");
    }

    #[test]
    fn deeply_nested_just() {
        // Single-item groups stay tight even when they'd theoretically expand.
        assert_eq!(pretty("Just (Just (Just 5))"), "Just (Just (Just 5))");
    }
}
