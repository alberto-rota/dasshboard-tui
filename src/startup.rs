//! Running dasshboard when a terminal opens -- strictly opt in.
//!
//! Installing the package only puts a binary on PATH. Claiming the
//! terminal-open slot is a second, explicit act (`--startup on`, or the row in
//! `s`), because that slot is usually already taken: on this author's machine
//! the dotfiles' `hsl-login.sh` starts herdr there, and two full-screen TUIs
//! cannot both own a new surface.
//!
//! The hook is two marked blocks in the login shell's rc, and the split is
//! forced by *when* each half has to run. `hsl-login.sh` is sourced as the very
//! last act of the dotfiles' rc and blocks until you quit herdr, so anything
//! placed after that source never reaches the screen -- part 1 therefore goes
//! before it and only sets that script's own `NO_HSL` escape hatch, while part 2
//! goes last, where PATH is complete and a tile can exec anything.
//!
//! Both halves test the same shell function, so they can never disagree about
//! whether this shell is getting a home screen. That is what keeps herdr
//! working: a shell dasshboard declines -- inside tmux, an agent, a herdr pane
//! -- never has `NO_HSL` set, so the dotfiles decide for themselves, exactly as
//! they would if dasshboard were not installed.
//!
//! The guard asks nothing about *which* terminal this is. It used to insist on
//! Ghostty, back when a tile could only open a Ghostty tab; a tile that takes
//! over the terminal it is already in works in any of them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Marker pairs, and the order they must appear in. Kept byte-identical to the
/// blocks earlier versions wrote by hand, so enabling over one of those
/// upgrades it in place instead of stacking a second copy.
const MARKERS: [(&str, &str); 2] = [
    ("# >>> dasshboard: 1 of 2 >>>", "# <<< dasshboard: 1 of 2 <<<"),
    ("# >>> dasshboard: 2 of 2 >>>", "# <<< dasshboard: 2 of 2 <<<"),
];

/// Set in every shell we hand off to, and tested by the guard below, so a shell
/// spawned *by* dasshboard never opens another one inside itself.
pub const SKIP_VAR: &str = crate::launch::SKIP_VAR;

/// Directories part 1 looks in, after the one the hook was written from.
/// `command -v` is the last resort rather than the first, because at part 1's
/// position it is *wrong*: see `finder`.
const FALLBACK_DIRS: [&str; 4] =
    ["$HOME/.local/bin", "$HOME/bin", "/opt/homebrew/bin", "/usr/local/bin"];

/// Find the binary without asking PATH.
///
/// This is the part that is easy to get wrong. Part 1 runs *above* the dotfiles'
/// rc, and that rc is what puts `~/.local/bin` on PATH -- so this early, PATH is
/// still the system default and `command -v dasshboard` answers "not installed".
/// A guard that believed it would hand the terminal-open slot straight back to
/// herdr and never reach part 2, which is exactly the bug that made a hooked
/// shell come up in herdr instead of the home screen.
///
/// So: the directory the hook was written from is baked in, a short list of the
/// usual places follows, and PATH is consulted last -- by which time, in part 2,
/// it is complete anyway. The answer lands in `_DASSHBOARD_BIN` so part 2 can
/// run the same binary the guard found.
fn finder(bin_dir: Option<&str>) -> String {
    let dirs: Vec<String> = bin_dir
        .into_iter()
        .map(str::to_string)
        .chain(FALLBACK_DIRS.iter().map(|d| d.to_string()))
        .map(|d| format!("\"{d}\""))
        .collect();
    format!(
        "_dasshboard_find() {{\n  \
           _DASSHBOARD_BIN=\n  \
           for _dasshboard_d in {dirs}; do\n    \
             if [ -x \"$_dasshboard_d/dasshboard\" ]; then\n      \
               _DASSHBOARD_BIN=$_dasshboard_d/dasshboard\n      \
               break\n    \
             fi\n  \
           done\n  \
           unset _dasshboard_d\n  \
           # Last, not first: complete by part 2, useless in part 1.\n  \
           [ -n \"$_DASSHBOARD_BIN\" ] || _DASSHBOARD_BIN=$(command -v dasshboard 2>/dev/null)\n  \
           [ -n \"$_DASSHBOARD_BIN\" ]\n\
         }}",
        dirs = dirs.join(" "),
    )
}

