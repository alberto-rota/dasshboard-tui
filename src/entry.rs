//! Turning `~/.ssh/config` and `config.toml` into the tiles on screen.

use std::path::Path;

use ratatui::style::Color;

use crate::config::{Config, HostEntry, OpenIn};
use crate::ssh;

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
    /// The whole command, already split -- never re-parsed by a shell.
    pub argv: Vec<String>,
    pub kind: Kind,
    pub origin: Option<Origin>,
    pub tint: Tint,
    pub hidden: bool,
    /// `None` defers to `[options] open_in`.
    pub open_in: Option<OpenIn>,
    /// Directories this tile can start in. More than zero turns activating it
    /// into a picker.
    pub folders: Vec<String>,
    pub defaults: Defaults,
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

    /// `d` has something to undo only when config.toml holds a block for it.
    pub fn has_own_block(&self) -> bool {
        matches!(self.origin, Some(Origin::Custom | Origin::SshOverridden))
    }

    /// The argv for one launch, with the chosen directory applied.
    ///
    /// Local commands just start there. For ssh the directory is on the far
    /// side, so it becomes a remote command: `-t` forces a tty (without it the
    /// remote shell is not interactive), and `exec $SHELL -l` replaces it so
    /// you get a login shell in that folder rather than a bare `sh`.
    pub fn argv_in(&self, dir: Option<&str>) -> Vec<String> {
        let (Some(dir), Kind::Ssh) = (dir.filter(|d| !d.is_empty()), self.kind) else {
            return self.argv.clone();
        };
        let mut argv = self.argv.clone();
        argv.push("-t".into());
        argv.push(format!("cd {} && exec ${{SHELL:-/bin/sh}} -l", sh_quote_path(dir)));
        argv
    }

    /// A local tile's directory is applied by the launcher, not baked into
    /// argv, so it is reported separately -- with `~` resolved, since the
    /// launcher is a `chdir` or a quoted `cd` and neither expands it.
    pub fn local_cwd(&self, dir: Option<&str>) -> Option<String> {
        matches!(self.kind, Kind::Local)
            .then_some(dir)
            .flatten()
            .filter(|d| !d.is_empty())
            .map(expand_home)
    }

    pub fn matches(&self, needle: &str) -> bool {
        let n = needle.to_lowercase();
        self.label.to_lowercase().contains(&n)
            || self.detail.to_lowercase().contains(&n)
            || self.jump().is_some_and(|j| j.to_lowercase().contains(&n))
    }
}

/// Resolve a leading `~` against the home directory, for a path this machine
/// will read.
///
/// Nothing downstream does it for us: a local folder becomes either a real
/// `chdir` or a `cd` inside single quotes, and neither expands a tilde -- which
/// is why `folders = ["~/Desktop"]` reported that the folder did not exist.
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
        folders: ov.map(|o| o.folders.clone()).unwrap_or_default(),
        kind: Kind::Ssh,
        label: h.alias,
        detail,
        jump,
        argv,
        defaults,
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
        folders: e.folders.clone(),
        defaults: Defaults::default(),
    }
}

