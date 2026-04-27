mod config;
mod error;
mod ghc;
mod highlight;
mod hoogle;
mod input;
mod json;
mod parse;
mod pretty;
mod render;
mod session;
mod style;

use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{Parser, Subcommand};
use reedline::{
    default_vi_insert_keybindings, default_vi_normal_keybindings, EditCommand, KeyCode,
    KeyModifiers, Keybindings, Reedline, ReedlineEvent, ReedlineMenu, Signal, Vi,
};

// TODO: revisit this cursor control
const CR: &str = "\r";
const CLEAR_LINE: &str = "\x1b[2K";

#[derive(Parser)]
#[command(name = "ghcitty", about = "Fast, friendly GHCi", version)]
struct Cli {
    #[arg(long)]
    json: bool,

    #[arg(long)]
    session: Option<String>,

    #[arg(long, short = 'c')]
    r#continue: bool,

    /// Force plain `ghci`, skipping stack/cabal auto-detection.
    #[arg(long)]
    plain: bool,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Eval { expr: String },
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("ghcitty: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> error::Result<()> {
    let config = config::Config::load();

    // Spawn GHCi and create session in parallel
    let session_name = cli.session.clone();
    let continue_flag = cli.r#continue;
    let sess_handle = std::thread::spawn(move || -> error::Result<session::Session> {
        if continue_flag && session_name.is_none() {
            Ok(session::Session::latest().unwrap_or(session::Session::new(None)?))
        } else {
            session::Session::new(session_name.as_deref())
        }
    });

    let mode = resolve_launch_mode(cli.plain);
    let ghc = Arc::new(Mutex::new(ghc::GhcProcess::spawn_with_mode(mode)?));
    let mut sess = sess_handle.join().unwrap()?;

    // Replay
    if cli.r#continue {
        let mut g = ghc.lock().unwrap();
        match sess.replay_exprs() {
            Ok(exprs) => {
                for expr in &exprs {
                    let result = g.eval(expr)?;
                    if !cli.json {
                        eprint!("{}", render::render(&result, &config, None));
                    }
                }
                if !cli.json {
                    eprintln!(
                        "{}",
                        style::dim().paint(format!(
                            "(restored {} expressions from {})",
                            exprs.len(),
                            sess.path().display()
                        ))
                    );
                }
            }
            Err(_) => {
                if !cli.json {
                    eprintln!("{}", style::dim().paint("(no session to restore)"));
                }
            }
        }
        drop(g);
    }

    match cli.command {
        Some(Cmd::Eval { expr }) => {
            if !cli.json && mode != ghc::LaunchMode::Plain {
                eprintln!("{}", style::dim().paint(format!("(via {})", mode.label())));
            }
            let mut g = ghc.lock().unwrap();
            let t0 = Instant::now();
            let result = g.eval(&expr)?;
            let elapsed = t0.elapsed();
            sess.record(&result)?;
            if cli.json {
                println!("{}", json::to_json(&result));
            } else {
                let out = render::render(&result, &config, Some(elapsed));
                if std::io::stdout().is_terminal() {
                    print!("{out}");
                } else {
                    print!("{}", render::strip_ansi(&out));
                }
            }
        }
        None => {
            repl(ghc, &mut sess, config, cli.json, mode)?;
        }
    }

    Ok(())
}

/// Word- and line-nav bindings on top of reedline's defaults. Only some
/// terminals deliver these keys, so the bindings are best-effort.
fn add_word_nav_bindings(kb: &mut Keybindings) {
    let edit = |cmd| ReedlineEvent::Edit(vec![cmd]);
    let word_left = edit(EditCommand::MoveWordLeft { select: false });
    let word_right = edit(EditCommand::MoveWordRight { select: false });

    // TODO: resvisit ... this can be finnicky
    // Option+Arrow on macOS: jump by word.
    // Some terminals send the arrow as Alt+Left/Right, others (with "Use
    // Option as Meta key" set) send Esc+b / Esc+f, which crossterm decodes
    // as Alt+b / Alt+f. Bind both forms.
    kb.add_binding(KeyModifiers::ALT, KeyCode::Left, word_left.clone());
    kb.add_binding(KeyModifiers::ALT, KeyCode::Right, word_right.clone());
    kb.add_binding(KeyModifiers::ALT, KeyCode::Char('b'), word_left);
    kb.add_binding(KeyModifiers::ALT, KeyCode::Char('f'), word_right);

    kb.add_binding(
        KeyModifiers::SUPER,
        KeyCode::Left,
        edit(EditCommand::MoveToLineStart { select: false }),
    );
    kb.add_binding(
        KeyModifiers::SUPER,
        KeyCode::Right,
        edit(EditCommand::MoveToLineEnd { select: false }),
    );
    kb.add_binding(
        KeyModifiers::ALT,
        KeyCode::Backspace,
        edit(EditCommand::BackspaceWord),
    );
}