/// The directory this binary is running from, baked into the hook as the first
/// place to look. `current_exe` resolves the symlink, which is what we want:
/// the install it points into is what `--startup on` is enabling.
fn binary_dir() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let s = dir.to_str()?;
    // A path we cannot quote safely is better left out than mis-quoted; the
    // fallback list will still find a normal install.
    (!s.contains('"') && !s.contains('$') && !s.contains('`')).then(|| s.to_string())
}

/// The guard, shared by both blocks. Deliberately POSIX so the same text works
/// in `.zshrc` and `.bashrc`.
const GUARD: &str = r#"_dasshboard_should_start() {
  # An explicit no, first, so it beats everything else. Exported by part 2
  # before the call, so every shell below a home screen inherits the answer --
  # including the one a local tile execs.
  [ -z "${DASSHBOARD_SKIP:-}" ] || return 1
  # Interactive shells only: this rules out `ssh host cmd`, scp/rsync and every
  # script that happens to source a shell rc.
  case $- in *i*) ;; *) return 1 ;; esac
  [ -z "${SSH_ORIGINAL_COMMAND:-}${SSH_CONNECTION:-}" ] || return 1
  # Any terminal will do -- without Ghostty a tile takes over this one instead
  # of opening a tab, which needs nothing from the terminal at all. Set
  # DASSHBOARD_ONLY_IN to a TERM_PROGRAM value to keep it to one of them.
  [ -z "${DASSHBOARD_ONLY_IN:-}" ] || [ "${TERM_PROGRAM:-}" = "$DASSHBOARD_ONLY_IN" ] || return 1
  [ -z "${TMUX:-}" ] || return 1
  # Already inside herdr. Every pane it spawns runs this rc, so without this the
  # first thing a new pane would do is draw a home screen inside itself.
  [ -z "${HERDR_ENV:-}${HERDR_PANE_ID:-}${HERDR_SOCKET_PATH:-}${HERDR_WORKSPACE_ID:-}" ] || return 1
  # An AI agent driving a shell, where a full-screen TUI is wrong.
  [ -z "${CLAUDECODE:-}${AI_AGENT:-}" ] || return 1
  # A real terminal on both ends, and one that can actually draw.
  [ -t 0 ] && [ -t 1 ] || return 1
  case "${TERM:-dumb}" in dumb|unknown|"") return 1 ;; esac
  # And there has to be something to run: an uninstall must not cost you the
  # workspace manager as well. Records the path for part 2 to launch.
  _dasshboard_find
}"#;

/// Part 1: hand the terminal-open slot over, for this shell only.
fn block_one(bin_dir: Option<&str>) -> String {
    format!(
        "\
# Part 1 of 2, and it has to come before the dotfiles' rc. When dasshboard is
# going to draw the home screen for this shell it owns the terminal-open slot,
# so nothing else may claim it: NO_HSL=1 is hsl-login.sh's own documented
# escape hatch, and that script is sourced at the end of the rc, by which time
# this has run. herdr is un-autostarted for this one shell, not disabled --
# type `hsl`, or give it a [[local]] tile.
#
# Two details carry the weight:
#
#   * conditional -- a shell dasshboard declines (tmux, a herdr pane, an agent,
#     a non-interactive shell) never gets NO_HSL, so the dotfiles start herdr
#     there exactly as they would if dasshboard were not installed;
#   * NOT exported -- hsl-login.sh is sourced by this same shell, so a plain
#     variable is enough, and it dies with the shell. An exported one would be
#     inherited by the shell a local tile execs, leaving that shell with the
#     workspace manager silently suppressed -- which is the one place you most
#     want it.
{finder}
{GUARD}
if _dasshboard_should_start; then NO_HSL=1; fi",
        finder = finder(bin_dir),
    )
}

/// Part 2: the home screen itself.
fn block_two() -> String {
    format!(
        "\
# Part 2 of 2: the home screen. Deliberately last, and after the dotfiles' rc,
# so PATH is complete before a tile can exec anything. Run, not exec'd, so
# quitting it (q) falls through to this same shell.
#
#     DASSHBOARD_SKIP=1 zsh -l            # a plain shell, no home screen
#     NO_HSL=1 DASSHBOARD_SKIP=1 zsh -l   # ...and no workspace manager either
#     homescreen                          # bring it back up by hand
#     dasshboard --startup off            # remove both of these blocks
#
# {SKIP_VAR} is exported before the call, so the shell a local tile execs
# comes up as a normal login shell instead of a second home screen -- and since
# part 1 asks this same question, that shell is where herdr gets to start.
#
# The binary is the one the guard found, not whatever `dasshboard` resolves to
# now -- part 1 already answered that question, and answered it without PATH.
alias homescreen='dasshboard'
# The `command -v` is for a half-removed hook: part 1 gone, this still here.
if command -v _dasshboard_should_start >/dev/null 2>&1 && _dasshboard_should_start; then
  export {SKIP_VAR}=1
  \"$_DASSHBOARD_BIN\"
fi
unset -f _dasshboard_should_start _dasshboard_find 2>/dev/null
unset _DASSHBOARD_BIN"
    )
}

