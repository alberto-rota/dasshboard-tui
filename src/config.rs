//! User config at `~/.config/dasshboard/config.toml`.
//!
//! Written by hand, or from the UI: `a` adds, `e` edits, `d` duplicates, `D`
//! deletes, `s` toggles options. Every UI edit rewrites whole blocks as *text* rather than
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
    /// `[[section]]` blocks -- the order tiles are drawn in, and the groups
    /// they are drawn under.
    #[serde(default, rename = "section")]
    pub sections: Vec<SectionEntry>,
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
    /// Prefix the tab title with the host's coloured circle.
    #[serde(default = "yes")]
    pub tab_emoji: bool,
    /// Whether hidden tiles are on screen when the home screen opens. `X`
    /// flips it for the session; this is only the state it starts in, which is
    /// why nothing writes it back -- revealing hidden tiles is a look, not a
    /// setting, and a peek should not change the file.
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
    /// Off the home screen until you ask for it. For the host you keep but
    /// rarely open -- a bastion you jump through, a box you touch twice a year.
    /// It is still a tile, still in its group, still one keystroke from being
    /// back; `X` shows every hidden tile and `x` puts this one back.
    #[serde(default)]
    pub hidden: bool,
    /// This tile was deleted. Only ever written for a host that comes from
    /// `~/.ssh/config`: that file is ssh's and is never rewritten, so the only
    /// place we can record the deletion is here. The host itself is untouched
    /// and still works as a `ProxyJump` target -- only the tile is gone.
    ///
    /// Unlike `hidden`, this is not a state a tile can be in and come back from
    /// on the board: nothing draws it, nothing counts it, and `a` with the same
    /// name is what undoes it.
    #[serde(default)]
    pub deleted: bool,
    /// Overrides `[options] open_in` for this host.
    pub open_in: Option<OpenIn>,
    /// The one directory this tile starts in. Absent is the default: wherever a
    /// login shell lands, which is home.
    pub folder: Option<String>,
    /// What to run there, on the far side. Absent is the default: a login shell.
    pub command: Option<String>,
    /// The old `folders = [...]`, read so a config written before tiles had one
    /// folder each still loads. The first becomes `folder`; `y` on a tile is how
    /// you get the rest back.
    #[serde(default, rename = "folders")]
    pub legacy_folders: Vec<String>,
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
    /// One directory, or empty for the default.
    pub folder: String,
    pub color: String,
    /// Off the screen until `X`. Kept through every other edit, which is what
    /// stops `e` on a hidden tile from quietly bringing it back.
    pub hidden: bool,
    /// Written as `deleted = true`, and only for a host from `~/.ssh/config`:
    /// every other tile is a block of ours, and deleting one takes the block.
    pub deleted: bool,
    pub open_in: Option<OpenIn>,
}

impl Draft {
    pub fn host(name: &str) -> Self {
        Self { block: Block::Host, name: name.to_string(), ..Default::default() }
    }

    /// Nothing but the name and the flag: deleting a host from `~/.ssh/config`
    /// must not pin its connection details on the way out.
    pub fn deleted_host(name: &str) -> Self {
        Self { deleted: true, ..Draft::host(name) }
    }

