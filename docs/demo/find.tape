# `/` filters by name, target or jump host; esc puts the board back.

Output "docs/find.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 800ms

Type "/"
Sleep 600ms
Type "orion"
Sleep 1800ms
Backspace 5
Sleep 600ms
# not a name -- the target matches too
Type "lan"
Sleep 1800ms
Backspace 3
Sleep 400ms
Type "gateway"
Sleep 1800ms
Escape
Sleep 1500ms
Type "q"
Sleep 800ms
