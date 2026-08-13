# dasshboard

[![PyPI](https://img.shields.io/badge/pypi-dasshboard-informational)](https://pypi.org/project/dasshboard/)

A home screen for your terminal. Every host in `~/.ssh/config` — plus anything you
add yourself — becomes a tile. Pick one and it connects.

![The board: moving between tiles, then opening one in this terminal](docs/board.gif)

## Install

```sh
uv tool install dasshboard      # or: pipx install dasshboard
dasshboard                      # try it
dasshboard --startup on         # ...then, if you want it with every terminal
```

Installing touches nothing but your PATH. `--startup on` is the only thing that
edits a shell rc, and `--startup off` undoes it exactly.

## Keys

| | |
|---|---|
| click / `⏎` / `space` | open the tile |
| `1`–`9` | open that numbered tile |
| `t` / `w` / `c` | open in a new tab / new window / this terminal, once |
| arrows / `hjkl` | move around |
| `g` / `G` | first / last |
| `/` | find by name, target, or jump host |
| `a` `e` `d` | add / edit / duplicate a tile |
| `D` | delete it (asks first) |
| `x` / `X` | hide it / show every hidden tile |
| `m` | grab it, arrows move it, `⏎` drops it |
| `s` / `S` | settings / groups |
| `r` | re-read both config files |
| `q` / `esc` | quit |

Everything is editable in the TUI — there is no `$EDITOR` handoff. `~/.ssh/config`
is never written to: editing one of its hosts writes an override block in
`config.toml` that merges on top of it.

## Config

`~/.config/dasshboard/config.toml` (`%APPDATA%\dasshboard\` on Windows), created
with a commented template on first run. `dasshboard --config` prints the path.

```toml
[options]
include_ssh_config = true   # false shows only the hosts defined here
show_hidden = false         # open with hidden tiles on screen; X toggles them
open_in = "tab"             # "tab", "window" or "current"

[theme]
primary = "#aaaaaa"
accent = "#ff0000"

[[local]]                   # a command on this machine
label = "MACBOOK-PRO"
command = "/bin/zsh"        # optional: your login shell by default
folder = "~/dasshboard-tui" # optional: home by default

[[host]]
name = "myserver"
hostname = "10.0.0.5"
user = "albe"
jump = "bastion"
folder = "/srv/app"         # optional: the far side's home by default
command = "tmux attach"     # optional: a login shell by default
color = "#4f8ab0"           # optional: auto-assigned by default
hidden = false              # true keeps it off the board until X

[[section]]                 # a group heading, drawn in this order
title = "work"
items = ["myserver", "bastion"]
```

Edits rewrite whole blocks as text, so your comments and formatting survive. A
TOML error is reported in the status line and leaves the board usable.

## Compatibility

The TUI runs anywhere — macOS, Linux, BSD, Windows Terminal. Opening a session in
**this terminal** works everywhere too.

Opening a **new tab or window** needs a terminal with an automation interface,
which today means Ghostty ≥ 1.2 on macOS; there the new surface also gets the
host's colour as a tint and its circle in the tab title. Everywhere else `tab` and
`window` resolve to `current` — they still parse and still travel in a shared
config, so nothing breaks. Override the guess with
`DASSHBOARD_BACKEND=ghostty|inplace`.

## Development

```sh
cargo build --release        # ~/.local/bin/dasshboard symlinks to the output
cargo test                   # parsing, escaping, colour assignment, layout
./target/release/dasshboard --list      # tiles and colours, no TUI
./target/release/dasshboard --open alex # connect to one host, no TUI
./docs/demo/record.sh                   # regenerate the gifs (needs vhs)
```

`src/platform.rs` holds the OS differences; `src/ghostty.rs` is the only module
that knows Ghostty exists. Layout is checked headlessly with ratatui's
`TestBackend`.

