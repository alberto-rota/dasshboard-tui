# `m` grabs a tile and the arrows then move it -- off the end of a group and it
# carries into the next. `S` is the groups themselves.

Output "docs/groups.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 600ms

Right
Sleep 600ms
Type "m"
Sleep 1200ms
Right
Sleep 700ms
Right
Sleep 900ms
# off the end of `work` and into `home`
Right
Sleep 1200ms
Right
Sleep 1200ms
Left
Sleep 1000ms
Enter
Sleep 1500ms

# the groups, and a new empty one saying what to do about it
Type "S"
Sleep 1500ms
Down
Sleep 600ms
Down
Sleep 600ms
Enter
Sleep 800ms
Type "lab"
Sleep 800ms
Enter
Sleep 1500ms
Escape
Sleep 1800ms

# move a tile into it
Type "G"
Sleep 600ms
Type "m"
Sleep 1000ms
Right
Sleep 1500ms
Enter
Sleep 2000ms
Type "q"
Sleep 800ms
