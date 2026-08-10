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
| `s` | settings — whether it opens with a terminal, options, destination and both colours |
| in the picker | `1`-`9` jump, `⏎` open, `esc` cancel |
| `r` | re-read both config files |
| `q` / `esc` | quit into the shell underneath |

## Folders

A tile can carry directories to start in. Give a host more than zero and
activating it asks which:

```
┏ open alex in ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃                                                ┃
┃ ▌ 1 home              no cd                    ┃
┃   2 atlas             /scratch/atlas           ┃
┃   3 thesis            /home/v120bb18/thesis    ┃
┃                                                ┃
┃   ↑↓ move   1-9 jump   ⏎ open   esc cancel     ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
```

`home` is always first, so a host with folders is never *forced* into one. A
one-off destination survives the picker: `w` then `2` opens that folder in a new
window.

```toml
[[host]]
name = "alex"
folders = ["/scratch/atlas", "/home/v120bb18/thesis"]

[[local]]
label = "MACBOOK-PRO"
command = "/bin/zsh"
folders = ["~/dasshboard-tui", "~/dotfiles"]
```

The two kinds get there differently, because one directory is on this machine
and the other isn't:

- **local** — a real `cd` before the command runs, so argv stays exactly what
  you configured.
- **ssh** — the directory is on the far side, so it becomes a remote command:
  `ssh <alias> -t 'cd <dir> && exec ${SHELL:-/bin/sh} -l'`. The `-t` forces a
  tty, without which the remote shell isn't interactive; the `exec $SHELL -l`
  leaves you in a login shell in that folder rather than a bare `sh`. The alias
  still leads, so `IdentityFile` and friends keep applying.

Paths are quoted for the shell that will read them, local or remote — and `~`
is handled on whichever side owns the home directory it means. A **local** one
is resolved here, because the destination is either a real `chdir` (which does
no expansion at all) or a quoted `cd` (which stops it), so `~/Desktop` would
otherwise be looked up as a directory *named* `~`. A **remote** one can't be
resolved here at all, so the tilde is left outside the quotes for the far side
to expand — `cd ~/'my dir'` — while the rest of the path stays one quoted word.
Only a leading `~` or `~/…`; `~user` needs a passwd lookup, and mid-path tildes
are literal in a shell too.

## Editing, in the TUI

Everything is editable without leaving the terminal UI — there is no `$EDITOR`
handoff. `a` and `e` open the same form, whose first row picks what you are
describing: an **ssh host** or a **local command**. The field set changes with
it, since the two share almost nothing past the name. Only the name is required
(plus `command` for a local); the rest falls back to ssh's own resolution. On the `color` row, `←`/`→` cycle `auto` and
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
position rather than moving it to the end. `a`/`e`/`d` work on `[[host]]` and
`[[local]]` blocks alike, keyed on `name` and `label` respectively — so a host
and a local tile may share a name without colliding.

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

## Launching, opt in

**Installing the package does not touch your shell.** It puts a binary on PATH
and nothing else; typing `dasshboard` is the whole of it. Opening with a
terminal is a second, explicit act, because that slot is usually already taken —
here by the dotfiles' `hsl-login.sh`, which starts herdr — and two full-screen
TUIs can't both own a new surface.

```sh
dasshboard --startup        # is it hooked? which file?
dasshboard --startup on     # hook it
dasshboard --startup off    # unhook it, exactly
dasshboard --startup print  # the hook as text, for a shell it can't write
```

The first row of `s` is the same switch, and says which file it will edit.

`on` writes two marked blocks into `~/.zshrc` (or `~/.bashrc`), keeping one
pre-install copy at `~/.zshrc.bak.dasshboard`. It is idempotent, so it doubles
as the upgrade path for an older hook, and `off` removes exactly what it added —
your rc comes back byte-identical, which is what hands the terminal-open slot
back to whatever had it.

The split into two blocks is forced by *when* each half must run.
`hsl-login.sh` is sourced as the last act of the dotfiles' rc and **blocks**
until you quit herdr, so anything after it never reaches the screen: part 1 goes
above, part 2 (the UI, where PATH is finally complete) goes at the very bottom.