/// Local tiles first, then ssh hosts: the config's own, then yours.
pub fn build(cfg: &Config, ssh_config: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = cfg
        .locals
        .iter()
        .map(|l| Entry {
            tint: Tint::resolve(l.color.as_ref(), &l.label),
            label: l.label.clone(),
            detail: l.detail.clone(),
            jump: None,
            argv: split_command(&l.command),
            kind: Kind::Local,
            origin: None,
            hidden: l.hidden,
            open_in: l.open_in,
            folders: l.folders.clone(),
            defaults: Defaults::default(),
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
            }
            entries.push(from_ssh_config(h, ov));
        }
    }
    entries.extend(
        cfg.hosts.iter().filter(|h| !claimed.contains(&h.name.as_str())).map(from_config),
    );

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HostEntry;

    fn entry(
        name: &str,
        hostname: Option<&str>,
        user: Option<&str>,
        port: Option<u16>,
        jump: Option<&str>,
    ) -> Entry {
        from_config(&HostEntry {
            name: name.into(),
            hostname: hostname.map(Into::into),
            user: user.map(Into::into),
            port,
            jump: jump.map(Into::into),
            color: None,
            hidden: false,
            open_in: None,
            folders: Vec::new(),
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
            name: name.into(),
            hostname: None,
            user: user.map(Into::into),
            port: None,
            jump: None,
            color: color.map(Into::into),
            hidden: false,
            open_in: None,
            folders: Vec::new(),
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

        let entries = build(&cfg, &sshcfg);
        let names: Vec<&str> = entries.iter().map(|e| e.label()).collect();
        assert_eq!(names, ["alex", "brand-new"], "one tile per name");
        assert_eq!(entries[0].tint().hex, "#9a68b0");
        assert_eq!(entries[0].origin(), Some(Origin::SshOverridden));
        assert_eq!(entries[1].origin(), Some(Origin::Custom));
    }

    /// The remote directory can't be a local `cd`, so it becomes a remote
    /// command. `-t` is what makes it an interactive shell rather than a
    /// one-shot; `exec $SHELL -l` is what leaves you in a login shell there.
    #[test]
    fn an_ssh_folder_becomes_a_remote_command() {
        let t = from_ssh_config(ssh_host("alex"), None);
        let argv = t.argv_in(Some("/scratch/project"));
        assert_eq!(argv[..2], ["ssh", "alex"], "the alias still leads");
        assert_eq!(argv[2], "-t", "a tty is required for an interactive shell");
        assert_eq!(argv[3], "cd '/scratch/project' && exec ${SHELL:-/bin/sh} -l");
        assert!(t.local_cwd(Some("/scratch/project")).is_none(), "not a local cd");
    }

    #[test]
    fn no_folder_leaves_the_command_untouched() {
        let t = from_ssh_config(ssh_host("alex"), None);
        assert_eq!(t.argv_in(None), t.argv);
        assert_eq!(t.argv_in(Some("")), t.argv, "an empty choice is no choice");
    }

    /// A quote in a path must not break out of the remote command.
    #[test]
    fn a_quote_in_a_remote_path_stays_quoted() {
        let t = from_ssh_config(ssh_host("alex"), None);
        let argv = t.argv_in(Some("/od'd"));
        assert!(argv[3].contains(r"'/od'\''d'"), "got {}", argv[3]);
    }

    /// A local tile's folder is a real chdir, not part of argv, so the command
    /// stays exactly what was configured.
    #[test]
    fn a_local_folder_is_a_cwd_not_an_argument() {
        let mut cfg = Config::default();
        cfg.options.include_ssh_config = false;
        cfg.locals.push(crate::config::LocalEntry {
            label: "MACBOOK".into(),
            detail: "local shell".into(),
            command: "/bin/zsh".into(),
            color: None,
            hidden: false,
            open_in: None,
            folders: vec!["~/code/app".into()],
        });
        let entries = build(&cfg, Path::new("/nonexistent"));
        let t = &entries[0];
        assert_eq!(t.folders, ["~/code/app"], "the config keeps the ~, for display");
        assert_eq!(t.argv_in(Some("~/code/app")), ["/bin/zsh"], "argv unchanged");
        // ...but what gets chdir'd into is a real path: nothing downstream of
        // here expands a tilde.
        let home = std::env::var("HOME").unwrap();
        assert_eq!(t.local_cwd(Some("~/code/app")).as_deref(), Some(&*format!("{home}/code/app")));
        assert_eq!(t.local_cwd(Some("~")).as_deref(), Some(&*home), "bare ~ is home");
        assert_eq!(
            t.local_cwd(Some("/tmp/x")).as_deref(),
            Some("/tmp/x"),
            "an absolute path is left alone"
        );
        assert_eq!(
            t.local_cwd(Some("~notauser/x")).as_deref(),
            Some("~notauser/x"),
            "~user is not ours to guess at"
        );
    }

    /// A remote home is on the far side, so the tilde has to reach the remote
    /// shell as syntax -- while the rest of the path stays quoted.
    #[test]
    fn a_remote_tilde_survives_the_quoting() {
        let t = from_ssh_config(ssh_host("alex"), None);
        let cmd = |d| t.argv_in(Some(d))[3].clone();
        assert_eq!(cmd("~/thesis"), "cd ~/'thesis' && exec ${SHELL:-/bin/sh} -l");
        assert_eq!(cmd("~"), "cd ~ && exec ${SHELL:-/bin/sh} -l");
        assert_eq!(cmd("~/my dir"), "cd ~/'my dir' && exec ${SHELL:-/bin/sh} -l");
        // The tilde must not become a licence to inject: everything after it is
        // still one quoted word.
        assert_eq!(cmd("~/od'd && rm -rf x"), r"cd ~/'od'\''d && rm -rf x' && exec ${SHELL:-/bin/sh} -l");
        assert_eq!(cmd("~notauser/x"), "cd '~notauser/x' && exec ${SHELL:-/bin/sh} -l");
    }
}

