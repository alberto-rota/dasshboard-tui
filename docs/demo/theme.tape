# `s` writes each row through as you change it: the board is restyled from a
# primary and an accent, so one colour changes the whole UI coherently.

Output "docs/theme.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 600ms

Type "s"
Sleep 1800ms

# what the board is built from: drop ~/.ssh/config and only our own tiles remain
Down
Sleep 800ms
Space
Sleep 1800ms
Space
Sleep 1500ms

# the two colours everything else is derived from
Down Down Down Down
Sleep 800ms
Right
Sleep 900ms
Right
Sleep 900ms
Right
Sleep 1200ms
Down
Sleep 800ms
Right
Sleep 900ms
Right
Sleep 1200ms
# or type a hex: it is written the moment it parses
Backspace 7
Sleep 500ms
Type "5ac8fa"
Sleep 2000ms
Escape
Sleep 2000ms
Type "q"
Sleep 800ms
