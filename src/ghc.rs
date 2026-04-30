use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use crate::error::{Error, Result};
use crate::parse::{self, EvalResult, SENTINEL};

/// PTY master fd for the active GHCi process. Read by the SIGINT handler to
/// forward ^C to GHCi (so the running expression aborts, not ghcitty itself).
static PTY_MASTER_FD: AtomicI32 = AtomicI32::new(-1);

/// How to launch GHCi: bare, or wrapped by stack/cabal so project modules
/// auto-load. Detected by `detect_project` checking `dir` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Plain,
    Stack,
    Cabal,
}

impl LaunchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "ghci",
            Self::Stack => "stack ghci",
            Self::Cabal => "cabal repl",
        }
    }
}

/// Detect project tooling in `dir` only (no parent walking). Returns the
/// matching launch mode if a marker file sits directly in `dir`, else None.
pub fn detect_project(dir: &Path) -> Option<LaunchMode> {
    if dir.join("stack.yaml").exists() {
        return Some(LaunchMode::Stack);
    }
    if dir.join("cabal.project").exists() {
        return Some(LaunchMode::Cabal);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("cabal") {
                return Some(LaunchMode::Cabal);
            }
        }
    }
    None
}

extern "C" fn forward_sigint(_sig: libc::c_int) {
    let fd = PTY_MASTER_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let buf = [0x03u8];
        // write(2) is async-signal-safe.
        unsafe {
            libc::write(fd, buf.as_ptr() as *const libc::c_void, 1);
        }
    }
}

#[cfg(unix)]
struct SigintGuard {
    old: libc::sigaction,
}

#[cfg(unix)]
impl SigintGuard {
    fn install() -> Self {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = forward_sigint as *const () as usize;
            libc::sigemptyset(&mut sa.sa_mask);
            // No SA_RESTART, so poll() returns EINTR and we loop to read
            // GHCi's interrupt response
            sa.sa_flags = 0;
            let mut old: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, &sa, &mut old);
            SigintGuard { old }
        }
    }
}

#[cfg(unix)]
impl Drop for SigintGuard {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGINT, &self.old, std::ptr::null_mut());
        }
    }
}

pub struct GhcProcess {
    child: Child,
    pty_master: std::fs::File,
    stdout: BufReader<std::process::ChildStdout>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    _stderr_thread: thread::JoinHandle<()>,
    /// mtime snapshot of loaded modules, for auto-reload
    loaded_mtimes: HashMap<PathBuf, SystemTime>,
}

impl GhcProcess {
    pub fn spawn_with_mode(mode: LaunchMode) -> Result<Self> {
        let (program, args) = match mode {
            LaunchMode::Plain => {
                let ghci = find_ghci().ok_or_else(|| Error::Ghc("ghci not found".into()))?;
                (ghci, vec!["-v0".to_string()])
            }
            LaunchMode::Stack => (
                "stack".to_string(),
                vec!["ghci".into(), "--ghci-options=-v0".into()],
            ),
            LaunchMode::Cabal => (
                "cabal".to_string(),
                vec!["repl".into(), "--repl-options=-v0".into()],
            ),
        };

        // PTY so GHCi stays in interactive mode
        let (master_fd, slave_fd) = open_pty()?;

        let slave_stdio = unsafe { Stdio::from(std::os::unix::io::OwnedFd::from_raw_fd(slave_fd)) };

        let mut child = Command::new(&program)
            .args(&args)
            .stdin(slave_stdio)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Ghc(format!("failed to spawn {program}: {e}")))?;

        let pty_master = unsafe { std::fs::File::from_raw_fd(master_fd) };
        PTY_MASTER_FD.store(master_fd, Ordering::Relaxed);
        let stdout = BufReader::new(child.stdout.take().unwrap());

        // stack/cabal print build progress on stderr before GHCi is ready;
        // tee it through so a long first build doesn't look like a hang.
        let tee_stderr = Arc::new(AtomicBool::new(mode != LaunchMode::Plain));
        let tee_clone = Arc::clone(&tee_stderr);

