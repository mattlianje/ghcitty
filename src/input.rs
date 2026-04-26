use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use nu_ansi_term::Style;
use reedline::{
    ColumnarMenu, Completer, Completer as ReedlineCompleter, Editor,
    Highlighter as ReedlineHighlighter, Hinter as ReedlineHinter, Menu, MenuBuilder, MenuEvent,
    MenuSettings, Painter, Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus,
    Span, StyledText, Suggestion, ValidationResult, Validator as ReedlineValidator,
};

use crate::ghc::GhcProcess;
use crate::highlight;

// ** Prompt **

pub struct GhciPrompt {
    pub json_mode: bool,
}

impl Prompt for GhciPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        if self.json_mode {
            Cow::Borrowed("")
        } else {
            Cow::Borrowed("\u{03bb}>")
        }
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed(" ")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("   ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!("({prefix}search: {}) ", history_search.term))
    }

    fn get_prompt_color(&self) -> reedline::Color {
        reedline::Color::White
    }
}

// ** Completer **

pub struct GhciCompleter {
    pub ghc: Arc<Mutex<GhcProcess>>,
}

/// Cap on candidates we'll fetch types for. ColumnarMenu switches to a
/// single-column layout once any candidate has a description, so showing
/// types for a long list would balloon the menu. Above this, we skip types.
const TYPE_DESC_CAP: usize = 12;

/// Slash commands implemented by ghcitty itself, which GHCi's `:complete repl`
/// doesn't know about. Trailing space on commands that take an argument so Tab
/// leaves the cursor ready to type.
const GHCITTY_CMDS: &[&str] = &[":scratch", ":edit", ":e", ":undo", ":doc ", ":hoogle "];

/// Prepend any ghcitty-native commands matching `word` to the GHCi completion
/// list. Prepended (not appended) so they win ties for ghost completion.
fn merge_ghcitty_cmds(word: &str, completions: Vec<String>) -> Vec<String> {
    if !word.starts_with(':') {
        return completions;
    }
    let matches: Vec<String> = GHCITTY_CMDS
        .iter()
        .filter(|c| c.starts_with(word))
        .map(|c| c.to_string())
        .collect();
    if matches.is_empty() {
        return completions;
    }
    let mut out = matches;
    for c in completions {
        if !out.iter().any(|o| o.trim_end() == c.trim_end()) {
            out.push(c);
        }
    }
    out
}

impl ReedlineCompleter for GhciCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let pos = pos.min(line.len());
        let prefix = &line[..pos];
        let word_start = prefix
            .rfind(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == ',')
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &prefix[word_start..];

        if word.is_empty() {
            return vec![];
        }

        let Ok(mut ghc) = self.ghc.lock() else {
            return vec![];
        };
        let completions = match ghc.complete(prefix) {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let completions = merge_ghcitty_cmds(word, completions);
        let want_types = completions.len() <= TYPE_DESC_CAP;

        completions
            .into_iter()
            .map(|c| {
                let description = if want_types && !c.starts_with(':') {
                    ghc.type_of(&c).ok().flatten()
                } else {
                    None
                };
                Suggestion {
                    value: c,
                    display_override: None,
                    description,
                    style: None,
                    extra: None,
                    span: Span::new(word_start, pos),
                    append_whitespace: false,
                    match_indices: None,
                }
            })
            .collect()
    }
}

// ** Hinter (ghost completion) **

struct HintCache {
    prefix: String,
    word_start: usize,
    completions: Vec<String>,
}

pub struct GhciHinter {
    pub ghc: Arc<Mutex<GhcProcess>>,
    cache: Option<HintCache>,
    current_hint: String,
    style: Style,
}

impl GhciHinter {
    pub fn new(ghc: Arc<Mutex<GhcProcess>>) -> Self {
        Self {
            ghc,
            cache: None,
            current_hint: String::new(),
            style: Style::new().dimmed(),
        }
    }
}

