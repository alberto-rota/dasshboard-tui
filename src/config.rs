//! User config at `~/.config/dasshboard/config.toml`.
//!
//! Written by hand, or from the UI: `a` adds, `e` edits, `d` deletes, `s`
//! toggles options. Every UI edit rewrites whole blocks as *text* rather than
//! re-serialising the document, so comments and hand-formatting survive -- a
//! round trip of add, edit and delete leaves the file byte-identical.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::platform;

/// Where activating a tile puts the new session.
///
/// A request, not a promise: only a terminal that can be driven from outside
/// (Ghostty on macOS) can honour the first two, and `launch::Backend::resolve`
/// collapses them to `Current` everywhere else. They are still worth storing --
/// the same config.toml is read on more than one machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenIn {
    /// A new terminal tab.
    #[default]
    Tab,
    /// A new terminal window.
    Window,
    /// This terminal, replacing the home screen.
    Current,
}

impl OpenIn {
    pub const ALL: [OpenIn; 3] = [OpenIn::Tab, OpenIn::Window, OpenIn::Current];

    pub fn label(self) -> &'static str {
        match self {
            OpenIn::Tab => "tab",
            OpenIn::Window => "window",
            OpenIn::Current => "current",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|o| *o == self).unwrap_or(0)
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub theme: ThemeCfg,
    /// `[[host]]` blocks -- extra ssh targets.
    #[serde(default, rename = "host")]
    pub hosts: Vec<HostEntry>,
    /// `[[local]]` blocks -- commands that take over the current tab.
    #[serde(default, rename = "local")]
    pub locals: Vec<LocalEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// When false, only hosts defined in this file are shown.
    #[serde(default = "yes")]
    pub include_ssh_config: bool,
    /// Tint the spawned Ghostty tab's background with the host's colour.
    #[serde(default = "yes")]
    pub tint_tabs: bool,
    /// Prefix the Ghostty tab title with the host's coloured circle.
    #[serde(default = "yes")]
    pub tab_emoji: bool,
    /// Reveal hosts marked `hidden`, so they can be unhidden again.
    #[serde(default)]
    pub show_hidden: bool,
    /// Default destination for a tile that doesn't name its own.
    #[serde(default)]
    pub open_in: OpenIn,
}

/// `[theme]`. Absent values fall back to the built-in pair.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeCfg {
    pub primary: Option<String>,
    pub accent: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            include_ssh_config: true,
            tint_tabs: true,
            tab_emoji: true,
            show_hidden: false,
            open_in: OpenIn::Tab,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostEntry {
    pub name: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub jump: Option<String>,
    /// `"#rrggbb"`. Omitted means a stable colour picked from the palette.
    pub color: Option<String>,
    /// Keep this host off the home screen. Useful for a bastion you only ever
    /// jump through and never open a shell on.
    #[serde(default)]
    pub hidden: bool,
    /// Overrides `[options] open_in` for this host.
    pub open_in: Option<OpenIn>,
    /// Directories to offer on connect. More than one turns activating the
    /// tile into a small picker.
    #[serde(default)]
    pub folders: Vec<String>,
}

/// Which array-of-tables a draft belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Block {
    #[default]
    Host,
    Local,
}

impl Block {
    fn header(self) -> &'static str {
        match self {
            Block::Host => "[[host]]",
            Block::Local => "[[local]]",
        }
    }

    /// The key that identifies a block of this kind.
    fn id_key(self) -> &'static str {
        match self {
            Block::Host => "name",
            Block::Local => "label",
        }
    }
}

/// A block on its way to disk. A struct rather than a dozen positional
/// strings, because most of them are interchangeable types.
#[derive(Debug, Default, Clone)]
pub struct Draft {
    pub block: Block,
    /// `name` for a host, `label` for a local.
    pub name: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub jump: String,
    pub command: String,
    pub detail: String,
    pub folders: Vec<String>,
    pub color: String,
    pub hidden: bool,
    pub open_in: Option<OpenIn>,
}

