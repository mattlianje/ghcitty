# Changelog

## [0.3.0] - 2026-05-01

### Added
- `ghcitty -- <args>` forwards everything after `--` to `stack ghci` / `cabal repl` / `ghci`.
- `:edit FILE` / `:e FILE` opens FILE in `$EDITOR`, then `:reload`s.
- `:set editor X` overrides `$EDITOR`/`$VISUAL` for the session.
- Bracket matching: vim-style background tint on cursor bracket pairs.
- Ghost hint shows a trailing `…` when more than one completion matches the prefix.
- `Ctrl+Z` exits on an empty line, matching GHCi.

### Changed
- `:!cmd`, `:edit`, `:e`, and `:scratch` run with inherited stdio so vi/less/htop see a real TTY.
- `:scratch` rejects arguments with a hint to use `:load <file>` instead of falling through to GHCi.
- History flushed after every line so a panic or kill doesn't drop what was just typed.
- `:q` / `:quit` no longer saved to history (Up after launch won't replay an exit).

### Fixed
- `:hoogle "[a] -> [a]"` and `:hoogle [a] -> [a]` both work (matched quotes stripped, query URL-encoded)
   - This was quite annoying but expressions should work now
- `putStr " "` and other no-newline output no longer wedge the read loop.
- `putStrLn "   "` keeps its leading/trailing whitespace in the rendered value.
- Runtime exceptions (`*** Exception: ...`) render with their body in pretty mode instead of an empty error.
- Bracket-match tint cleared on submit, so the line frozen into scrollback is plain.

## [0.2.0] - 2026-04-26

### Added
- Auto-detect stack/cabal projects, launch via `stack ghci` / `cabal repl`. `--plain` forces bare ghci.
- `:scratch` opens a persistent `Scratch.hs`, `:load`s on save.
- Pretty-printer for `Show` output (records, lists, tuples)
- `:config` lists runtime config; `:config_<key>` toggles bools or sets vals
- Types shown inline in tab-completion suggestions.
- Ghcitty commands in completions and ghost hints
- Word-nav keys: Option+Arrow, Cmd+Arrow, Option+Backspace.

### Changed
- Passthrough output classified line-by-line so compiler chatter isn't highlighted as Haskell.
- GHC 9.6+ inline diagnostics render with message body on the first line.
- `:!shell` runs interactively, forwards stdin.

### Fixed
- `Ctrl+C` aborts the running expression instead of killing ghcitty.
- `:set prompt` rejected with a message instead of deadlocking.
- `make install` warns when a stale `ghcitty` shadows the new one on `PATH`.

## [0.1.0] - 2026-04-16

### Added
- Syntax highlighting
- Structured error display with expected/actual diffs, auto-import hints, and error code links
- Tab completion dropdowns via `:complete repl` with full line context
- Fish-style ghost completions after 2+ characters
- Hoogle integration (`:hoogle`, `:doc`) with local CLI fallback to web API
- Binding explorer (`:/`, `:/query`) for fuzzy searching bound names
- `$EDITOR` integration (`:edit`, `Ctrl+G`) — eval on save
- Auto-detected multiline with Up/Down navigation and blank-line submit
- Bracketed paste support treated as `:{/:}` blocks
- Auto-saved, resumable sessions (`--session`, `--continue`)
- JSON output mode (`--json`) for tooling
- Auto-reload on file changes — no manual `:r` needed
- Vi mode
- `:undo <N>` to roll back the last N expressions
- `~/.ghcitty` config file (`pretty_errors`, `show_timing`)
- Nix flake support
