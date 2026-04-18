use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub pretty_errors: bool,
    pub show_timing: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pretty_errors: true,
            show_timing: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Config::default(),
        };
        let map = parse_ini(&content);
        Config {
            pretty_errors: map
                .get("pretty_errors")
                .or(map.get("pretty-errors"))
                .map(|v| v == "true" || v == "yes" || v == "1" || v == "on")
                .unwrap_or(true),
            show_timing: map
                .get("show_timing")
                .or(map.get("show-timing"))
                .map(|v| v == "true" || v == "yes" || v == "1" || v == "on")
                .unwrap_or(false),
        }
    }
}

fn config_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".ghcitty")
    } else {
        PathBuf::from(".ghcitty")
    }
}

fn parse_ini(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            map.insert(key.trim().to_lowercase(), val.trim().to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ini_basic() {
        let content = "# comment\npretty_errors = false\n";
        let map = parse_ini(content);
        assert_eq!(map.get("pretty_errors").unwrap(), "false");
    }

    #[test]
    fn test_parse_ini_whitespace() {
        let content = "  pretty-errors  =  yes  \n";
        let map = parse_ini(content);
        assert_eq!(map.get("pretty-errors").unwrap(), "yes");
    }

    #[test]
    fn test_config_default() {
        let c = Config::default();
        assert!(c.pretty_errors);
    }
}