        let stderr = child.stderr.take().unwrap();
        let stderr_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_capture = Arc::clone(&stderr_lines);
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    if tee_clone.load(Ordering::Relaxed) {
                        eprintln!("{line}");
                    } else {
                        stderr_capture.lock().unwrap().push(line);
                    }
                }
            }
        });

        let mut proc = GhcProcess {
            child,
            pty_master,
            stdout,
            stderr_lines,
            _stderr_thread: stderr_thread,
            loaded_mtimes: HashMap::new(),
        };

        // Init: fuzzy match the first sentinel (GHCi's default prompt may be on the same line)
        proc.send_raw(":set prompt \"\"")?;
        proc.send_raw(":set prompt-cont \"\"")?;
        proc.send_raw(&format!("putStrLn \"{}\"", SENTINEL))?;
        if mode == LaunchMode::Plain {
            proc.read_until_sentinel_fuzzy()?;
        } else {
            proc.read_until_sentinel_streaming()?;
        }

        // Stop teeing now that GHCi is ready; subsequent stderr is per-command
        // diagnostics that the renderer wants to format.
        tee_stderr.store(false, Ordering::Relaxed);

        // Set sentinel as the prompt and clear continuation prompt in one batch
        proc.send_raw(&format!(":set prompt \"\\n{}\\n\"", SENTINEL))?;
        proc.send_raw(":set prompt-cont \"\"")?;
        proc.read_until_sentinel()?; // prompt from :set prompt
        proc.read_until_sentinel()?; // prompt from :set prompt-cont

        Ok(proc)
    }

    fn send_raw(&mut self, input: &str) -> Result<()> {
        writeln!(self.pty_master, "{input}")?;
        self.pty_master.flush()?;
        Ok(())
    }

    /// Fuzzy: matches sentinel anywhere on a line (needed during init when
    /// GHCi's default prompt may appear on the same line as our sentinel).
    fn read_until_sentinel_fuzzy(&mut self) -> Result<String> {
        let mut output = String::new();
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err(Error::Ghc("ghci process exited unexpectedly".into()));
            }
            if line.contains(SENTINEL) {
                break;
            }
            output.push_str(&line);
        }
        output.pop(); // remove final newline
        Ok(output)
    }

    /// Like `read_until_sentinel_fuzzy`, but tees each pre-sentinel line to
    /// the user's stderr. Used during project-mode init so a long stack/cabal
    /// build doesn't look like ghcitty has hung.
    fn read_until_sentinel_streaming(&mut self) -> Result<()> {
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err(Error::Ghc(
                    "ghci wrapper exited during init (build failed?)".into(),
                ));
            }
            if line.contains(SENTINEL) {
                return Ok(());
            } // Might give an extra newline, but good enough for init streaming output
            eprint!("{line}");
        }
    }

    fn read_until_sentinel(&mut self) -> Result<String> {
        let mut output = String::new();
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err(Error::Ghc("ghci process exited unexpectedly".into()));
            }
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            if trimmed == SENTINEL {
                break;
            }
            output.push_str(&line);
        }
        output.pop(); // remove final newline
        Ok(output)
    }

    fn drain_stderr(&self) -> Vec<String> {
        thread::sleep(std::time::Duration::from_millis(5));
        let mut lines = self.stderr_lines.lock().unwrap();
        lines.drain(..).collect()
    }

    pub fn command(&mut self, cmd: &str) -> Result<(String, Vec<String>)> {
        self.drain_stderr();

        if cmd.contains('\n') {
            self.send_raw(":{")?;
            for line in cmd.lines() {
                self.send_raw(line)?;
            }
            self.send_raw(":}")?;
        } else {
            self.send_raw(cmd)?;
        }

        let stdout = self.read_until_sentinel()?;
        let stderr = self.drain_stderr();
        Ok((stdout, stderr))
    }

    /// Like `command`, but falls back to interactive IO (live output + stdin forwarding)
    /// if GHCi doesn't respond within 200ms.
    #[cfg(unix)]
    pub fn command_interactive(&mut self, cmd: &str) -> Result<(String, Vec<String>, bool)> {
        // While an expression is running, ^C from the terminal must abort the
        // expression (forwarded to GHCi via the PTY) instead of killing us.
        let _sigint = SigintGuard::install();

        self.drain_stderr();

        if cmd.contains('\n') {
            self.send_raw(":{")?;
            for line in cmd.lines() {
                self.send_raw(line)?;
            }
            self.send_raw(":}")?;
        } else {
            self.send_raw(cmd)?;
        }

        let stdout_fd = self.stdout.get_ref().as_raw_fd();
        let mut output = String::new();

        // Phase 1:
        // Try to read everything quickly (200ms window).
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        loop {
            while !self.stdout.buffer().is_empty() {
                let mut line = String::new();
                let n = self.stdout.read_line(&mut line)?;
                if n == 0 {
                    return Err(Error::Ghc("ghci process exited unexpectedly".into()));
                }
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed == SENTINEL {
                    let stderr = self.drain_stderr();
                    return Ok((output, stderr, false));
                }
                output.push_str(&line);
            }

            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            let mut pollfd = libc::pollfd {
                fd: stdout_fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
            let ret = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };

            if ret > 0 && (pollfd.revents & libc::POLLIN) != 0 {
                let mut line = String::new();
                let n = self.stdout.read_line(&mut line)?;
                if n == 0 {
                    return Err(Error::Ghc("ghci process exited unexpectedly".into()));
                }
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed == SENTINEL {
                    let stderr = self.drain_stderr();
                    return Ok((output, stderr, false));
                }
                output.push_str(&line);
            } else {
                break;
            }
        }

        // Phase 2:
        // We enter interactive mode - print (buffered) output
        // and forward to STDIN
        print!("{output}");
        std::io::stdout().flush().ok();

        loop {
            while !self.stdout.buffer().is_empty() {
                let mut line = String::new();
                let n = self.stdout.read_line(&mut line)?;
                if n == 0 {
                    return Err(Error::Ghc("ghci process exited unexpectedly".into()));
                }
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed == SENTINEL {
                    let stderr = self.drain_stderr();
                    return Ok((output, stderr, true));
                }
                print!("{line}");
                std::io::stdout().flush().ok();
                output.push_str(&line);
            }

            // Block until ghci produces output or user types...
            let mut pollfds = [
                libc::pollfd {
                    fd: stdout_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: 0,
                    events: libc::POLLIN,
                    revents: 0,
                }, // terminal stdin
            ];

            let ret = unsafe { libc::poll(pollfds.as_mut_ptr(), 2, -1) };
            if ret < 0 {
                continue;
            }

            if pollfds[0].revents & libc::POLLIN != 0 {
                let mut line = String::new();
                let n = self.stdout.read_line(&mut line)?;
                if n == 0 {
                    return Err(Error::Ghc("ghci process exited unexpectedly".into()));
                }
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed == SENTINEL {
                    let stderr = self.drain_stderr();
                    return Ok((output, stderr, true));
                }
                print!("{line}");
                std::io::stdout().flush().ok();
                output.push_str(&line);
            }

            // Forward terminal stdin to GHCi via PTY master
            if pollfds[1].revents & libc::POLLIN != 0 {
                let mut user_line = String::new();
                if std::io::stdin().read_line(&mut user_line).is_ok() && !user_line.is_empty() {
                    writeln!(self.pty_master, "{}", user_line.trim_end())?;
                    self.pty_master.flush()?;
                }
            }
        }
    }

    pub fn type_of(&mut self, expr: &str) -> Result<Option<String>> {
        let (raw, _stderr) = self.command(&format!(":type {expr}"))?;
        Ok(parse::parse_type_output(&format!(
            "{SENTINEL}\n{raw}{SENTINEL}"
        )))
    }

    pub fn eval(&mut self, expr: &str) -> Result<EvalResult> {
        let is_let = parse::is_let_binding(expr);
        let type_str = if is_let { None } else { self.type_of(expr)? };

        let (raw, stderr) = self.command(expr)?;
        let (value, mut diagnostics) =
            parse::parse_eval_output(&format!("{SENTINEL}\n{raw}{SENTINEL}"));

        if diagnostics.is_empty() && !stderr.is_empty() {
            let stderr_text = stderr.join("\n");
            let (_, stderr_diags) =
                parse::parse_eval_output(&format!("{SENTINEL}\n{stderr_text}\n{SENTINEL}"));
            if !stderr_diags.is_empty() {
                diagnostics = stderr_diags;
            } else if !stderr_text.trim().is_empty() {
                diagnostics.push(parse::simple_diagnostic("error", stderr_text));
            }
        }

        let type_str = if is_let && diagnostics.is_empty() {
            if let Some(name) = parse::let_bound_name(expr) {
                self.type_of(&name)?
            } else {
                None
            }
        } else {
            type_str
        };

        Ok(EvalResult {
            expr: expr.to_string(),
            type_str,
            value,
            diagnostics,
        })
    }

    #[cfg(unix)]
    pub fn eval_interactive(&mut self, expr: &str) -> Result<(EvalResult, bool)> {
        let is_let = parse::is_let_binding(expr);
        let type_str = if is_let { None } else { self.type_of(expr)? };

        let (raw, stderr, was_interactive) = self.command_interactive(expr)?;
        let (value, mut diagnostics) =
            parse::parse_eval_output(&format!("{SENTINEL}\n{raw}{SENTINEL}"));

        if diagnostics.is_empty() && !stderr.is_empty() {
            let stderr_text = stderr.join("\n");
            let (_, stderr_diags) =
                parse::parse_eval_output(&format!("{SENTINEL}\n{stderr_text}\n{SENTINEL}"));
            if !stderr_diags.is_empty() {
                diagnostics = stderr_diags;
            } else if !stderr_text.trim().is_empty() {
                diagnostics.push(parse::simple_diagnostic("error", stderr_text));
            }
        }

        let type_str = if is_let && diagnostics.is_empty() {
            if let Some(name) = parse::let_bound_name(expr) {
                self.type_of(&name)?
            } else {
                None
            }
        } else {
            type_str
        };

        Ok((
            EvalResult {
                expr: expr.to_string(),
                type_str,
                value,
                diagnostics,
            },
            was_interactive,
        ))
    }

    // Because `:complete repl` prefixes its output w/ a count header
    pub fn complete(&mut self, prefix: &str) -> Result<Vec<String>> {
        // GHCi parses commands line-by-line, so an embedded newline in the
        // quoted prefix would terminate `:complete repl` early. Inside a
        // `:{ :}` block reedline hands us the whole multi-line buffer, so
        // narrow to the segment after the last newline before quoting.
        let prefix = prefix.rsplit('\n').next().unwrap_or(prefix);
        let escaped = prefix.replace('\\', "\\\\").replace('"', "\\\"");
        let (raw, _stderr) = self.command(&format!(":complete repl \"{escaped}\""))?;
        let mut completions = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            if i == 0 {
                continue;
            }
            let trimmed = line.trim();
            if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
                completions.push(trimmed[1..trimmed.len() - 1].to_string());
            }
        }
        Ok(completions)
    }

    // Forward a raw `:` command to GHCi and
    // fold stderr into res becuse some commands send there
    pub fn passthrough(&mut self, cmd: &str) -> Result<String> {
        let (stdout, stderr) = self.command(cmd)?;
        let mut out = stdout.trim().to_string();
        if !stderr.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&stderr.join("\n"));
        }
        Ok(out)
    }

    pub fn ghc_version(&mut self) -> Result<String> {
        self.command("import qualified System.Info")?;
        self.command("import qualified Data.Version")?;
        let (raw, _) = self.command("Data.Version.showVersion System.Info.fullCompilerVersion")?;
        let version = raw.trim().trim_matches('"').to_string();
        if version.is_empty() {
            Ok("unknown".into())
        } else {
            Ok(version)
        }
    }

    /// Parse `:show modules` output and snapshot file mtimes.
    pub fn snapshot_loaded_modules(&mut self) {
        let Ok((raw, _)) = self.command(":show modules") else {
            return;
        };
        let mut mtimes = HashMap::new();
        for line in raw.lines() {
            // Format: "ModuleName    ( /path/to/File.hs, interpreted )"
            let line = line.trim();
            if let Some(start) = line.find("( ") {
                if let Some(end) = line[start..].find(',') {
                    let path = PathBuf::from(line[start + 2..start + end].trim());
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            mtimes.insert(path, mtime);
                        }
                    }
                }
            }
        }
        self.loaded_mtimes = mtimes;
    }

    /// Check if any loaded file has been modified since last snapshot.
    /// If so, send `:r` and re-snapshot. Returns reload output if reloaded.
    pub fn check_reload(&mut self) -> Option<String> {
        if self.loaded_mtimes.is_empty() {
            return None;
        }
        let changed = self.loaded_mtimes.iter().any(|(path, &old_mtime)| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| t > old_mtime)
                .unwrap_or(false)
        });
        if !changed {
            return None;
        }
        let (stdout, stderr) = self.command(":r").ok()?;
        self.snapshot_loaded_modules();
        let mut out = stdout.trim().to_string();
        if !stderr.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&stderr.join("\n"));
        }
        Some(out)
    }

    pub fn quit(&mut self) -> Result<()> {
        let _ = self.send_raw(":quit");
        // Give GHCi a moment to exit gracefully, then kill.
        for _ in 0..20 {
            if let Ok(Some(_)) = self.child.try_wait() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for GhcProcess {
    fn drop(&mut self) {
        let _ = self.quit();
    }
}

fn open_pty() -> Result<(i32, i32)> {
    let mut master: libc::c_int = 0;
    let mut slave: libc::c_int = 0;
    let ret = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ret != 0 {
        return Err(Error::Ghc("failed to create PTY for GHCi".into()));
    }

    // Disable echo on the PTY so our commands aren't reflected back to stdout.
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        libc::tcgetattr(slave, &mut termios);
        termios.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
        libc::tcsetattr(slave, libc::TCSANOW, &termios);
    }

    Ok((master, slave))
}

fn find_ghci() -> Option<String> {
    if which_exists("ghci") {
        return Some("ghci".into());
    }
    let home = std::env::var("HOME").ok()?;
    let candidates = [
        format!("{home}/.ghcup/bin/ghci"),
        format!("{home}/.local/bin/ghci"),
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.clone());
        }
    }
    None
}