impl ReedlineHinter for GhciHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        _history: &dyn reedline::History,
        _use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.current_hint.clear();

        let pos = pos.min(line.len());
        if pos != line.len() || line.is_empty() {
            return String::new();
        }

        let prefix = &line[..pos];
        let word_start = prefix
            .rfind(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == ',')
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &prefix[word_start..];

        if word.len() < 2 {
            return String::new();
        }

        // Try cache first
        let first = {
            let cache_hit = self.cache.as_ref().and_then(|c| {
                if c.word_start == word_start
                    && word_start <= c.prefix.len()
                    && word_start <= prefix.len()
                    && prefix[..word_start] == c.prefix[..c.word_start]
                {
                    let cached_word = &c.prefix[c.word_start..];
                    if word.starts_with(cached_word) {
                        return c.completions.iter().find(|s| s.starts_with(word)).cloned();
                    }
                }
                None
            });

            if let Some(hit) = cache_hit {
                hit
            } else {
                let completions = {
                    let Ok(mut ghc) = self.ghc.lock() else {
                        return String::new();
                    };
                    match ghc.complete(prefix) {
                        Ok(c) => c,
                        Err(_) => return String::new(),
                    }
                };
                let completions = merge_ghcitty_cmds(word, completions);
                let first = completions.iter().find(|s| s.starts_with(word)).cloned();
                self.cache = Some(HintCache {
                    prefix: prefix.to_string(),
                    word_start,
                    completions,
                });
                match first {
                    Some(f) => f,
                    None => return String::new(),
                }
            }
        };

        if first.len() > word.len() && first.starts_with(word) {
            let suffix = &first[word.len()..];
            self.current_hint = suffix.to_string();
            self.style.paint(suffix).to_string()
        } else {
            String::new()
        }
    }

    fn complete_hint(&self) -> String {
        self.current_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        // Return up to the next word boundary in the hint
        let mut end = 0;
        let mut found_alnum = false;
        for (i, c) in self.current_hint.char_indices() {
            if c.is_alphanumeric() || c == '_' || c == '\'' {
                found_alnum = true;
                end = i + c.len_utf8();
            } else if found_alnum {
                break;
            } else {
                end = i + c.len_utf8();
            }
        }
        self.current_hint[..end].to_string()
    }
}

// ** Highlighter **

pub struct HaskellHighlighter;

impl ReedlineHighlighter for HaskellHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut st = StyledText::new();
        if line.is_empty() {
            return st;
        }
        for (style, text) in highlight::highlight_styled(line) {
            st.push((style, text));
        }
        st
    }
}

// ** Multiline Validator **

pub struct HaskellValidator;

impl ReedlineValidator for HaskellValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if is_incomplete(line) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

// ColumnarMenu reports the full item count from menu_required_lines(), so
// reedline's painter scrolls the terminal to make room, and those lines
// are gone from the viewport permanently. Cap it to ~1/3 the terminal
// height instead; ColumnarMenu::menu_string() already clips to whatever
// available_lines it's given, so paging through completions still works.

pub struct PagedColumnarMenu {
    inner: ColumnarMenu,
}

impl Menu for PagedColumnarMenu {
    fn settings(&self) -> &MenuSettings {
        self.inner.settings()
    }

    fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    fn menu_event(&mut self, event: MenuEvent) {
        self.inner.menu_event(event);
    }

    fn can_quick_complete(&self) -> bool {
        self.inner.can_quick_complete()
    }

    fn can_partially_complete(
        &mut self,
        values_updated: bool,
        editor: &mut Editor,
        completer: &mut dyn Completer,
    ) -> bool {
        self.inner
            .can_partially_complete(values_updated, editor, completer)
    }

    fn update_values(&mut self, editor: &mut Editor, completer: &mut dyn Completer) {
        self.inner.update_values(editor, completer);
    }

    fn update_working_details(
        &mut self,
        editor: &mut Editor,
        completer: &mut dyn Completer,
        painter: &Painter,
    ) {
        self.inner
            .update_working_details(editor, completer, painter);
    }

    fn replace_in_buffer(&self, editor: &mut Editor) {
        self.inner.replace_in_buffer(editor);
    }

