//! `:core` / `:stg` viewer. GHCi runs interpreted bytecode, so it can't show
//! optimized output. We drop the expression into a temp module, compile it
//! out-of-process with `-O2` plus a dump flag, then extract and highlight it.

use std::collections::HashMap;
use std::process::Command;

use crate::error::{Error, Result};
use crate::ghc::LaunchMode;
use crate::{highlight, style};

#[derive(Clone, Copy)]
pub enum DumpKind {
    Core,
    Stg,
    Cmm,
}

impl DumpKind {
    fn dump_flag(self) -> &'static str {
        match self {
            DumpKind::Core => "-ddump-simpl",
            DumpKind::Stg => "-ddump-stg-final",
            DumpKind::Cmm => "-ddump-cmm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DumpKind::Core => "core",
            DumpKind::Stg => "stg",
            DumpKind::Cmm => "cmm",
        }
    }
}

/// Name we wrap the expression in. No leading underscore: `_name` parses as a
/// named typed hole, not a binding.
const BINDING: &str = "ghcittyDump";

/// Compile `expr` under `-O2` and return the highlighted dump, or GHC's error
/// if it doesn't typecheck. `bindings` are the session's `let` definitions,
/// spliced in as top-level decls so `:core <name>` resolves prompt-defined names.
pub fn dump(
    kind: DumpKind,
    expr: &str,
    imports: &[String],
    bindings: &[String],
    mode: LaunchMode,
) -> Result<String> {
    let dir = std::env::temp_dir().join("ghcitty-core");
    std::fs::create_dir_all(&dir)?;
    let src = dir.join("GhcittyDump.hs");
    std::fs::write(&src, build_module(expr, imports, bindings))?;

    let out_dir = dir.to_string_lossy().to_string();
    let src_path = src.to_string_lossy().to_string();
    let flags = [
        "-O2",
        "-fforce-recomp",
        kind.dump_flag(),
        "-dsuppress-all",
        "-dsuppress-uniques",
        "-dno-typeable-binds",
        "-c",
        "-outputdir",
        &out_dir,
        &src_path,
    ];

    let output = run_ghc(mode, &flags)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let section = extract_section(&stdout);
    if section.trim().is_empty() {
        // No dump means it didn't compile; surface GHC's error verbatim.
        let msg = stderr.trim();
        let msg = if msg.is_empty() {
            "no output produced (does the expression typecheck?)".to_string()
        } else {
            msg.to_string()
        };
        return Err(Error::Ghc(msg));
    }
    let filtered = match kind {
        // Cmm is banner-delimited sections, not `name = ...` bindings.
        DumpKind::Cmm => filter_cmm(&section),
        DumpKind::Core | DumpKind::Stg => filter_reachable(&section),
    };
    Ok(render(&filtered))
}

fn build_module(expr: &str, imports: &[String], bindings: &[String]) -> String {
    let mut src = String::from("{-# OPTIONS_GHC -w #-}\nmodule GhcittyDump where\n");
    for imp in imports {
        src.push_str(imp);
        src.push('\n');
    }
    // Session `let` bindings become top-level decls. GHC treats module-level
    // bindings as mutually recursive, so order doesn't matter.
    for binding in bindings {
        src.push_str(binding);
        src.push('\n');
    }
    // Parenthesize so a multi-token expression binds as a whole. Indent the
    // body past column 1 or GHC's layout rule reads it as a new top-level decl.
    src.push_str(&format!("{BINDING} =\n  (\n"));
    for line in expr.lines() {
        src.push_str("    ");
        src.push_str(line);
        src.push('\n');
    }
    src.push_str("  )\n");
    src
}

