Output demos/completions-multiline.gif

Set Shell zsh
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms
Set Width 1100
Set Height 600

Type "ghcitty"
Enter
Sleep 1.5s

# 1. Tab completion — big list, scroll around, narrow to Data.Char
Type "import Data."
Sleep 400ms
Tab
Sleep 800ms
Down
Down
Down
Sleep 400ms
Up
Up
Up
Sleep 400ms
Type "Ch"
Sleep 500ms
Tab
Sleep 600ms
Enter
Sleep 400ms

# 2. Ghost hint on module name
Type "import Data.Li"
Sleep 900ms
Type "st"
Sleep 300ms
Enter
Sleep 400ms

# 3. Multiline — ghost hints on intercalate and toUpper
Type "let shout xs ="
Enter
Sleep 300ms

Type "      inter"
Sleep 900ms
Type "calate"
Sleep 200ms
Type ' ", " $'
Enter
Sleep 200ms

Type "        filter (not . null) $"
Enter
Sleep 200ms

Type "          map (map toUp"
Sleep 800ms
Type "per) xs"
Enter
Sleep 300ms

# Navigate up into the buffer and tweak the intercalate separator
Up
Sleep 300ms
Up
Sleep 300ms
Up
Sleep 500ms
Ctrl+e
Sleep 200ms
Backspace 6
Sleep 300ms
Type '" | " $'
Sleep 400ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Enter
Sleep 1.5s

# 4. Empty string in the list — filter (not . null) earns its keep
Type 'shout ["Arcole", "", "Austerlitz"]'
Enter
Sleep 1.5s

Sleep 500ms
