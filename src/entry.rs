//! Turning `~/.ssh/config` and `config.toml` into the tiles on screen.

use std::path::Path;

use ratatui::style::Color;

use crate::config::{self, Config, HostEntry, OpenIn};
use crate::{platform, ssh};

/// A host's identity colour, and the emoji that stands in for it where only
/// text can go -- which is where the Ghostty tab title lives.
pub struct Swatch {
    pub hex: &'static str,
    pub emoji: &'static str,
}

/// Identity colours for hosts.
///
/// The list is sized and tuned to the coloured-circle emoji, one each, because
/// a tab title can carry a glyph but not a colour. Anything richer would give
/// two hosts the same circle, which defeats the point of glancing at the tab
/// bar. Red is excluded on purpose: the chrome spends #ff0000 on selection
/// alone, and a host that owned red would look permanently selected.
pub const PALETTE: [Swatch; 8] = [
    Swatch { hex: "#d08442", emoji: "🟠" },
    Swatch { hex: "#c9a227", emoji: "🟡" },
    Swatch { hex: "#6fa85a", emoji: "🟢" },
    Swatch { hex: "#4f8ab0", emoji: "🔵" },
    Swatch { hex: "#9a68b0", emoji: "🟣" },
    Swatch { hex: "#a0714f", emoji: "🟤" },
    Swatch { hex: "#b8bcc4", emoji: "⚪" },
    Swatch { hex: "#6a6f78", emoji: "⚫" },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tint {
    pub hex: String,
    pub color: Color,
    /// Nearest palette circle; used in the tab title.
    pub emoji: &'static str,
}

fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((b(0)?, b(2)?, b(4)?))
}

impl Tint {
    pub fn parse(hex: &str) -> Option<Tint> {
        let (r, g, b) = rgb(hex)?;
        Some(Tint {
            color: Color::Rgb(r, g, b),
            emoji: nearest_emoji(r, g, b),
            hex: format!("#{}", hex.trim().trim_start_matches('#').to_lowercase()),
        })
    }

    /// The palette colour a bare name would get, ignoring what else is around.
    pub fn for_name(name: &str) -> Tint {
        Tint::slot(Tint::preferred(name))
    }

    /// The palette slot a name hashes to, before collisions are considered.
    fn preferred(name: &str) -> usize {
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in name.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        (hash % PALETTE.len() as u64) as usize
    }

    pub fn slot(i: usize) -> Tint {
        Tint::parse(PALETTE[i % PALETTE.len()].hex).expect("palette is valid hex")
    }

    fn resolve(explicit: Option<&String>, name: &str) -> Tint {
        explicit.and_then(|h| Tint::parse(h)).unwrap_or_else(|| Tint::for_name(name))
    }
}

/// Squared distance in RGB. Crude as colour science, but it only has to pick
/// between eight well-separated circles, and a hand-written hex should get the
/// circle a person would have picked.
fn nearest_emoji(r: u8, g: u8, b: u8) -> &'static str {
    PALETTE
        .iter()
        .min_by_key(|s| {
            let (pr, pg, pb) = rgb(s.hex).expect("palette is valid hex");
            let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
            d(pr, r) + d(pg, g) + d(pb, b)
        })
        .map(|s| s.emoji)
        .unwrap_or("⚪")
}

/// Colours for a whole screen at once.
///
/// Hashing alone collides: with eight slots and six hosts, several land
/// together. Distinguishing tabs is the entire point, so a taken slot probes
/// forward to the next free one. The cost is that a host's colour depends on
/// what else is on screen -- adding a host can shift a later one. Explicit
/// colours claim their slot first and never move.
fn assign_tints(items: &[(String, Option<String>)]) -> Vec<Tint> {
    let mut used: Vec<String> = items
        .iter()
        .filter_map(|(_, e)| e.as_ref().and_then(|h| Tint::parse(h)).map(|t| t.hex))
        .collect();

    items
        .iter()
        .map(|(name, explicit)| {
            if let Some(t) = explicit.as_ref().and_then(|h| Tint::parse(h)) {
                return t;
            }
            let start = Tint::preferred(name);
            let free = (0..PALETTE.len())
                .map(|k| Tint::slot(start + k))
                .find(|t| !used.contains(&t.hex));
            // More hosts than palette slots: fall back to the hashed slot and
            // accept the repeat rather than leaving a tile colourless.
            let t = free.unwrap_or_else(|| Tint::slot(start));
            used.push(t.hex.clone());
            t
        })
        .collect()
}

/// Where a tile came from. `~/.ssh/config` is never rewritten; a host from it
/// can still be customised, by a `[[host]]` block of the same name in
/// config.toml that merges on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// From ~/.ssh/config, untouched.
    Ssh,
    /// From ~/.ssh/config, with a config.toml block layered over it.
    SshOverridden,
    /// Defined only in config.toml.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Runs a command on this machine.
    Local,
    /// Connects somewhere.
    Ssh,
}

/// What `~/.ssh/config` already says about a host, shown as placeholders in the
/// edit form so it is obvious what a blank field will inherit.
#[derive(Debug, Clone, Default)]
pub struct Defaults {
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub jump: String,
}

