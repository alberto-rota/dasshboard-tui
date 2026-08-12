# `x` puts a tile away, `X` shows every hidden one, `d` deletes -- and says what
# deleting a host from ~/.ssh/config actually means.

Output "docs/hide.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 600ms

Right Right
Sleep 800ms
Type "x"
Sleep 1800ms
# revealing is a look, not a setting: nothing is written
Type "X"
Sleep 2000ms
Type "X"
Sleep 1500ms

# deleting asks first, and says what it will write
Type "G"
Sleep 800ms
Type "d"
Sleep 2500ms
Type "y"
Sleep 2500ms
Type "q"
Sleep 800ms
