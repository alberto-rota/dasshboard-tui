//! Where a session opens, and what this machine is actually able to do.
//!
//! Opening a *new tab* is the one thing dasshboard cannot do portably: it needs
//! a terminal that exposes an automation interface, which here means Ghostty on
//! macOS. Everywhere else -- Linux, Windows, and macOS in some other terminal --
//! a tile takes over the terminal it was launched from, which needs nothing from
//! the terminal at all.
//!
//! So the destination a tile asks for is a *request*, and `Backend::resolve` is
//! what turns it into something this machine can honour. Nothing downstream of
//! that has to know which platform it is on.

use crate::config::OpenIn;

/// Set in every session we spawn or hand off to, and checked by the shell rc
/// hook, so a shell started from the home screen never draws a second one
/// inside itself.
pub const SKIP_VAR: &str = "DASSHBOARD_SKIP";

/// Overrides detection, for a terminal we guess wrong about: `ghostty` or
/// `inplace`. Anything else is ignored rather than fatal.
pub const BACKEND_VAR: &str = "DASSHBOARD_BACKEND";

/// `hsl-login.sh`'s documented escape hatch, set for the shell that takes over
/// *after* a tile's command has ended -- and nowhere else.
///
/// That shell reads the same rc as any other, whose last act is to start the
/// workspace manager in a shell dasshboard declined. Which is exactly right for
/// the tile that *is* a shell -- that is how you still reach herdr -- and exactly
/// wrong here: quitting a `command = "herdr"` tile would land in a shell that
/// starts herdr again, on a loop. `SKIP_VAR` alone cannot say this, since it is
/// about the home screen rather than about what else owns a new terminal.
pub const NO_WORKSPACE_VAR: &str = "NO_HSL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// macOS running under Ghostty: new tabs and windows over AppleScript,
    /// tinted and titled.
    Ghostty,
    /// Everywhere else: the session replaces the home screen in the terminal
    /// that is already open.
    InPlace,
}

impl Backend {
    /// Whether a *new* surface can be opened, as opposed to taking over this
    /// one.
    pub fn can_spawn(self) -> bool {
        self == Backend::Ghostty
    }

    /// What this machine can honour of a requested destination. `tab` and
    /// `window` degrade to `current` rather than failing, so a config written
    /// on a Mac still works when it lands on a Linux box.
    pub fn resolve(self, want: OpenIn) -> OpenIn {
        if self.can_spawn() { want } else { OpenIn::Current }
    }

    /// One short line for the settings panel; empty when nothing is degraded.
    pub fn note(self) -> &'static str {
        match self {
            Backend::Ghostty => "",
            Backend::InPlace => "needs Ghostty",
        }
    }

}

/// The decision, with the environment passed in so it can be tested from any
/// platform rather than only the one running the tests.
fn choose(macos: bool, forced: Option<&str>, term_program: Option<&str>, ghostty_env: bool) -> Backend {
    match forced.map(str::trim) {
        Some("ghostty") => return Backend::Ghostty,
        Some("inplace" | "current" | "none") => return Backend::InPlace,
        // An unrecognised value is not worth refusing to start over.
        _ => {}
    }
    // Ghostty's shell integration exports both of these; either alone is
    // enough, since one of them survives a `su` or a stripped environment more
    // often than the other.
    let ghostty = ghostty_env
        || term_program.is_some_and(|t| t.eq_ignore_ascii_case("ghostty"));
    if macos && ghostty { Backend::Ghostty } else { Backend::InPlace }
}

/// What this process can do, from the environment it was started in.
///
/// Deliberately keyed on the *running* terminal rather than on whether Ghostty
/// is installed: driving Ghostty from a session inside iTerm or Alacritty would
/// answer a keypress by opening a tab in a different application's window,
/// which is not what anybody asked for.
pub fn backend() -> Backend {
    choose(
        cfg!(target_os = "macos"),
        std::env::var(BACKEND_VAR).ok().as_deref(),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
            || std::env::var_os("GHOSTTY_BIN_DIR").is_some(),
    )
}

/// What a session's surface is called: the tile's name, behind its coloured
/// circle when `tab_emoji` is on.
///
/// A tab title is text, so the circle is the only way a tile's colour can reach
/// the tab bar itself. Control characters are dropped because the title reaches
/// the terminal inside an OSC sequence, which a stray BEL or ESC would cut
/// short -- and the names come from `~/.ssh/config`, which is not ours to
/// vouch for.
pub fn tab_title(label: &str, emoji: Option<&str>) -> String {
    let clean: String = label.chars().filter(|c| !c.is_control()).collect();
    match emoji {
        Some(e) => format!("{e} {clean}"),
        None => clean,
    }
}