/// One button on the home screen.
///
/// Local and ssh tiles used to be separate shapes with separate launch paths.
/// They are one now because `open_in` made the difference disappear: every tile
/// is a command plus a destination, and "run it here" is a destination like any
/// other rather than a property of being local.
pub struct Entry {
    pub label: String,
    pub detail: String,
    pub jump: Option<String>,
    /// The whole command, already split -- never re-parsed by a shell. Complete:
    /// the folder and the command are decided when the tile is built, not when
    /// it is pressed, because a tile is one place and one thing to run.
    pub argv: Vec<String>,
    pub kind: Kind,
    pub origin: Option<Origin>,
    pub tint: Tint,
    /// Off the screen until `X` asks for it. A real tile in every other way:
    /// it holds its place in its group and comes back with one keystroke,
    /// which is the whole of the difference between hiding and deleting.
    pub hidden: bool,
    /// `None` defers to `[options] open_in`.
    pub open_in: Option<OpenIn>,
    /// The directory this tile starts in, as it was configured -- `~` and all,
    /// so the screen shows what the file says. `None` means the default: home.
    pub folder: Option<String>,
    /// What runs there, as configured. `None` means a login shell.
    pub command: Option<String>,
    /// A local tile's folder is a real `chdir` rather than part of `argv`, so it
    /// travels separately -- already resolved, since neither a `chdir` nor a
    /// quoted `cd` expands a `~`.
    pub cwd: Option<String>,
    /// Whether a shell takes the surface over when `argv` ends.
    ///
    /// A configured command is not always something you sit in: `echo hi` prints
    /// a line and exits, and a session that *was* that command dies with it --
    /// leaving a tab you cannot type in, or a home screen that blinked and came
    /// back. So a local tile with a command of its own runs it and then hands
    /// what is left to a shell in the same folder.
    ///
    /// Only local tiles set it. An ssh session carries the same idea inside its
    /// remote command, where the shell that has to be started is the far side's
    /// (see `remote_tail`) -- and a tile that *is* a shell already has nothing to
    /// outlive, so it still `exec`s outright with no wrapper left behind.
    pub shell_after: bool,
    pub defaults: Defaults,
    /// Which group this tile is drawn under: an index into `Board::sections`.
    /// Entries are stored in drawing order, so a section is always one
    /// contiguous run of them.
    pub section: usize,
}

/// The home screen: every tile in the order it is drawn, and the title of each
/// group they fall into.
///
/// The two travel together because neither is meaningful alone -- a section
/// index means nothing without the titles, and the titles mean nothing without
/// tiles ordered to match.
pub struct Board {
    pub entries: Vec<Entry>,
    pub sections: Vec<String>,
}

impl Entry {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn jump(&self) -> Option<&str> {
        self.jump.as_deref()
    }

    pub fn is_local(&self) -> bool {
        self.kind == Kind::Local
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn origin(&self) -> Option<Origin> {
        self.origin
    }

    pub fn tint(&self) -> &Tint {
        &self.tint
    }

    /// Where this tile lands, in the few columns a tile has spare -- the one
    /// thing that tells two copies of one host apart.
    ///
    /// The `command` is deliberately not here. A tile has room for one short
    /// line, a command is the longer and less distinguishing half of the pair,
    /// and the border is a place to read a *destination* rather than an argv.
    pub fn note(&self) -> Option<String> {
        self.folder.clone()
    }

    pub fn matches(&self, needle: &str) -> bool {
        let n = needle.to_lowercase();
        let has = |s: &str| s.to_lowercase().contains(&n);
        has(&self.label)
            || has(&self.detail)
            || self.jump().is_some_and(has)
            // A duplicate differs from its original in nothing else, so the
            // filter has to be able to see the difference too.
            || self.folder.as_deref().is_some_and(has)
            || self.command.as_deref().is_some_and(has)
    }
}

/// The far side's login shell, which is what a session is when nothing else is
/// asked of it -- and what it becomes again when what *was* asked of it ends.
const REMOTE_SHELL: &str = "exec ${SHELL:-/bin/sh} -l";

/// The tail that puts an ssh session in a directory, running something.
///
/// The directory is on the far side, so it cannot be a local `chdir`: it becomes
/// a remote command instead. `-t` forces a tty -- without it the remote shell is
/// not interactive -- and `exec $SHELL -l` is what leaves you in a login shell in
/// the folder rather than a bare `sh`. A command of your own is passed as
/// written: it is a line for the remote shell, and second-guessing it with an
/// `exec` would break the ones that are more than a program name.
///
/// **The session outlives the command.** Your command runs and then the login
/// shell takes over, because a command is not always something you sit in: `echo
/// hi` prints one line and exits, and exec'ing it would close the session on the
/// output it just produced. So it is a group followed by the shell, and the pair
/// stays behind the `cd` -- a folder that isn't there should end the session with
/// ssh's error, not drop you into a shell somewhere else.
fn remote_tail(folder: Option<&str>, command: Option<&str>) -> Option<Vec<String>> {
    let folder = folder.filter(|d| !d.is_empty()).map(sh_quote_path);
    let command = command.filter(|c| !c.is_empty());
    let script = match (folder, command) {
        (Some(d), Some(c)) => format!("cd {d} && {{ {c}; {REMOTE_SHELL}; }}"),
        (Some(d), None) => format!("cd {d} && {REMOTE_SHELL}"),
        (None, Some(c)) => format!("{c}; {REMOTE_SHELL}"),
        (None, None) => return None,
    };
    Some(vec!["-t".to_string(), script])
}

/// Resolve a leading `~` against the home directory, for a path this machine
/// will read.
///
/// Nothing downstream does it for us: a local folder becomes either a real
/// `chdir` or a `cd` inside single quotes, and neither expands a tilde -- which
/// is why `folder = "~/Desktop"` reported that the folder did not exist.
pub use crate::platform::expand_home;

/// Quote a path for a shell, leaving a leading `~` for that shell to expand.
///
/// A remote directory is the one case we cannot resolve ourselves -- the home
/// it is relative to is on the far side -- so the tilde has to survive as
/// syntax: `cd ~/'my dir'`. Everything after it is still quoted, so spaces and
/// quotes in the path stay harmless.
fn sh_quote_path(s: &str) -> String {
    match s.strip_prefix('~') {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => match rest.trim_start_matches('/') {
            "" => "~/".to_string(),
            tail => format!("~/{}", sh_quote(tail)),
        },
        _ => sh_quote(s),
    }
}

fn sh_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''") } else { out.push(c) }
    }
    out.push('\'');
    out
}

