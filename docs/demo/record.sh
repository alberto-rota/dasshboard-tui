#!/bin/sh
# Record the README's GIFs with vhs, against a made-up machine.
#
# Every recording runs in a scratch HOME built from ./fixtures, so the hosts,
# folders and the config the tapes edit are all invented -- nothing here reads
# or writes the real ~/.ssh/config, ~/.config/dasshboard or ~/.zshrc.
#
#   ./docs/demo/record.sh            # all of them
#   ./docs/demo/record.sh board.tape # just one
#
# Needs: vhs, ffmpeg, ttyd (brew install vhs ffmpeg ttyd). vhs renders through a
# headless Chromium it downloads on first run; point ROD_BROWSER_BIN at one you
# already have if that download is not an option.

set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
bin="$root/target/release/dasshboard"

[ -x "$bin" ] || { echo "build it first: cargo build --release" >&2; exit 1; }

# One scratch home per run, thrown away at the end: the tapes press `a`, `d`,
# `x` and `s`, all of which write, and a recording has to start from the same
# board every time.
demo=$(mktemp -d "${TMPDIR:-/tmp}/dasshboard-demo.XXXXXX")
trap 'rm -rf "$demo"' EXIT INT TERM

reset_home() {
  rm -rf "$demo/home"
  mkdir -p "$demo/home/.ssh" "$demo/home/.config/dasshboard" "$demo/home/bin" \
           "$demo/home/projects/aurora/src" "$demo/home/thesis" "$demo/home/notes"
  # something for the opened session's `ls` to print
  touch "$demo/home/projects/aurora/Cargo.toml" \
        "$demo/home/projects/aurora/README.md" \
        "$demo/home/projects/aurora/src/main.rs"
  # what the NOTES tile prints before it leaves you a shell there
  printf '%s\n' \
    '- ship the tab-colour patch' \
    '- reply to the cluster ticket' \
    '- book the flight' > "$demo/home/notes/today.md"
  cp "$here/fixtures/ssh_config" "$demo/home/.ssh/config"
  cp "$here/fixtures/config.toml" "$demo/home/.config/dasshboard/config.toml"
  cp "$here/fixtures/zshrc" "$demo/home/.zshrc"
  chmod 600 "$demo/home/.ssh/config"
  ln -sf "$bin" "$demo/home/bin/dasshboard"
}

HOME="$demo/home"
XDG_CONFIG_HOME="$demo/home/.config"
PATH="$demo/home/bin:$PATH"
SHELL=/bin/zsh
# vhs is not Ghostty, so a tile would take over the terminal -- which is the
# right thing to show, and the one that works in every terminal.
DASSHBOARD_BACKEND=inplace
DASSHBOARD_SKIP=
TERM=xterm-256color
export HOME XDG_CONFIG_HOME PATH SHELL DASSHBOARD_BACKEND TERM
unset DASSHBOARD_SKIP

reset_home
# vhs is run from the repo root, so a tape's Output path is repo-relative;
# the shell inside the recording starts in the demo home instead.
cd "$root"

# `_`-prefixed tapes are includes and scratch probes, not recordings.
[ "$#" -gt 0 ] || set -- "$here"/[a-z]*.tape

for tape in "$@"; do
  case $tape in */*) ;; *) tape="$here/$tape" ;; esac
  reset_home
  echo "==> $(basename "$tape")"
  vhs "$tape"
done
