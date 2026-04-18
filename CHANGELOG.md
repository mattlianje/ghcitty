# Changelog

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