    /// The same tile under a new name -- what `y` writes.
    pub fn renamed(&self, name: &str) -> Self {
        Self { name: name.to_string(), ..self.clone() }
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
            command: h.command.clone().unwrap_or_default(),
            folder: h.folder.clone().unwrap_or_default(),
            color: h.color.clone().unwrap_or_default(),
            hidden: h.hidden,
            deleted: h.deleted,
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
            command: l.command.clone().unwrap_or_default(),
            detail: l.detail.clone(),
            folder: l.folder.clone().unwrap_or_default(),
            color: l.color.clone().unwrap_or_default(),
            hidden: l.hidden,
            deleted: l.deleted,
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
    /// Absent is the default: this machine's login shell.
    pub command: Option<String>,
    pub color: Option<String>,
    /// See `HostEntry::hidden`.
    #[serde(default)]
    pub hidden: bool,
    /// See `HostEntry::deleted`. A local tile is only ever ours, so `d` takes
    /// its block out entirely and this is never written -- it is read only so
    /// that a hand-written `deleted = true` does what it says.
    #[serde(default)]
    pub deleted: bool,
    pub open_in: Option<OpenIn>,
    /// The one directory the command starts in. Absent means home.
    pub folder: Option<String>,
    /// See `HostEntry::legacy_folders`.
    #[serde(default, rename = "folders")]
    pub legacy_folders: Vec<String>,
}

/// A `[[section]]` block: a titled group of tiles, in the order they appear on
/// screen.
///
/// Membership is by name rather than a key on the tile itself, for two reasons.
/// A host from `~/.ssh/config` can then be placed and reordered without being
/// given a `[[host]]` block of its own -- the same reason hiding one is allowed
/// to create a block, except here no block is needed at all. And one list per
/// group reads, and rewrites, far better than an `order` number scattered
/// across a dozen blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionEntry {
    /// Drawn above the group. Empty means an untitled group -- which is what
    /// every tile is in before anyone makes a section, so the screen looks the
    /// same until one exists.
    #[serde(default)]
    pub title: String,
    /// The `name`s and `label`s in this group, in order.
    #[serde(default)]
    pub items: Vec<String>,
}

/// Spread `labels` over the configured sections, so that what comes back is a
/// complete arrangement rather than a partial one: every label appears exactly
/// once, a name no longer on screen is dropped, and anything unplaced -- a host
/// that turned up in `~/.ssh/config` since the sections were written -- lands
/// in a trailing untitled group.
///
/// This is the shape the UI moves tiles around in and writes back, which is why
/// it has to be total: a layout that omitted a tile would lose it the moment
/// anything else moved.
pub fn layout(sections: &[SectionEntry], labels: &[String]) -> Vec<SectionEntry> {
    let mut placed: Vec<&String> = Vec::new();
    let mut out: Vec<SectionEntry> = Vec::new();
    for s in sections {
        let mut items = Vec::new();
        for name in &s.items {
            if labels.contains(name) && !placed.contains(&name) {
                placed.push(name);
                items.push(name.clone());
            }
        }
        out.push(SectionEntry { title: s.title.clone(), items });
    }

    let rest: Vec<String> =
        labels.iter().filter(|l| !placed.contains(l)).cloned().collect();
    if !rest.is_empty() {
        // Appended to the last group when it is already untitled, rather than
        // opening a second anonymous one next to it -- they would draw as one
        // run of tiles anyway, and two is a distinction with no picture.
        match out.last_mut().filter(|s| s.title.is_empty()) {
            Some(s) => s.items.extend(rest),
            None => out.push(SectionEntry { title: String::new(), items: rest }),
        }
    }
    out
}

/// Which section a tile is in, and where in it.
fn locate(layout: &[SectionEntry], label: &str) -> Option<(usize, usize)> {
    layout
        .iter()
        .enumerate()
        .find_map(|(s, sec)| sec.items.iter().position(|i| i == label).map(|i| (s, i)))
}

/// Move a tile one place earlier or later, crossing into the neighbouring
/// section when it runs off the end of its own -- so stepping is the only
/// operation needed to both reorder within a group and move between groups.
/// False when there is nowhere left to go.
pub fn step(layout: &mut [SectionEntry], label: &str, forward: bool) -> bool {
    let Some((s, i)) = locate(layout, label) else { return false };
    if forward {
        if i + 1 < layout[s].items.len() {
            layout[s].items.swap(i, i + 1);
        } else if s + 1 < layout.len() {
            let it = layout[s].items.remove(i);
            layout[s + 1].items.insert(0, it);
        } else {
            return false;
        }
    } else if i > 0 {
        layout[s].items.swap(i - 1, i);
    } else if s > 0 {
        let it = layout[s].items.remove(i);
        layout[s - 1].items.push(it);
    } else {
        return false;
    }
    true
}

/// Where a tile sits as far as the screen is concerned: its group, and its
/// position among the tiles of that group you can actually see.
///
/// Stepping is defined over the whole layout, hidden and filtered-out tiles
/// included, but a step onto one of those looks like nothing happened -- so the
/// mover repeats until *this* changes.
pub fn shown_position(
    layout: &[SectionEntry],
    label: &str,
    shown: &[String],
) -> Option<(usize, usize)> {
    let (s, i) = locate(layout, label)?;
    Some((s, layout[s].items[..i].iter().filter(|n| shown.contains(n)).count()))
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
        Ok(mut cfg) => {
            fold_legacy_folders(&mut cfg);
            (cfg, None)
        }
        Err(e) => {
            let msg = e.message().lines().next().unwrap_or("invalid TOML").to_string();
            (Config::default(), Some(format!("config.toml: {msg}")))
        }
    }
}

