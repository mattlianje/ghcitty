Output demos/auto-multiline-nav.gif

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

# Auto-multiline fib with navigation
Type "let fib n ="
Sleep 300ms
Enter
Sleep 400ms
Type "      case n of"
Sleep 300ms
Enter
Sleep 400ms
Type "        0 -> 0"
Sleep 300ms
Enter
Sleep 400ms
Type "        1 -> 1"
Sleep 300ms
Enter
Sleep 400ms
Type "        _ -> fib (n-1) + fib (n-2)"
Sleep 300ms
Enter
Sleep 600ms

# Navigate back up through the block
Up
Sleep 300ms
Up
Sleep 300ms
Up
Sleep 300ms
Up
Sleep 500ms
# We're on "case n of" — looks good, go back down
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms

# Blank line to submit
Ctrl+e
Sleep 200ms
Enter
Sleep 1.5s

Type "map fib [0..10]"
Sleep 400ms
Enter
Sleep 1.5s

Sleep 1s