/// Whether the hook is in place, and whether it is whole.
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    Off,
    On,
    /// One block without the other -- a hand-edit or a failed write. `enable`
    /// repairs it.
    Partial,
}

impl State {
    pub fn is_on(&self) -> bool {
        matches!(self, State::On)
    }

    pub fn label(&self) -> &'static str {
        match self {
            State::Off => "off",
            State::On => "on",
            State::Partial => "half-installed",
        }
    }
}

/// The rc file the hook goes in, from `$SHELL`.
///
/// Only zsh and bash are wired up, because the hook is POSIX shell and there is
/// no honest way to write it into a file whose syntax we are guessing at.
/// Anything else -- fish, and every native Windows shell -- gets an error
/// naming itself and the one-line manual equivalent instead.
pub fn rc_path() -> Result<PathBuf, String> {
    // Git Bash and WSL set $SHELL on Windows too, and their rc files are real
    // POSIX ones, so the shell is what decides here rather than the platform.
    let shell = std::env::var("SHELL").unwrap_or_default();
    let name = Path::new(&shell).file_name().and_then(|s| s.to_str()).unwrap_or("");
    let name = name.strip_suffix(".exe").unwrap_or(name);
    match name {
        "zsh" => Ok(match std::env::var_os("ZDOTDIR") {
            Some(d) if !d.is_empty() => PathBuf::from(d).join(".zshrc"),
            _ => home().join(".zshrc"),
        }),
        "bash" => Ok(home().join(".bashrc")),
        "" if cfg!(windows) => Err(
            "dasshboard does not edit Windows shell profiles -- add a `dasshboard` line to \
             the end of your PowerShell profile ($PROFILE) to open it with every terminal"
                .into(),
        ),
        "" => Err("$SHELL is not set, so there is no rc file to hook".into()),
        other => Err(format!(
            "dasshboard only knows how to hook zsh and bash, not {other} -- \
             run `dasshboard --startup print` and paste the result into your rc by hand"
        )),
    }
}

fn home() -> PathBuf {
    crate::platform::home()
}

pub fn state_at(rc: &Path) -> State {
    let text = fs::read_to_string(rc).unwrap_or_default();
    let found: Vec<bool> =
        MARKERS.iter().map(|(b, _)| text.lines().any(|l| l.trim() == *b)).collect();
    match (found[0], found[1]) {
        (true, true) => State::On,
        (false, false) => State::Off,
        _ => State::Partial,
    }
}

/// The live answer for this machine. Cheap enough to call on a reload, not on
/// every frame -- `App` caches it.
pub fn state() -> State {
    match rc_path() {
        Ok(p) => state_at(&p),
        Err(_) => State::Off,
    }
}

/// Write both blocks, replacing any that are already there. Idempotent: running
/// it twice leaves the file byte-identical, so it doubles as the upgrade path
/// for an older hook.
///
/// Part 1 goes to the top of the file because that is the only position
/// guaranteed to be before whatever sources `hsl-login.sh`; part 2 goes to the
/// bottom for the same reason in reverse.
pub fn enable_at(rc: &Path) -> io::Result<()> {
    let text = fs::read_to_string(rc).unwrap_or_default();
    backup_once(rc, &text)?;

    let mut lines = strip(&text);
    let block1 = wrap(0, &block_one(binary_dir().as_deref()));
    let block2 = wrap(1, &block_two());

    // Exactly one blank line between us and whatever was first. A blank line
    // that was already at the very top goes, since part 1 sits above it either
    // way -- keeping it would leave a widening gap after every enable.
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    if !lines.is_empty() {
        lines.insert(0, String::new());
    }
    for (i, l) in block1.into_iter().enumerate() {
        lines.insert(i, l);
    }
    lines.push(String::new());
    lines.extend(block2);
    write(rc, lines)
}

