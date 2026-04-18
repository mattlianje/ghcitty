mod config;
mod error;
mod ghc;
mod highlight;
mod hoogle;
mod input;
mod json;
mod parse;
mod render;
mod session;

use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{Parser, Subcommand};
use reedline::{
    default_vi_insert_keybindings, default_vi_normal_keybindings, KeyCode, KeyModifiers, Reedline,
    ReedlineEvent, ReedlineMenu, Signal, Vi,
};

#[derive(Parser)]
#[command(name = "ghcitty", about = "Fast, friendly GHCi", version)]
struct Cli {
    #[arg(long)]
    json: bool,

    #[arg(long)]
    session: Option<String>,

    #[arg(long, short = 'c')]
    r#continue: bool,

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

    let ghc = Arc::new(Mutex::new(ghc::GhcProcess::spawn()?));
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
                        "\x1b[2m(restored {} expressions from {})\x1b[0m",
                        exprs.len(),
                        sess.path().display()
                    );
                }
            }
            Err(_) => {
                if !cli.json {
                    eprintln!("\x1b[2m(no session to restore)\x1b[0m");
                }
            }
        }
        drop(g);
    }

    match cli.command {
        Some(Cmd::Eval { expr }) => {
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
            repl(ghc, &mut sess, &config, cli.json)?;
        }
    }

    Ok(())
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
    config: &config::Config,
    json_mode: bool,
) -> error::Result<()> {
    if !json_mode {
        let mut g = ghc.lock().unwrap();
        // Batch: get version + snapshot in single lock hold
        let version = g.ghc_version().unwrap_or("?".into());
        g.snapshot_loaded_modules();
        drop(g);
        eprintln!(
            "\x1b[1mghcitty {}\x1b[0m \x1b[2m(GHC {version})\x1b[0m",
            env!("CARGO_PKG_VERSION")
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
    let mut vi_normal = default_vi_normal_keybindings();
    vi_normal.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('g'),
        ReedlineEvent::OpenEditor,
    );

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
        .use_bracketed_paste(true);

    let prompt = input::GhciPrompt { json_mode };

    loop {
        // Auto-reload changed files
        if !json_mode {
            let mut g = ghc.lock().unwrap();
            if let Some(output) = g.check_reload() {
                eprintln!("\x1b[2m(reloading...)\x1b[0m");
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

            // :undo [N]
            if expr == ":undo" || expr.starts_with(":undo ") {
                let n: usize = expr
                    .strip_prefix(":undo")
                    .unwrap_or("")
                    .trim()
                    .parse()
                    .unwrap_or(1);
                drop(g);
                match do_undo(&ghc, sess, config, json_mode, n) {
                    Ok(count) => {
                        if !json_mode {
                            eprintln!("\x1b[2m(undid {n}, replayed {count} expressions)\x1b[0m");
                        }
                    }
                    Err(e) => eprintln!("undo: {e}"),
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
                            render::render_interactive_tail(&result, config, Some(elapsed));
                        } else {
                            print!("{}", render::render(&result, config, Some(elapsed)));
                        }
                    }
                    Ok(None) => {
                        if !json_mode {
                            eprintln!("\x1b[2m(empty buffer, nothing to eval)\x1b[0m");
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
                    eprint!("\x1b[2m(searching hoogle...)\x1b[0m\r");
                    if let Some(result) = hoogle::doc(name) {
                        eprint!("\x1b[2K"); // clear the "searching" line
                        print!("{}", render::render_hoogle_doc(&result));
                    } else {
                        eprintln!("\x1b[2K\x1b[2m(no docs found for '{name}')\x1b[0m");
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
                    eprint!("\x1b[2m(searching hoogle...)\x1b[0m\r");
                    let results = hoogle::search(query, 10);
                    eprint!("\x1b[2K"); // clear the "searching" line
                    print!("{}", render::render_hoogle_results(&results));
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
                render::render_interactive_tail(&result, &config, Some(elapsed));
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
