# A tile whose command is not something you sit in. `cat today.md` prints three
# lines and exits -- and the session it opened stays, as a shell in that folder.

Output "docs/command.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 600ms

# the border says where it goes; what it runs is in the config, not on the tile
Type "G"
Sleep 500ms
Left
Sleep 400ms
Left
Sleep 2s
Enter
Sleep 2500ms

# the command has already run and exited -- this is the shell it left behind
Type "ls"
Sleep 300ms
Enter
Sleep 1500ms
Type "exit"
Sleep 300ms
Enter
Sleep 1800ms
