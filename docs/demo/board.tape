# The board itself: look around, then open a tile in this terminal and come
# back out of it into the shell that launched the home screen.

Output "docs/board.gif"
Source "docs/demo/_setup.tape"

Type "dasshboard" Enter
Sleep 2s

Right
Sleep 600ms
Right
Sleep 600ms
Type "j"
Sleep 900ms
Type "G"
Sleep 900ms
Type "g"
Sleep 900ms

# down to the local tile and open it: no Ghostty here, so the session takes
# over this terminal.
Type "G"
Sleep 400ms
Left
Left
Left
Sleep 1s
Enter
Sleep 2s
Type "ls"
Sleep 300ms
Enter
Sleep 1500ms

# quitting the session lands back in the shell dasshboard was started from
Type "exit"
Sleep 300ms
Enter
Sleep 2s
