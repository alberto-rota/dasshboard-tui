# `a` adds a tile, `y` duplicates one so the copy can start somewhere else.

Output "docs/add.gif"
Source "docs/demo/_setup.tape"

Hide
Type "dasshboard" Enter
Sleep 2s
Show
Sleep 800ms

Type "a"
Sleep 1s
# kind row: ssh host is already picked, so tab straight into the fields
Tab
Sleep 400ms
Type "vault"
Sleep 500ms
Tab
Type "vault.eu-west.example.net"
Sleep 500ms
Tab
Type "svc"
Sleep 500ms
# port, jump: left to what ssh resolves
Tab Tab Tab
Sleep 400ms
Type "/srv/vault"
Sleep 600ms
# past command to the colour row, and pick one
Tab Tab
Sleep 500ms
Right
Sleep 400ms
Right
Sleep 400ms
Right
Sleep 900ms
Enter
Sleep 2s

# the copy is another tile for the same host, with its own folder
Type "y"
Sleep 1800ms
Type "e"
Sleep 1s
# our own block, so the name is editable and focus starts on it
Tab Tab Tab Tab Tab
Sleep 400ms
Backspace 10
Type "/srv/vault/backups"
Sleep 800ms
Enter
Sleep 2s
Type "q"
Sleep 800ms