    fn menu_required_lines(&self, terminal_columns: u16) -> u16 {
        let real = self.inner.menu_required_lines(terminal_columns);
        // Cap to terminal height minus room for the prompt, so the
        // painter never scrolls the terminal to make space for the menu.
        let term_height = terminal_height();
        // Cap to ~1/3 of the terminal so the painter never has to scroll
        // the prompt far from its original position.  `terminal_height - 3`
        // was too loose: on a 40-row terminal it requested 37 lines, pushing
        // the prompt to the very top.
        let max = (term_height / 3).max(5);
        real.min(max)
    }

    fn menu_string(&self, available_lines: u16, use_ansi_coloring: bool) -> String {
        self.inner.menu_string(available_lines, use_ansi_coloring)
    }

    fn min_rows(&self) -> u16 {
        self.inner.min_rows()
    }

    fn get_values(&self) -> &[Suggestion] {
        self.inner.get_values()
    }
}

fn terminal_height() -> u16 {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 {
            ws.ws_row
        } else {
            24
        }
    }
}

/// Build the completion dropdown menu.
pub fn completion_menu() -> PagedColumnarMenu {
    let inner = ColumnarMenu::default()
        .with_name("completion_menu")
        .with_columns(4)
        .with_column_padding(2)
        .with_marker(" ");
    PagedColumnarMenu { inner }
}

/// ** HEURISTICS MULTILINE **
/// We basically check for starting :{ then cycle
/// through different tokens to send us into multiline
/// TODO: test more
pub fn is_incomplete(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.starts_with(":{") {
        let has_close = input.lines().any(|l| l.trim() == ":}");
        return !has_close;
    }

    if trimmed.starts_with(':') {
        return false;
    }

    // A blank trailing line means "submit now"
    if input.ends_with('\n') {
        return false;
    }

    if has_unbalanced_delimiters(trimmed) {
        return true;
    }

    if has_unclosed_string(trimmed) {
        return true;
    }

    let last_line = trimmed.lines().last().unwrap_or("").trim();

    if last_line.ends_with('\\') {
        return true;
    }

    if ends_with_continuation(last_line) {
        return true;
    }

    if trimmed.contains('\n') && is_in_indented_block(trimmed) {
        return true;
    }

    false
}

fn is_in_indented_block(input: &str) -> bool {
    let lines: Vec<&str> = input.lines().collect();
    if lines.len() < 2 {
        return false;
    }

    let last_line = lines.last().unwrap_or(&"");
    if !last_line.starts_with(' ') && !last_line.starts_with('\t') {
        return false;
    }

    for line in &lines[..lines.len() - 1] {
        if ends_with_continuation(line.trim()) {
            return true;
        }
    }
    false
}