impl Draft {
    pub fn host(name: &str) -> Self {
        Self { block: Block::Host, name: name.to_string(), ..Default::default() }
    }
}

impl From<&HostEntry> for Draft {
    fn from(h: &HostEntry) -> Self {
        Self {
            block: Block::Host,
            name: h.name.clone(),
            hostname: h.hostname.clone().unwrap_or_default(),
            user: h.user.clone().unwrap_or_default(),
            port: h.port.map(|p| p.to_string()).unwrap_or_default(),
            jump: h.jump.clone().unwrap_or_default(),
            folders: h.folders.clone(),
            color: h.color.clone().unwrap_or_default(),
            hidden: h.hidden,
            open_in: h.open_in,
            ..Default::default()
        }
    }
}

impl From<&LocalEntry> for Draft {
    fn from(l: &LocalEntry) -> Self {
        Self {
            block: Block::Local,
            name: l.label.clone(),
            command: l.command.clone(),
            detail: l.detail.clone(),
            folders: l.folders.clone(),
            color: l.color.clone().unwrap_or_default(),
            hidden: l.hidden,
            open_in: l.open_in,
            ..Default::default()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEntry {
    pub label: String,
    #[serde(default)]
    pub detail: String,
    pub command: String,
    pub color: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    pub open_in: Option<OpenIn>,
    /// Directories to start the command in.
    #[serde(default)]
    pub folders: Vec<String>,
}

pub fn dir() -> PathBuf {
    config_base().join("dasshboard")
}

/// `$XDG_CONFIG_HOME` if it is set -- it is the explicit answer, and people who
/// set it mean it. Then `%APPDATA%` on Windows, which is where a Windows
/// program is expected to keep its settings, and `~/.config` everywhere else.
fn config_base() -> PathBuf {
    if let Some(v) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(v);
    }
    if cfg!(windows) {
        if let Some(v) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
            return PathBuf::from(v);
        }
    }
    home().join(".config")
}

/// Move a config written under the old name, once, so the rename doesn't look
/// like a factory reset.
fn migrate_old_config() {
    let old = config_base().join("dasshboard");
    let new = dir();
    if new.exists() || !old.exists() {
        return;
    }
    let _ = fs::rename(&old, &new);
}

pub fn path() -> PathBuf {
    dir().join("config.toml")
}

/// The workspace manager, if this machine has one: `hsl` (herdr plus its status
/// line) before plain `herdr`, the same order and for the same reason as the
/// dotfiles' own `hsl-login.sh`.
///
/// The local tile is what you press to get to it once the home screen owns the
/// terminal-open slot, so shipping it pointed at a bare shell would quietly
/// cost you the thing that used to open with a terminal. `~/.local/bin` is
/// searched even when PATH has not caught up with it yet, which is exactly the
/// state a first run is in.
fn workspace_manager() -> Option<(String, PathBuf)> {
    for name in ["hsl", "herdr"] {
        let local_bin = home().join(".local/bin").join(name);
        let found =
            platform::which(name).or_else(|| platform::is_executable(&local_bin).then_some(local_bin));
        if let Some(p) = found {
            return Some((name.to_string(), p));
        }
    }
    None
}

fn home() -> PathBuf {
    platform::home()
}

/// Load the config, writing a starter file the first time. A parse error is
/// returned alongside an empty config rather than replacing it -- a typo in the
/// file must never cost you the home screen.
pub fn load() -> (Config, Option<String>) {
    migrate_old_config();
    let path = path();
    if !path.exists() {
        if let Err(e) = write_template(&path) {
            return (Config::default(), Some(format!("could not create {}: {e}", path.display())));
        }
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return (Config::default(), Some(format!("could not read config: {e}"))),
    };
    match toml::from_str::<Config>(&text) {
        Ok(cfg) => (cfg, None),
        Err(e) => {
            let msg = e.message().lines().next().unwrap_or("invalid TOML").to_string();
            (Config::default(), Some(format!("config.toml: {msg}")))
        }
    }
}

/// The first-run file. The detected workspace manager is written in as a real
/// block so the shipped tile and a hand-written one are the same mechanism.
fn write_template(path: &PathBuf) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().unwrap_or(&dir()))?;

    let mut s = String::from(
        "# dasshboard\n\
         #\n\
         # Tiles on your home screen, shown alongside the hosts read from\n\
         # ~/.ssh/config. Everything here is editable from the UI -- `a` add,\n\
         # `e` edit, `d` delete, `x` hide, `s` settings -- or by hand.\n\
         \n\
         [options]\n\
         # Set to false to show only the hosts defined below.\n\
         include_ssh_config = true\n\
         # Tint each spawned Ghostty tab's background with its host's colour.\n\
         tint_tabs = true\n\
         # Where a tile opens: \"tab\", \"window\" or \"current\".\n\
         open_in = \"tab\"\n\
         # Prefix the tab title with the host's coloured circle.\n\
         tab_emoji = true\n\
         \n",
    );

    // A tile for this machine. It runs the workspace manager where there is
    // one, because that is what used to open with a terminal, and a plain login
    // shell where there is not. `current` either way: a local command takes
    // over this tab rather than opening another one.
    let (detail, command) = match workspace_manager() {
        Some((name, path)) => (format!("{name} · workspace manager"), path.display().to_string()),
        None => ("local shell".to_string(), platform::login_shell()),
    };
    s.push_str(&format!(
        "# Local tiles run a command on this machine. `open_in` decides where:\n\
         # a new tab, a new window, or \"current\" to take over this one.\n\
         # Outside Ghostty on macOS every tile opens in the current terminal.\n\
         [[local]]\nlabel = {:?}\ndetail = {detail:?}\ncommand = {command:?}\n\
         open_in = \"current\"\n\n",
        platform::machine_name(),
    ));

    s.push_str(
        "# Extra ssh hosts. Only `name` is required -- with nothing else it is\n\
         # passed to ssh as-is, so anything ssh already resolves just works.\n\
         #\n\
         # `color` sets the tile dot and the tab tint. Leave it out and one is\n\
         # picked from the palette by name, the same on every run. `hidden`\n\
         # keeps a host off the screen -- handy for a jump host you never open\n\
         # a shell on. Turn on show_hidden to see and unhide them.\n\
         #\n\
         # [[host]]\n\
         # name = \"myserver\"\n\
         # hostname = \"10.0.0.5\"\n\
         # user = \"albe\"\n\
         # port = 22\n\
         # jump = \"bastion\"\n\
         # color = \"#4f8ab0\"\n\
         # hidden = false\n\
         # open_in = \"window\"\n\
         \n\
         # Your two colours; everything else on screen is derived from them.\n\
         [theme]\n\
         primary = \"#aaaaaa\"\n\
         accent = \"#ff0000\"\n",
    );

    fs::write(path, s)
}