/// Remove both blocks, leaving everything else exactly as it was -- including
/// the blank lines, which is why `enable` then `disable` gives the file back
/// byte-identical. (The one exception is a blank line that was the very first
/// in the file; `enable` drops those, since part 1 goes above them regardless.)
pub fn disable_at(rc: &Path) -> io::Result<bool> {
    let text = fs::read_to_string(rc).unwrap_or_default();
    if state_at(rc) == State::Off {
        return Ok(false);
    }
    let lines = strip(&text);
    write(rc, lines)?;
    Ok(true)
}

pub fn enable() -> Result<PathBuf, String> {
    let rc = rc_path()?;
    enable_at(&rc).map_err(|e| format!("could not write {}: {e}", rc.display()))?;
    Ok(rc)
}

pub fn disable() -> Result<(PathBuf, bool), String> {
    let rc = rc_path()?;
    let removed = disable_at(&rc).map_err(|e| format!("could not write {}: {e}", rc.display()))?;
    Ok((rc, removed))
}

/// The hook as text, for a shell we do not write to ourselves.
pub fn script() -> String {
    let mut s = wrap(0, &block_one(binary_dir().as_deref())).join("\n");
    s.push_str("\n\n# ...then, at the very end of the file:\n\n");
    s.push_str(&wrap(1, &block_two()).join("\n"));
    s.push('\n');
    s
}

fn wrap(i: usize, body: &str) -> Vec<String> {
    let (begin, end) = MARKERS[i];
    let mut out = vec![begin.to_string()];
    out.extend(body.lines().map(str::to_string));
    out.push(end.to_string());
    out
}

/// Drop every marked block, taking one adjacent blank line with each so the
/// file does not grow an empty gap per install/uninstall cycle.
fn strip(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (begin, end) in MARKERS {
        while let Some(from) = lines.iter().position(|l| l.trim() == begin) {
            // An unterminated block would otherwise eat the rest of the file:
            // take just the marker line and leave the body for the user to see.
            let to = lines[from..]
                .iter()
                .position(|l| l.trim() == end)
                .map_or(from, |off| from + off);
            lines.drain(from..=to);
            // The blank line we inserted with the block, whichever side it
            // ended up on. After, for a block at the top; before, at the bottom.
            if lines.get(from).is_some_and(|l| l.trim().is_empty()) {
                lines.remove(from);
            } else if from > 0 && lines[from - 1].trim().is_empty() {
                lines.remove(from - 1);
            }
        }
    }
    lines
}

/// One copy of the rc as it was before dasshboard ever touched it. Never
/// overwritten, so a second enable cannot clobber the only pristine copy.
fn backup_once(rc: &Path, text: &str) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let bak = rc.with_file_name(format!(
        "{}.bak.dasshboard",
        rc.file_name().and_then(|s| s.to_str()).unwrap_or("rc")
    ));
    if bak.exists() {
        return Ok(());
    }
    fs::write(bak, text)
}

pub fn backup_path(rc: &Path) -> PathBuf {
    rc.with_file_name(format!(
        "{}.bak.dasshboard",
        rc.file_name().and_then(|s| s.to_str()).unwrap_or("rc")
    ))
}

