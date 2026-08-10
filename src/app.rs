//! App state and input handling.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::entry::{self, Entry};
use crate::ghostty;
use crate::config::{HostDraft, OpenIn};
use crate::theme::Theme;
use crate::ui::{
    ACCENT_PRESETS, COLOR_ROW, ColorChoice, FIELDS, Form, HIDDEN_ROW, OPEN_ROW,
    PRIMARY_PRESETS, SettingRow,
};
use crate::{config, ssh};

pub enum Mode {
    Browse,
    Filter,
    Add(Form),
    Edit(Form),
    /// Index into `entries` of the tile awaiting a y/n.
    ConfirmDelete(usize),
    /// Focused row in the settings list, plus the colour text being typed.
    Settings { focus: usize, buf: String },
}

pub struct Status {
    pub text: String,
    pub good: bool,
    pub at: Instant,
}

pub struct App {
    pub ssh_config: PathBuf,
    pub entries: Vec<Entry>,
    pub filter: String,
    pub mode: Mode,
    pub sel: usize,
    pub scroll: usize,
    pub hover: Option<usize>,
    pub status: Option<Status>,
    /// Rebuilt every frame; maps a screen region to an index in the visible list.
    pub hitboxes: Vec<(Rect, usize)>,
    pub cols: usize,
    pub quit: bool,
    /// Set when a Local tile is chosen: exec'd after the terminal is restored,
    /// so the command inherits this tab instead of opening a new one.
    pub handoff: Option<Vec<String>>,
    pub include_ssh_config: bool,
    pub tint_tabs: bool,
    pub tab_emoji: bool,
    pub show_hidden: bool,
    pub open_in: OpenIn,
    pub theme: Theme,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            ssh_config: ssh::default_config_path(),
            entries: Vec::new(),
            filter: String::new(),
            mode: Mode::Browse,
            sel: 0,
            scroll: 0,
            hover: None,
            status: None,
            hitboxes: Vec::new(),
            cols: 1,
            quit: false,
            handoff: None,
            include_ssh_config: true,
            tint_tabs: true,
            tab_emoji: true,
            show_hidden: false,
            open_in: OpenIn::Tab,
            theme: Theme::default(),
        };
        app.load(false);
        app
    }

    fn load(&mut self, announce: bool) {
        let (cfg, err) = config::load();
        self.include_ssh_config = cfg.options.include_ssh_config;
        self.tint_tabs = cfg.options.tint_tabs;
        self.tab_emoji = cfg.options.tab_emoji;
        self.show_hidden = cfg.options.show_hidden;
        self.open_in = cfg.options.open_in;
        self.theme = Theme::new(
            cfg.theme.primary.as_deref().unwrap_or(crate::theme::DEFAULT_PRIMARY),
            cfg.theme.accent.as_deref().unwrap_or(crate::theme::DEFAULT_ACCENT),
        );
        self.entries = entry::build(&cfg, &self.ssh_config);
        self.clamp_selection();
        match (err, announce) {
            (Some(e), _) => self.say(e, false),
            (None, true) => {
                let n = self.entries.len();
                self.say(format!("reloaded — {n} tile{}", if n == 1 { "" } else { "s" }), true)
            }
            (None, false) => {}
        }
    }

    pub fn reload(&mut self) {
        self.load(true);
    }

    /// Keep the cursor on a real tile. Must count *visible* entries, not all of
    /// them -- hiding the last tile leaves the hidden one in `entries`, so
    /// clamping against that length strands the cursor past the end.
    pub fn clamp_selection(&mut self) {
        let n = self.visible().len();
        self.sel = self.sel.min(n.saturating_sub(1));
    }

    pub fn setting_rows(&self) -> Vec<SettingRow> {
        vec![
            SettingRow::Toggle {
                key: "include_ssh_config",
                label: "show hosts from ~/.ssh/config",
                on: self.include_ssh_config,
            },
            SettingRow::Toggle {
                key: "tint_tabs",
                label: "tint the new tab's background",
                on: self.tint_tabs,
            },
            SettingRow::Toggle {
                key: "tab_emoji",
                label: "coloured circle in the tab title",
                on: self.tab_emoji,
            },
            SettingRow::Toggle {
                key: "show_hidden",
                label: "reveal hidden hosts",
                on: self.show_hidden,
            },
            SettingRow::Choice {
                key: "open_in",
                label: "open sessions in",
                value: self.open_in.label().to_string(),
            },
            SettingRow::Color {
                key: "primary",
                label: "primary colour",
                value: self.theme.primary_hex.clone(),
            },
            SettingRow::Color {
                key: "accent",
                label: "accent colour",
                value: self.theme.accent_hex.clone(),
            },
        ]
    }

    /// Indices into `entries` that pass the current filter.
    pub fn visible(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.show_hidden || !e.hidden())
            .filter(|(_, e)| self.filter.is_empty() || e.matches(&self.filter))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn say(&mut self, text: String, good: bool) {
        self.status = Some(Status { text, good, at: Instant::now() });
    }

    pub fn expire_status(&mut self) {
        if self.status.as_ref().is_some_and(|s| s.at.elapsed() > Duration::from_secs(6)) {
            self.status = None;
        }
    }

    fn selected_entry(&self, vis: &[usize]) -> Option<usize> {
        vis.get(self.sel).copied()
    }

    /// Put the cursor back on a tile by name, after the list has been rebuilt.
    fn focus_named(&mut self, name: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.label() == name) {
            let vis = self.visible();
            if let Some(s) = vis.iter().position(|&e| e == i) {
                self.sel = s;
            }
        }
    }

    pub fn activate(&mut self, vis: &[usize]) {
        self.activate_in(vis, None);
    }

    /// `force` is a one-off destination from `t`/`w`/`c`; otherwise the tile's
    /// own `open_in`, else the global default.
    pub fn activate_in(&mut self, vis: &[usize], force: Option<OpenIn>) {
        let Some(ei) = self.selected_entry(vis) else { return };
        let e = &self.entries[ei];
        if e.argv.is_empty() {
            self.say(format!("{} has no command to run", e.label), false);
            return;
        }
        let where_to = force.or(e.open_in).unwrap_or(self.open_in);
        let (label, argv) = (e.label.clone(), e.argv.clone());
        let tint = self.tint_tabs.then(|| e.tint.hex.clone());
        let emoji = self.tab_emoji.then_some(e.tint.emoji);

        if where_to == OpenIn::Current {
            // Handed to main() after the terminal is restored, so the command
            // inherits a clean screen and this tab.
            self.handoff = Some(argv);
            self.quit = true;
            return;
        }
        match ghostty::open(where_to, &label, &argv, tint.as_deref(), emoji) {
            Ok(_) => self.say(format!("opened {} — {label}", where_to.label()), true),
            Err(e) => self.say(e, false),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, vis: &[usize]) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match &mut self.mode {
            Mode::Browse => self.key_browse(key, vis),
            Mode::Filter => self.key_filter(key, vis),
            Mode::Add(_) | Mode::Edit(_) => self.key_form(key),
            Mode::ConfirmDelete(_) => self.key_confirm(key),
            Mode::Settings { .. } => self.key_settings(key),
        }
    }

    fn key_browse(&mut self, key: KeyEvent, vis: &[usize]) {
        let cols = self.cols.max(1);
        let last = vis.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
                self.filter.clear();
                self.sel = 0;
            }
            KeyCode::Char('a') => self.mode = Mode::Add(Form::new()),
            KeyCode::Char('e') => self.begin_edit(vis),
            KeyCode::Char('d') => self.begin_delete(vis),
            KeyCode::Char('s') => self.mode = Mode::Settings { focus: 0, buf: String::new() },
            KeyCode::Char('x') => self.toggle_hidden(vis),
            KeyCode::Char('r') => self.reload(),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(vis),
            KeyCode::Char('t') => self.activate_in(vis, Some(OpenIn::Tab)),
            KeyCode::Char('w') => self.activate_in(vis, Some(OpenIn::Window)),
            KeyCode::Char('c') => self.activate_in(vis, Some(OpenIn::Current)),
            KeyCode::Left | KeyCode::Char('h') => self.sel = self.sel.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => self.sel = (self.sel + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.sel.saturating_sub(cols),
            KeyCode::Down | KeyCode::Char('j') => self.sel = (self.sel + cols).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.sel = 0,
            KeyCode::End | KeyCode::Char('G') => self.sel = last,
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < vis.len() {
                    self.sel = i;
                    self.activate(vis);
                }
            }
            _ => {}
        }
    }

    fn key_filter(&mut self, key: KeyEvent, vis: &[usize]) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.filter.clear();
                self.sel = 0;
            }
            KeyCode::Enter => {
                self.mode = Mode::Browse;
                self.activate(vis);
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.sel = 0;
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.sel = 0;
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------- editing

    /// Open a host in the form. Hosts from `~/.ssh/config` are editable too:
    /// saving writes a `[[host]]` override of the same name into config.toml
    /// and leaves the ssh config alone. Their name is the key that links the
    /// two, so it is the one field that can't be changed.
    fn begin_edit(&mut self, vis: &[usize]) {
        let Some(i) = self.selected_entry(vis) else { return };
        let tile = &self.entries[i];
        if tile.is_local() {
            self.say("local tiles are defined in config.toml by hand".into(), false);
            return;
        }

        let name = tile.label.clone();
        let placeholders = [
            String::new(),
            tile.defaults.hostname.clone(),
            tile.defaults.user.clone(),
            tile.defaults.port.clone(),
            tile.defaults.jump.clone(),
        ];
        let from_ssh_config = tile.origin() != Some(entry::Origin::Custom);

        // Only what config.toml actually says goes in the fields; anything
        // inherited stays blank so it keeps tracking ~/.ssh/config.
        let (cfg, _) = config::load();
        let ov = cfg.hosts.iter().find(|h| h.name == name);
        let values = [
            name.clone(),
            ov.and_then(|h| h.hostname.clone()).unwrap_or_default(),
            ov.and_then(|h| h.user.clone()).unwrap_or_default(),
            ov.and_then(|h| h.port.map(|p| p.to_string())).unwrap_or_default(),
            ov.and_then(|h| h.jump.clone()).unwrap_or_default(),
        ];
        self.mode = Mode::Edit(Form::edit(
            values,
            ov.and_then(|h| h.color.as_deref()),
            placeholders,
            from_ssh_config,
            ov.is_some_and(|h| h.hidden),
            ov.and_then(|h| h.open_in),
        ));
    }

    fn key_form(&mut self, key: KeyEvent) {
        let (Mode::Add(form) | Mode::Edit(form)) = &mut self.mode else { return };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Browse,
            KeyCode::Tab | KeyCode::Down => form.move_focus(true),
            KeyCode::BackTab | KeyCode::Up => form.move_focus(false),
            KeyCode::Enter => self.submit_form(),

            // On the colour row the arrows pick a swatch; on a text row they
            // would do nothing useful, so they move between fields instead.
            KeyCode::Left if form.focus == COLOR_ROW => form.color = form.color.cycle(false),
            KeyCode::Right if form.focus == COLOR_ROW => form.color = form.color.cycle(true),
            KeyCode::Backspace if form.focus == COLOR_ROW => {
                form.color = match &form.color {
                    ColorChoice::Custom(s) if s.len() > 1 => {
                        let mut s = s.clone();
                        s.pop();
                        ColorChoice::Custom(s)
                    }
                    _ => ColorChoice::Auto,
                }
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if form.focus == HIDDEN_ROW => {
                form.hidden = !form.hidden
            }
            KeyCode::Left if form.focus == OPEN_ROW => form.cycle_open_in(false),
            KeyCode::Right | KeyCode::Char(' ') if form.focus == OPEN_ROW => {
                form.cycle_open_in(true)
            }
            KeyCode::Char(c) if form.focus == COLOR_ROW => {
                // Typing over a preset starts a fresh hex rather than appending
                // to one the user never wrote.
                form.color = match &form.color {
                    ColorChoice::Custom(s) => ColorChoice::Custom(format!("{s}{c}")),
                    _ => ColorChoice::Custom(if c == '#' { c.into() } else { format!("#{c}") }),
                }
            }

            KeyCode::Backspace => {
                form.values[form.focus].pop();
            }
            KeyCode::Char(c) => form.values[form.focus].push(c),
            _ => {}
        }
    }

    fn submit_form(&mut self) {
        let (Mode::Add(form) | Mode::Edit(form)) = &mut self.mode else { return };
        let v: Vec<String> = form.values.iter().map(|s| s.trim().to_string()).collect();
        let color = form.color.to_config();
        let editing = form.editing.clone();
        let hidden = form.hidden;
        let open_in = form.open_in;

        // Validate before touching the file -- a rejected form keeps what you
        // typed, so a typo costs one keystroke rather than the whole entry.
        let clashes = self
            .entries
            .iter()
            .any(|e| e.label() == v[0] && Some(e.label()) != editing.as_deref());
        let problem = if v[0].is_empty() {
            Some("name is required".to_string())
        } else if v[0].split_whitespace().count() > 1 {
            Some("name cannot contain spaces".to_string())
        } else if clashes {
            Some(format!("{} already exists", v[0]))
        } else if !v[3].is_empty() && v[3].parse::<u16>().is_err() {
            Some("port must be a number".to_string())
        } else if !color.is_empty() && crate::entry::Tint::parse(&color).is_none() {
            Some("color must be #rrggbb".to_string())
        } else {
            None
        };
        if let Some(p) = problem {
            form.error = Some(p);
            return;
        }

        let draft = HostDraft {
            name: v[0].clone(),
            hostname: v[1].clone(),
            user: v[2].clone(),
            port: v[3].clone(),
            jump: v[4].clone(),
            color,
            hidden,
            open_in,
        };
        match config::save_host(editing.as_deref(), &draft) {
            Ok(updated) => {
                let what = match (&editing, updated) {
                    (Some(_), true) => "updated",
                    // First edit of an ~/.ssh/config host: no block existed.
                    (Some(_), false) => "customised",
                    (None, _) => "added",
                };
                let name = v[0].clone();
                self.mode = Mode::Browse;
                self.load(false);
                self.focus_named(&name);
                self.say(format!("{what} {name}"), true);
            }
            Err(e) => form.error = Some(format!("write failed: {e}")),
        }
    }

    /// Hide or reveal the selected host, writing (or creating) its block. A
    /// hidden host stays in ~/.ssh/config and still works as a ProxyJump -- it
    /// just stops taking up a tile.
    fn toggle_hidden(&mut self, vis: &[usize]) {
        let Some(i) = self.selected_entry(vis) else { return };
        if self.entries[i].is_local() {
            self.say("local tiles are defined in config.toml by hand".into(), false);
            return;
        }
        let name = self.entries[i].label().to_string();
        let (cfg, _) = config::load();
        let mut draft = cfg
            .hosts
            .iter()
            .find(|h| h.name == name)
            .map(HostDraft::from)
            .unwrap_or_else(|| HostDraft::named(&name));
        draft.hidden = !draft.hidden;

        match config::save_host(Some(&name), &draft) {
            Ok(_) => {
                self.load(false);
                self.focus_named(&name);
                let msg = if draft.hidden {
                    format!("hid {name} — s to reveal hidden")
                } else {
                    format!("revealed {name}")
                };
                self.say(msg, true);
            }
            Err(e) => self.say(format!("write failed: {e}"), false),
        }
    }

    fn begin_delete(&mut self, vis: &[usize]) {
        let Some(i) = self.selected_entry(vis) else { return };
        if self.entries[i].has_own_block() {
            self.mode = Mode::ConfirmDelete(i);
        } else if self.entries[i].is_local() {
            self.say("local tiles are defined in config.toml by hand".into(), false);
        } else {
            let name = self.entries[i].label();
            self.say(format!("{name} has no customisation to remove"), false);
        }
    }

    fn key_confirm(&mut self, key: KeyEvent) {
        let Mode::ConfirmDelete(i) = self.mode else { return };
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let name = self.entries[i].label().to_string();
                // Removing the block of an ~/.ssh/config host reverts it to
                // whatever that file says; the tile itself stays.
                let reverting = self.entries[i].origin() == Some(entry::Origin::SshOverridden);
                self.mode = Mode::Browse;
                match config::remove_host(&name) {
                    Ok(true) => {
                        self.load(false);
                        self.focus_named(&name);
                        let what = if reverting { "reverted" } else { "deleted" };
                        self.say(format!("{what} {name}"), true);
                    }
                    Ok(false) => self.say(format!("{name} not found in config.toml"), false),
                    Err(e) => self.say(format!("write failed: {e}"), false),
                }
            }
            _ => self.mode = Mode::Browse,
        }
    }

    fn key_settings(&mut self, key: KeyEvent) {
        let Mode::Settings { focus, buf } = &self.mode else { return };
        let (focus_now, mut buf_now) = (*focus, buf.clone());
        let rows = self.setting_rows_len();

        match key.code {
            KeyCode::Esc | KeyCode::Char('s') => {
                self.mode = Mode::Browse;
                return;
            }
            KeyCode::Up | KeyCode::BackTab => {
                let next = (focus_now + rows - 1) % rows;
                self.enter_row(next);
                return;
            }
            KeyCode::Down | KeyCode::Tab => {
                let next = (focus_now + 1) % rows;
                self.enter_row(next);
                return;
            }
            _ => {}
        }

        let row = self.setting_rows().remove(focus_now);
        match row {
            SettingRow::Toggle { key: k, on, .. } => {
                if matches!(
                    key.code,
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right
                ) {
                    self.write_setting(|| config::set_option(k, !on), &format!("{k} = {}", !on));
                }
            }
            SettingRow::Choice { key: k, .. } => {
                let delta: i32 = match key.code {
                    KeyCode::Left => -1,
                    KeyCode::Right | KeyCode::Char(' ') | KeyCode::Enter => 1,
                    _ => return,
                };
                let n = OpenIn::ALL.len() as i32;
                let next = (self.open_in.index() as i32 + delta).rem_euclid(n);
                let v = OpenIn::ALL[next as usize].label();
                self.write_setting(|| config::set_option_str(k, v), &format!("{k} = {v}"));
            }
            SettingRow::Color { key: k, .. } => {
                let presets: &[&str] =
                    if k == "primary" { &PRIMARY_PRESETS } else { &ACCENT_PRESETS };
                match key.code {
                    KeyCode::Left | KeyCode::Right => {
                        let d: i32 = if key.code == KeyCode::Right { 1 } else { -1 };
                        let cur = presets.iter().position(|p| *p == buf_now).unwrap_or(0) as i32;
                        let n = presets.len() as i32;
                        buf_now = presets[(cur + d).rem_euclid(n) as usize].to_string();
                    }
                    KeyCode::Backspace => {
                        buf_now.pop();
                    }
                    KeyCode::Char(c) => {
                        if buf_now.is_empty() && c != '#' {
                            buf_now.push('#');
                        }
                        if buf_now.len() < 7 {
                            buf_now.push(c);
                        }
                    }
                    _ => return,
                }
                // Only a complete, valid hex is written; a half-typed one just
                // sits in the buffer so the UI never flickers through garbage.
                if crate::theme::rgb(&buf_now).is_some() {
                    let v = buf_now.clone();
                    self.write_setting(|| config::set_theme(k, &v), &format!("{k} = {v}"));
                }
                if let Mode::Settings { buf, .. } = &mut self.mode {
                    *buf = buf_now;
                }
            }
        }
    }

    fn setting_rows_len(&self) -> usize {
        self.setting_rows().len()
    }

    /// Move the cursor, seeding the edit buffer from whatever the new row holds.
    fn enter_row(&mut self, next: usize) {
        let seed = match self.setting_rows().remove(next) {
            SettingRow::Color { value, .. } => value,
            _ => String::new(),
        };
        self.mode = Mode::Settings { focus: next, buf: seed };
    }

    /// Write through immediately and reload, so the file and the screen can
    /// never disagree about what is set.
    fn write_setting(
        &mut self,
        write: impl FnOnce() -> std::io::Result<()>,
        said: &str,
    ) {
        let focus_buf = match &self.mode {
            Mode::Settings { focus, buf } => Some((*focus, buf.clone())),
            _ => None,
        };
        match write() {
            Ok(()) => {
                self.load(false);
                self.say(said.to_string(), true);
            }
            Err(e) => self.say(format!("write failed: {e}"), false),
        }
        if let Some((focus, buf)) = focus_buf {
            self.mode = Mode::Settings { focus, buf };
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent, vis: &[usize]) {
        // Modals own the screen; a stray click must not fire a tile behind them.
        if !matches!(self.mode, Mode::Browse | Mode::Filter) {
            return;
        }
        let hit = self
            .hitboxes
            .iter()
            .find(|(r, _)| {
                m.column >= r.x
                    && m.column < r.x + r.width
                    && m.row >= r.y
                    && m.row < r.y + r.height
            })
            .map(|&(_, i)| i);

        match m.kind {
            MouseEventKind::Moved => self.hover = hit,
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = hit {
                    self.sel = i;
                    self.activate(vis);
                }
            }
            MouseEventKind::ScrollDown => self.scroll += 1,
            MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(1),
            _ => {}
        }
    }
}

// Keeps `FIELDS` referenced from one place even though the form draws it.
const _: () = assert!(FIELDS.len() == COLOR_ROW);