/// Build the compiler invocation for the launch mode so the temp module sees
/// the same package set. Plain ghci uses `ghc`, projects use the wrapper.
fn run_ghc(mode: LaunchMode, flags: &[&str]) -> Result<std::process::Output> {
    let mut cmd = match mode {
        LaunchMode::Plain => {
            let mut c = Command::new("ghc");
            c.args(flags);
            c
        }
        LaunchMode::Stack => {
            let mut c = Command::new("stack");
            c.arg("ghc").arg("--").args(flags);
            c
        }
        LaunchMode::Cabal => {
            let mut c = Command::new("cabal");
            c.args(["exec", "-v0", "--", "ghc"]).args(flags);
            c
        }
    };
    cmd.output()
        .map_err(|e| Error::Ghc(format!("failed to run compiler: {e}")))
}

/// Keep everything from the `==== ... ====` banner onward, dropping leading
/// noise and trailing blank lines.
fn extract_section(stdout: &str) -> String {
    let start = stdout
        .lines()
        .position(|l| l.trim_start().starts_with("===================="));
    let body = match start {
        Some(i) => stdout.lines().skip(i).collect::<Vec<_>>().join("\n"),
        None => return String::new(),
    };
    body.trim_end().to_string()
}

/// True for chars that can appear in a Core/STG identifier.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\'' || c == '#' || c == '$'
}

/// We splice all session `let` bindings into the temp module, so the dump also
/// contains unrelated ones. Keep only the banner plus bindings reachable from
/// `ghcittyDump`. Returns the whole section if the shape is unexpected.
fn filter_reachable(section: &str) -> String {
    // Set banner / "Result size" lines aside; group the rest into bindings
    // separated by blank lines.
    let mut header: Vec<&str> = Vec::new();
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("====") || trimmed.starts_with("Result size") {
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
            header.push(line);
        } else if line.trim().is_empty() {
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(line);
        }
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }

    // The binder of a block is the first token of its first definition line,
    // skipping `-- RHS size` comments and `[IdInfo]` annotations.
    let binder = |block: &[Vec<&str>], i: usize| -> Option<String> {
        for line in &block[i] {
            if line.starts_with(char::is_whitespace) {
                continue;
            }
            let t = line.trim_start();
            if t.starts_with("--") || t.starts_with('[') {
                continue;
            }
            let name: String = t.chars().take_while(|&c| is_ident_char(c)).collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
        None
    };

    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    for i in 0..blocks.len() {
        if let Some(name) = binder(&blocks, i) {
            name_to_idx.insert(name, i);
        }
    }

    // Reachability from the wrapper binding. Bail to the full section if we
    // can't find it (unexpected dump shape).
    let Some(&root) = name_to_idx.get(BINDING) else {
        return section.to_string();
    };
    let mut keep = vec![false; blocks.len()];
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        if keep[i] {
            continue;
        }
        keep[i] = true;
        for line in &blocks[i] {
            for tok in line.split(|c: char| !is_ident_char(c)) {
                if let Some(&j) = name_to_idx.get(tok) {
                    if !keep[j] {
                        stack.push(j);
                    }
                }
            }
        }
    }

    let mut out: Vec<String> = header.iter().map(|l| l.to_string()).collect();
    for (i, block) in blocks.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if !out.is_empty() {
            out.push(String::new());
        }
        out.extend(block.iter().map(|l| l.to_string()));
    }
    out.join("\n").trim_end().to_string()
}

/// GHC emits one `==== Output Cmm ====` banner per top-level symbol. Keep only
/// chunks mentioning our wrapper (`ghcittyDump` and floated `ghcittyDump1`..);
/// spliced session bindings get their own `*_closure` symbols and are dropped.
/// Returns the whole section if nothing matched.
fn filter_cmm(section: &str) -> String {
    let mut chunks: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in section.lines() {
        if line.trim_start().starts_with("==================== Output Cmm") && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push(line);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }

    let kept: Vec<String> = chunks
        .into_iter()
        .filter(|chunk| chunk.iter().any(|l| l.contains(BINDING)))
        .map(|chunk| chunk.join("\n").trim_end().to_string())
        .collect();
    if kept.is_empty() {
        return section.to_string();
    }
    kept.join("\n\n")
}

