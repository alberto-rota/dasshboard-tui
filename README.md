# dasshboard

[![PyPI](https://img.shields.io/badge/pypi-dasshboard-informational)](https://pypi.org/project/dasshboard/)

A home screen for your terminal. Every host in `~/.ssh/config` — plus anything
you add yourself — becomes a clickable tile. Activating one connects: under
Ghostty on macOS that is a new tab tinted with the host's colour, and in every
other terminal it takes over the one you are already in.

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

- **ssh** — connects to a host.
- **local** (tagged `local`) — runs a command on this machine.

Both end up in the same three places, chosen by `open_in`: a **new tab**, a
**new window**, or **this terminal**. Taking over this terminal is the one that
works everywhere: dasshboard restores the terminal and then hands the session
the process, so it is replaced rather than nested, and quitting the session
returns you to the shell that started dasshboard.

## Compatibility

The TUI is portable — it is Rust and [ratatui](https://ratatui.rs), and it draws
the same picture everywhere. What is *not* portable is opening a **new tab**:
that needs the terminal to expose an automation interface, and today that means
Ghostty on macOS. So dasshboard has two levels, and picks between them by
looking at the terminal it was started in.

| | macOS + Ghostty | any other terminal (macOS, Linux, BSD) | Windows |
|---|---|---|---|
| the TUI, mouse, colours | ✅ | ✅ | ✅ |
| ssh tiles and local tiles | ✅ | ✅ | ✅ |
| open in **this terminal** | ✅ | ✅ | ✅ |
| open in a **new tab / window** | ✅ tinted, titled | → this terminal | → this terminal |
| tab tint + coloured circle | ✅ | — | — |
| `--startup on` | ✅ zsh, bash | ✅ zsh, bash | by hand, see below |

**Level 1 — macOS running Ghostty ≥ 1.2.** Everything above. `t`/`w`/`c` pick a
new tab, a new window, or this one; the new surface carries the host's colour as
a background tint and its circle in the tab title.

**Level 2 — everywhere else.** Every tile opens in the terminal you launched
dasshboard from, `ssh` replacing the home screen and returning you to your shell
when it ends. Nothing is silently broken: `tab` and `window` still parse, still
save, and still travel in a config.toml shared between machines — they simply
resolve to `current` on a machine that cannot honour them, and the settings
panel says so. The `t`/`w`/`c` hints disappear from the footer, since there is
nothing to choose between.

Detection is on the **running** terminal (`TERM_PROGRAM`, or Ghostty's own
`GHOSTTY_*` variables), not on whether Ghostty is installed — otherwise pressing
`⏎` in iTerm would answer by opening a tab in a different application. Override
it with `DASSHBOARD_BACKEND=ghostty` or `DASSHBOARD_BACKEND=inplace` if that
guess is ever wrong.

What dasshboard asks of a terminal is otherwise ordinary: UTF-8, 24-bit colour
(it degrades to the nearest 256 in terminals without it), and mouse reporting if
you want to click tiles. On Windows use [Windows
Terminal](https://aka.ms/terminal) — `conhost.exe` will run it, but the box
drawing and the coloured circles are not worth looking at there. `ssh` itself
comes with Windows 10 1809 and later; `Add-WindowsCapability -Online -Name
OpenSSH.Client` if it is missing. Hosts are read from `%USERPROFILE%\.ssh\config`,
which is where OpenSSH for Windows puts them.

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
| `t` / `w` / `c` | open in a new tab / new window / this terminal, once (Ghostty only; elsewhere all three mean `c`) |
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
risks the options dasshboard doesn't model. Instead, editing one of its hosts
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

Everything lands in `~/.config/dasshboard/config.toml` — `$XDG_CONFIG_HOME` if
you set it, and `%APPDATA%\dasshboard\config.toml` on Windows — created with a
commented template on first run. `dasshboard --config` prints the path this
machine chose:

```toml
[options]
include_ssh_config = true   # false shows only the hosts defined here
tint_tabs = true            # tint the new tab's background (Ghostty only)
tab_emoji = true            # coloured circle in the tab title (Ghostty only)
show_hidden = false         # reveal hidden hosts, to unhide them
open_in = "tab"             # "tab", "window" or "current"; "current" elsewhere

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

## How a session is opened

Two paths, and `src/launch.rs` is the whole of the decision between them —
`src/ghostty.rs` is the only module that knows Ghostty exists, and nothing
reaches it unless Ghostty is the terminal we are running in.

### In this terminal — everywhere

The TUI is torn down first (alternate screen off, raw mode off), and only then
does the session start, so it comes up on a clean screen owning the terminal
outright. On Unix that is a real `exec`: dasshboard *becomes* ssh, leaving no
wrapper process holding the tty, and when the connection ends the shell that
launched dasshboard is what you land back in. Windows has no `exec`, so there
the session runs as a child on the same console and dasshboard exits with its
status — the only difference is a sleeping parent behind it.

`DASSHBOARD_SKIP=1` is exported across the handoff. A local tile usually runs a
shell, that shell reads the same rc that started dasshboard, and without the
flag the first thing it would do is draw a second home screen inside the first.

### In a new Ghostty tab — macOS

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
same file and every option dasshboard doesn't model — `IdentityFile`,
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

Only zsh and bash are written to, because the hook is POSIX shell and there is
no honest way to write it into a file whose syntax we would be guessing at.
**fish** and every native Windows shell get an error naming themselves instead.
On Windows the equivalent is one line at the end of your PowerShell profile
(`notepad $PROFILE`), guarded so a shell dasshboard launched doesn't draw a
second home screen inside itself:

```powershell
if (-not $env:DASSHBOARD_SKIP) { dasshboard }
```

The guard asks **nothing about which terminal this is** — it used to insist on
Ghostty, back when a tile could only open a Ghostty tab, but a tile that takes
over the terminal it is already in works in all of them. To keep it to one
terminal anyway, set `DASSHBOARD_ONLY_IN` to a `TERM_PROGRAM` value (e.g.
`export DASSHBOARD_ONLY_IN=ghostty`) above the hook; the generated block tests
it, so this survives the next `--startup on`.

The split into two blocks is forced by *when* each half must run.
`hsl-login.sh` is sourced as the last act of the dotfiles' rc and **blocks**
until you quit herdr, so anything after it never reaches the screen: part 1 goes
above, part 2 (the UI, where PATH is finally complete) goes at the very bottom.

Both halves ask the same shell function, and that is the load-bearing part:

- **`NO_HSL=1` is conditional.** Part 1 sets `hsl-login.sh`'s own escape hatch
  only in a shell dasshboard is actually going to draw in. Every shell it
  declines — inside tmux, a herdr pane, an agent, a non-interactive shell — gets
  herdr exactly as if dasshboard were not installed. An unconditional `export` here
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

Built here, the wheel comes out tagged `macosx_11_0_arm64` — maturin tags what
it built, not what the code supports. There is nothing macOS-only left in the
source, so the same `uv build` on a Linux or Windows runner (or under
[`maturin` in a container](https://www.maturin.rs/distribution)) produces a
wheel for that platform. Until those are published, `pip` falls back to the
sdist on anything else and builds from source, which needs a Rust toolchain but
otherwise works: see [Compatibility](#compatibility) for what you get.

## Development

```sh
cargo build --release        # ~/.local/bin/dasshboard symlinks to the output
cargo test                   # parsing, escaping, colour assignment, layout
cargo test layout_dump -- --nocapture   # eyeball every screen at four sizes
./target/release/dasshboard --list      # tiles, colours and circles, no TUI
./target/release/dasshboard --open alex # connect to one host, no TUI
./target/release/dasshboard --config    # print the config path
./target/release/dasshboard --startup   # is it hooked into the shell rc?
```

Portability is checked without leaving the Mac. `cargo check` links nothing, so
a target's std library is all it needs:

```sh
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
cargo check --all-targets --target x86_64-pc-windows-msvc
cargo check --all-targets --target x86_64-unknown-linux-gnu
DASSHBOARD_BACKEND=inplace cargo test layout_dump -- --nocapture   # the other screen
```

`src/platform.rs` is where the differences live — home directory, hostname,
executable bit, `PATH` — so the rest of the tree never has to have a `cfg!` in
it. The two that cannot be abstracted are marked: the `exec`/spawn split in
`main.rs::hand_off`, and `src/ghostty.rs` as a whole.

`src/startup.rs` owns the rc hook and its tests run against a scratch file, so
they never touch your real `~/.zshrc`:
`enable_then_disable_leaves_the_rc_byte_identical` is the uninstall promise,
`enabling_twice_is_the_same_as_once` the reinstall one, and
`no_hsl_is_conditional_and_unexported` pins the two properties that keep herdr
working. `a_hooked_shell_starts_the_home_screen_when_path_is_incomplete` runs
the generated blocks in a real `zsh` with the binary deliberately off PATH,
which is the state part 1 actually runs in and the one a text assertion missed;
`any_terminal_gets_the_home_screen_now` and
`only_in_restricts_the_hook_to_one_terminal` run the same shell for the guard's
terminal policy.

`without_a_spawner_every_destination_becomes_a_handoff` is the portability
promise in one assertion: with no way to open a tab, a tile that asks for one
lands in this terminal rather than reaching the AppleScript backend and failing
there.

The binary is symlinked rather than copied, so `cargo build --release` takes
effect immediately. Don't `cargo clean` without rebuilding.

Layout is verified headlessly with ratatui's `TestBackend`, which is how the
grid gets checked without a terminal: `every_tile_is_clickable_and_none_overlap`
asserts the hitboxes cover the visible list and never overlap,
`tiles_never_escape_the_viewport` runs the same check down to 30×10, and
`content_is_centred` measures the whitespace on all four sides.

The config writers take their target path as a parameter, so the round-trip
tests run against a scratch file and never touch your real config.