fn has_unbalanced_delimiters(input: &str) -> bool {
    let mut depth_paren: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut in_string = false;
    let mut in_char = false;
    let mut in_line_comment = false;
    let mut in_block_comment: i32 = 0;

    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];
        let next = if i + 1 < len { chars[i + 1] } else { '\0' };

        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if in_block_comment > 0 {
            if ch == '{' && next == '-' {
                in_block_comment += 1;
                i += 2;
                continue;
            } else if ch == '-' && next == '}' {
                in_block_comment -= 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if in_string {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if in_char {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == '\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }

        if ch == '-' && next == '-' {
            in_line_comment = true;
            i += 2;
            continue;
        }
        if ch == '{' && next == '-' {
            in_block_comment += 1;
            i += 2;
            continue;
        }

        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            _ => {}
        }
        i += 1;
    }

    depth_paren > 0 || depth_bracket > 0 || depth_brace > 0
}

fn has_unclosed_string(input: &str) -> bool {
    let mut in_string = false;
    let mut prev = '\0';

    for ch in input.chars() {
        if ch == '"' && prev != '\\' {
            in_string = !in_string;
        }
        prev = ch;
    }

    in_string
}

fn ends_with_continuation(line: &str) -> bool {
    let s = line.trim_end();
    if s.ends_with("->")
        || s.ends_with("=>")
        || s.ends_with('=')
        || s.ends_with("++")
        || s.ends_with("<>")
        || s.ends_with('$')
        || s.ends_with('.')
        || s.ends_with('|')
        || s.ends_with(',')
        || s.ends_with("::")
    {
        return true;
    }
    let last_word = s
        .rsplit_once(char::is_whitespace)
        .map(|(_, w)| w)
        .unwrap_or(s);
    matches!(
        last_word,
        "do" | "where" | "of" | "let" | "in" | "then" | "else"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balanced_parens() {
        assert!(!is_incomplete("map (+1) [1,2,3]"));
    }

    #[test]
    fn test_merge_ghcitty_cmds_prepends_matches() {
        let merged = merge_ghcitty_cmds(":scr", vec![":script".into(), ":show".into()]);
        assert_eq!(merged.first().map(String::as_str), Some(":scratch"));
        assert!(merged.iter().any(|c| c == ":script"));
    }

    #[test]
    fn test_merge_ghcitty_cmds_no_colon_prefix() {
        let cs = vec!["map".into(), "mapM".into()];
        assert_eq!(merge_ghcitty_cmds("ma", cs.clone()), cs);
    }

    #[test]
    fn test_merge_ghcitty_cmds_no_match_passthrough() {
        let cs = vec![":show".into(), ":set".into()];
        assert_eq!(merge_ghcitty_cmds(":sh", cs.clone()), cs);
    }

    #[test]
    fn test_merge_ghcitty_cmds_arg_command_has_trailing_space() {
        let merged = merge_ghcitty_cmds(":doc", vec![]);
        assert_eq!(merged, vec![":doc ".to_string()]);
    }

    #[test]
    fn test_unbalanced_paren() {
        assert!(is_incomplete("map (+1) [1,2,3"));
    }

    #[test]
    fn test_unbalanced_brace() {
        assert!(is_incomplete("let { x = 1"));
    }

    #[test]
    fn test_do_continuation() {
        assert!(is_incomplete("main = do"));
    }

    #[test]
    fn test_where_continuation() {
        assert!(is_incomplete("f x = g x where"));
    }

    #[test]
    fn test_arrow_continuation() {
        assert!(is_incomplete("f = \\x ->"));
    }

    #[test]
    fn test_equals_continuation() {
        assert!(is_incomplete("f x ="));
    }

    #[test]
    fn test_comma_continuation() {
        assert!(is_incomplete("[1,2,"));
    }

    #[test]
    fn test_backslash_continuation() {
        assert!(is_incomplete("longExpr \\"));
    }

    #[test]
    fn test_complete_let() {
        assert!(!is_incomplete("let x = 1"));
    }

    #[test]
    fn test_complete_multiline() {
        assert!(!is_incomplete("let x = 1\n    y = 2\nin x + y"));
    }

    #[test]
    fn test_unclosed_string() {
        assert!(is_incomplete("putStrLn \"hello"));
    }

    #[test]
    fn test_closed_string() {
        assert!(!is_incomplete("putStrLn \"hello\""));
    }

    #[test]
    fn test_string_with_escaped_quote() {
        assert!(!is_incomplete(r#"putStrLn "say \"hi\"""#));
    }

    #[test]
    fn test_comment_doesnt_count() {
        assert!(!is_incomplete("1 + 1 -- this is (unbalanced"));
    }

    #[test]
    fn test_block_comment() {
        assert!(!is_incomplete("{- ( -} 1 + 1"));
    }

    #[test]
    fn test_empty() {
        assert!(!is_incomplete(""));
    }

    #[test]
    fn test_colon_command_not_incomplete() {
        assert!(!is_incomplete(":type map"));
    }

    #[test]
    fn test_dollar_continuation() {
        assert!(is_incomplete("putStrLn $"));
    }

    #[test]
    fn test_pipe_continuation() {
        assert!(is_incomplete("x |"));
    }

    #[test]
    fn test_case_of() {
        assert!(is_incomplete("case x of"));
    }

    #[test]
    fn test_then_else() {
        assert!(is_incomplete("if True then"));
        assert!(is_incomplete("if True then 1 else"));
    }

    #[test]
    fn test_concat_continuation() {
        assert!(is_incomplete("[1,2] ++"));
    }

    #[test]
    fn test_blank_line_submits() {
        assert!(!is_incomplete("f x =\n  x + 1\n\n"));
        assert!(!is_incomplete("f x =\n  x + 1\n"));
        assert!(!is_incomplete("let f x y =\n    x + y + 15\n"));
    }
}