/// Append a `[[host]]` block. Appending (rather than rewriting) is what keeps
/// the rest of the file byte-identical.
/// Write a host block: update the one named `original` if it exists, otherwise
/// append. Returns true when an existing block was rewritten.
///
/// The fallback is what lets a host from ~/.ssh/config be customised -- the
/// first edit has no block to update yet.
pub fn save_block(original: Option<&str>, d: &Draft) -> std::io::Result<bool> {
    save_block_at(&path(), original, d)
}

fn save_block_at(path: &Path, original: Option<&str>, d: &Draft) -> std::io::Result<bool> {
    if let Some(o) = original {
        if update_block_at(path, o, d)? {
            return Ok(true);
        }
    }
    append_block_at(path, d).map(|()| false)
}

fn append_block_at(path: &Path, d: &Draft) -> std::io::Result<()> {
    let mut text = fs::read_to_string(path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("\n{}\n", d.block.header()));
    text.push_str(&block_body(d));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

/// The body of a block, as TOML lines without the header. Empty fields are
/// omitted rather than written blank, so they keep inheriting.
fn block_body(d: &Draft) -> String {
    let mut s = format!("{} = {:?}\n", d.block.id_key(), d.name);
    let pairs: &[(&str, &String)] = match d.block {
        Block::Host => &[
            ("hostname", &d.hostname),
            ("user", &d.user),
            ("jump", &d.jump),
            ("color", &d.color),
        ],
        Block::Local => &[("command", &d.command), ("detail", &d.detail), ("color", &d.color)],
    };
    for (key, value) in pairs {
        if !value.trim().is_empty() {
            s.push_str(&format!("{key} = {:?}\n", value.trim()));
        }
    }
    if d.block == Block::Host {
        if let Ok(p) = d.port.trim().parse::<u16>() {
            s.push_str(&format!("port = {p}\n"));
        }
    }
    let folders: Vec<String> = d
        .folders
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .map(|f| format!("{f:?}"))
        .collect();
    if !folders.is_empty() {
        s.push_str(&format!("folders = [{}]\n", folders.join(", ")));
    }
    if d.hidden {
        s.push_str("hidden = true\n");
    }
    if let Some(o) = d.open_in {
        s.push_str(&format!("open_in = {:?}\n", o.label()));
    }
    s
}

/// Block boundaries are top-level headers; a block runs to the next one.
fn is_header(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('[') && !t.starts_with('#')
}

/// Find the block of this kind whose id key matches: returns (header, end) line
/// indices, `end` exclusive.
fn find_block(lines: &[&str], block: Block, name: &str) -> Option<(usize, usize)> {
    let target = format!("{} = {name:?}", block.id_key());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == block.header() {
            let mut end = i + 1;
            while end < lines.len() && !is_header(lines[end]) {
                end += 1;
            }
            if lines[i + 1..end].iter().any(|l| l.trim() == target) {
                return Some((i, end));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    None
}

/// Rewrite one `[[host]]` block in place, keeping its position in the file (and
/// therefore its position on screen) and every other line untouched.
#[allow(clippy::too_many_arguments)]
fn update_block_at(path: &Path, original: &str, d: &Draft) -> std::io::Result<bool> {
    let text = fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    let Some((start, end)) = find_block(&lines, d.block, original) else { return Ok(false) };

    // Trailing blanks belong to the separation between blocks, not the block.
    let mut body_end = end;
    while body_end > start + 1 && lines[body_end - 1].trim().is_empty() {
        body_end -= 1;
    }

    let mut out: Vec<String> = lines[..=start].iter().map(|s| s.to_string()).collect();
    out.extend(block_body(d).lines().map(String::from));
    out.extend(lines[body_end..].iter().map(|s| s.to_string()));

    let mut joined = out.join("\n");
    joined.push('\n');
    fs::write(path, joined)?;
    Ok(true)
}

/// Remove the `[[host]]` block whose `name` matches, leaving every other line
/// alone. Returns whether anything was removed.
pub fn remove_block(block: Block, name: &str) -> std::io::Result<bool> {
    remove_block_at(&path(), block, name)
}

fn remove_block_at(path: &Path, block: Block, name: &str) -> std::io::Result<bool> {
    let text = fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    let Some((start, end)) = find_block(&lines, block, name) else { return Ok(false) };

    let mut out: Vec<&str> = lines[..start].to_vec();
    // Also drop the blank line the block was separated by, so repeated
    // add/delete cycles don't accumulate gaps.
    if out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.extend_from_slice(&lines[end..]);

    let mut joined = out.join("\n");
    joined.push('\n');
    fs::write(path, joined)?;
    Ok(true)
}

pub fn set_option(key: &str, value: bool) -> std::io::Result<()> {
    set_kv_at(&path(), "options", key, &value.to_string())
}

pub fn set_option_str(key: &str, value: &str) -> std::io::Result<()> {
    set_kv_at(&path(), "options", key, &format!("{value:?}"))
}

pub fn set_theme(key: &str, value: &str) -> std::io::Result<()> {
    set_kv_at(&path(), "theme", key, &format!("{value:?}"))
}

/// Set `key = literal` inside `[table]`, creating either if absent.
///
/// Scoped to the table on purpose: `primary` in `[theme]` must not be found
/// and rewritten because some other table happens to use the same key name.
fn set_kv_at(path: &Path, table: &str, key: &str, literal: &str) -> std::io::Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let header = format!("[{table}]");

    let start = lines.iter().position(|l| l.trim() == header);
    let Some(start) = start else {
        if !lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(format!("{key} = {literal}"));
        return write_lines(path, lines);
    };

    let end = lines[start + 1..]
        .iter()
        .position(|l| is_header(l))
        .map_or(lines.len(), |i| start + 1 + i);

    let existing = lines[start + 1..end].iter().position(|l| {
        let t = l.trim_start();
        !t.starts_with('#')
            && t.strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('='))
    });
    match existing {
        Some(i) => lines[start + 1 + i] = format!("{key} = {literal}"),
        None => lines.insert(start + 1, format!("{key} = {literal}")),
    }
    write_lines(path, lines)
}

fn write_lines(path: &Path, lines: Vec<String>) -> std::io::Result<()> {
    let mut joined = lines.join("\n");
    joined.push('\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch file per test; the real config is never touched.
    fn scratch(name: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("termhome-test-{name}.toml"));
        fs::write(&p, body).unwrap();
        p
    }

    fn parse(p: &PathBuf) -> Config {
        toml::from_str(&fs::read_to_string(p).unwrap()).unwrap()
    }

    fn draft(name: &str) -> Draft {
        Draft::host(name)
    }

    const SEED: &str = "# a comment someone wrote\n\
                        [options]\n\
                        include_ssh_config = true\n\
                        \n\
                        # keep me too\n\
                        [[local]]\n\
                        label = \"herdr\"\n\
                        command = \"hsl\"\n";

    #[test]
    fn add_then_edit_then_delete_leaves_the_file_as_it_started() {
        let p = scratch("roundtrip", SEED);
        let full = Draft {
            block: Block::Host,
            name: "srv".into(),
            hostname: "10.0.0.5".into(),
            user: "albe".into(),
            port: "2222".into(),
            jump: "bastion".into(),
            color: "#4f8ab0".into(),
            hidden: false,
            open_in: None,
            ..Default::default()
        };
        assert!(!save_block_at(&p, None, &full).unwrap(), "appended, not updated");

        let cfg = parse(&p);
        let h = cfg.hosts.iter().find(|h| h.name == "srv").unwrap();
        assert_eq!(h.hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(h.port, Some(2222));
        assert_eq!(h.color.as_deref(), Some("#4f8ab0"));

        // Renaming and clearing optional fields must both stick.
        let mut renamed = draft("srv2");
        renamed.hostname = "10.0.0.9".into();
        assert!(save_block_at(&p, Some("srv"), &renamed).unwrap(), "updated in place");
        let cfg = parse(&p);
        let h = cfg.hosts.iter().find(|h| h.name == "srv2").unwrap();
        assert_eq!(h.hostname.as_deref(), Some("10.0.0.9"));
        assert_eq!(h.user, None, "cleared field must be dropped, not kept");
        assert_eq!(h.port, None);
        assert_eq!(h.color, None);

        assert!(remove_block_at(&p, Block::Host, "srv2").unwrap());
        assert_eq!(fs::read_to_string(&p).unwrap(), SEED, "back to byte-identical");
    }

    /// Hiding a host that has no block yet must create one, and hiding is the
    /// only thing that block should say.
    #[test]
    fn hiding_a_bare_ssh_config_host_writes_a_minimal_block() {
        let p = scratch("hide", SEED);
        let mut d = draft("csnhr");
        d.hidden = true;
        assert!(!save_block_at(&p, Some("csnhr"), &d).unwrap(), "no block existed, so appended");

        let cfg = parse(&p);
        let h = cfg.hosts.iter().find(|h| h.name == "csnhr").unwrap();
        assert!(h.hidden);
        assert_eq!(h.hostname, None, "hiding must not pin the connection details");
        assert_eq!(h.user, None);
        assert_eq!(h.color, None);

        // And unhiding takes the block back out to nothing but the name.
        d.hidden = false;
        assert!(save_block_at(&p, Some("csnhr"), &d).unwrap(), "now updates in place");
        let text = fs::read_to_string(&p).unwrap();
        assert!(!text.contains("hidden"), "the flag is dropped, not written false");
    }

    #[test]
    fn hiding_preserves_the_rest_of_an_existing_block() {
        let p = scratch("hide-keep", SEED);
        let mut d = draft("srv");
        d.color = "#4f8ab0".into();
        d.user = "albe".into();
        save_block_at(&p, None, &d).unwrap();

        // What the `x` key does: reload the block, flip one bit, write it back.
        let existing = parse(&p).hosts.into_iter().find(|h| h.name == "srv").unwrap();
        let mut back = Draft::from(&existing);
        back.hidden = true;
        save_block_at(&p, Some("srv"), &back).unwrap();

        let h = parse(&p).hosts.into_iter().find(|h| h.name == "srv").unwrap();
        assert!(h.hidden);
        assert_eq!(h.color.as_deref(), Some("#4f8ab0"), "colour survives hiding");
        assert_eq!(h.user.as_deref(), Some("albe"));
    }

    #[test]
    fn other_blocks_and_comments_survive_an_edit() {
        let p = scratch("preserve", SEED);
        save_block_at(&p, None, &draft("a")).unwrap();
        save_block_at(&p, None, &draft("b")).unwrap();
        let mut changed = draft("a");
        changed.hostname = "changed".into();
        save_block_at(&p, Some("a"), &changed).unwrap();

        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("# a comment someone wrote"));
        assert!(text.contains("# keep me too"));
        assert!(text.contains("[[local]]"));
        let cfg = parse(&p);
        assert_eq!(cfg.hosts.len(), 2);
        assert_eq!(cfg.hosts[0].name, "a");
        assert_eq!(cfg.hosts[1].name, "b");
    }

    #[test]
    fn editing_keeps_a_host_in_place_rather_than_moving_it_to_the_end() {
        let p = scratch("order", SEED);
        for n in ["one", "two", "three"] {
            save_block_at(&p, None, &draft(n)).unwrap();
        }
        let mut d = draft("one");
        d.hostname = "h".into();
        save_block_at(&p, Some("one"), &d).unwrap();
        let names: Vec<String> = parse(&p).hosts.into_iter().map(|h| h.name).collect();
        assert_eq!(names, ["one", "two", "three"], "tile order must not shuffle");
    }

    #[test]
    fn missing_targets_are_reported_not_invented() {
        let p = scratch("missing", SEED);
        assert!(!update_block_at(&p, "ghost", &draft("ghost")).unwrap());
        assert!(!remove_block_at(&p, Block::Host, "ghost").unwrap());
        assert_eq!(fs::read_to_string(&p).unwrap(), SEED);
    }

    #[test]
    fn toggling_an_option_rewrites_it_and_adds_a_missing_one() {
        let p = scratch("options", SEED);
        set_kv_at(&p, "options", "include_ssh_config", "false").unwrap();
        set_kv_at(&p, "options", "show_hidden", "true").unwrap();
        let cfg = parse(&p);
        assert!(!cfg.options.include_ssh_config, "existing key rewritten");
        assert!(cfg.options.show_hidden, "absent key added");
        assert!(cfg.options.tint_tabs, "untouched key keeps its default");
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text.matches("include_ssh_config").count(), 1, "written once");
    }

    /// `[theme]` gets its own table, and a key name that also exists elsewhere
    /// must not be rewritten in the wrong one.
    #[test]
    fn theme_and_options_are_written_to_separate_tables() {
        let p = scratch("tables", SEED);
        set_kv_at(&p, "theme", "primary", "\"#7fd1b9\"").unwrap();
        set_kv_at(&p, "theme", "accent", "\"#ffb703\"").unwrap();
        set_kv_at(&p, "options", "open_in", "\"window\"").unwrap();

        let cfg = parse(&p);
        assert_eq!(cfg.theme.primary.as_deref(), Some("#7fd1b9"));
        assert_eq!(cfg.theme.accent.as_deref(), Some("#ffb703"));
        assert_eq!(cfg.options.open_in, OpenIn::Window);

        // Setting the same key twice replaces rather than duplicating.
        set_kv_at(&p, "theme", "primary", "\"#112233\"").unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text.matches("primary").count(), 1);
        assert_eq!(parse(&p).theme.primary.as_deref(), Some("#112233"));
    }

    #[test]
    fn a_per_host_open_in_round_trips() {
        let p = scratch("openin", SEED);
        let mut d = draft("srv");
        d.open_in = Some(OpenIn::Current);
        save_block_at(&p, None, &d).unwrap();
        assert_eq!(parse(&p).hosts[0].open_in, Some(OpenIn::Current));
    }

    /// Local tiles go through the same writer, into their own array.
    #[test]
    fn a_local_block_round_trips_with_its_folders() {
        let p = scratch("local", SEED);
        let d = Draft {
            block: Block::Local,
            name: "MACBOOK".into(),
            command: "/bin/zsh".into(),
            detail: "local shell".into(),
            folders: vec!["~/code/one".into(), "~/code/two".into()],
            ..Default::default()
        };
        assert!(!save_block_at(&p, None, &d).unwrap());

        let cfg = parse(&p);
        let l = cfg.locals.iter().find(|l| l.label == "MACBOOK").unwrap();
        assert_eq!(l.command, "/bin/zsh");
        assert_eq!(l.folders, ["~/code/one", "~/code/two"]);

        // The seeded [[local]] must be untouched, and removal must find the
        // right one of the two.
        assert_eq!(cfg.locals.len(), 2);
        assert!(remove_block_at(&p, Block::Local, "MACBOOK").unwrap());
        assert_eq!(parse(&p).locals.len(), 1);
    }

    #[test]
    fn folders_round_trip_on_a_host() {
        let p = scratch("folders", SEED);
        let mut d = draft("srv");
        d.folders = vec!["/srv/app".into(), "/var/log".into()];
        save_block_at(&p, None, &d).unwrap();
        assert_eq!(parse(&p).hosts[0].folders, ["/srv/app", "/var/log"]);

        // Clearing them drops the key rather than writing an empty array.
        d.folders.clear();
        save_block_at(&p, Some("srv"), &d).unwrap();
        assert!(!fs::read_to_string(&p).unwrap().contains("folders"));
    }

    /// A host and a local may share a name without colliding, because the
    /// writer keys on the block's own id field.
    #[test]
    fn blocks_of_different_kinds_do_not_collide() {
        let p = scratch("kinds", SEED);
        save_block_at(&p, None, &draft("twin")).unwrap();
        let local = Draft {
            block: Block::Local,
            name: "twin".into(),
            command: "/bin/sh".into(),
            ..Default::default()
        };
        save_block_at(&p, None, &local).unwrap();
        assert!(remove_block_at(&p, Block::Local, "twin").unwrap());

        let cfg = parse(&p);
        assert!(cfg.hosts.iter().any(|h| h.name == "twin"), "the host survives");
        assert!(!cfg.locals.iter().any(|l| l.label == "twin"), "the local is gone");
    }

    #[test]
    fn a_quote_in_a_value_cannot_break_the_toml() {
        let p = scratch("quoting", SEED);
        let mut d = draft("odd");
        d.hostname = "a\"b\\c".into();
        save_block_at(&p, None, &d).unwrap();
        assert_eq!(parse(&p).hosts[0].hostname.as_deref(), Some("a\"b\\c"));
    }
}
