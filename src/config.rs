use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub pretty_errors: bool,
    pub pretty_values: bool,
    pub show_timing: bool,
    pub max_output_lines: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pretty_errors: true,
            pretty_values: true,
            show_timing: false,
            max_output_lines: 20,
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
            pretty_errors: bool_setting(&map, "pretty_errors", true),
            pretty_values: bool_setting(&map, "pretty_values", true),
            show_timing: bool_setting(&map, "show_timing", false),
            max_output_lines: usize_setting(&map, "max_output_lines", 20),
        }
    }
}

fn bool_setting(map: &HashMap<String, String>, key: &str, default: bool) -> bool {
    let dashed = key.replace('_', "-");
    map.get(key)
        .or_else(|| map.get(&dashed))
        .map(|v| matches!(v.as_str(), "true" | "yes" | "1" | "on"))
        .unwrap_or(default)
}

fn usize_setting(map: &HashMap<String, String>, key: &str, default: usize) -> usize {
    let dashed = key.replace('_', "-");
    map.get(key)
        .or_else(|| map.get(&dashed))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
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