/// Resolves how to launch GHCi. Auto-detects a stack/cabal project only when
/// a marker file sits directly in cwd; `--plain` forces bare ghci.
fn resolve_launch_mode(plain_flag: bool) -> ghc::LaunchMode {
    if plain_flag {
        return ghc::LaunchMode::Plain;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    ghc::detect_project(&cwd).unwrap_or(ghc::LaunchMode::Plain)
}

fn config_list(config: &config::Config, json_mode: bool) {
    if json_mode {
        let map: serde_json::Map<String, serde_json::Value> = config
            .entries()
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v)))
            .collect();
        println!(
            "{}",
            serde_json::json!({ "command": ":config", "config": map })
        );
    } else {
        for (k, v) in config.entries() {
            eprintln!(":config_{k} = {}", style::dim().paint(v));
        }
    }
}

/// Dispatch `:config_<key> [value]`. Bool keys toggle when no value is given;
/// numeric keys require a value (no value prints the current one).
fn config_dispatch(expr: &str, config: &mut config::Config, json_mode: bool) {
    let mut tokens = expr.split_whitespace();
    let cmd = tokens.next().unwrap_or("");
    let key = match cmd.strip_prefix(":config_") {
        Some(k) if !k.is_empty() => k,
        _ => {
            eprintln!("config: missing key (try :config for the list)");
            return;
        }
    };
    let arg = tokens.next();

    let kind = config_kind(key);
    let new_value = match (kind, arg) {
        (Some(ConfigKind::Bool), None) => match key {
            "pretty_errors" => {
                config.pretty_errors = !config.pretty_errors;
                config.pretty_errors.to_string()
            }
            "pretty_print" => {
                config.pretty_print = !config.pretty_print;
                config.pretty_print.to_string()
            }
            "show_timing" => {
                config.show_timing = !config.show_timing;
                config.show_timing.to_string()
            }
            _ => unreachable!(),
        },
        (Some(_), Some(v)) => match config.set(key, v) {
            Ok(()) => v.to_string(),
            Err(e) => {
                eprintln!("config: {e}");
                return;
            }
        },
        (Some(ConfigKind::Number), None) => {
            let v = config
                .entries()
                .into_iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .unwrap_or_default();
            if json_mode {
                println!(
                    "{}",
                    serde_json::json!({"command": cmd, "key": key, "value": v})
                );
            } else {
                eprintln!(":config_{key} = {}", style::dim().paint(v));
            }
            return;
        }
        (None, _) => {
            eprintln!("config: unknown key '{key}' (try :config for the list)");
            return;
        }
    };

    if json_mode {
        println!(
            "{}",
            serde_json::json!({"command": cmd, "key": key, "value": new_value})
        );
    } else {
        eprintln!(":config_{key} = {}", style::dim().paint(new_value));
    }
}

#[derive(Copy, Clone)]
enum ConfigKind {
    Bool,
    Number,
}

fn config_kind(key: &str) -> Option<ConfigKind> {
    match key {
        "pretty_errors" | "pretty_print" | "show_timing" => Some(ConfigKind::Bool),
        "max_output_lines" | "max_output_chars" => Some(ConfigKind::Number),
        _ => None,
    }
}

/// Detects `:set prompt`, `:set prompt-cont`, `:set prompt-function`, etc.
/// Also catches `:seti prompt ...` (the GHCi alias for `:set` in interactive mode).
fn is_set_prompt(expr: &str) -> bool {
    let mut tokens = expr.split_whitespace();
    let cmd = tokens.next().unwrap_or("");
    if cmd != ":set" && cmd != ":seti" {
        return false;
    }
    matches!(tokens.next(), Some(arg) if arg == "prompt" || arg.starts_with("prompt-"))
}