/// Split a configured `command` into argv.
///
/// Whitespace separates arguments, so `zsh -l` is two of them, but a quoted run
/// stays one word -- which is what makes a Windows program reachable at all,
/// since the usual place for one is under `C:\Program Files`. Quotes are the
/// whole of the grammar: nothing here is ever handed back to a shell to
/// re-parse, so there is no expansion to model and no injection to prevent.
fn split_command(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;

    for c in command.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                // An empty quoted word is still a word: `""` is an argument.
                started = true;
                quote = Some(c);
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        out.push(word);
    }
    out
}

/// Swap `ssh` for `autossh`, so a dropped connection -- the laptop slept, the
/// network changed -- gets ssh restarted instead of leaving a dead tile.
///
/// `-M 0` turns off autossh's own monitoring port, which needs the remote end
/// to allow port forwarding; the `ServerAlive` pair is what notices the
/// connection is gone without it, sleep being the case that matters most since
/// a socket left dangling by it can otherwise sit quiet far longer than either
/// side is willing to wait.
pub(crate) fn use_autossh(argv: &mut Vec<String>) {
    argv.splice(
        0..1,
        [
            "autossh".to_string(),
            "-M".to_string(),
            "0".to_string(),
            "-o".to_string(),
            "ServerAliveInterval=15".to_string(),
            "-o".to_string(),
            "ServerAliveCountMax=3".to_string(),
        ],
    );
}

/// An `~/.ssh/config` host, with an optional config.toml block layered on top.
///
/// The alias is always the last argument, so ssh re-reads the same file we did
/// and every option we don't model (IdentityFile, ForwardX11, ...) still
/// applies. Overrides ride in front of it as `-o Key=Value`, which takes
/// precedence over the config file without replacing the lookup -- passing
/// `user@host` instead would silently drop the alias's identity settings.
fn from_ssh_config(h: ssh::Host, ov: Option<&HostEntry>) -> Entry {
    let defaults = Defaults {
        hostname: h.hostname.clone().unwrap_or_default(),
        user: h.user.clone().unwrap_or_default(),
        port: h.port.clone().unwrap_or_default(),
        jump: h.proxy_jump.clone().unwrap_or_default(),
    };

    let mut argv: Vec<String> = vec!["ssh".into()];
    if let Some(o) = ov {
        for (key, value) in [
            ("HostName", o.hostname.as_deref()),
            ("User", o.user.as_deref()),
            ("ProxyJump", o.jump.as_deref()),
        ] {
            if let Some(v) = value.filter(|v| !v.is_empty()) {
                argv.push("-o".into());
                argv.push(format!("{key}={v}"));
            }
        }
        if let Some(p) = o.port {
            argv.push("-o".into());
            argv.push(format!("Port={p}"));
        }
    }
    argv.push(h.alias.clone());
    let folder = ov.and_then(|o| o.folder.clone()).filter(|f| !f.is_empty());
    let command = ov.and_then(|o| o.command.clone()).filter(|c| !c.is_empty());
    argv.extend(remote_tail(folder.as_deref(), command.as_deref()).unwrap_or_default());

    let pick = |over: Option<String>, base: &str| -> Option<String> {
        over.filter(|s| !s.is_empty()).or_else(|| (!base.is_empty()).then(|| base.to_string()))
    };
    let hostname = pick(ov.and_then(|o| o.hostname.clone()), &defaults.hostname);
    let user = pick(ov.and_then(|o| o.user.clone()), &defaults.user);
    let port = pick(ov.and_then(|o| o.port.map(|p| p.to_string())), &defaults.port);
    let jump = pick(ov.and_then(|o| o.jump.clone()), &defaults.jump);

    let host = hostname.unwrap_or_else(|| h.alias.clone());
    let mut detail = match user {
        Some(u) => format!("{u}@{host}"),
        None => host,
    };
    if let Some(p) = &port {
        detail.push(':');
        detail.push_str(p);
    }

    Entry {
        tint: Tint::resolve(ov.and_then(|o| o.color.as_ref()), &h.alias),
        origin: Some(if ov.is_some() { Origin::SshOverridden } else { Origin::Ssh }),
        hidden: ov.is_some_and(|o| o.hidden),
        open_in: ov.and_then(|o| o.open_in),
        folder,
        command,
        cwd: None,
        shell_after: false,
        kind: Kind::Ssh,
        label: h.alias,
        detail,
        jump,
        argv,
        defaults,
        section: 0,
    }
}

