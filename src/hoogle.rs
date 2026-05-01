use std::process::Command;

#[derive(Debug, Clone)]
pub struct HoogleResult {
    pub module: String,
    pub name: String,
    pub signature: String,
    pub doc: String,
    pub url: String,
}

/// CLI first, web API fallback.
pub fn search(query: &str, count: usize) -> Vec<HoogleResult> {
    let query = unquote(query);
    search_cli(query, count).unwrap_or_else(|| search_web(query, count).unwrap_or_default())
}

pub fn doc(name: &str) -> Option<HoogleResult> {
    let name = unquote(name);
    // Try CLI with --info flag first
    if let Some(result) = doc_cli(name) {
        return Some(result);
    }
    // Fall back to web search, take best match
    let results = search_web(name, 1)?;
    results.into_iter().next()
}

/// Drop a matched pair of surrounding quotes, so `:hoogle "[a] -> [a]"`
/// works the same as `:hoogle [a] -> [a]` (issue #26).
fn unquote(s: &str) -> &str {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return s[1..s.len() - 1].trim();
        }
    }
    s
}

fn search_cli(query: &str, count: usize) -> Option<Vec<HoogleResult>> {
    let output = Command::new("hoogle")
        .args(["search", "--count", &count.to_string(), query])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(parse_cli_line)
        .collect();
    Some(results)
}

fn doc_cli(name: &str) -> Option<HoogleResult> {
    let output = Command::new("hoogle")
        .args(["search", "--info", name])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = stdout.trim();
    if text.is_empty() || text.starts_with("No results") {
        return None;
    }

    // --info output format:
    // module Name
    // name :: Type
    //
    // Documentation paragraph(s)
    // URL
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return None;
    }

    // First line is often the signature
    let sig_line = lines[0].trim();
    let (name_part, sig_part) = if let Some(idx) = sig_line.find("::") {
        (
            sig_line[..idx].trim().to_string(),
            sig_line[idx + 2..].trim().to_string(),
        )
    } else {
        (name.to_string(), String::new())
    };

    // Collect doc lines (skip signature, skip trailing URL)
    let mut doc_lines = Vec::new();
    let mut module = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            continue;
        }
        // Module line often looks like "Data.List" on its own
        if i == 1
            && !trimmed.is_empty()
            && !trimmed.contains(' ')
            && trimmed
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
        {
            module = trimmed.to_string();
            continue;
        }
        doc_lines.push(*line);
    }

    let doc = doc_lines.join("\n").trim().to_string();
    let url = lines
        .iter()
        .rev()
        .find(|l| l.starts_with("http"))
        .unwrap_or(&"")
        .to_string();

    Some(HoogleResult {
        module,
        name: name_part,
        signature: sig_part,
        doc,
        url,
    })
}

fn parse_cli_line(line: &str) -> Option<HoogleResult> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try to find :: separator
    if let Some(sig_idx) = trimmed.find("::") {
        let before = trimmed[..sig_idx].trim();
        let sig = trimmed[sig_idx + 2..].trim().to_string();

        // Before :: is "Module.Name functionName" or just "functionName"
        let (module, name) = if let Some(space_idx) = before.rfind(' ') {
            (
                before[..space_idx].trim().to_string(),
                before[space_idx + 1..].trim().to_string(),
            )
        } else {
            (String::new(), before.to_string())
        };

        Some(HoogleResult {
            module,
            name,
            signature: sig,
            doc: String::new(),
            url: String::new(),
        })
    } else {
        // Lines without :: (e.g., module names, data types, classes)
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        let (module, name) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (String::new(), trimmed.to_string())
        };
        Some(HoogleResult {
            module,
            name,
            signature: String::new(),
            doc: String::new(),
            url: String::new(),
        })
    }
}

fn search_web(query: &str, count: usize) -> Option<Vec<HoogleResult>> {
    // Use curl to hit Hoogle JSON API (avoids needing an HTTP client dep)
    let encoded = percent_encode(query);
    let url = format!(
        "https://hoogle.haskell.org/?mode=json&hoogle={}&start=1&count={}",
        encoded, count
    );

    let output = Command::new("curl")
        .args(["-s", "-m", "5", &url])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_hoogle_json(&body)
}

