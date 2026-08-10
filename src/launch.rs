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
) -> Result<String, String> {
    match backend() {
        Backend::Ghostty => crate::ghostty::open(where_to, label, argv, cwd, tint, emoji),
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
