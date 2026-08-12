# Shared by every tape: the terminal the recordings are made in.
# 1200x620 at font 15 is ~119x32 cells: three tiles wide, and tall enough for the
# whole board, a panel over it, and the footer on the bottom edge.

Set Shell "zsh"
Set FontSize 15
Set Width 1200
Set Height 620
Set Padding 20
Set Theme "TokyoNight"
Set Framerate 24
Set TypingSpeed 70ms

Hide
Type "cd ~ && clear" Enter
Sleep 500ms
Show