fn from_config(e: &HostEntry) -> Entry {
    let host = e.hostname.clone().unwrap_or_else(|| e.name.clone());
    let target = match &e.user {
        Some(u) => format!("{u}@{host}"),
        None => host.clone(),
    };

    let mut argv: Vec<String> = vec!["ssh".into()];
    if let Some(j) = &e.jump {
        argv.push("-J".into());
        argv.push(j.clone());
    }
    if let Some(p) = e.port {
        argv.push("-p".into());
        argv.push(p.to_string());
    }
    argv.push(target.clone());
    let folder = e.folder.clone().filter(|f| !f.is_empty());
    let command = e.command.clone().filter(|c| !c.is_empty());
    argv.extend(remote_tail(folder.as_deref(), command.as_deref()).unwrap_or_default());

    let detail = match e.port {
        Some(p) => format!("{target}:{p}"),
        None => target,
    };
    Entry {
        tint: Tint::resolve(e.color.as_ref(), &e.name),
        label: e.name.clone(),
        detail,
        jump: e.jump.clone(),
        argv,
        kind: Kind::Ssh,
        origin: Some(Origin::Custom),
        hidden: e.hidden,
        open_in: e.open_in,
        folder,
        command,
        cwd: None,
        shell_after: false,
        defaults: Defaults::default(),
        section: 0,
    }
}

/// Local tiles first, then ssh hosts: the config's own, then yours -- the order
/// tiles are in before any `[[section]]` block has a say.
///
/// A tile marked `deleted` is not built at all. It is not a tile in a state, it
/// is a tile that is gone: nothing counts it, nothing colours around it, and
/// nothing can put the cursor on it. The flag only exists because a host from
/// `~/.ssh/config` has no block of ours to remove.
///
/// `hidden` is the opposite kind of thing and is built like any other tile.
/// It is a state the tile is *in* -- drawn when `X` asks for it, counted in its
/// group, holding its place in the arrangement -- so the decision about it is
/// made in `App::visible`, once per frame, and not here.
fn collect(cfg: &Config, ssh_config: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = cfg
        .locals
        .iter()
        .filter(|l| !l.deleted)
        .map(|l| {
            let command = l.command.clone().filter(|c| !c.is_empty());
            let folder = l.folder.clone().filter(|f| !f.is_empty());
            // A tile with no command of its own is a shell on this machine,
            // which is what a terminal would have opened anyway. Asked for only
            // when it is needed: finding it is a PATH walk on Windows.
            let argv = match &command {
                Some(c) => split_command(c),
                None => split_command(&platform::login_shell()),
            };
            Entry {
                tint: Tint::resolve(l.color.as_ref(), &l.label),
                label: l.label.clone(),
                detail: l.detail.clone(),
                jump: None,
                argv,
                kind: Kind::Local,
                origin: None,
                hidden: l.hidden,
                open_in: l.open_in,
                cwd: folder.as_deref().map(expand_home),
                folder,
                shell_after: command.is_some(),
                command,
                defaults: Defaults::default(),
                section: 0,
            }
        })
        .collect();

    let mut claimed: Vec<&str> = Vec::new();
    if cfg.options.include_ssh_config {
        for h in ssh::load(ssh_config) {
            // A config.toml block with the same name customises this host in
            // place rather than adding a second tile for it.
            let ov = cfg.hosts.iter().find(|c| c.name == h.alias);
            if let Some(o) = ov {
                claimed.push(&o.name);
                // The block is the deletion, so the host it names is struck off
                // the board -- while ~/.ssh/config keeps it, and so does ssh.
                if o.deleted {
                    continue;
                }
            }
            entries.push(from_ssh_config(h, ov));
        }
    }
    entries.extend(
        cfg.hosts
            .iter()
            .filter(|h| !h.deleted && !claimed.contains(&h.name.as_str()))
            .map(from_config),
    );

    if cfg.options.use_autossh {
        for e in entries.iter_mut().filter(|e| e.kind == Kind::Ssh) {
            use_autossh(&mut e.argv);
        }
    }

    // Colours are only decidable once the whole screen is known.
    let explicit = |name: &str| -> Option<String> {
        cfg.locals
            .iter()
            .find(|l| l.label == name)
            .and_then(|l| l.color.clone())
            .or_else(|| cfg.hosts.iter().find(|h| h.name == name).and_then(|h| h.color.clone()))
    };
    let specs: Vec<(String, Option<String>)> =
        entries.iter().map(|e| (e.label.clone(), explicit(&e.label))).collect();
    for (e, tint) in entries.iter_mut().zip(assign_tints(&specs)) {
        e.tint = tint;
    }
    entries
}

