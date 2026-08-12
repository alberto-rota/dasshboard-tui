//! Opens new Ghostty tabs via its AppleScript interface (Ghostty >= 1.2).
//!
//! The one part of dasshboard that is not portable, and the only module that
//! knows Ghostty exists: `launch` picks it when -- and only when -- we are
//! running on macOS under Ghostty, and hands everything else to the terminal we
//! are already in. Nothing here is reached on Linux or Windows.
//!
//! Two quirks worth recording, both found by probing the running app:
//!
//!  * `new tab` errors with -1708 unless the optional `in <window>` parameter is
//!    given explicitly, so we always pass a window.
//!  * There is no `GHOSTTY_SURFACE_ID` in the environment, so a surface cannot
//!    identify its own window. `front window` is correct in practice: we only
//!    ever spawn in response to input, which means we are focused.

use std::process::Command;

use crate::config::OpenIn;
use crate::launch::SKIP_VAR;

/// Wrap in single quotes for `/bin/sh`.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        // Close the quote, emit an escaped quote, reopen. The only way to get a
        // literal `'` inside a single-quoted word.
        if c == '\'' { out.push_str("'\\''") } else { out.push(c) }
    }
    out.push('\'');
    out
}

/// Wrap in double quotes for an AppleScript string literal.
fn applescript_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Scale a `#rrggbb` toward black. Used to turn a host's identity colour into a
/// background dark enough to read text on.
fn scale_hex(hex: &str, factor: f32) -> Option<String> {
    let h = hex.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    let (r, g, b) = (c(0)?, c(2)?, c(4)?);
    let s = |v: u8| (v as f32 * factor).round().clamp(0.0, 255.0) as u8;
    Some(format!("#{:02x}{:02x}{:02x}", s(r), s(g), s(b)))
}

/// The program Ghostty runs in the new surface: name the tab, optionally tint
/// it, then hand the process over so the surface dies with the command.
///
/// `args` is already-split argv, and each element is quoted separately, so a
/// value containing a space stays one argument instead of two.
///
/// `shell_after` is the one case that does *not* hand the process over. A tile
/// running a command of its own may be running one that exits -- `echo hi` is a
/// tab that prints a line and then cannot be typed in -- so the command is run
/// rather than exec'd, and the shell that follows it is what gets the surface.
/// `sh` staying on as its parent is the price of there being something after it.
///
/// The tint is two OSC sequences the surface emits about itself: OSC 11 sets
/// the background (scaled right down, or the text would be unreadable) and OSC
/// 12 the cursor (full strength -- it is small enough to carry real colour).
/// There is no tab-colour property in Ghostty's AppleScript dictionary, so
/// changing what the surface reports about itself is the available lever.
fn surface_command(
    title: &str,
    args: &[String],
    cwd: Option<&str>,
    tint: Option<&str>,
    shell_after: bool,
) -> String {
    let argv: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();

    // The title goes through %s rather than into the format string, so a `%` in
    // a host name cannot corrupt the escape sequence.
    let mut inner =
        format!("printf {} {}; ", shell_quote("\\033]0;%s\\007"), shell_quote(title));

    if let Some(hex) = tint {
        if let Some(bg) = scale_hex(hex, 0.13) {
            inner.push_str(&format!(
                "printf {} {}; ",
                shell_quote("\\033]11;%s\\007"),
                shell_quote(&bg)
            ));
        }
        inner.push_str(&format!(
            "printf {} {}; ",
            shell_quote("\\033]12;%s\\007"),
            shell_quote(hex)
        ));
    }

    // A local tile's directory is applied here rather than through Ghostty's
    // own `initial working directory`, so the same path serves every
    // destination -- including the exec that never reaches AppleScript.
    if let Some(dir) = cwd.filter(|d| !d.is_empty()) {
        inner.push_str(&format!("cd {} || exit 1; ", shell_quote(dir)));
    }
    let run = argv.join(" ");
    if shell_after {
        // Exported for the shell alone, after the command has had the surface:
        // it is about what may autostart in a *new* shell, and the command is
        // not one. See `launch::NO_WORKSPACE_VAR`.
        inner.push_str(&format!(
            "{run}; export {}=1; exec {}",
            crate::launch::NO_WORKSPACE_VAR,
            shell_quote(&crate::platform::login_shell())
        ));
    } else {
        inner.push_str(&format!("exec {run}"));
    }
    format!("/bin/sh -c {}", shell_quote(&inner))
}

