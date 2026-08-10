# dasshboard

[![PyPI](https://img.shields.io/badge/pypi-dasshboard-informational)](https://pypi.org/project/dasshboard/)

A home screen for Ghostty. Every host in `~/.ssh/config` — plus anything you add
yourself — becomes a clickable tile. Activating one opens a new Ghostty tab
connected to it, tinted with that host's colour.

The whole block is centred in both axes, and tiles keep a fixed width — a wide
terminal gets whitespace, not vast tiles.

```
   ◆  DASSHBOARD                                                5 ssh · 1 local
   ━━━━━━━━━━━━━━──────────────────────────────────────────────────────────────

   ┏ 1 ━━━━━━━━━━━━━━━━━━━━━━━━ local ┓  ╭ 2 ───────────────────────────────╮
   ┃▌ ● MACBOOK-PRO                   ┃  │  ● alex                          │
   ┃    local shell                   ┃  │    v120bb18@alex.nhr.fau.de ⤳    │
   ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛  ╰──────────────────────────────────╯

   ⏎ open   t/w/c tab·win·here   / find   a add   e edit   x hide   s settings
```

Two kinds of tile:

- **ssh** — opens a **new Ghostty tab** and connects.
- **local** (tagged `local`) — **takes over this tab**. termhome restores the
  terminal and then `exec`s the command, so the process is replaced rather than
  nested; quitting it returns you to the shell that started termhome.

## Colour

The chrome is exactly two colours — a **primary** and an **accent**, `#aaaaaa`
and `#ff0000` out of the box — and the discipline is that **the accent is never
decoration**. It marks the selection, the brand diamond, and errors; nothing
else. Three tile states read as three border weights rather than three hues:
thick accent when selected, primary when hovered, hairline when idle.

Both are yours to change, in `s` or in `[theme]`. Everything else is *derived*
from the pair rather than listed separately, so a new primary restyles the whole
UI coherently instead of leaving four hand-picked greys behind:

| shade | from | used for |
|---|---|---|
| bright | primary → white, 73% | the one word that should stand out |
| muted | primary × 0.62 | secondary text |
| faint | primary × 0.34 | hairlines and idle borders |
| accent-dim | accent mixed 25% toward primary, × 0.70 | tags and hover |

The ratios were reverse-engineered from the original hand-picked palette, so the
defaults come out byte-identical to what they were before this became runtime —
`the_default_theme_reproduces_the_original_shades` pins that. In the settings
panel the arrows cycle presets and typing takes a hex; a colour is written the
moment it parses, so the UI restyles as you type and never flickers through a
half-finished value.

Hosts carry a **third** colour that is theirs alone, shown as the dot on the
tile. That palette deliberately excludes red — a host that owned red would be
indistinguishable from the cursor — and its eight entries are desaturated to sit
against `#aaaaaa` without shouting over it.

A name hashes to a preferred slot, and a taken slot probes forward to the next
free one, so no two tiles on screen share a colour. The trade-off is that a
host's colour depends on what else is on screen: adding a host can shift a
later one. Pin any host with `color = "#rrggbb"` and it never moves.

**The palette is sized to the emoji, not the other way round.** A Ghostty tab
title is text, so a coloured circle is the only way a host's colour can reach
the tab bar itself — the background tint colours the surface, not the strip you
actually scan. There are nine circle emoji and red is spoken for, which leaves
eight, one per palette slot. A richer palette would give two hosts the same
circle and defeat the point. A hand-written hex gets the nearest circle by RGB
distance, so `color = "#2255cc"` still shows 🔵.

One caveat: ⚫ is low-contrast on a dark tab bar. If a host lands on it and you'd
rather it didn't, pin that host to another colour.

## Keys

| | |
|---|---|
| click / `⏎` / `space` | open the tile |
| `1`–`9` | open that numbered tile directly |
| arrows / `hjkl` | move (`j`/`k` move by a row, so they respect the grid) |
| `g` / `G` | first / last |
| `/` | find by name, target, or jump host; `esc` clears |
| `a` | add a host |
| `e` | edit the selected host — including ones from `~/.ssh/config` |
| `x` | hide the selected host (or unhide it, with `show_hidden` on) |
| `d` | delete a host, or revert an `~/.ssh/config` one (asks first) |
| `t` / `w` / `c` | open in a new tab / new window / this surface, once |
| `s` | settings — options, destination and both colours |
| `r` | re-read both config files |
| `q` / `esc` | quit into the shell underneath |

## Editing, in the TUI

Everything is editable without leaving the terminal UI — there is no `$EDITOR`
handoff. `a` and `e` open the same form; only `name` is required, and the rest
falls back to ssh's own resolution. On the `color` row, `←`/`→` cycle `auto` and
the eight presets, or type a hex; the row previews the swatch and the exact
circle the tab will carry. `s` opens settings and writes each toggle through
immediately, so the file and the screen can never disagree.

### Customising a host from `~/.ssh/config`

`~/.ssh/config` is **never written to** — it's ssh's file, and rewriting it
risks the options termhome doesn't model. Instead, editing one of its hosts
creates a `[[host]]` block of the same **name** in `config.toml`, which merges
on top of it:

```
   name     alex  from ~/.ssh/config
 ▌ hostname ▌  alex.nhr.fau.de  ~/.ssh/config
   user       v120bb18  ~/.ssh/config
   port       22
   jump       csnhr  ~/.ssh/config
   color    ● auto   🟤 tab
```

The name is the key joining the two files, so it's shown but locked, and focus
skips it. Every blank field displays what it inherits and keeps tracking the ssh
config; only what you actually fill in is written. So a colour override is just:

```toml
[[host]]
name = "alex"
color = "#c9a227"
```

That still launches as plain `ssh alex`. When you *do* override a connection
field it rides in as `-o HostName=…` **in front of the alias**, which takes
precedence over the config file without replacing the lookup — passing
`user@host` instead would silently drop the alias's `IdentityFile`, `ForwardX11`
and everything else.

An override customises the existing tile rather than adding a second one, and
`d` on such a host offers to **revert** it: the block goes, the tile stays.

### Hiding a host

Not every `Host` in `~/.ssh/config` is somewhere you open a shell. A bastion
that only exists to be jumped through is still a host ssh needs and a tile you
never want. `x` hides the selected one:

```toml
[[host]]
name = "csnhr"
hidden = true
```

That is the whole block — hiding pins nothing else, so the host keeps tracking
`~/.ssh/config` and keeps working as a `ProxyJump` target for everything that
routes through it. Only the tile goes.

To get one back, turn on `show_hidden` in `s`: hidden hosts reappear dimmed and
tagged `hidden`, and `x` puts them back. The form has the same toggle, so
editing a hidden host can't silently unhide it.

`--list` always prints hidden hosts, marked, since it exists to show you what is
actually configured.

Everything lands in `~/.config/dasshboard/config.toml`, created with a
commented template on first run:

```toml
[options]
include_ssh_config = true   # false shows only the hosts defined here
tint_tabs = true            # tint the new tab's background
tab_emoji = true            # coloured circle in the tab title
show_hidden = false         # reveal hidden hosts, to unhide them
open_in = "tab"             # "tab", "window" or "current"

[theme]
primary = "#aaaaaa"
accent = "#ff0000"

[[local]]                   # a command on this machine
label = "MACBOOK-PRO"
detail = "local shell"
command = "/bin/zsh"

[[host]]
name = "myserver"
hostname = "10.0.0.5"
user = "albe"
port = 22
jump = "bastion"
color = "#4f8ab0"
hidden = false
open_in = "window"          # overrides the global for this host
```

Edits rewrite whole blocks **as text** rather than re-serialising the document,
so your comments and formatting survive: `add_then_edit_then_delete_leaves_the_file_as_it_started`
asserts the file comes back byte-identical, and editing a host keeps it in
position rather than moving it to the end. `a`/`e`/`d` only ever touch
`[[host]]` blocks in this file. Local tiles are the one thing still edited by
hand; `e` on one says so rather than failing quietly.

A TOML syntax error is reported in the status line and leaves the previous
screen usable; a typo must never cost you the home screen.

## How tabs are opened

Ghostty ≥ 1.2 ships an AppleScript dictionary, so no key-injection hacks:

```applescript
tell application "Ghostty"
  set cfg to {command:"/bin/sh -c '...'", wait after command:true}
  new tab in front window with configuration cfg
end tell
```

Three things worth knowing, all found by probing the running app:

- `new tab` fails with `-1708` unless the *optional* `in <window>` parameter is
  passed explicitly. `src/ghostty.rs` always passes `front window`.
- There is no `GHOSTTY_SURFACE_ID` in the environment, so a surface can't
  identify its own window. `front window` is correct anyway — we only spawn in
  response to input, which means we're focused.
- The dictionary has **no tab-colour property**, so the colour reaches the tab
  two ways. The title gets the host's circle emoji (`tab_emoji`), which is the
  reliable one. The surface also emits OSC 11 to set its background (scaled to
  13%, or text would be unreadable) and OSC 12 for the cursor at full strength
  (`tint_tabs`); whether Ghostty's tab bar picks that up is unverified here.

The spawned command is `sh -c 'printf <title>; <tint>; exec ssh <args>'`. Each
argv element is quoted separately, so a value containing a space stays one
argument to ssh. `exec` hands the process over so the tab dies with the
connection, and `wait after command:true` keeps a failed connection on screen
instead of flashing the tab shut.

## Config parsing

`src/ssh.rs` handles `Host` (multiple aliases per line), `HostName`, `User`,
`Port`, `ProxyJump`, `Include` (globs, `~`, paths relative to `~/.ssh`), and
`Key=value` as well as `Key value`. Wildcard patterns (`Host *`) are skipped
since they configure other hosts rather than naming one, and `Match` blocks end
the current host so their conditional keys aren't misattributed.

Hosts from `~/.ssh/config` are launched by **alias alone**, so ssh re-reads the
same file and every option termhome doesn't model — `IdentityFile`,
`ForwardX11`, `IdentitiesOnly` — still applies. Overrides preserve that by
riding in as `-o Key=Value` before the alias rather than replacing it.

## Launching

`~/.zshrc` has two marked blocks. See the comments there; the short version is
that the dotfiles' `hsl-login.sh` already claimed the terminal-open slot and
blocks until you quit herdr, so part 1 sets `NO_HSL=1` *before* that source and
part 2 runs termhome *after* it (where PATH is complete). herdr is
un-autostarted, not disabled: type `hsl`, or press its tile.

The guards mirror `hsl-login.sh`'s. The `HERDR_*` one is load-bearing: every
herdr pane sources this rc, so without it each new pane would open a termhome
inside itself.

**Escape hatches** — `DASSHBOARD_SKIP=1 zsh -l` for a bare shell, `homescreen` to
bring the UI back up, or delete the two marked blocks (`~/.zshrc.bak.termhome`
is the pre-install copy; removing them restores herdr autostart).

## Install

```sh
uv tool install dasshboard      # or: pipx install dasshboard
```

The wheel is a Rust binary with a console-script entry point — there is no
Python in it. maturin is the build backend and `uv build` drives it:

```sh
uv build          # -> dist/*.whl and dist/*.tar.gz
uv publish        # needs a PyPI token
```

The wheel is tagged `macosx_11_0_arm64`, because the whole thing talks to
Ghostty over AppleScript. On any other platform pip will fall back to the sdist
and build from source, which works if you have a Rust toolchain but still needs
macOS and Ghostty to be useful.

## Development

```sh
cargo build --release        # ~/.local/bin/dasshboard symlinks to the output
cargo test                   # parsing, escaping, colour assignment, layout
cargo test layout_dump -- --nocapture   # eyeball every screen at four sizes
./target/release/dasshboard --list      # tiles, colours and circles, no TUI
./target/release/dasshboard --open alex # spawn one tab, no TUI
./target/release/dasshboard --config    # print the config path
```

The binary is symlinked rather than copied, so `cargo build --release` takes
effect immediately. Don't `cargo clean` without rebuilding.

Layout is verified headlessly with ratatui's `TestBackend`, which is how the
grid gets checked without a terminal: `every_tile_is_clickable_and_none_overlap`
asserts the hitboxes cover the visible list and never overlap,
`tiles_never_escape_the_viewport` runs the same check down to 30×10, and
`content_is_centred` measures the whitespace on all four sides.

The config writers take their target path as a parameter, so the round-trip
tests run against a scratch file and never touch your real config.