/// Rename the terminal we are already in, on the way out of it.
///
/// The other half of `tab_title`. A tab we *spawn* is named by the command
/// Ghostty runs in it; a tab we hand over to a session has to be named here, or
/// it keeps whatever the home screen was called -- so `c` used to leave you in
/// a tab still labelled after the terminal that opened it.
///
/// OSC 0 is not a Ghostty feature: it is what every terminal has always
/// understood by "set the title", which is why this lives in `launch` and not
/// in `ghostty`. A terminal that ignores it is no worse off than before.
pub fn rename_current_tab(title: &str) {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]0;{title}\x07");
    let _ = out.flush();
}

/// Open `argv` somewhere that is not this terminal.
///
/// Only ever reached for a destination `resolve` has already agreed to, so the
/// `InPlace` arm is a guard rather than a path anyone walks.
pub fn spawn(
    where_to: OpenIn,
    label: &str,
    argv: &[String],
    cwd: Option<&str>,
    tint: Option<&str>,
    emoji: Option<&str>,
    shell_after: bool,
) -> Result<String, String> {
    match backend() {
        Backend::Ghostty => {
            crate::ghostty::open(where_to, label, argv, cwd, tint, emoji, shell_after)
        }
        Backend::InPlace => {
            Err("this terminal cannot open new tabs -- opens here instead".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this exists to prevent: a keypress in iTerm opening a tab in
    /// Ghostty. Ghostty's own variables are what make it Ghostty, not being on
    /// a Mac.
    #[test]
    fn ghostty_is_only_chosen_when_ghostty_is_the_terminal() {
        assert_eq!(choose(true, None, Some("ghostty"), false), Backend::Ghostty);
        assert_eq!(choose(true, None, Some("Ghostty"), false), Backend::Ghostty, "case-insensitive");
        assert_eq!(choose(true, None, None, true), Backend::Ghostty, "its own env var is enough");
        assert_eq!(choose(true, None, Some("iTerm.app"), false), Backend::InPlace);
        assert_eq!(choose(true, None, Some("Apple_Terminal"), false), Backend::InPlace);
        assert_eq!(choose(true, None, None, false), Backend::InPlace);
    }

    /// AppleScript is macOS-only, so Ghostty on Linux still lands in place.
    #[test]
    fn no_platform_but_macos_gets_the_ghostty_backend() {
        assert_eq!(choose(false, None, Some("ghostty"), true), Backend::InPlace);
    }

    #[test]
    fn the_override_wins_both_ways_and_junk_is_ignored() {
        assert_eq!(choose(false, Some("ghostty"), None, false), Backend::Ghostty);
        assert_eq!(choose(true, Some("inplace"), Some("ghostty"), true), Backend::InPlace);
        assert_eq!(choose(true, Some(" ghostty "), None, false), Backend::Ghostty, "trimmed");
        assert_eq!(
            choose(true, Some("kitty"), Some("ghostty"), true),
            Backend::Ghostty,
            "an unknown value falls through to detection, it does not disable it"
        );
    }

    /// One title for both destinations: a session should read the same in the
    /// tab bar whether it opened a tab or took this one over.
    #[test]
    fn a_tab_title_is_the_circle_and_the_name() {
        assert_eq!(tab_title("alex", Some("🔵")), "🔵 alex");
        assert_eq!(tab_title("alex", None), "alex", "no circle, no space");
        // The title travels inside an OSC sequence, which a control character
        // would cut short -- and these names come out of ~/.ssh/config.
        assert_eq!(tab_title("a\x07b\x1bc", Some("🔵")), "🔵 abc");
    }

    /// A config written on a Mac has to keep working when it lands on a Linux
    /// or Windows box, so an impossible destination degrades instead of failing.
    #[test]
    fn tab_and_window_degrade_to_current_without_a_spawner() {
        for want in OpenIn::ALL {
            assert_eq!(Backend::InPlace.resolve(want), OpenIn::Current, "{want:?}");
            assert_eq!(Backend::Ghostty.resolve(want), want, "{want:?}");
        }
        assert!(!Backend::InPlace.can_spawn());
        assert!(Backend::Ghostty.can_spawn());
    }
}