fn write(rc: &Path, lines: Vec<String>) -> io::Result<()> {
    let mut joined = lines.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    fs::write(rc, joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("dasshboard-startup-{name}"));
        let _ = fs::remove_file(backup_path(&p));
        fs::write(&p, body).unwrap();
        p
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    const RC: &str = "export EDITOR=nvim\n\n\
                      # >>> dotfiles shell additions >>>\n\
                      source \"$HOME/dotfiles/shell/shellrc_additions.sh\"\n\
                      # <<< dotfiles shell additions <<<\n";

    /// The whole promise of the uninstall: your rc comes back exactly as it was,
    /// which is what restores herdr's autostart.
    #[test]
    fn enable_then_disable_leaves_the_rc_byte_identical() {
        let p = scratch("roundtrip", RC);
        enable_at(&p).unwrap();
        assert_ne!(read(&p), RC, "nothing was written");
        assert!(disable_at(&p).unwrap());
        assert_eq!(read(&p), RC);
    }

    /// Enabling over an existing hook upgrades it in place. Twice must not stack
    /// a second copy, or every reinstall would add another home screen.
    #[test]
    fn enabling_twice_is_the_same_as_once() {
        let p = scratch("idempotent", RC);
        enable_at(&p).unwrap();
        let once = read(&p);
        enable_at(&p).unwrap();
        assert_eq!(read(&p), once);
        assert_eq!(
            once.lines().filter(|l| l.trim() == MARKERS[1].0).count(),
            1,
            "one launcher, not two"
        );
        assert!(disable_at(&p).unwrap());
        assert_eq!(read(&p), RC, "and it still uninstalls cleanly");
    }

    /// Part 1 exists to beat the dotfiles' rc to the terminal-open slot, so it
    /// has to land above whatever sources it; part 2 has to come after, where
    /// PATH is complete.
    #[test]
    fn the_blocks_straddle_the_dotfiles_source() {
        let p = scratch("order", RC);
        enable_at(&p).unwrap();
        let text = read(&p);
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at(MARKERS[0].0) < at("shellrc_additions.sh"));
        assert!(at("shellrc_additions.sh") < at(MARKERS[1].0));
        assert!(at(MARKERS[0].1) < at(MARKERS[1].0), "blocks must not nest");
    }

    /// The bug this whole conditional exists for: an unconditional `export
    /// NO_HSL=1` suppressed herdr in shells dasshboard never drew in, and in the
    /// shell a local tile execs.
    #[test]
    fn no_hsl_is_conditional_and_unexported() {
        let text = block_one(None);
        assert!(text.contains("if _dasshboard_should_start; then NO_HSL=1; fi"));
        assert!(!text.contains("export NO_HSL"), "an exported NO_HSL leaks into local tiles");
        for block in [block_one(None), block_two()] {
            assert!(block.contains("_dasshboard_should_start"), "both halves share the guard");
        }
    }

    /// Part 1 runs above the rc that puts ~/.local/bin on PATH, so it must find
    /// the binary by path. It once used `command -v`, decided dasshboard was not
    /// installed, and let herdr take the slot -- a hooked shell came up in the
    /// workspace manager instead of the home screen.
    #[test]
    fn part_one_finds_the_binary_without_asking_path() {
        let text = block_one(Some("/opt/dassh/bin"));
        let find = text.split("_dasshboard_find() {").nth(1).expect("no finder");
        let body = find.split("\n}").next().unwrap();
        assert!(body.contains("\"/opt/dassh/bin\""), "the install dir is baked in first");
        assert!(body.contains("$HOME/.local/bin"), "and the usual places follow");
        let by_path = body.find("command -v").expect("PATH is still a fallback");
        assert!(body.find("-x \"$_dasshboard_d/dasshboard\"").unwrap() < by_path, "PATH last");
        assert!(text.contains("_dasshboard_find\n}"), "the guard ends on the lookup");
        assert!(block_two().contains("\"$_DASSHBOARD_BIN\""), "part 2 runs what it found");
    }

    /// Run the generated hook in a real `zsh`, and report what it printed.
    ///
    /// The rc is the shape the blocks are written for: part 1, then a stand-in
    /// for `hsl-login.sh` sourced by the rc that also completes PATH, then part
    /// 2. PATH deliberately excludes the binary's directory, because that is
    /// the state part 1 actually runs in -- a text assertion missed this once
    /// and a hooked shell came up in the workspace manager instead.
    ///
    /// The tty tests are dropped from the copy under test because cargo's
    /// stdout is a pipe; every other condition is verbatim. `None` means there
    /// is no zsh here to ask.
    fn hooked_zsh(name: &str, env: &[(&str, &str)]) -> Option<String> {
        use std::process::Command;
        if Command::new("zsh").arg("-c").arg("exit").status().is_err() {
            return None;
        }
        let dir = std::env::temp_dir().join(format!("dasshboard-shell-{name}"));
        let bin = dir.join("bin");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("dasshboard"), "#!/bin/sh\necho MARK-dasshboard\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(bin.join("dasshboard"), fs::Permissions::from_mode(0o755)).unwrap();
        }

        let tty = "  [ -t 0 ] && [ -t 1 ] || return 1\n";
        let one = block_one(bin.to_str());
        assert!(one.contains(tty), "the tty test moved; this test is stale");
        let rc = format!(
            "{}\nexport PATH=\"{}:$PATH\"\n\
             if [ -z \"${{NO_HSL:-}}\" ]; then echo MARK-hsl; fi\n{}\n",
            wrap(0, &one.replace(tty, "")).join("\n"),
            bin.display(),
            wrap(1, &block_two()).join("\n"),
        );
        fs::write(dir.join(".zshrc"), rc).unwrap();

        let mut cmd = Command::new("zsh");
        cmd.arg("-i")
            .arg("-c")
            .arg("exit")
            .env_clear()
            .env("HOME", &dir)
            .env("ZDOTDIR", &dir)
            // Deliberately without `bin`: the dotfiles add it too late for part 1.
            .env("PATH", "/usr/bin:/bin")
            .env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("zsh");
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    fn a_hooked_shell_starts_the_home_screen_when_path_is_incomplete() {
        let Some(text) = hooked_zsh("ghostty", &[("TERM_PROGRAM", "ghostty")]) else { return };
        assert!(text.contains("MARK-dasshboard"), "no home screen: {text:?}");
        assert!(!text.contains("MARK-hsl"), "herdr claimed the slot too: {text:?}");
    }

    /// The guard used to insist on Ghostty, because a tile could only open a
    /// Ghostty tab. A tile that takes over the terminal it is already in works
    /// in any of them, so an unknown terminal -- or none at all -- must still
    /// get the home screen.
    #[test]
    fn any_terminal_gets_the_home_screen_now() {
        for (name, env) in [
            ("bare", vec![]),
            ("xterm", vec![("TERM_PROGRAM", "Apple_Terminal")]),
            ("vscode", vec![("TERM_PROGRAM", "vscode")]),
        ] {
            let Some(text) = hooked_zsh(name, &env) else { return };
            assert!(text.contains("MARK-dasshboard"), "{name}: no home screen: {text:?}");
        }
    }

    /// ...and the way back, for one terminal only, without hand-editing a
    /// block that `--startup on` regenerates.
    #[test]
    fn only_in_restricts_the_hook_to_one_terminal() {
        let only = [("DASSHBOARD_ONLY_IN", "ghostty")];
        let Some(text) = hooked_zsh("only-in-no", &[only[0], ("TERM_PROGRAM", "vscode")]) else {
            return;
        };
        assert!(!text.contains("MARK-dasshboard"), "should have declined: {text:?}");
        assert!(text.contains("MARK-hsl"), "declining must hand the slot back: {text:?}");

        let text = hooked_zsh("only-in-yes", &[only[0], ("TERM_PROGRAM", "ghostty")]).unwrap();
        assert!(text.contains("MARK-dasshboard"), "the named terminal still starts: {text:?}");
    }

    /// A fresh rc, and an empty one, both have to work -- no backup to write and
    /// no leading blank line to leave behind.
    #[test]
    fn an_empty_rc_gets_just_the_two_blocks() {
        let p = scratch("empty", "");
        assert!(!backup_path(&p).exists());
        enable_at(&p).unwrap();
        assert!(!backup_path(&p).exists(), "nothing was there to back up");
        let text = read(&p);
        assert!(text.starts_with(MARKERS[0].0), "no leading blank line");
        assert!(text.ends_with("2 of 2 <<<\n"));
        assert!(disable_at(&p).unwrap());
        assert_eq!(read(&p), "");
    }

    #[test]
    fn state_reports_off_on_and_half_installed() {
        let p = scratch("state", RC);
        assert_eq!(state_at(&p), State::Off);
        assert!(!disable_at(&p).unwrap(), "nothing to remove");

        enable_at(&p).unwrap();
        assert_eq!(state_at(&p), State::On);

        // Hand-delete the launcher and the hook is half there; enable repairs it.
        let kept: Vec<String> = read(&p)
            .lines()
            .take_while(|l| l.trim() != MARKERS[1].0)
            .map(str::to_string)
            .collect();
        write(&p, kept).unwrap();
        assert_eq!(state_at(&p), State::Partial);
        enable_at(&p).unwrap();
        assert_eq!(state_at(&p), State::On);
    }

    /// The pre-install copy is the way back if a hook ever locks someone out, so
    /// a second enable must not overwrite it with an already-hooked file.
    #[test]
    fn the_backup_keeps_the_pristine_copy() {
        let p = scratch("backup", RC);
        enable_at(&p).unwrap();
        assert_eq!(read(&backup_path(&p)), RC);
        enable_at(&p).unwrap();
        assert_eq!(read(&backup_path(&p)), RC, "still the pre-install copy");
    }

    /// A block whose end marker someone deleted must cost one line, not the rest
    /// of the file.
    #[test]
    fn an_unterminated_block_does_not_eat_the_rc() {
        let body = format!("{}\nNO_HSL=1\nexport EDITOR=nvim\n", MARKERS[0].0);
        let p = scratch("unterminated", &body);
        let lines = strip(&read(&p));
        assert!(lines.iter().any(|l| l == "export EDITOR=nvim"), "kept: {lines:?}");
    }
}