/// Every tile, in the order and the groups the config asks for.
///
/// Colours are decided by `collect`, *before* the arrangement is applied, and
/// that ordering is deliberate: moving a tile is then a move and nothing else.
/// Assign after, and dragging one host across the screen would repaint a
/// stranger three tiles away.
pub fn build(cfg: &Config, ssh_config: &Path) -> Board {
    let mut pool = collect(cfg, ssh_config);
    let labels: Vec<String> = pool.iter().map(|e| e.label.clone()).collect();

    let mut entries: Vec<Entry> = Vec::with_capacity(pool.len());
    let mut sections: Vec<String> = Vec::new();
    for (s, sec) in config::layout(&cfg.sections, &labels).iter().enumerate() {
        sections.push(sec.title.clone());
        for name in &sec.items {
            if let Some(i) = pool.iter().position(|e| e.label == *name) {
                let mut e = pool.remove(i);
                e.section = s;
                entries.push(e);
            }
        }
    }
    if sections.is_empty() {
        sections.push(String::new());
    }
    // A host and a local tile may share a name, and a layout lists that name
    // once, so the second of the pair can be left holding nothing. It joins the
    // last group rather than dropping off the screen.
    let last = sections.len() - 1;
    for mut e in pool {
        e.section = last;
        entries.push(e);
    }
    Board { entries, sections }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HostEntry;

    fn host_entry(name: &str) -> HostEntry {
        HostEntry {
            name: name.into(),
            hostname: None,
            user: None,
            port: None,
            jump: None,
            color: None,
            hidden: false,
            deleted: false,
            open_in: None,
            folder: None,
            command: None,
            legacy_folders: Vec::new(),
        }
    }

    fn entry(
        name: &str,
        hostname: Option<&str>,
        user: Option<&str>,
        port: Option<u16>,
        jump: Option<&str>,
    ) -> Entry {
        from_config(&HostEntry {
            hostname: hostname.map(Into::into),
            user: user.map(Into::into),
            port,
            jump: jump.map(Into::into),
            ..host_entry(name)
        })
    }

    /// `zsh -l` has to be two arguments, and a Windows program has to survive
    /// living under `C:\Program Files` -- which needs quotes to mean something.
    #[test]
    fn a_command_splits_on_spaces_unless_they_are_quoted() {
        let split = |s: &str| split_command(s);
        assert_eq!(split("/bin/zsh -l"), ["/bin/zsh", "-l"]);
        assert_eq!(split("  /bin/zsh   -l  "), ["/bin/zsh", "-l"], "runs of space are one break");
        assert_eq!(split(""), Vec::<String>::new());
        assert_eq!(
            split(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#),
            [r"C:\Program Files\PowerShell\7\pwsh.exe", "-NoLogo"]
        );
        assert_eq!(split("'/usr/local/my tools/sh'"), ["/usr/local/my tools/sh"]);
        // Quotes delimit; they are not part of the argument, and they may open
        // and close mid-word.
        assert_eq!(split(r#"ssh -o "User=a b" host"#), ["ssh", "-o", "User=a b", "host"]);
        assert_eq!(split(r#"a"b c"d"#), ["ab cd"]);
        // An unterminated quote yields the rest as one word rather than
        // dropping it: a tile that still runs beats a tile that vanishes.
        assert_eq!(split(r#"cmd "unclosed arg"#), ["cmd", "unclosed arg"]);
    }

    #[test]
    fn bare_name_is_passed_to_ssh_untouched() {
        assert_eq!(entry("box", None, None, None, None).argv, ["ssh", "box"]);
    }

    #[test]
    fn user_host_port_and_jump_become_argv() {
        let t = entry("srv", Some("10.0.0.5"), Some("albe"), Some(2222), Some("bastion"));
        assert_eq!(t.argv, ["ssh", "-J", "bastion", "-p", "2222", "albe@10.0.0.5"]);
        assert_eq!(t.detail, "albe@10.0.0.5:2222");
    }

    #[test]
    fn args_are_separate_words_not_a_shell_string() {
        // A host name with a space must stay one argv element, never split.
        let t = entry("odd", Some("a b"), None, None, None);
        assert_eq!(t.argv, ["ssh", "a b"]);
    }

    #[test]
    fn a_host_keeps_its_colour_across_runs() {
        assert_eq!(Tint::for_name("alex").hex, Tint::for_name("alex").hex);
    }

    #[test]
    fn explicit_colour_wins_and_bad_hex_falls_back() {
        assert_eq!(Tint::resolve(Some(&"#1A2B3C".to_string()), "x").hex, "#1a2b3c");
        assert_eq!(Tint::resolve(Some(&"#1A2B3C".to_string()), "x").color, Color::Rgb(26, 43, 60));
        let bad = Tint::resolve(Some(&"not-a-colour".to_string()), "x");
        assert!(PALETTE.iter().any(|s| s.hex == bad.hex));
    }

    #[test]
    fn no_palette_colour_is_the_accent_red() {
        assert!(!PALETTE.iter().any(|s| s.hex == "#ff0000"));
    }

    /// The tab title carries an emoji instead of a colour, so the mapping has to
    /// be injective or two hosts get the same circle.
    #[test]
    fn every_palette_slot_has_its_own_emoji() {
        let mut seen: Vec<&str> = PALETTE.iter().map(|s| s.emoji).collect();
        let total = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), total);
    }

    #[test]
    fn a_palette_colour_maps_to_its_own_emoji() {
        for s in &PALETTE {
            assert_eq!(Tint::parse(s.hex).unwrap().emoji, s.emoji, "{}", s.hex);
        }
    }

    #[test]
    fn a_hand_written_hex_gets_a_sensible_circle() {
        assert_eq!(Tint::parse("#2255cc").unwrap().emoji, "🔵");
        assert_eq!(Tint::parse("#e0b020").unwrap().emoji, "🟡");
        assert_eq!(Tint::parse("#111111").unwrap().emoji, "⚫");
    }

    #[test]
    fn colours_are_distinct_up_to_the_palette_size() {
        let names = ["herdr", "alex", "csnhr", "mufasa", "zima", "elcap"];
        let specs: Vec<_> = names.iter().map(|n| (n.to_string(), None)).collect();
        let mut hexes: Vec<String> = assign_tints(&specs).into_iter().map(|t| t.hex).collect();
        let total = hexes.len();
        hexes.sort();
        hexes.dedup();
        assert_eq!(hexes.len(), total, "every tile must get its own colour");
    }

    #[test]
    fn explicit_colours_are_reserved_before_auto_ones() {
        let taken = Tint::for_name("zima").hex;
        let specs =
            vec![("pinned".to_string(), Some(taken.clone())), ("zima".to_string(), None)];
        let out = assign_tints(&specs);
        assert_eq!(out[0].hex, taken);
        assert_ne!(out[1].hex, taken);
    }

    #[test]
    fn more_hosts_than_slots_still_yields_a_colour_each() {
        let specs: Vec<_> = (0..PALETTE.len() + 5).map(|i| (format!("h{i}"), None)).collect();
        let out = assign_tints(&specs);
        assert_eq!(out.len(), specs.len());
        assert!(out.iter().all(|t| PALETTE.iter().any(|s| s.hex == t.hex)));
    }

    fn ssh_host(alias: &str) -> ssh::Host {
        ssh::Host {
            alias: alias.into(),
            hostname: Some(format!("{alias}.example.org")),
            user: Some("base".into()),
            port: None,
            proxy_jump: Some("bastion".into()),
        }
    }

    fn override_for(name: &str, color: Option<&str>, user: Option<&str>) -> HostEntry {
        HostEntry {
            user: user.map(Into::into),
            color: color.map(Into::into),
            ..host_entry(name)
        }
    }

    /// Recolouring an ~/.ssh/config host must change nothing about how it
    /// connects: still the bare alias, so ssh does its own lookup.
    #[test]
    fn a_colour_override_does_not_touch_the_ssh_args() {
        let t = from_ssh_config(ssh_host("alex"), Some(&override_for("alex", Some("#4f8ab0"), None)));
        assert_eq!(t.argv, ["ssh", "alex"]);
        assert_eq!(t.tint.hex, "#4f8ab0");
        assert_eq!(t.origin, Some(Origin::SshOverridden));
        assert_eq!(t.detail, "base@alex.example.org", "still shows the inherited user");
    }

    /// Overriding a connection field rides in as `-o`, in front of the alias,
    /// so IdentityFile and friends from ~/.ssh/config still apply.
    #[test]
    fn a_field_override_becomes_a_dash_o_flag_before_the_alias() {
        let t = from_ssh_config(ssh_host("alex"), Some(&override_for("alex", None, Some("root"))));
        assert_eq!(t.argv, ["ssh", "-o", "User=root", "alex"]);
        assert_eq!(t.argv.last().unwrap(), "alex", "the alias stays the target");
        assert_eq!(t.detail, "root@alex.example.org");
    }

    #[test]
    fn defaults_are_carried_for_the_forms_placeholders() {
        let t = from_ssh_config(ssh_host("alex"), None);
        assert_eq!(t.defaults.hostname, "alex.example.org");
        assert_eq!(t.defaults.user, "base");
        assert_eq!(t.defaults.jump, "bastion");
        assert_eq!(t.origin, Some(Origin::Ssh));
        assert_eq!(t.argv, ["ssh", "alex"]);
    }

    /// An override of an existing alias customises that tile; it must not add
    /// a duplicate one.
    #[test]
    fn an_override_does_not_create_a_second_tile() {
        let dir = std::env::temp_dir().join("termhome-entry-test");
        std::fs::create_dir_all(&dir).unwrap();
        let sshcfg = dir.join("config");
        std::fs::write(&sshcfg, "Host alex\n  HostName alex.example.org\n").unwrap();

        let mut cfg = Config::default();
        cfg.hosts.push(override_for("alex", Some("#9a68b0"), None));
        cfg.hosts.push(override_for("brand-new", None, None));

        let entries = build(&cfg, &sshcfg).entries;
        let names: Vec<&str> = entries.iter().map(|e| e.label()).collect();
        assert_eq!(names, ["alex", "brand-new"], "one tile per name");
        assert_eq!(entries[0].tint().hex, "#9a68b0");
        assert_eq!(entries[0].origin(), Some(Origin::SshOverridden));
        assert_eq!(entries[1].origin(), Some(Origin::Custom));
    }

    /// Deleting a host from `~/.ssh/config` is the one deletion that cannot
    /// take a block away, so it leaves one behind saying so -- and that block
    /// must not draw a tile of its own instead.
    #[test]
    fn a_deleted_host_is_not_on_the_board_at_all() {
        let dir = std::env::temp_dir().join("dasshboard-entry-deleted");
        std::fs::create_dir_all(&dir).unwrap();
        let sshcfg = dir.join("config");
        std::fs::write(&sshcfg, "Host alex\nHost csnhr\n").unwrap();

        let mut cfg = Config::default();
        cfg.hosts.push(HostEntry { deleted: true, ..host_entry("csnhr") });
        cfg.hosts.push(HostEntry { deleted: true, ..host_entry("gone") });
        cfg.locals.push(crate::config::LocalEntry {
            label: "box".into(),
            detail: String::new(),
            command: None,
            color: None,
            hidden: false,
            deleted: true,
            open_in: None,
            folder: None,
            legacy_folders: Vec::new(),
        });

        let names: Vec<String> =
            build(&cfg, &sshcfg).entries.iter().map(|e| e.label().to_string()).collect();
        assert_eq!(names, ["alex"], "only the host nobody deleted");
    }

    /// The global switch to autossh reaches every ssh tile -- one from
    /// `~/.ssh/config`, one defined only in config.toml -- and leaves a local
    /// tile alone: it has no connection to keep alive.
    #[test]
    fn use_autossh_option_swaps_the_launcher_for_every_ssh_tile_but_not_local_ones() {
        let dir = std::env::temp_dir().join("dasshboard-entry-autossh");
        std::fs::create_dir_all(&dir).unwrap();
        let sshcfg = dir.join("config");
        std::fs::write(&sshcfg, "Host alex\n").unwrap();

        let mut cfg = Config::default();
        cfg.options.use_autossh = true;
        cfg.hosts.push(override_for("srv", None, None));
        cfg.locals.push(crate::config::LocalEntry {
            label: "box".into(),
            detail: String::new(),
            command: Some("echo hi".into()),
            color: None,
            hidden: false,
            deleted: false,
            open_in: None,
            folder: None,
            legacy_folders: Vec::new(),
        });

        let entries = build(&cfg, &sshcfg).entries;
        let argv_of = |l: &str| entries.iter().find(|e| e.label() == l).unwrap().argv.clone();
        assert_eq!(argv_of("alex")[0], "autossh", "from ~/.ssh/config");
        assert_eq!(argv_of("srv")[0], "autossh", "from config.toml alone");
        assert_eq!(argv_of("box")[0], "echo", "a local tile has no ssh to wrap");
    }

    // ------------------------------------------------------------- sections

    fn board_of(hosts: &[&str], sections: Vec<crate::config::SectionEntry>) -> Board {
        let mut cfg = Config::default();
        cfg.options.include_ssh_config = false;
        cfg.sections = sections;
        for h in hosts {
            cfg.hosts.push(override_for(h, None, None));
        }
        build(&cfg, Path::new("/nonexistent"))
    }

    fn sec(title: &str, items: &[&str]) -> crate::config::SectionEntry {
        crate::config::SectionEntry {
            title: title.into(),
            items: items.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// With nothing configured the screen is one untitled group in build order,
    /// which is what it was before sections existed.
    #[test]
    fn a_board_with_no_sections_is_one_untitled_group() {
        let b = board_of(&["a", "b"], Vec::new());
        assert_eq!(b.sections, [""]);
        assert_eq!(b.entries.iter().map(|e| e.label()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(b.entries.iter().all(|e| e.section == 0));
    }

    /// The `[[section]]` blocks, not the build order, decide what is drawn where
    /// -- and each group is one contiguous run, which is what lets the grid draw
    /// a title above it.
    #[test]
    fn sections_reorder_the_board_and_stay_contiguous() {
        let b = board_of(&["a", "b", "c"], vec![sec("last", &["c"]), sec("first", &["b", "a"])]);
        assert_eq!(b.sections, ["last", "first"]);
        assert_eq!(b.entries.iter().map(|e| e.label()).collect::<Vec<_>>(), ["c", "b", "a"]);
        assert_eq!(b.entries.iter().map(|e| e.section).collect::<Vec<_>>(), [0, 1, 1]);
    }

    /// A host nobody has placed yet -- one that turned up in ~/.ssh/config since
    /// the sections were written -- must still be on screen.
    #[test]
    fn an_unplaced_tile_lands_after_the_last_group() {
        let b = board_of(&["a", "b"], vec![sec("work", &["b"])]);
        assert_eq!(b.sections, ["work", ""]);
        assert_eq!(b.entries.iter().map(|e| e.label()).collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(b.entries[1].section, 1);
    }

    /// Moving a tile is a move and nothing else: colours are assigned before the
    /// arrangement, so rearranging the screen never repaints it.
    #[test]
    fn rearranging_does_not_change_any_colour() {
        let plain = board_of(&["one", "two", "three"], Vec::new());
        let moved =
            board_of(&["one", "two", "three"], vec![sec("x", &["three", "one"]), sec("y", &["two"])]);
        for e in &plain.entries {
            let same = moved.entries.iter().find(|m| m.label() == e.label()).unwrap();
            assert_eq!(same.tint().hex, e.tint().hex, "{} was repainted", e.label());
        }
    }

    // ------------------------------------------------- folders and commands

    fn local(label: &str, command: Option<&str>, folder: Option<&str>) -> Entry {
        let mut cfg = Config::default();
        cfg.options.include_ssh_config = false;
        cfg.locals.push(crate::config::LocalEntry {
            label: label.into(),
            detail: "local shell".into(),
            command: command.map(Into::into),
            color: None,
            hidden: false,
            deleted: false,
            open_in: None,
            folder: folder.map(Into::into),
            legacy_folders: Vec::new(),
        });
        build(&cfg, Path::new("/nonexistent")).entries.remove(0)
    }

    fn remote(folder: Option<&str>, command: Option<&str>) -> Entry {
        from_ssh_config(
            ssh_host("alex"),
            Some(&HostEntry {
                folder: folder.map(Into::into),
                command: command.map(Into::into),
                ..host_entry("alex")
            }),
        )
    }

    /// The remote directory can't be a local `cd`, so it becomes a remote
    /// command. `-t` is what makes it an interactive shell rather than a
    /// one-shot; `exec $SHELL -l` is what leaves you in a login shell there.
    #[test]
    fn an_ssh_folder_becomes_a_remote_command() {
        let t = remote(Some("/scratch/project"), None);
        assert_eq!(t.argv[..2], ["ssh", "alex"], "the alias still leads");
        assert_eq!(t.argv[2], "-t", "a tty is required for an interactive shell");
        assert_eq!(t.argv[3], "cd '/scratch/project' && exec ${SHELL:-/bin/sh} -l");
        assert!(t.cwd.is_none(), "not a local cd");
    }

    /// A command of your own runs instead of the login shell, and rides along
    /// with the folder when there is one -- but the shell still follows it, so
    /// the session outlives a command that exits.
    #[test]
    fn an_ssh_command_runs_before_the_login_shell() {
        let t = remote(None, Some("tmux attach"));
        assert_eq!(t.argv, ["ssh", "alex", "-t", "tmux attach; exec ${SHELL:-/bin/sh} -l"]);
        // Written as given: it is a line for the remote shell, and the ones that
        // are more than a program name would not survive being second-guessed.
        // The braces are why: without them the `&&` would reach past the group
        // and the shell would start whether the command ran or not.
        let both = remote(Some("~/proj"), Some("tmux attach || tmux new"));
        assert_eq!(
            both.argv[3],
            "cd ~/'proj' && { tmux attach || tmux new; exec ${SHELL:-/bin/sh} -l; }"
        );
    }

    /// The point of the tail: a command that prints a line and exits leaves a
    /// session you can still type in, rather than a surface that closed on its
    /// own output.
    #[test]
    fn a_command_that_exits_still_leaves_a_shell() {
        let t = remote(None, Some("echo hi"));
        assert!(t.argv[3].starts_with("echo hi; "), "the command runs first: {}", t.argv[3]);
        assert!(t.argv[3].ends_with(REMOTE_SHELL), "and the shell has the session after it");
        // A folder puts both behind the `cd`: a directory that is not there
        // should end the session with ssh's error, not open a shell elsewhere.
        let d = remote(Some("/srv/app"), Some("echo hi"));
        assert_eq!(d.argv[3], "cd '/srv/app' && { echo hi; exec ${SHELL:-/bin/sh} -l; }");
    }

    #[test]
    fn neither_folder_nor_command_leaves_the_ssh_args_alone() {
        assert_eq!(from_ssh_config(ssh_host("alex"), None).argv, ["ssh", "alex"]);
        assert_eq!(remote(Some(""), Some("")).argv, ["ssh", "alex"], "empty is not a value");
    }

    /// A quote in a path must not break out of the remote command.
    #[test]
    fn a_quote_in_a_remote_path_stays_quoted() {
        let t = remote(Some("/od'd"), None);
        assert!(t.argv[3].contains(r"'/od'\''d'"), "got {}", t.argv[3]);
    }

    /// A local tile's folder is a real chdir, not part of argv, so the command
    /// stays exactly what was configured.
    #[test]
    fn a_local_folder_is_a_cwd_not_an_argument() {
        let t = local("MACBOOK", Some("/bin/zsh"), Some("~/code/app"));
        assert_eq!(t.folder.as_deref(), Some("~/code/app"), "the config keeps the ~, for display");
        assert_eq!(t.argv, ["/bin/zsh"], "argv unchanged");
        // ...but what gets chdir'd into is a real path: nothing downstream of
        // here expands a tilde.
        let home = std::env::var("HOME").unwrap();
        assert_eq!(t.cwd.as_deref(), Some(&*format!("{home}/code/app")));
        assert_eq!(local("m", None, Some("~")).cwd.as_deref(), Some(&*home), "bare ~ is home");
        assert_eq!(
            local("m", None, Some("/tmp/x")).cwd.as_deref(),
            Some("/tmp/x"),
            "an absolute path is left alone"
        );
        assert_eq!(
            local("m", None, Some("~notauser/x")).cwd.as_deref(),
            Some("~notauser/x"),
            "~user is not ours to guess at"
        );
    }

    /// A local tile with no command is a shell on this machine, which is what
    /// the terminal would have opened anyway.
    #[test]
    fn a_local_tile_with_no_command_runs_the_login_shell() {
        let t = local("MACBOOK", None, None);
        assert_eq!(t.argv, split_command(&crate::platform::login_shell()));
        assert!(t.command.is_none(), "nothing to show on the tile");
        assert!(t.cwd.is_none(), "and nowhere in particular to start");
    }

    /// The local half of the same promise. A tile that *is* a shell has nothing
    /// to outlive and still execs outright; a tile with a command of its own is
    /// followed by a shell, because `echo hi` is a session that would otherwise
    /// end on the line it just printed.
    #[test]
    fn a_local_command_is_followed_by_a_shell_and_a_bare_shell_is_not() {
        assert!(!local("MACBOOK", None, None).shell_after, "a shell is its own session");
        for c in ["echo hi", "nvim", "herdr"] {
            let t = local("MACBOOK", Some(c), None);
            assert!(t.shell_after, "{c} may exit, and the terminal has to survive it");
            assert_eq!(t.argv, split_command(c), "argv is still exactly what was configured");
        }
        // The far side runs its own shell, inside the remote command.
        assert!(!remote(None, Some("echo hi")).shell_after, "not this machine's business");
    }

    /// A remote home is on the far side, so the tilde has to reach the remote
    /// shell as syntax -- while the rest of the path stays quoted.
    #[test]
    fn a_remote_tilde_survives_the_quoting() {
        let cmd = |d: &str| remote(Some(d), None).argv[3].clone();
        assert_eq!(cmd("~/thesis"), "cd ~/'thesis' && exec ${SHELL:-/bin/sh} -l");
        assert_eq!(cmd("~"), "cd ~ && exec ${SHELL:-/bin/sh} -l");
        assert_eq!(cmd("~/my dir"), "cd ~/'my dir' && exec ${SHELL:-/bin/sh} -l");
        // The tilde must not become a licence to inject: everything after it is
        // still one quoted word.
        assert_eq!(cmd("~/od'd && rm -rf x"), r"cd ~/'od'\''d && rm -rf x' && exec ${SHELL:-/bin/sh} -l");
        assert_eq!(cmd("~notauser/x"), "cd '~notauser/x' && exec ${SHELL:-/bin/sh} -l");
    }

    /// Two tiles for one host differ in nothing but where they land, so that is
    /// what the screen and the filter have to go on.
    #[test]
    fn a_duplicates_folder_is_what_tells_it_apart() {
        let t = remote(Some("~/thesis"), Some("tmux attach"));
        assert_eq!(t.note().as_deref(), Some("~/thesis"), "the folder, not the argv");
        assert_eq!(
            remote(None, Some("tmux attach")).note(),
            None,
            "a command alone says nothing about where the tile goes"
        );
        assert!(t.matches("thesis"), "the filter can see the folder");
        assert!(t.matches("tmux"));
        assert!(from_ssh_config(ssh_host("alex"), None).note().is_none(), "nothing to say");
    }
}