fn render(section: &str) -> String {
    let mut out = String::new();
    for line in section.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("====") || trimmed.starts_with("Result size") || trimmed.starts_with("= {")
        {
            out.push_str(&style::dim().paint(line).to_string());
        } else {
            out.push_str(&highlight::highlight_input(line));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_module_includes_imports_and_bindings() {
        let src = build_module(
            "y",
            &["import Data.List".to_string()],
            &["y = (\\x -> x + 1) 41".to_string()],
        );
        assert!(src.contains("module GhcittyDump where"));
        assert!(src.contains("import Data.List"));
        assert!(src.contains("y = (\\x -> x + 1) 41"));
        assert!(src.contains("ghcittyDump ="));
        assert!(src.contains("    y"));
    }

    #[test]
    fn extract_section_drops_leading_noise() {
        let stdout = "[1 of 1] Compiling GhcittyDump\n\
            ==================== Tidy Core ====================\n\
            Result size of Tidy Core = {terms: 1}\n\
            ghcittyDump = I# 1#\n\n\n";
        let section = extract_section(stdout);
        assert!(section.starts_with("===================="));
        assert!(section.contains("ghcittyDump = I# 1#"));
        assert!(!section.ends_with('\n'));
    }

    #[test]
    fn extract_section_empty_when_no_banner() {
        assert_eq!(extract_section("error: boom\n"), "");
    }

    #[test]
    fn filter_reachable_drops_unrelated_bindings() {
        let section = "==================== Final STG: ====================\n\
            y = IS! [42#];\n\n\
            ghcittyDump5 = I#! [2#];\n\n\
            ghcittyDump1 = :! [ghcittyDump5 []];\n\n\
            ghcittyDump = :! [ghcittyDump5 ghcittyDump1];";
        let kept = filter_reachable(section);
        assert!(kept.contains("ghcittyDump ="));
        assert!(kept.contains("ghcittyDump5 ="));
        assert!(kept.contains("ghcittyDump1 ="));
        // The stale `let y` binding isn't referenced, so it's gone.
        assert!(!kept.contains("y = IS!"));
        assert!(kept.starts_with("===================="));
    }

    #[test]
    fn filter_reachable_keeps_referenced_session_binding() {
        // `:core y` wraps `y` itself, so the wrapper references it.
        let section = "==================== Final STG: ====================\n\
            y = IS! [42#];\n\n\
            ghcittyDump = y;";
        let kept = filter_reachable(section);
        assert!(kept.contains("y = IS!"));
        assert!(kept.contains("ghcittyDump = y"));
    }

    #[test]
    fn filter_cmm_keeps_only_wrapper_sections() {
        let section = "==================== Output Cmm ====================\n\
            [section \"\"data\" . y_closure\" {\n\
            \x20    y_closure:\n\
            \x20        const I#_con_info;\n\
            \x20        const 42;\n\
            \x20}]\n\n\n\
            ==================== Output Cmm ====================\n\
            [section \"\"data\" . ghcittyDump_closure\" {\n\
            \x20    ghcittyDump_closure:\n\
            \x20        const I#_con_info;\n\
            \x20        const 2;\n\
            \x20}]";
        let kept = filter_cmm(section);
        assert!(kept.contains("ghcittyDump_closure"));
        assert!(!kept.contains("y_closure"));
    }

    #[test]
    fn filter_cmm_passes_through_when_nothing_matched() {
        let section = "==================== Output Cmm ====================\n\
            [section \"\"data\" . y_closure\" { y_closure: }]";
        assert_eq!(filter_cmm(section), section);
    }

    #[test]
    fn filter_reachable_passes_through_unknown_shape() {
        let section = "==================== Tidy Core ====================\nsomething weird";
        assert_eq!(filter_reachable(section), section);
    }
}