Both halves ask the same shell function, and that is the load-bearing part:

- **`NO_HSL=1` is conditional.** Part 1 sets `hsl-login.sh`'s own escape hatch
  only in a shell dasshboard is actually going to draw in. Every shell it
  declines — inside tmux, a herdr pane, an agent, not Ghostty — gets herdr
  exactly as if dasshboard were not installed. An unconditional `export` here
  suppressed the workspace manager machine-wide, which is the bug this shape
  fixes.
- **and unexported.** `hsl-login.sh` is *sourced* by the same shell, so a plain
  variable reaches it. An exported one would be inherited by the shell a local
  tile execs — the one place you most want herdr — and silently suppress it
  there.
- **the binary is found by path, not by PATH.** Part 1 runs *above* the rc that
  puts `~/.local/bin` on PATH, so `command -v dasshboard` there answers "not
  installed" — and a guard that believed it handed the slot straight back to
  herdr, so a hooked shell came up in the workspace manager and part 2 never
  ran. `--startup on` bakes in the directory it was enabled from, a short list
  of the usual places follows, and PATH is consulted last, where part 2 has a
  complete one anyway. Part 2 then launches exactly what the guard found.
- **`DASSHBOARD_SKIP` is exported**, both by part 2 and by dasshboard itself
  around a handoff, so a shell below a home screen never opens a second one
  inside itself. That same flag is what makes the first point work: the local
  tile lands in a normal login shell, and *that* is where herdr starts.

The `HERDR_*` guard is load-bearing for the same family of reasons: every herdr
pane sources this rc, so without it each new pane would open a dasshboard inside
itself.

**Escape hatches** — `DASSHBOARD_SKIP=1 zsh -l` for a shell with no home screen,
`NO_HSL=1` alongside it for one with no workspace manager either, `homescreen` to
bring the UI back up, and `dasshboard --startup off` to undo the whole thing.

### The local tile

Once the home screen owns the terminal-open slot, the local tile is how you
reach the thing that used to open with a terminal — so a first run points it at
`hsl`, or plain `herdr`, whichever is installed, and only falls back to
`$SHELL` on a machine with neither. Either way it works: a tile running a bare
shell reads the same rc, whose guard now declines, so the dotfiles start herdr
in it.

## Install

```sh
uv tool install dasshboard      # or: pipx install dasshboard
dasshboard                      # try it
dasshboard --startup on         # ...then, if you want it with every terminal
```

Install is inert on purpose: the wheel has no install hook, no launch agent and
no shell snippet. Nothing but `--startup on` (or the first row of `s`) will edit
a file of yours — see [Launching, opt in](#launching-opt-in).

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
./target/release/dasshboard --startup   # is it hooked into the shell rc?
```

`src/startup.rs` owns the rc hook and its tests run against a scratch file, so
they never touch your real `~/.zshrc`:
`enable_then_disable_leaves_the_rc_byte_identical` is the uninstall promise,
`enabling_twice_is_the_same_as_once` the reinstall one, and
`no_hsl_is_conditional_and_unexported` pins the two properties that keep herdr
working. `a_hooked_shell_starts_the_home_screen_when_path_is_incomplete` runs
the generated blocks in a real `zsh` with the binary deliberately off PATH,
which is the state part 1 actually runs in and the one a text assertion missed.

The binary is symlinked rather than copied, so `cargo build --release` takes
effect immediately. Don't `cargo clean` without rebuilding.

Layout is verified headlessly with ratatui's `TestBackend`, which is how the
grid gets checked without a terminal: `every_tile_is_clickable_and_none_overlap`
asserts the hitboxes cover the visible list and never overlap,
`tiles_never_escape_the_viewport` runs the same check down to 30×10, and
`content_is_centred` measures the whitespace on all four sides.

The config writers take their target path as a parameter, so the round-trip
tests run against a scratch file and never touch your real config.
