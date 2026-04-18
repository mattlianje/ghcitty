Output demos/vi-mode.gif

Set Shell zsh
Set Theme "Catppuccin Mocha"
Set TypingSpeed 70ms
Set Width 1100
Set Height 600

Hide
Type "ghcitty"
Enter
Sleep 3s
Ctrl+l
Sleep 500ms
Show

# Start typing something
Type "map (*2) [1..5]"
Sleep 400ms
# Escape to normal mode — show cursor movement
Escape
Sleep 500ms
Type "h"
Sleep 200ms
Type "h"
Sleep 200ms
Type "h"
Sleep 200ms
Type "h"
Sleep 400ms
# Back to insert and submit
Type "A"
Sleep 300ms
Enter
Sleep 1s

# Ctrl+G opens $EDITOR
Ctrl+g
Sleep 1.5s
# Type an expression in vim
Type "imap (*3) [1..10]"
Sleep 500ms
# Save and quit
Escape
Sleep 300ms
Type ":wq"
Enter
Sleep 1s
Enter
Sleep 1.5s

Sleep 1s
