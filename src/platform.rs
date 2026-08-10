//! The handful of things that are not the same on every operating system.
//!
//! Everything else in dasshboard is portable, so this is where "where is home",
//! "what counts as executable" and "what is this machine called" get answered
//! once instead of in five places with five different Unix assumptions.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The user's home directory.
///
/// `HOME` first, because it is what a POSIX shell sets and what Git Bash / WSL
/// set on Windows too. `USERPROFILE` is the native Windows answer, and the
/// `HOMEDRIVE`+`HOMEPATH` pair is the older one, still set on domain machines.
pub fn home() -> PathBuf {
    for var in ["HOME", "USERPROFILE"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            return PathBuf::from(v);
        }
    }
    match (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
        // Concatenated as text, not `join`ed: HOMEPATH is rooted (`\Users\me`),
        // and pushing a rooted path replaces the drive instead of following it.
        (Ok(d), Ok(p)) if !d.is_empty() && !p.is_empty() => PathBuf::from(format!("{d}{p}")),
        // Deliberately a path that will simply not exist, rather than `.`: a
        // relative fallback would read `./.ssh/config` out of whatever
        // directory you happened to start in.
        _ => PathBuf::from("/"),
    }
}

/// Resolve a leading `~` against the home directory, for a path this machine
/// will read. Only a *leading* tilde, and only `~` or `~/...`: mid-path tildes
/// are literal in a shell too, and `~user` needs a passwd lookup we would only
/// get half right.
pub fn expand_home(dir: &str) -> String {
    let Some(rest) = dir.strip_prefix('~') else { return dir.to_string() };
    if !(rest.is_empty() || rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\'))) {
        return dir.to_string();
    }
    match home().to_str() {
        Some(h) if !h.is_empty() => format!("{}{rest}", h.trim_end_matches(['/', '\\'])),
        _ => dir.to_string(),
    }
}

/// This machine's short name, for the local tile's label. Falls back rather
/// than failing: a launcher must still come up somewhere `hostname` is missing.
pub fn machine_name() -> String {
    // Windows has no `hostname -s`, but it does set COMPUTERNAME, which is
    // already the short form.
    if cfg!(windows) {
        if let Ok(n) = std::env::var("COMPUTERNAME") {
            if !n.is_empty() {
                return n;
            }
        }
    }
    let short: &[&str] = if cfg!(windows) { &[] } else { &["-s"] };
    // `-s` trims the domain; the bare call covers the systems whose hostname(1)
    // does not take it.
    output("hostname", short)
        .or_else(|| output("hostname", &[]))
        .unwrap_or_else(|| "local".into())
}

/// The command a "shell on this machine" tile should run.
pub fn login_shell() -> String {
    if let Ok(s) = std::env::var("SHELL") {
        if !s.is_empty() {
            return s;
        }
    }
    if cfg!(windows) {
        // PowerShell if it is there, since that is what a Windows terminal
        // profile opens; COMSPEC (cmd.exe) is the one that always exists.
        // Recorded by name rather than by path: both live under `Program
        // Files`, and a bare name needs no quoting wherever it is written down.
        for name in ["pwsh", "powershell"] {
            if which(name).is_some() {
                return name.into();
            }
        }
        return std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
    }
    "/bin/sh".into()
}

/// Whether the file at `p` is something this system would run.
#[cfg(unix)]
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Windows has no execute bit; the extension is what decides, and `which` only
/// ever hands us candidates that already carry one.
#[cfg(not(unix))]
pub fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// The first executable named `name` on `PATH`.
///
/// `split_paths` rather than a split on `:`, because Windows separates PATH
/// with `;` -- and on Windows the name alone is not a program, so every
/// extension in PATHEXT is tried against each directory.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.as_os_str().is_empty())
        .flat_map(|d| candidates(&d, name))
        .find(|p| is_executable(p))
}

fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    let plain = dir.join(name);
    if !cfg!(windows) || name.contains('.') {
        return vec![plain];
    }
    let pathext = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    std::iter::once(plain)
        .chain(
            pathext
                .split(';')
                .filter(|e| !e.is_empty())
                .map(|ext| dir.join(format!("{name}{ext}"))),
        )
        .collect()
}

/// Trimmed stdout of a command that succeeded, if it did.
fn output(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launcher has to come up even with nothing useful in the
    /// environment, so every one of these has a last resort rather than an
    /// unwrap.
    #[test]
    fn every_lookup_has_a_fallback() {
        assert!(!machine_name().is_empty());
        assert!(!login_shell().is_empty());
        assert!(!home().as_os_str().is_empty());
    }

    #[test]
    fn a_leading_tilde_is_the_only_one_expanded() {
        let h = home().display().to_string();
        assert_eq!(expand_home("~"), h);
        assert_eq!(expand_home("~/code"), format!("{h}/code"));
        assert_eq!(expand_home("/tmp/x"), "/tmp/x");
        assert_eq!(expand_home("~notauser/x"), "~notauser/x", "~user is not ours to guess at");
        assert_eq!(expand_home("/a/~/b"), "/a/~/b", "mid-path tildes are literal");
    }

    /// Whatever else it finds, `which` must not claim a directory is a program.
    #[test]
    fn which_finds_a_program_and_not_a_directory() {
        let found = which(if cfg!(windows) { "cmd" } else { "sh" });
        if let Some(p) = found {
            assert!(p.is_file(), "{} is not a file", p.display());
        }
        assert!(which("definitely-not-a-program-9c2f").is_none());
    }
}
