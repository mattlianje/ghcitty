use std::fs;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::parse::EvalResult;

const MAX_LINES: usize = 500;

pub struct Session {
    path: PathBuf,
    line_count: usize,
}

pub fn history_path() -> PathBuf {
    dirs_fallback().join("ghcitty").join("history")
}

fn sessions_dir() -> Result<PathBuf> {
    let dir = dirs_fallback().join("ghcitty");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn dirs_fallback() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else if let Some(home) = home_dir() {
        home.join(".local").join("share")
    } else {
        PathBuf::from(".ghcitty")
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

impl Session {
    pub fn new(name: Option<&str>) -> Result<Self> {
        let dir = sessions_dir()?;
        let filename = match name {
            Some(n) => format!("{n}.hs"),
            None => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                format!("session-{ts}.hs")
            }
        };
        let path = dir.join(&filename);

        // Update the `latest` symlink
        let latest = dir.join("latest.hs");
        let _ = fs::remove_file(&latest);
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&filename, &latest);
        }

        // Count existing lines if file already exists
        let line_count = fs::read_to_string(&path)
            .map(|c| c.lines().count())
            .unwrap_or(0);

        Ok(Session { path, line_count })
    }

    pub fn latest() -> Result<Self> {
        let dir = sessions_dir()?;
        let latest = dir.join("latest.hs");
        let target = fs::read_link(&latest)
            .map_err(|e| Error::Session(format!("no latest session: {e}")))?;
        let path = if target.is_relative() {
            dir.join(target)
        } else {
            target
        };
        let line_count = fs::read_to_string(&path)
            .map(|c| c.lines().count())
            .unwrap_or(0);
        Ok(Session { path, line_count })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Append as `-- λ> expr` / `-- result :: Type` comments.
    pub fn record(&mut self, result: &EvalResult) -> Result<()> {
        // Soft-trim: if we're over MAX_LINES, trim the file first
        if self.line_count >= MAX_LINES {
            self.trim()?;
        }

        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        writeln!(f, "-- \u{03bb}> {}", result.expr)?;
        self.line_count += 1;

        if !result.value.is_empty() {
            let type_suffix = result
                .type_str
                .as_deref()
                .map(|t| format!(" :: {t}"))
                .unwrap_or_default();
            for line in result.value.lines() {
                writeln!(f, "-- {line}{type_suffix}")?;
                self.line_count += 1;
            }
        }
        for d in &result.diagnostics {
            writeln!(
                f,
                "-- [{severity}] {msg}",
                severity = d.severity,
                msg = d.message
            )?;
            self.line_count += 1;
        }
        writeln!(f)?;
        self.line_count += 1;
        Ok(())
    }

    fn trim(&mut self) -> Result<()> {
        let content = fs::read_to_string(&self.path)?;
        let lines: Vec<&str> = content.lines().collect();
        let keep = MAX_LINES / 2;
        if lines.len() <= keep {
            return Ok(());
        }
        let trimmed: Vec<&str> = lines[lines.len() - keep..].to_vec();
        let mut new_content = format!("-- (trimmed at {} lines)\n\n", lines.len());
        for line in &trimmed {
            new_content.push_str(line);
            new_content.push('\n');
        }
        fs::write(&self.path, new_content)?;
        self.line_count = keep + 2; // +2 for the notice
        Ok(())
    }

    /// Rewrite the session file with only the given expressions.
    pub fn rewrite(&mut self, exprs: &[String]) -> Result<()> {
        use std::io::Write;
        let mut f = fs::File::create(&self.path)?;
        self.line_count = 0;
        for expr in exprs {
            writeln!(f, "-- \u{03bb}> {expr}")?;
            self.line_count += 1;
        }
        Ok(())
    }

    pub fn replay_exprs(&self) -> Result<Vec<String>> {
        let content = fs::read_to_string(&self.path).map_err(|e| {
            Error::Session(format!("cannot read session {}: {e}", self.path.display()))
        })?;
        let prefix = "-- \u{03bb}> ";
        let exprs = content
            .lines()
            .filter_map(|line| line.strip_prefix(prefix))
            .map(|s| s.to_string())
            .collect();
        Ok(exprs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::EvalResult;

    #[test]
    fn test_record_and_replay() {
        let dir = std::env::temp_dir().join("ghcitty-test-record");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-session.hs");
        let _ = fs::remove_file(&path);

        let mut session = Session {
            path: path.clone(),
            line_count: 0,
        };
        session
            .record(&EvalResult {
                expr: "map (+1) [1,2,3]".into(),
                type_str: Some("[Integer]".into()),
                value: "[2,3,4]".into(),
                diagnostics: vec![],
            })
            .unwrap();
        session
            .record(&EvalResult {
                expr: "1 + 1".into(),
                type_str: Some("Integer".into()),
                value: "2".into(),
                diagnostics: vec![],
            })
            .unwrap();

        let exprs = session.replay_exprs().unwrap();
        assert_eq!(exprs, vec!["map (+1) [1,2,3]", "1 + 1"]);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("-- \u{03bb}> map (+1) [1,2,3]"));
        assert!(content.contains("-- [2,3,4] :: [Integer]"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_soft_trim() {
        let dir = std::env::temp_dir().join("ghcitty-test-trim");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-trim.hs");
        let _ = fs::remove_file(&path);

        // Write a file with MAX_LINES lines
        let mut content = String::new();
        for i in 0..MAX_LINES {
            content.push_str(&format!("-- \u{03bb}> expr_{i}\n"));
        }
        fs::write(&path, &content).unwrap();

        let mut session = Session {
            path: path.clone(),
            line_count: MAX_LINES,
        };

        // Record one more — should trigger trim.
        session
            .record(&EvalResult {
                expr: "final".into(),
                type_str: None,
                value: "done".into(),
                diagnostics: vec![],
            })
            .unwrap();

        let new_content = fs::read_to_string(&path).unwrap();
        assert!(new_content.contains("trimmed at"));
        assert!(new_content.lines().count() < MAX_LINES);
        // Should still contain the latest expressions
        assert!(new_content.contains("-- \u{03bb}> final"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_symlink_created() {
        // This test only checks that new() doesn't panic
        // and creates a file path that makes sense
        let session = Session::new(Some("test-symlink")).unwrap();
        assert!(session.path().to_string_lossy().contains("test-symlink.hs"));
        let _ = fs::remove_file(session.path());
    }
}