fn dedent(input: &str) -> String {
    let all_lines: Vec<&str> = input.lines().collect();
    let start = all_lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(0);
    let end = all_lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    let lines = &all_lines[start..end];

    if lines.is_empty() {
        return String::new();
    }

    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|l| l.get(min_indent..).unwrap_or_else(|| l.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn repl(
    ghc: Arc<Mutex<ghc::GhcProcess>>,
    sess: &mut session::Session,
    mut config: config::Config,
    json_mode: bool,
    mode: ghc::LaunchMode,
) -> error::Result<()> {
    if !json_mode {
        let mut g = ghc.lock().unwrap();
        // Batch: get version + snapshot in single lock hold
        let version = g.ghc_version().unwrap_or("?".into());
        g.snapshot_loaded_modules();
        drop(g);
        let suffix = match mode {
            ghc::LaunchMode::Plain => format!("(GHC {version})"),
            other => format!("(GHC {version}, via {})", other.label()),
        };
        eprintln!(
            "{} {}",
            style::bold().paint(format!("ghcitty {}", env!("CARGO_PKG_VERSION"))),
            style::dim().paint(suffix),
        );
    }

    let completer = Box::new(input::GhciCompleter {
        ghc: Arc::clone(&ghc),
    });
    let hinter = Box::new(input::GhciHinter::new(Arc::clone(&ghc)));
    let highlighter = Box::new(input::HaskellHighlighter);
    let validator = Box::new(input::HaskellValidator);

    let menu = input::completion_menu();

    let mut vi_insert = default_vi_insert_keybindings();
    vi_insert.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    vi_insert.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('g'),
        ReedlineEvent::OpenEditor,
    );
    add_word_nav_bindings(&mut vi_insert);

    let mut vi_normal = default_vi_normal_keybindings();
    vi_normal.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('g'),
        ReedlineEvent::OpenEditor,
    );
    add_word_nav_bindings(&mut vi_normal);

    let history_path = session::history_path();
    let history = Box::new(
        reedline::FileBackedHistory::with_file(1000, history_path)
            .map_err(|e| error::Error::Io(std::io::Error::other(e.to_string())))?,
    );

    let editor_cmd = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());
    let temp_file = std::env::temp_dir().join("ghcitty-edit.hs");

    let mut line_editor = Reedline::create()
        .with_completer(completer)
        .with_hinter(hinter)
        .with_highlighter(highlighter)
        .with_validator(validator)
        .with_history(history)
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_edit_mode(Box::new(Vi::new(vi_insert, vi_normal)))
        .with_buffer_editor(std::process::Command::new(editor_cmd), temp_file)
        .with_quick_completions(true)
        .use_bracketed_paste(true);

    let prompt = input::GhciPrompt { json_mode };

    loop {
        // Auto-reload changed files
        if !json_mode {
            let mut g = ghc.lock().unwrap();
            if let Some(output) = g.check_reload() {
                eprintln!("{}", style::dim().paint("(reloading...)"));
                if !output.is_empty() {
                    print!("{}", render::render_passthrough(&output));
                }
            }
        }

        let raw_input = match line_editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => line,
            Ok(Signal::CtrlC) => continue,
            Ok(Signal::CtrlD) => break,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("reedline: {e}");
                break;
            }
        };

        // Strip :{ ... :} wrapper if present
        let input = if raw_input.trim_start().starts_with(":{") {
            let body: Vec<&str> = raw_input
                .lines()
                .skip(1) // skip :{
                .filter(|l| l.trim() != ":}")
                .collect();
            dedent(&body.join("\n"))
        } else {
            dedent(raw_input.trim())
        };

        let expr = input.trim().to_string();
        if expr.is_empty() {
            continue;
        }

        let mut g = ghc.lock().unwrap();

        if expr == "/" || (expr.starts_with('/') && !expr.starts_with("/=")) {
            let pattern = if expr.len() > 1 {
                Some(&expr[1..])
            } else {
                None
            };
            let output = g.passthrough(":show bindings")?;
            if json_mode {
                println!("{}", serde_json::json!({"command": "/", "output": output}));
            } else {
                print!("{}", render::render_bindings(&output, pattern));
            }
            drop(g);
            continue;
        }

        if expr.starts_with(':') {
            if expr == ":q" || expr == ":quit" {
                break;
            }

            // ghcitty drives GHCi via a sentinel prompt; letting the user
            // change it would deadlock the read loop
            if is_set_prompt(&expr) {
                if json_mode {
                    println!(
                        "{}",
                        serde_json::json!({
                            "command": expr,
                            "error": "ghcitty controls the GHCi prompt; :set prompt is not supported",
                        })
                    );
                } else {
                    eprintln!(
                        "{}",
                        style::dim().paint(
                            "(ghcitty controls the GHCi prompt, :set prompt is not supported)"
                        )
                    );
                }
                drop(g);
                continue;
            }

            // :config lists current settings. Per-knob commands like
            // :config_pretty_print toggle/set individual values. Session-only;
            // ~/.ghcitty supplies the persisted defaults.
            if expr == ":config" {
                drop(g);
                config_list(&config, json_mode);
                continue;
            }
            if expr.starts_with(":config_") {
                drop(g);
                config_dispatch(&expr, &mut config, json_mode);
                continue;
            }

            // :undo [N]
            if expr == ":undo" || expr.starts_with(":undo ") {
                let n: usize = expr
                    .strip_prefix(":undo")
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .unwrap_or(1);
                drop(g);
                match do_undo(&ghc, sess, &config, json_mode, n) {
                    Ok(count) => {
                        if !json_mode {
                            eprintln!(
                                "{}",
                                style::dim()
                                    .paint(format!("(undid {n}, replayed {count} expressions)"))
                            );
                        }
                    }
                    Err(e) => eprintln!("undo: {e}"),
                }
                continue;
            }
            if expr == ":scratch" {
                drop(g);
                match open_scratch() {
                    Ok(Some(path)) => {
                        let mut g = ghc.lock().unwrap();
                        let cmd = format!(":load {}", path.display());
                        let output = g.passthrough(&cmd)?;
                        g.snapshot_loaded_modules();
                        if json_mode {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "command": ":scratch",
                                    "path": path.display().to_string(),
                                    "output": output,
                                })
                            );
                        } else if !output.is_empty() {
                            print!("{}", render::render_passthrough(&output));
                        }
                    }
                    Ok(None) => {
                        if !json_mode {
                            eprintln!(
                                "{}",
                                style::dim().paint("(empty scratch buffer, nothing loaded)")
                            );
                        }
                    }
                    Err(e) => eprintln!("scratch: {e}"),
                }
                continue;
            }
            if expr == ":edit" || expr == ":e" {
                drop(g);
                match open_editor(None) {
                    Ok(Some(code)) => {
                        let mut g = ghc.lock().unwrap();
                        let t0 = Instant::now();
                        let (result, was_interactive) = g.eval_interactive(&code)?;
                        let elapsed = t0.elapsed();
                        sess.record(&result)?;
                        if json_mode {
                            println!("{}", json::to_json(&result));
                        } else if was_interactive {
                            print!(
                                "{}",
                                render::render_interactive_tail(&result, &config, Some(elapsed))
                            );
                        } else {
                            print!("{}", render::render(&result, &config, Some(elapsed)));
                        }
                    }
                    Ok(None) => {
                        if !json_mode {
                            eprintln!("{}", style::dim().paint("(empty buffer, nothing to eval)"));
                        }
                    }
                    Err(e) => eprintln!("edit: {e}"),
                }
                continue;
            }

            if let Some(name) = expr.strip_prefix(":doc ").map(str::trim) {
                drop(g);
                if json_mode {
                    let results = hoogle::search(name, 1);
                    println!(
                        "{}",
                        serde_json::json!({"command": ":doc", "query": name, "results": results.len()})
                    );
                } else {
                    eprint!("{}{CR}", style::dim().paint("(searching hoogle...)"));
                    if let Some(result) = hoogle::doc(name) {
                        eprint!("{CLEAR_LINE}");
                        print!("{}", render::render_hoogle_doc(&result));
                    } else {
                        eprintln!(
                            "{CLEAR_LINE}{}",
                            style::dim().paint(format!("(no docs found for '{name}')"))
                        );
                    }
                }
                continue;
            }

            if let Some(query) = expr.strip_prefix(":hoogle ").map(str::trim) {
                drop(g);
                if json_mode {
                    let results = hoogle::search(query, 10);
                    println!(
                        "{}",
                        serde_json::json!({"command": ":hoogle", "query": query, "count": results.len()})
                    );
                } else {
                    eprint!("{}{CR}", style::dim().paint("(searching hoogle...)"));
                    let results = hoogle::search(query, 10);
                    eprint!("{CLEAR_LINE}");
                    print!("{}", render::render_hoogle_results(&results));
                }
                continue;
            }

            // Shell commands skip the passthrough renderer: output is shell
            // text (not Haskell) and may need stdin forwarding for cat/less.
            if expr.starts_with(":!") {
                let (stdout, stderr, was_interactive) = g.command_interactive(&expr)?;
                if json_mode {
                    let mut combined = stdout;
                    if !stderr.is_empty() {
                        if !combined.is_empty() && !combined.ends_with('\n') {
                            combined.push('\n');
                        }
                        combined.push_str(&stderr.join("\n"));
                    }
                    println!(
                        "{}",
                        serde_json::json!({"command": expr, "output": combined})
                    );
                } else {
                    if !was_interactive {
                        print!("{stdout}");
                    }
                    if !stderr.is_empty() {
                        eprintln!("{}", stderr.join("\n"));
                    }
                }
                continue;
            }

            let output = g.passthrough(&expr)?;

            // Re-snapshot after :load/:reload/:add
            if expr.starts_with(":l") || expr.starts_with(":r") || expr.starts_with(":add") {
                g.snapshot_loaded_modules();
            }

            if json_mode {
                println!("{}", serde_json::json!({"command": expr, "output": output}));
            } else {
                print!("{}", render::render_passthrough(&output));
            }
        } else {
            let t0 = Instant::now();
            let (result, was_interactive) = g.eval_interactive(&expr)?;
            let elapsed = t0.elapsed();
            sess.record(&result)?;
            if json_mode {
                println!("{}", json::to_json(&result));
            } else if was_interactive {
                print!(
                    "{}",
                    render::render_interactive_tail(&result, &config, Some(elapsed))
                );
            } else {
                print!("{}", render::render(&result, &config, Some(elapsed)));
            }
        }
    }

    Ok(())
}