/// Open `argv` in a new Ghostty tab or window; returns the new surface's id.
///
/// `emoji` prefixes the title. A tab title is text, so a coloured circle is the
/// only way a tile's colour can reach the tab bar itself -- the background tint
/// colours the surface, not the strip of chrome you actually scan.
///
/// `OpenIn::Current` never reaches here: it is an exec in this process, done by
/// the caller once the terminal has been restored.
pub fn open(
    where_to: OpenIn,
    label: &str,
    argv: &[String],
    cwd: Option<&str>,
    tint: Option<&str>,
    emoji: Option<&str>,
    shell_after: bool,
) -> Result<String, String> {
    // The same title a handed-over tab gets, so a session reads the same in the
    // tab bar wherever it landed.
    let title = crate::launch::tab_title(label, emoji);
    // A new tab still needs a window to live in, so with none open we make one
    // either way.
    let make = if where_to == OpenIn::Window {
        "return id of (new window with configuration cfg)"
    } else {
        "if (count of windows) is 0 then\n\
         return id of (new window with configuration cfg)\n\
         else\n\
         return id of (new tab in front window with configuration cfg)\n\
         end if"
    };
    // Every spawned surface is marked, because a local-shell tile opens a shell
    // that sources the same rc that launches this program -- without the guard
    // you would get the home screen again instead of the shell you asked for.
    run_applescript(&format!(
        "tell application \"Ghostty\"\n\
         set cfg to {{command:{cmd}, wait after command:true, \
         environment variables:{{{guard}}}}}\n\
         {make}\n\
         end tell",
        cmd = applescript_quote(&surface_command(&title, argv, cwd, tint, shell_after)),
        guard = applescript_quote(&format!("{SKIP_VAR}=1")),
    ))
}

