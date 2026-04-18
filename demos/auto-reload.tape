Output demos/auto-reload.gif

Set Shell zsh
Set Theme "Catppuccin Mocha"
Set TypingSpeed 5ms
Set Width 1800
Set Height 800

Hide

# Clean up any leftover session or swap files from a previous run
Type "tmux kill-session -t demo 2>/dev/null; rm -f /tmp/Greet.hs /tmp/.Greet.hs.swp"
Enter
Sleep 200ms

# Create Greet.hs v1 before tmux starts
Type "cat > /tmp/Greet.hs << 'HSEOF'"
Enter
Type "module Greet where"
Enter
Enter
Type "greet :: String -> String"
Enter
Type 'greet name = "Hello, " ++ name ++ "!"'
Enter
Type "HSEOF"
Enter
Sleep 200ms

# Boot tmux with a named session so we can kill it cleanly at the end
Type "tmux new-session -s demo"
Enter
Sleep 300ms

# Start ghcitty in the left pane
Type "ghcitty"
Enter
Sleep 3s

# Vertical split — new right pane gets focus
Ctrl+b
Sleep 50ms
Type "%"
Sleep 300ms

# Go back to left pane and clear ghcitty's startup banner
Ctrl+b
Sleep 50ms
Left
Sleep 100ms
Ctrl+l
Sleep 300ms

# Right pane: open vim so the buffer is already visible when recording starts
Ctrl+b
Sleep 50ms
Right
Sleep 200ms
Type "vim /tmp/Greet.hs"
Enter
Sleep 1s

Set TypingSpeed 70ms
Show

# Jump to left pane (ghcitty) to load and test
Ctrl+b
Sleep 50ms
Left
Sleep 200ms

Type ":load /tmp/Greet.hs"
Enter
Sleep 1.5s

Type 'greet "Haskell"'
Enter
Sleep 2s

# Switch to right pane (vim is still open) to make the edit
Ctrl+b
Sleep 50ms
Right
Sleep 200ms

# Search for "Hello" and change the word
Type "/Hello"
Enter
Sleep 500ms
Type "cw"
Sleep 100ms
Type "Bonjour"
Escape
Sleep 300ms

# Save — vim writes the file, updating its mtime
Type ":w"
Enter
Sleep 1s

# Back to ghcitty — check_reload fires before the next prompt
Ctrl+b
Sleep 50ms
Left
Sleep 200ms

Enter
Sleep 800ms

Type 'greet "Haskell"'
Enter
Sleep 2s

Sleep 1s

Hide
Type "tmux kill-session -t demo"
Enter
Sleep 200ms