/// Undo last `n` expressions by replaying the rest.
fn do_undo(
    ghc: &Arc<Mutex<ghc::GhcProcess>>,
    sess: &mut session::Session,
    config: &config::Config,
    json_mode: bool,
    n: usize,
) -> error::Result<usize> {
    let exprs = sess.replay_exprs().unwrap_or_default();
    let keep = exprs.len().saturating_sub(n);
    let replay = &exprs[..keep];

    // Reset and replay
    let mut g = ghc.lock().unwrap();
    g.command(":load")?;

    for expr in replay {
        let result = g.eval(expr)?;
        if !json_mode && !result.diagnostics.is_empty() {
            eprint!("{}", render::render(&result, config, None));
        }
    }

    sess.rewrite(replay)?;
    Ok(keep)
}

/// Open the persistent scratch module in $EDITOR. Seeds with a module skeleton
/// on first use so `:load` succeeds without the user having to type the header.
/// Returns the path to load if the file has any non-blank content, or None.
fn open_scratch() -> error::Result<Option<std::path::PathBuf>> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    let path = session::scratch_path()?;
    if !path.exists() {
        let seed = "\
module Scratch where

-- Top-level declarations only (no `let` here, that's REPL-only).
-- Save the file to load these into the session.
--
-- double :: Int -> Int
-- double x = x * 2
";
        std::fs::write(&path, seed)?;
    }

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| error::Error::Ghc(format!("failed to launch {editor}: {e}")))?;

    if !status.success() {
        return Err(error::Error::Ghc(format!("{editor} exited with {status}")));
    }

    let content = std::fs::read_to_string(&path)?;
    if content.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(path))
    }
}