fn which_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ghcitty-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_stack_at_root() {
        let root = fresh_dir("project-stack");
        fs::write(root.join("stack.yaml"), "resolver: lts-22.0\n").unwrap();
        assert_eq!(detect_project(&root), Some(LaunchMode::Stack));
    }

    #[test]
    fn detects_cabal_project_at_root() {
        let root = fresh_dir("project-cabal");
        fs::write(root.join("cabal.project"), "packages: .\n").unwrap();
        assert_eq!(detect_project(&root), Some(LaunchMode::Cabal));
    }

    #[test]
    fn detects_bare_cabal_file() {
        let root = fresh_dir("project-bare-cabal");
        fs::write(root.join("foo.cabal"), "name: foo\n").unwrap();
        assert_eq!(detect_project(&root), Some(LaunchMode::Cabal));
    }

    #[test]
    fn does_not_walk_up_from_subdir() {
        let root = fresh_dir("project-no-walkup");
        fs::write(root.join("stack.yaml"), "resolver: lts-22.0\n").unwrap();
        let nested = root.join("src/lib/deep");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(detect_project(&nested), None);
    }

    #[test]
    fn stack_wins_over_cabal_at_same_level() {
        let root = fresh_dir("project-stack-wins");
        fs::write(root.join("stack.yaml"), "resolver: lts-22.0\n").unwrap();
        fs::write(root.join("foo.cabal"), "name: foo\n").unwrap();
        assert_eq!(detect_project(&root), Some(LaunchMode::Stack));
    }

    #[test]
    fn no_project_returns_none() {
        let root = fresh_dir("project-none");
        assert_eq!(detect_project(&root), None);
    }
}