fn parse_hoogle_json(json: &str) -> Option<Vec<HoogleResult>> {
    let arr: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = arr.as_array()?;

    let results = arr
        .iter()
        .filter_map(|entry| {
            let url = entry.get("url")?.as_str().unwrap_or("").to_string();
            let module = entry
                .get("module")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let item = entry.get("item")?.as_str().unwrap_or("");
            let docs = entry.get("docs").and_then(|d| d.as_str()).unwrap_or("");

            // Strip HTML tags from item: "<b>sort</b> :: Ord a => [a] -> [a]"
            let clean_item = strip_html(item);
            let (name, signature) = if let Some(idx) = clean_item.find("::") {
                (
                    clean_item[..idx].trim().to_string(),
                    clean_item[idx + 2..].trim().to_string(),
                )
            } else {
                (clean_item.trim().to_string(), String::new())
            };

            Some(HoogleResult {
                module,
                name,
                signature,
                doc: strip_html(docs),
                url,
            })
        })
        .collect();

    Some(results)
}

/// The hoogle JSON endpoint returns nothing for raw `[`, `]`, `>` etc. in
/// the query (issue #26). Spaces go to `+`, like a form post; everything
/// outside the RFC 3986 unreserved set gets `%HH`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b' ' => out.push('+'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
    }
    // Decode common HTML entities
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cli_line_with_module() {
        let r = parse_cli_line("Data.List sort :: Ord a => [a] -> [a]").unwrap();
        assert_eq!(r.module, "Data.List");
        assert_eq!(r.name, "sort");
        assert_eq!(r.signature, "Ord a => [a] -> [a]");
    }

    #[test]
    fn test_parse_cli_line_no_module() {
        let r = parse_cli_line("map :: (a -> b) -> [a] -> [b]").unwrap();
        assert_eq!(r.name, "map");
        assert_eq!(r.signature, "(a -> b) -> [a] -> [b]");
    }

    #[test]
    fn test_parse_hoogle_json() {
        let json = r#"[{"url":"https://example.com","module":{"name":"Data.List"},"item":"<b>sort</b> :: Ord a =&gt; [a] -&gt; [a]","docs":"Sort a list."}]"#;
        let results = parse_hoogle_json(json).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "sort");
        assert_eq!(results[0].module, "Data.List");
        assert!(results[0].signature.contains("Ord a"));
        assert_eq!(results[0].doc, "Sort a list.");
    }

    #[test]
    fn test_strip_html() {
        assert_eq!(
            strip_html("<b>sort</b> :: Ord a =&gt; [a]"),
            "sort :: Ord a => [a]"
        );
    }

    #[test]
    fn unquote_strips_double_quotes() {
        // Issue #26: hoogle has to receive `[a] -> [a]`, not `"[a] -> [a]"`.
        assert_eq!(unquote("\"[a] -> [a]\""), "[a] -> [a]");
    }

    #[test]
    fn unquote_strips_single_quotes() {
        assert_eq!(unquote("'sort'"), "sort");
    }

    #[test]
    fn unquote_keeps_unquoted() {
        assert_eq!(unquote("[a] -> [a]"), "[a] -> [a]");
        assert_eq!(unquote("sort"), "sort");
    }

    #[test]
    fn unquote_keeps_mismatched_quotes() {
        // Asymmetric quoting isn't a closing pair, leave it alone.
        assert_eq!(unquote("\"foo'"), "\"foo'");
        assert_eq!(unquote("\"foo"), "\"foo");
    }

    #[test]
    fn percent_encode_type_signature() {
        // `[a] -> [a]` would silently fail without proper encoding.
        assert_eq!(percent_encode("[a] -> [a]"), "%5Ba%5D+-%3E+%5Ba%5D");
    }

    #[test]
    fn percent_encode_passes_unreserved() {
        assert_eq!(percent_encode("sort"), "sort");
        assert_eq!(percent_encode("a.b-c_d~e"), "a.b-c_d~e");
    }

    #[test]
    fn percent_encode_handles_special_chars() {
        // `&` has to be encoded or it'd break out of the query param.
        assert_eq!(percent_encode("a b&c"), "a+b%26c");
    }
}
