//! Minimal `~/.ssh/config` reader.
//!
//! We only care about enough of the grammar to build a launcher: the `Host`
//! aliases you can actually connect to, plus the few fields worth showing on a
//! button. Wildcard patterns (`Host *`) are skipped -- they configure other
//! hosts rather than naming one.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Host {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<String>,
    pub proxy_jump: Option<String>,
}

impl Host {
    fn new(alias: String) -> Self {
        Self { alias, hostname: None, user: None, port: None, proxy_jump: None }
    }

}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn default_config_path() -> PathBuf {
    home().join(".ssh").join("config")
}

/// Parse `path` (following `Include` directives) into a de-duplicated host list,
/// preserving the order aliases appear in the file.
pub fn load(path: &Path) -> Vec<Host> {
    let mut hosts = Vec::new();
    parse_file(path, &mut hosts, 0);
    let mut seen = HashSet::new();
    hosts.retain(|h| seen.insert(h.alias.clone()));
    hosts
}

fn parse_file(path: &Path, out: &mut Vec<Host>, depth: usize) {
    // ssh itself caps Include nesting; any small bound also breaks include cycles.
    if depth > 8 {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else { return };

    // Indices into `out` for the Host block we are currently inside. `out` is
    // append-only, so these stay valid even when an Include pushes more hosts.
    let mut current: Vec<usize> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = split_kv(line) else { continue };
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                current.clear();
                for pat in value.split_whitespace() {
                    // `*`/`?` are patterns, `!` is a negation -- none name a host.
                    if pat.starts_with('!') || pat.contains('*') || pat.contains('?') {
                        continue;
                    }
                    current.push(out.len());
                    out.push(Host::new(unquote(pat).to_string()));
                }
            }
            // A Match block is conditional, so stop attributing keys to the
            // preceding Host.
            "match" => current.clear(),
            "include" => {
                for token in value.split_whitespace() {
                    for p in expand_include(unquote(token)) {
                        parse_file(&p, out, depth + 1);
                    }
                }
            }
            k @ ("hostname" | "user" | "port" | "proxyjump") => {
                let value = unquote(value);
                for &i in &current {
                    let h = &mut out[i];
                    let slot = match k {
                        "hostname" => &mut h.hostname,
                        "user" => &mut h.user,
                        "port" => &mut h.port,
                        _ => &mut h.proxy_jump,
                    };
                    // ssh takes the first value that wins; so do we.
                    if slot.is_none() {
                        *slot = Some(value.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Split `Key value` or `Key=value` (both are legal, with optional spaces).
fn split_kv(line: &str) -> Option<(&str, &str)> {
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    if end == 0 {
        return None;
    }
    let key = &line[..end];
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').map_or(rest, str::trim_start);
    let rest = rest.trim();
    if rest.is_empty() { None } else { Some((key, rest)) }
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
}

/// Include paths are relative to `~/.ssh` unless absolute, and may glob.
fn expand_include(token: &str) -> Vec<PathBuf> {
    let path = if let Some(rest) = token.strip_prefix("~/") {
        home().join(rest)
    } else {
        let p = PathBuf::from(token);
        if p.is_absolute() { p } else { home().join(".ssh").join(p) }
    };
    match glob::glob(&path.to_string_lossy()) {
        Ok(paths) => paths.flatten().filter(|p| p.is_file()).collect(),
        Err(_) => vec![path],
    }
}