/// Open $EDITOR, return contents on save.
fn open_editor(seed: Option<&str>) -> error::Result<Option<String>> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    let dir = std::env::temp_dir();
    let path = dir.join("ghcitty-edit.hs");
    if let Some(content) = seed {
        std::fs::write(&path, content)?;
    } else {
        std::fs::write(&path, "")?;
    }

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| error::Error::Ghc(format!("failed to launch {editor}: {e}")))?;

    if !status.success() {
        return Err(error::Error::Ghc(format!("{editor} exited with {status}")));
    }

    let content = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);

    let trimmed = content.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_prompt_detected() {
        assert!(is_set_prompt(":set prompt"));
        assert!(is_set_prompt(":set prompt foo"));
        assert!(is_set_prompt(":set prompt \"ghci> \""));
        assert!(is_set_prompt(":set prompt-cont"));
        assert!(is_set_prompt(":set prompt-cont \"  \""));
        assert!(is_set_prompt(
            ":set prompt-function (\\_ _ -> return \"%\")"
        ));
        assert!(is_set_prompt(":seti prompt foo"));
    }

    #[test]
    fn other_set_passes_through() {
        assert!(!is_set_prompt(":set -fwarn-unused-imports"));
        assert!(!is_set_prompt(":set +s"));
        assert!(!is_set_prompt(":set"));
        assert!(!is_set_prompt(":load Foo.hs"));
        assert!(!is_set_prompt("prompt = 1"));
        assert!(!is_set_prompt(":show prompt"));
    }
}
