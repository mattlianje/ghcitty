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
- Tab completion dropdowns
- Fish-style ghost completions
- Hoogle integration
- Binding explorer
- `:edit` opens `$EDITOR`, evals on save
- Auto-detected multiline
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

`ghcitty` wraps `ghci`, so run it through your build tool to pick up project dependencies
(same as `cabal repl`)

```
cabal exec -- ghcitty
stack exec -- ghcitty
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
:undo <N>                  Undo last <N> expressions 
```

## Config

`~/.ghcitty` (key=value):

```
pretty_errors = true   # structured error display (default: true)
show_timing = true     # show eval timing (default: false)
```

## FAQ

**Why a GHCi wrapper?**<br>
First and foremost, for personal use (and frivolous good fun ... to each his own). The precise goal was to have a smooth and aesthetic way to interact with Haskell programs.

It might also be of small use to happy hackers who still program.

**How does it work?**<br>
- Commands are fenced with sentinel markers so ghcitty knows exactly where your output begins and ends.
- For eval, it sends `:type expr` first to capture the type, then the expression itself. 
- Definitions skip the type query and look up the bound name after. 
- All GHCi `:` commands pass through untouched.

**How do the multiline heuristics work?**<br>
Basically "automatic" multiline uses a pretty simple and reliable heuristic...
- We enter multiline on trailing `=`, `do`, `where`, `etc`, unbalanced brackets, etc
- `Blank line` + `<RET>` submits your expression
- Bracketed paste is treated as if you used a `:{/:}` block

**How does completion work?**<br>
- `<TAB>` opens a columnar menu using `:complete repl` with full line context, so `:m + Data.Li<Tab>` completes module names.
- Ghost completions show the top match dimmed after 2+ chars.

**How does the hoogle integration work?**<br>
It tries the local `hoogle` CLI first, falls back to web API...

**How do sessions work?**<br>
Every eval is appended to `~/.local/share/ghcitty/<session>.hs`. `--continue` replays on startup.
