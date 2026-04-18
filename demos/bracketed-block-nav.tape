Output demos/bracketed-block-nav.gif

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

# :{ :} block with navigation
Type ":{"
Enter
Sleep 300ms
Type "let fib 0 = 0"
Enter
Sleep 300ms
Type "    fib 1 = 1"
Enter
Sleep 300ms
Type "    fib n = fib (n-1) + fib (n-2)"
Sleep 400ms

# Navigate back up through the block
Up
Sleep 300ms
Up
Sleep 400ms
# Review the lines
Down
Sleep 300ms
Down
Sleep 400ms
# Make sure we're at end of line before continuing
Ctrl+e

Enter
Sleep 300ms
Type ":}"
Enter
Sleep 1.5s

Type "map fib [0..10]"
Sleep 400ms
Enter
Sleep 1.5s

Sleep 1s