/// Take the old `folders = [...]` down to the one folder a tile has now.
///
/// A tile is one place and one command, so a list of them cannot be honoured any
/// more -- but a config that has one must still come up. The first is kept,
/// because it is the one that used to be offered first, and the rest are
/// dropped without comment: the status line is for something that has just
/// happened, and this has been true of the file since the day it was written.
/// `y` is how you get a second tile for a folder you miss.
fn fold_legacy_folders(cfg: &mut Config) {
    let fold = |folder: &mut Option<String>, legacy: &mut Vec<String>| {
        let mut old = std::mem::take(legacy).into_iter();
        if folder.is_none() {
            *folder = old.next();
        }
    };
    for h in &mut cfg.hosts {
        fold(&mut h.folder, &mut h.legacy_folders);
    }
    for l in &mut cfg.locals {
        fold(&mut l.folder, &mut l.legacy_folders);
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
         # `e` edit, `d` duplicate, `D` delete, `s` settings -- or by hand.\n\
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
         # Open with hidden tiles already on screen. X toggles them either way.\n\
         show_hidden = false\n\
         \n",
    );

    // A tile for this machine. It runs the workspace manager where there is
    // one, because that is what used to open with a terminal, and a plain login
    // shell where there is not. `current` either way: a local command takes
    // over this tab rather than opening another one.
    // The command is written down only when it is not the default: a tile
    // without one is already a login shell, and saying so twice would put a path
    // on the tile that tells you nothing you cannot read off its label.
    let (detail, command) = match workspace_manager() {
        Some((name, path)) => (
            format!("{name} · workspace manager"),
            format!("command = {:?}\n", path.display().to_string()),
        ),
        None => ("local shell".to_string(), String::new()),
    };
    s.push_str(&format!(
        "# Local tiles run a command on this machine. `open_in` decides where:\n\
         # a new tab, a new window, or \"current\" to take over this one.\n\
         # Outside Ghostty on macOS every tile opens in the current terminal.\n\
         #\n\
         # `command` defaults to your login shell and `folder` to your home\n\
         # directory. One of each per tile: press d on a tile to duplicate it\n\
         # and point the copy somewhere else.\n\
         #\n\
         # A command that is not something you sit in is fine: it runs, and\n\
         # then a shell takes the session over in the same folder, so what it\n\
         # printed is still on screen.\n\
         [[local]]\nlabel = {:?}\ndetail = {detail:?}\n{command}\
         open_in = \"current\"\n\n",
        platform::machine_name(),
    ));

    s.push_str(
        "# Extra ssh hosts. Only `name` is required -- with nothing else it is\n\
         # passed to ssh as-is, so anything ssh already resolves just works.\n\
         #\n\
         # `color` sets the tile dot and the tab tint. Leave it out and one is\n\
         # picked from the palette by name, the same on every run.\n\
         #\n\
         # Two ways off the screen, and they mean different things. `x` hides a\n\
         # tile -- for the host you keep but rarely open -- and `X` shows every\n\
         # hidden one again, so it is one keystroke away rather than gone.\n\
         # `d` deletes. For a host that comes from ~/.ssh/config there is no\n\
         # block to remove -- that file is never written to -- so the tile is\n\
         # struck off with `deleted = true` instead. The host keeps working as a\n\
         # ProxyJump target either way; only the tile goes. Delete that line, or\n\
         # press `a` and give the same name, to get it back.\n\
         #\n\
         # `folder` is the one directory the session starts in, and `command`\n\
         # what runs there -- a login shell in your home directory if neither\n\
         # is given. Both are on the far side for an ssh host, and so is the\n\
         # login shell that takes over when the command ends.\n\
         #\n\
         # [[host]]\n\
         # name = \"myserver\"\n\
         # hostname = \"10.0.0.5\"\n\
         # user = \"albe\"\n\
         # port = 22\n\
         # jump = \"bastion\"\n\
         # folder = \"/srv/app\"\n\
         # command = \"tmux attach\"\n\
         # color = \"#4f8ab0\"\n\
         # hidden = false\n\
         # open_in = \"window\"\n\
         \n\
         # Groups, in the order they are drawn. `m` grabs the selected tile and\n\
         # the arrows move it; `S` makes, renames and reorders the groups\n\
         # themselves. Names are `name`s and `label`s, so a host from\n\
         # ~/.ssh/config can be placed without a block of its own. Anything not\n\
         # listed is drawn, untitled, after the last group.\n\
         #\n\
         # [[section]]\n\
         # title = \"work\"\n\
         # items = [\"myserver\", \"bastion\"]\n\
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

/// Write a new block directly below the block named `after`, falling back to the
/// end of the file when that one has no block of its own.
///
/// Where a block sits is where its tile is drawn, as long as no `[[section]]`
/// says otherwise -- so a duplicate belongs next to the tile it copies, in the
/// file as much as on screen.
pub fn insert_block_after(after: &str, d: &Draft) -> std::io::Result<()> {
    insert_block_after_at(&path(), after, d)
}

fn insert_block_after_at(path: &Path, after: &str, d: &Draft) -> std::io::Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let Some((_, end)) = find_block(&lines, d.block, after) else {
        return append_block_at(path, d);
    };

    let mut out: Vec<String> = lines[..end].iter().map(|s| s.to_string()).collect();
    if !out.last().is_some_and(|l| l.trim().is_empty()) {
        out.push(String::new());
    }
    out.push(d.block.header().to_string());
    out.extend(block_body(d).lines().map(String::from));
    if end < lines.len() {
        out.push(String::new());
    }
    out.extend(lines[end..].iter().map(|s| s.to_string()));
    write_lines(path, out)
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
            ("command", &d.command),
            ("folder", &d.folder),
            ("color", &d.color),
        ],
        Block::Local => &[
            ("command", &d.command),
            ("detail", &d.detail),
            ("folder", &d.folder),
            ("color", &d.color),
        ],
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
    if d.hidden {
        s.push_str("hidden = true\n");
    }
    if d.deleted {
        s.push_str("deleted = true\n");
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

/// Replace every `[[section]]` block with these, keeping the position of the
/// first one.
///
/// The one place a block is rewritten wholesale rather than patched. A section
/// is a list of names in an order the UI shuffles, so there is no field to edit
/// in place and nothing a comment inside it could be about; keeping the position
/// is what stops the file growing a new tail every time a tile moves.
pub fn write_sections(sections: &[SectionEntry]) -> std::io::Result<()> {
    write_sections_at(&path(), sections)
}

fn write_sections_at(path: &Path, sections: &[SectionEntry]) -> std::io::Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();

    // An untitled empty group says nothing -- it is what the layout gives you
    // before anyone makes a section, and writing it down would only be noise.
    let keep: Vec<&SectionEntry> = sections
        .iter()
        .filter(|s| !s.title.trim().is_empty() || !s.items.is_empty())
        .collect();
    let rendered = render_sections(&keep);

    let mut out: Vec<String> = Vec::new();
    let mut written = false;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "[[section]]" {
            let mut end = i + 1;
            while end < lines.len() && !is_header(lines[end]) {
                end += 1;
            }
            if !written {
                out.extend(rendered.iter().cloned());
                // Whatever follows is another table, so it needs the blank line
                // the removed blocks were separated by.
                if end < lines.len() && !rendered.is_empty() {
                    out.push(String::new());
                }
                written = true;
            }
            i = end;
            continue;
        }
        out.push(lines[i].to_string());
        i += 1;
    }

    if !written && !rendered.is_empty() {
        if !out.last().is_some_and(|l| l.trim().is_empty()) {
            out.push(String::new());
        }
        out.extend(rendered);
    }
    // Dropping the last section must not leave the gap it sat in behind, or
    // repeated edits accumulate blank lines.
    while out.len() > 1 && out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    write_lines(path, out)
}

