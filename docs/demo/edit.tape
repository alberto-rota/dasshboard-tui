# `e` on a host that only exists in ~/.ssh/config: every blank field shows what
# it inherits, and only what you fill in is written -- into config.toml, never
# into ssh's own file.

Output "docs/edit.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 600ms

Right Right Right
Sleep 800ms
Type "e"
Sleep 2500ms

# the name is the key joining the two files, so focus skips it
Tab
Sleep 500ms
Tab
Sleep 500ms
Tab
Sleep 500ms
Tab
Sleep 500ms
Type "/srv/build"
Sleep 800ms
Tab Tab
Sleep 600ms
Right
Sleep 500ms
Right
Sleep 500ms
Right
Sleep 1200ms
Enter
Sleep 2500ms
Type "q"
Sleep 800ms
