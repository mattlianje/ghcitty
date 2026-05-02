<div align="right">
  <sub><em>Part of <a href="https://github.com/mattlianje/d4"><img src="https://raw.githubusercontent.com/mattlianje/d4/master/pix/d4.png" width="23"></a> <a href="https://github.com/mattlianje/d4">d4</a></em></sub>
</div>

<p align="center">
  <img src="https://raw.githubusercontent.com/mattlianje/ghcitty/master/pix/ghcitty_github.png" width="350">
</p>
<!--
# <img src="https://raw.githubusercontent.com/mattlianje/d4/refs/heads/master/pix/ghcitty.png" width="60"> ghcitty
-->

# ghcitty

**Fast, friendly GHCi**

**ghcitty** is a tiny Rust binary that wraps GHCi with a snappy, delightful frontend

<p align="center">
  <img src="https://raw.githubusercontent.com/mattlianje/ghcitty/master/demos/completions-multiline.gif" width="600"><br>
  <sub><em>(demo) Tab completion, ghost hints, smart multiline</em></sub>
</p>

## Features

- Syntax highlighting
- Structured errors with expected/actual diffs, auto-import hints, error code links
- Tab completion with inline types
- Fish-style ghost completions
- Pretty-printed `Show` output (records, lists, tuples)
- Hoogle integration
- Binding explorer
- Auto-detect stack/cabal projects
- Bracketed paste
- Auto-saved, resumable sessions
- JSON mode for tooling
- Auto-reload on file changes
- Vi mode

## Of note

At the end of the day, ghcitty is just a modest GHCi frontend w/ a tiny memory footprint to give some niceties to an already formidable, fun and proven REPL.

## Install

### Prerequisites
You just need a [GHC](https://www.haskell.org/ghcup/install/) and [Rust](https://rust-lang.org/tools/install/) installed

### Cargo

[![Crates.io](https://img.shields.io/crates/v/ghcitty.svg)](https://crates.io/crates/ghcitty)

```
cargo install ghcitty
```

And use with:
```
ghcitty
```

### From source

```
git clone https://github.com/mattlianje/ghcitty.git
cd ghcitty
cargo install --path .
```

### Nix

```
nix profile install github:mattlianje/ghcitty
```

### Cabal / Stack

Run `ghcitty` from a directory with `stack.yaml`, `cabal.project`, or `*.cabal`
and it launches via `stack ghci` / `cabal repl`. The banner shows which:

```
ghcitty 0.3.0 (GHC 9.6.7, via cabal repl)
```

Pass `--plain` to force bare `ghci`. Anything after `--` is forwarded
verbatim to the underlying invocation uv-style...

```
ghcitty -- --flag mypkg:dev              # stack ghci --flag mypkg:dev
ghcitty -- lib:mylib -O0                 # cabal repl lib:mylib -O0
ghcitty --plain -- -package text         # ghci -package text
```

## Usage

```
ghcitty                           Interactive REPL
ghcitty eval "map (+1) [1,2,3]"   One-shot eval
ghcitty --json eval "1 + 1"       JSON output
ghcitty --session work            Named session
ghcitty --continue                Restore last session
```

## Tour

#### Auto-multiline with navigation
Multiline is auto-detected. Up/Down to move between lines, blank line to submit.

`:{` `:}` blocks also work as per the usual, with in-buffer navigation.

<img src="https://raw.githubusercontent.com/mattlianje/ghcitty/master/demos/auto-multiline-nav.gif" width="600">


#### Hoogle
`:hoogle` to search by name or type, `:doc` for Haddock docs.

<img src="https://raw.githubusercontent.com/mattlianje/ghcitty/master/demos/hoogle.gif" width="600">

#### Vi mode and $EDITOR
Vi keybindings. `Ctrl+G` opens `$EDITOR`, evals on save.

<img src="https://raw.githubusercontent.com/mattlianje/ghcitty/master/demos/vi-mode.gif" width="600">

#### Auto-reload
Edit a loaded file and ghcitty picks up the changes automatically without `:r`

<img src="https://raw.githubusercontent.com/mattlianje/ghcitty/master/demos/auto-reload.gif" width="600">

## Commands

All GHCi `:` commands pass through. Extras:
```
:hoogle <???>              Search Hoogle for <???>
:doc <???>                 Haddock docs for <???> via Hoogle
:/ OR :/<???>              Show all bindings OR fuzzy search for <???> binding
:e OR :edit OR <CTRL> + g  Open $EDITOR, eval on save
:scratch                   Open the persistent Scratch.hs in $EDITOR, :load on save (no args)
:undo <N>                  Undo last <N> expressions
:config                    List runtime config
:config_<key> [value]      Toggle a bool, or set a value (session-only)
```

## Config

`~/.ghcitty` (key=value) sets the persisted defaults:

```
pretty_errors = true
pretty_print = true 
max_output_lines = 50    # 0 disables
max_output_chars = 3000  # 0 disable
show_timing = false      # show eval timing (default: false)
```

Tweak any of these for the current session with `:config_<key>`. Bool keys
toggle when called with no argument:

```
:config_pretty_print
:config_max_output_lines 200
:config_max_output_lines 0
# Show all options...
:config
```

## FAQ

**Why a GHCi wrapper?**<br>
First and foremost, for personal use (and frivolous good fun ... to each his own). The precise goal was to have a smooth and aesthetic way to interact with Haskell programs.

It might also be of small use to happy hackers who still program.

**How does it work?**<br>
- Commands are fenced with sentinel markers so ghcitty knows exactly where your output begins and ends.
- For eval, it sends `:type expr` first to capture the type, then the expression itself.
- Definitions skip the type query and look up the bound name after.
- All GHCi `:` commands pass through. The one exception is `:set prompt`: ghcitty drives GHCi via a sentinel prompt, so it rejects prompt changes rather than deadlocking.
- `Ctrl+C` is forwarded to GHCi via the PTY so a long-running expression aborts instead of killing the REPL.

**How do the multiline heuristics work?**<br>
Basically "automatic" multiline uses a pretty simple and reliable heuristic...
- We enter multiline on trailing `=`, `do`, `where`, `etc`, unbalanced brackets, etc
- `Blank line` + `<RET>` submits your expression
- Bracketed paste is treated as if you used a `:{/:}` block

**How does completion work?**<br>
- `<TAB>` opens a columnar menu using `:complete repl` with full line context, so `:m + Data.Li<Tab>` completes module names.
- For short candidate lists, the type of each match shows alongside it.
- Ghcitty's own slash commands (`:scratch`, `:config_*`, `:edit`, `:undo`, `:doc`, `:hoogle`) appear in completions and ghost hints too.
- Ghost completions show the top match dimmed after 2+ chars.

**How does the hoogle integration work?**<br>
It tries the local `hoogle` CLI first, falls back to web API...

**How do sessions work?**<br>
Every eval is appended to `~/.local/share/ghcitty/<session>.hs`. `--continue` replays on startup.