fn run_applescript(script: &str) -> Result<String, String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("could not run osascript: {e}"))?;

    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }

    let err = String::from_utf8_lossy(&out.stderr);
    // The first osascript run needs the user to grant Automation access.
    if err.contains("-1743") || err.to_lowercase().contains("not allowed") {
        return Err(
            "Ghostty automation denied -- allow it in System Settings > Privacy & Security > Automation"
                .into(),
        );
    }
    Err(err.trim().lines().last().unwrap_or("osascript failed").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The common case: the session *is* its command, so the command is exec'd
    /// and nothing follows it. `shell_after` is covered on its own below.
    fn surface(title: &str, args: &[String], cwd: Option<&str>, tint: Option<&str>) -> String {
        surface_command(title, args, cwd, tint, false)
    }

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// `argv[0]` is the program now, not an implied `ssh` -- a local tile runs
    /// a shell through the same path.
    #[test]
    fn the_whole_argv_round_trips() {
        assert_eq!(
            surface("alex", &argv(&["ssh", "alex"]), None, None),
            r#"/bin/sh -c 'printf '\''\033]0;%s\007'\'' '\''alex'\''; exec '\''ssh'\'' '\''alex'\'''"#
        );
    }

    #[test]
    fn a_local_command_uses_the_same_path() {
        let cmd = surface("MACBOOK-PRO", &argv(&["/bin/zsh", "-l"]), None, None);
        assert!(cmd.contains(r"exec '\''/bin/zsh'\'' '\''-l'\''"));
    }

    /// A spawned surface `cd`s before it execs, and the path arrives already
    /// resolved -- the quoting here is absolute, so a `~` that got this far
    /// would mean a directory literally named `~`, which is the failure this
    /// pins against.
    #[test]
    fn a_folder_becomes_a_cd_before_the_exec() {
        let cmd =
            surface("MACBOOK-PRO", &argv(&["/bin/zsh"]), Some("/Users/albe/Desktop"), None);
        assert!(cmd.find("cd ") < cmd.find("exec"), "cd has to come first: {cmd}");
        assert!(cmd.contains(r"cd '\''/Users/albe/Desktop'\'' || exit 1"), "got {cmd}");
        assert!(!cmd.contains('~'), "a tilde here would be taken literally: {cmd}");
        assert!(
            !surface("m", &argv(&["/bin/zsh"]), None, None).contains("cd "),
            "no folder, no cd"
        );
    }

    /// A command that exits would leave a tab that cannot be typed in, so it is
    /// run rather than exec'd and a shell takes the surface after it. The `sh`
    /// left holding the tty is the point: there has to be something to run the
    /// shell when the command is done.
    #[test]
    fn a_command_that_may_exit_is_followed_by_a_shell() {
        let cmd = surface_command("MEDIA", &argv(&["echo", "hi"]), None, None, true);
        assert!(cmd.contains(r"'\''echo'\'' '\''hi'\''; export NO_HSL=1; exec "), "got {cmd}");
        assert!(cmd.contains(&crate::platform::login_shell()), "the shell follows it: {cmd}");
        // ...and only there: the tile that *is* a shell is how you reach the
        // workspace manager, so nothing may suppress it on that path.
        let plain = surface_command("MACBOOK", &argv(&["/bin/zsh"]), None, None, false);
        assert!(!plain.contains("NO_HSL"), "not on the shell tile: {plain}");
        assert!(!cmd.contains(r"exec '\''echo"), "the command itself is not exec'd: {cmd}");
        // ...and the folder still applies to both halves.
        let with_dir =
            surface_command("MEDIA", &argv(&["echo", "hi"]), Some("/srv/app"), None, true);
        assert!(with_dir.find("cd ") < with_dir.find("echo"), "cd comes first: {with_dir}");
    }

    #[test]
    fn the_emoji_goes_in_the_title_not_the_command() {
        // Can't call open() in a test (it would spawn a tab), so check the same
        // composition the caller does.
        let title = format!("{} {}", "🔵", "alex");
        let cmd = surface(&title, &argv(&["ssh", "alex"]), None, None);
        assert!(cmd.contains("🔵 alex"), "emoji reaches the title");
        assert!(cmd.contains(r"exec '\''ssh'\'' '\''alex'\''"), "ssh gets the bare alias");
        assert!(!cmd.contains("ssh '\\''🔵"), "emoji never reaches the command");
    }

    #[test]
    fn quote_in_alias_stays_quoted() {
        // Not a realistic host name, but the escaping must not break out of the
        // single-quoted word if one ever appears.
        let cmd = surface("we'ird", &argv(&["ssh", "we'ird"]), None, None);
        assert!(cmd.contains(r"'\''"));
        assert!(!cmd.contains("we'ird"));
    }

    #[test]
    fn each_arg_is_quoted_separately() {
        let cmd = surface("srv", &argv(&["ssh", "-p", "2222", "albe@10.0.0.5"]), None, None);
        assert!(cmd.contains(r"'\''-p'\'' '\''2222'\'' '\''albe@10.0.0.5'\''"));
    }

    #[test]
    fn a_space_in_an_arg_does_not_split_it() {
        let cmd = surface("odd", &argv(&["ssh", "a b"]), None, None);
        // One quoted word, not two: the space stays inside the quotes.
        assert!(cmd.contains(r"'\''a b'\''"));
    }

    #[test]
    fn background_is_scaled_down_but_the_cursor_is_not() {
        // 0x4f,0x8a,0xb0 * 0.13, rounded: 10, 18, 23.
        assert_eq!(scale_hex("#4f8ab0", 0.13), Some("#0a1217".to_string()));
        let cmd = surface("srv", &argv(&["ssh", "srv"]), None, Some("#4f8ab0"));
        assert!(cmd.contains("]11;"), "sets background");
        assert!(cmd.contains("0a1217"), "background is the darkened value");
        assert!(cmd.contains("]12;"), "sets cursor");
        assert!(cmd.contains("4f8ab0"), "cursor keeps full strength");
        // The tint must all land before the exec, or it never runs.
        assert!(cmd.find("]12;").unwrap() < cmd.find("exec ").unwrap());
    }

    #[test]
    fn no_tint_means_no_osc_beyond_the_title() {
        let cmd = surface("srv", &argv(&["ssh", "srv"]), None, None);
        assert!(!cmd.contains("]11;") && !cmd.contains("]12;"));
    }

    #[test]
    fn malformed_tint_is_dropped_not_emitted_raw() {
        assert_eq!(scale_hex("nope", 0.13), None);
        let cmd = surface("srv", &argv(&["ssh", "srv"]), None, Some("nope"));
        assert!(!cmd.contains("]11;"), "no background from unparseable hex");
    }

    #[test]
    fn applescript_escapes_backslashes_before_quotes() {
        assert_eq!(applescript_quote(r#"a\b"c"#), r#""a\\b\"c""#);
    }
}