/// Blank lines go *between* sections, so the caller decides whether the run
/// needs one after it -- appending at the end of a file does not.
fn render_sections(sections: &[&SectionEntry]) -> Vec<String> {
    let mut out = Vec::new();
    for s in sections {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push("[[section]]".to_string());
        if !s.title.trim().is_empty() {
            out.push(format!("title = {:?}", s.title.trim()));
        }
        out.extend(items_lines(&s.items));
    }
    out
}

/// `items = [...]` on one line while it fits, one name per line when it does
/// not: a group of a dozen tiles is still meant to be readable by hand.
fn items_lines(items: &[String]) -> Vec<String> {
    let quoted: Vec<String> = items.iter().map(|i| format!("{i:?}")).collect();
    let one = format!("items = [{}]", quoted.join(", "));
    if one.len() <= 76 {
        return vec![one];
    }
    let mut out = vec!["items = [".to_string()];
    out.extend(quoted.iter().map(|q| format!("    {q},")));
    out.push("]".to_string());
    out
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
            deleted: false,
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

    /// Deleting a host that lives in ~/.ssh/config leaves a block saying only
    /// that: the file it comes from is not ours to edit, and a deletion that
    /// pinned its connection details would resurrect them on the way back.
    #[test]
    fn deleting_a_bare_ssh_config_host_writes_a_minimal_block() {
        let p = scratch("delete-ssh", SEED);
        let d = Draft::deleted_host("csnhr");
        assert!(!save_block_at(&p, Some("csnhr"), &d).unwrap(), "no block existed, so appended");

        let cfg = parse(&p);
        let h = cfg.hosts.iter().find(|h| h.name == "csnhr").unwrap();
        assert!(h.deleted);
        assert_eq!(h.hostname, None, "deleting must not pin the connection details");
        assert_eq!(h.user, None);
        assert_eq!(h.color, None);

        // Adding it back writes the same block without the flag.
        assert!(save_block_at(&p, Some("csnhr"), &draft("csnhr")).unwrap(), "updates in place");
        let text = fs::read_to_string(&p).unwrap();
        assert!(!text.contains("deleted"), "the flag is dropped, not written false");
    }

    /// A host whose block was customised loses that block's contents when it is
    /// deleted -- it is a deletion, not a hiding -- but the rest of the file is
    /// untouched and the block keeps its position.
    #[test]
    fn deleting_a_customised_host_strips_its_block_to_the_flag() {
        let p = scratch("delete-custom", SEED);
        let mut d = draft("srv");
        d.color = "#4f8ab0".into();
        d.user = "albe".into();
        save_block_at(&p, None, &d).unwrap();

        save_block_at(&p, Some("srv"), &Draft::deleted_host("srv")).unwrap();

        let h = parse(&p).hosts.into_iter().find(|h| h.name == "srv").unwrap();
        assert!(h.deleted);
        assert_eq!(h.color, None, "the customisation goes with the tile");
        assert_eq!(h.user, None);
        assert!(fs::read_to_string(&p).unwrap().contains("# a comment someone wrote"));
    }

    /// Two flags, two meanings, and neither may be read as the other: `hidden`
    /// is a tile you can bring back with one key, `deleted` is one that is gone.
    #[test]
    fn hidden_and_deleted_are_separate_flags() {
        let p = scratch("flags", SEED);
        fs::write(
            &p,
            "[[host]]\nname = \"csnhr\"\nhidden = true\n\n\
             [[host]]\nname = \"gone\"\ndeleted = true\n\n\
             [[local]]\nlabel = \"box\"\nhidden = true\n",
        )
        .unwrap();
        let cfg = parse(&p);
        assert!(cfg.hosts[0].hidden && !cfg.hosts[0].deleted);
        assert!(cfg.hosts[1].deleted && !cfg.hosts[1].hidden);
        assert!(cfg.locals[0].hidden && !cfg.locals[0].deleted);
    }

    /// What `x` does: reload the block, flip one bit, write it back. Everything
    /// else about the tile has to survive, or hiding would be a way of losing
    /// the colour you picked.
    #[test]
    fn hiding_preserves_the_rest_of_the_block() {
        let p = scratch("hide-keep", SEED);
        let mut d = draft("srv");
        d.color = "#4f8ab0".into();
        d.user = "albe".into();
        save_block_at(&p, None, &d).unwrap();

        let existing = parse(&p).hosts.into_iter().find(|h| h.name == "srv").unwrap();
        let mut back = Draft::from(&existing);
        back.hidden = true;
        save_block_at(&p, Some("srv"), &back).unwrap();

        let h = parse(&p).hosts.into_iter().find(|h| h.name == "srv").unwrap();
        assert!(h.hidden);
        assert_eq!(h.color.as_deref(), Some("#4f8ab0"), "the colour survives hiding");
        assert_eq!(h.user.as_deref(), Some("albe"));

        // And unhiding drops the key rather than writing it false.
        back.hidden = false;
        save_block_at(&p, Some("srv"), &back).unwrap();
        assert!(!fs::read_to_string(&p).unwrap().contains("hidden"));
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
        set_kv_at(&p, "options", "tab_emoji", "false").unwrap();
        let cfg = parse(&p);
        assert!(!cfg.options.include_ssh_config, "existing key rewritten");
        assert!(!cfg.options.tab_emoji, "absent key added");
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
    fn a_local_block_round_trips_with_its_folder() {
        let p = scratch("local", SEED);
        let d = Draft {
            block: Block::Local,
            name: "MACBOOK".into(),
            command: "/bin/zsh".into(),
            detail: "local shell".into(),
            folder: "~/code/one".into(),
            ..Default::default()
        };
        assert!(!save_block_at(&p, None, &d).unwrap());

        let cfg = parse(&p);
        let l = cfg.locals.iter().find(|l| l.label == "MACBOOK").unwrap();
        assert_eq!(l.command.as_deref(), Some("/bin/zsh"));
        assert_eq!(l.folder.as_deref(), Some("~/code/one"));

        // The seeded [[local]] must be untouched, and removal must find the
        // right one of the two.
        assert_eq!(cfg.locals.len(), 2);
        assert!(remove_block_at(&p, Block::Local, "MACBOOK").unwrap());
        assert_eq!(parse(&p).locals.len(), 1);
    }

    /// One folder and one command per tile, both optional: absent means home and
    /// a login shell, which is what a bare `ssh host` already gives you.
    #[test]
    fn a_folder_and_a_command_round_trip_on_a_host() {
        let p = scratch("folder", SEED);
        let mut d = draft("srv");
        d.folder = "/srv/app".into();
        d.command = "tmux attach".into();
        save_block_at(&p, None, &d).unwrap();
        let h = &parse(&p).hosts[0];
        assert_eq!(h.folder.as_deref(), Some("/srv/app"));
        assert_eq!(h.command.as_deref(), Some("tmux attach"));

        // Clearing them drops the keys rather than writing them blank.
        d.folder.clear();
        d.command.clear();
        save_block_at(&p, Some("srv"), &d).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(!text.contains("folder") && !text.contains("command = \"\""));
        assert_eq!(parse(&p).hosts[0].folder, None);
    }

    /// A config written when a tile could hold a list of folders must still come
    /// up, quietly. The first is kept -- it is the one that used to be offered
    /// first -- and the rest go without a word: nothing has just happened, the
    /// file has said this all along.
    #[test]
    fn the_old_folders_list_folds_down_to_one() {
        let p = scratch("legacy", SEED);
        fs::write(
            &p,
            "[[host]]\nname = \"zima\"\nfolders = [\"~\", \"~/services\"]\n\n\
             [[host]]\nname = \"solo\"\nfolders = [\"/srv\"]\n\n\
             [[local]]\nlabel = \"box\"\ncommand = \"/bin/zsh\"\nfolders = [\"~/Desktop\"]\n",
        )
        .unwrap();
        let mut cfg = parse(&p);
        fold_legacy_folders(&mut cfg);

        assert_eq!(cfg.hosts[0].folder.as_deref(), Some("~"));
        assert_eq!(cfg.hosts[1].folder.as_deref(), Some("/srv"));
        assert_eq!(cfg.locals[0].folder.as_deref(), Some("~/Desktop"));
        assert!(cfg.hosts.iter().all(|h| h.legacy_folders.is_empty()), "read once, not twice");
    }

    /// The one thing loading a config is allowed to say is that it could not be
    /// read. An old `folders = [...]` is not that: it parses, it comes up, and
    /// a status line about it on every single start is noise.
    #[test]
    fn loading_an_old_config_says_nothing() {
        let p = scratch("legacy-quiet", SEED);
        fs::write(&p, "[[host]]\nname = \"zima\"\nfolders = [\"~\", \"~/services\"]\n").unwrap();
        let mut cfg = parse(&p);
        fold_legacy_folders(&mut cfg);
        assert_eq!(cfg.hosts[0].folder.as_deref(), Some("~"), "still folded, just quietly");
    }

    /// A duplicate belongs next to what it copies -- in the file, which is the
    /// screen order until a `[[section]]` says otherwise.
    #[test]
    fn an_inserted_block_lands_below_the_one_it_copies() {
        let p = scratch("insert", SEED);
        for n in ["one", "two"] {
            save_block_at(&p, None, &draft(n)).unwrap();
        }
        let mut copy = draft("one-2");
        copy.folder = "/srv/app".into();
        insert_block_after_at(&p, "one", &copy).unwrap();

        let names: Vec<String> = parse(&p).hosts.into_iter().map(|h| h.name).collect();
        assert_eq!(names, ["one", "one-2", "two"], "beside it, not at the end");
        // The file is still the file: nothing else moved, nothing was lost.
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("# a comment someone wrote"));
        assert!(text.contains("[[local]]"));
        assert_eq!(parse(&p).hosts[1].folder.as_deref(), Some("/srv/app"));

        // A tile with no block of its own -- a bare ~/.ssh/config host -- has
        // nowhere to be inserted below, so the copy goes at the end.
        insert_block_after_at(&p, "nobody", &draft("ghost")).unwrap();
        let names: Vec<String> = parse(&p).hosts.into_iter().map(|h| h.name).collect();
        assert_eq!(names, ["one", "one-2", "two", "ghost"]);
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

    // ------------------------------------------------------------- sections

    fn section(title: &str, items: &[&str]) -> SectionEntry {
        SectionEntry {
            title: title.into(),
            items: items.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn names(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|s| s.to_string()).collect()
    }

    /// The whole point of `layout`: whatever the file says, what comes back
    /// accounts for every tile on screen exactly once.
    #[test]
    fn a_layout_covers_every_tile_and_only_real_ones() {
        let cfg = vec![section("work", &["b", "gone", "a"]), section("home", &["c"])];
        let out = layout(&cfg, &names(&["a", "b", "c", "d"]));

        assert_eq!(out[0].title, "work");
        assert_eq!(out[0].items, ["b", "a"], "file order wins; a stale name is dropped");
        assert_eq!(out[1].items, ["c"]);
        // `d` was never placed, so it lands in a trailing untitled group rather
        // than being guessed into one of the named ones.
        assert_eq!(out[2].title, "");
        assert_eq!(out[2].items, ["d"]);
    }

    #[test]
    fn no_sections_means_one_untitled_group_in_build_order() {
        let out = layout(&[], &names(&["a", "b"]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "");
        assert_eq!(out[0].items, ["a", "b"]);
    }

    /// A name listed twice must not draw twice, and leftovers join an untitled
    /// last group instead of opening a second anonymous one beside it.
    #[test]
    fn duplicates_and_leftovers_do_not_multiply_groups() {
        let cfg = vec![section("work", &["a", "a"]), section("", &["b"])];
        let out = layout(&cfg, &names(&["a", "b", "c"]));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].items, ["a"]);
        assert_eq!(out[1].items, ["b", "c"]);
    }

    #[test]
    fn stepping_reorders_within_a_group_then_crosses_into_the_next() {
        let mut l = vec![section("work", &["a", "b"]), section("home", &["c"])];

        assert!(step(&mut l, "a", true));
        assert_eq!(l[0].items, ["b", "a"], "swaps with its neighbour");

        // Off the end of its own group and into the head of the next one.
        assert!(step(&mut l, "a", true));
        assert_eq!(l[0].items, ["b"]);
        assert_eq!(l[1].items, ["a", "c"]);

        // And back, to the *end* of the group it came from.
        assert!(step(&mut l, "a", false));
        assert_eq!(l[0].items, ["b", "a"]);
        assert_eq!(l[1].items, ["c"]);

        // The two ends of the screen are walls, not wraps.
        assert!(!step(&mut l, "b", false));
        assert!(!step(&mut l, "c", true));
        assert!(!step(&mut l, "nobody", true));
    }

    /// An empty group is a real destination -- it is what you get right after
    /// making one, and a tile has to be able to step into it.
    #[test]
    fn a_tile_can_step_into_an_empty_group() {
        let mut l = vec![section("work", &["a"]), section("home", &[])];
        assert!(step(&mut l, "a", true));
        assert_eq!(l[0].items, Vec::<String>::new());
        assert_eq!(l[1].items, ["a"]);
    }

    /// The mover counts in visible tiles, since a step onto a hidden one would
    /// look on screen like nothing at all had happened.
    #[test]
    fn shown_position_skips_what_is_not_on_screen() {
        let l = vec![section("work", &["a", "secret", "b"])];
        let shown = names(&["a", "b"]);
        assert_eq!(shown_position(&l, "a", &shown), Some((0, 0)));
        assert_eq!(shown_position(&l, "secret", &shown), Some((0, 1)));
        assert_eq!(shown_position(&l, "b", &shown), Some((0, 1)), "same slot as hidden");
        assert_eq!(shown_position(&l, "nobody", &shown), None);
    }

    #[test]
    fn sections_are_written_and_read_back() {
        let p = scratch("sections", SEED);
        write_sections_at(&p, &[section("work", &["a", "b"]), section("", &["c"])]).unwrap();

        let cfg = parse(&p);
        assert_eq!(cfg.sections.len(), 2);
        assert_eq!(cfg.sections[0].title, "work");
        assert_eq!(cfg.sections[0].items, ["a", "b"]);
        assert_eq!(cfg.sections[1].title, "", "an untitled group needs no title key");
        assert_eq!(cfg.sections[1].items, ["c"]);
        // The rest of the file is still there.
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("# a comment someone wrote"));
        assert!(text.contains("[[local]]"));
    }

    /// Moving a tile rewrites the blocks; doing it repeatedly must not grow the
    /// file or walk the sections toward the end of it.
    #[test]
    fn rewriting_sections_replaces_them_in_place() {
        let p = scratch("sections-place", SEED);
        write_sections_at(&p, &[section("work", &["a"])]).unwrap();
        // Something after the blocks, so "in place" means anything at all.
        write_lines(&p, {
            let mut l: Vec<String> =
                fs::read_to_string(&p).unwrap().lines().map(String::from).collect();
            l.push(String::new());
            l.push("[theme]".into());
            l.push("primary = \"#aaaaaa\"".into());
            l
        })
        .unwrap();
        let before = fs::read_to_string(&p).unwrap();

        for _ in 0..3 {
            write_sections_at(&p, &[section("work", &["a"])]).unwrap();
        }
        assert_eq!(fs::read_to_string(&p).unwrap(), before, "idempotent, byte for byte");

        // And the theme table is still after them, not orphaned above.
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.find("[[section]]").unwrap() < text.find("[theme]").unwrap());
        assert_eq!(parse(&p).theme.primary.as_deref(), Some("#aaaaaa"));
    }

    /// Sections are the one thing the UI owns outright, so removing the last
    /// one has to take the whole block with it -- and leave the file it was
    /// added to exactly as it was.
    #[test]
    fn dropping_every_section_leaves_the_file_as_it_started() {
        let p = scratch("sections-gone", SEED);
        write_sections_at(&p, &[section("work", &["a"])]).unwrap();
        assert!(fs::read_to_string(&p).unwrap().contains("[[section]]"));

        write_sections_at(&p, &[]).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), SEED, "back to byte-identical");

        // An untitled group with nothing in it is what the layout hands back
        // before anyone makes a section; writing it down would be noise.
        write_sections_at(&p, &[section("", &[])]).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), SEED);
    }

    /// A long group has to stay readable by hand, which one 400-column line is
    /// not -- and it still has to parse.
    #[test]
    fn a_long_group_wraps_one_name_per_line() {
        let p = scratch("sections-long", SEED);
        let many: Vec<String> = (0..12).map(|i| format!("host-number-{i}")).collect();
        write_sections_at(&p, &[SectionEntry { title: "lots".into(), items: many.clone() }])
            .unwrap();

        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("items = [\n"), "wrapped, not one long line");
        assert!(text.lines().all(|l| l.len() < 80));
        assert_eq!(parse(&p).sections[0].items, many);
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
