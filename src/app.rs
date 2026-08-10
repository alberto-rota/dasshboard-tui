//! App state and input handling.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::entry::{self, Entry};
use crate::launch::{self, Backend};
use crate::config::{Block, Draft, OpenIn};
use crate::theme::Theme;
use crate::ui::{ACCENT_PRESETS, ColorChoice, Form, KIND_ROW, PRIMARY_PRESETS, SettingRow};
use crate::{config, platform, ssh, startup};

pub enum Mode {
    Browse,
    Filter,
    Add(Form),
    Edit(Form),
    /// Index into `entries` of the tile awaiting a y/n.
    ConfirmDelete(usize),
    /// Focused row in the settings list, plus the colour text being typed.
    Settings { focus: usize, buf: String },
    /// Choosing where to land. `force` carries a one-off destination through
    /// the picker so `w` then a folder still opens a window.
    Folders { entry: usize, sel: usize, force: Option<OpenIn> },
}

/// `~/.zshrc` rather than `/Users/albe/.zshrc`: a settings row has 56 columns.
fn short_home(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match platform::home().to_str() {
        Some(h) if !h.is_empty() && s.starts_with(h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
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
    /// Directory to chdir into before the handoff exec.
    pub handoff_cwd: Option<String>,
    pub include_ssh_config: bool,
    pub tint_tabs: bool,
    pub tab_emoji: bool,
    pub show_hidden: bool,
    pub open_in: OpenIn,
    pub theme: Theme,
    /// Whether the shell rc opens the home screen with a terminal. Lives in the
    /// rc, not config.toml, so it is read on load rather than every frame.
    pub startup: startup::State,
    /// What this terminal can do with a session. Read once: it is a property of
    /// the environment we were started in, which cannot change under us.
    pub backend: Backend,
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
            handoff_cwd: None,
            include_ssh_config: true,
            tint_tabs: true,
            tab_emoji: true,
            show_hidden: false,
            open_in: OpenIn::Tab,
            theme: Theme::default(),
            startup: startup::State::Off,
            backend: launch::backend(),
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
        self.startup = startup::state();
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
        // Three rows describe something only a tab-opening terminal can do.
        // They stay visible and stay editable -- a config is often written on
        // one machine and read on another -- but they say when they are inert
        // here rather than pretending to work.
        let inert = self.backend.note();
        vec![
            // First, because it is the only one that changes what happens
            // outside this process -- and it starts off.
            SettingRow::Startup {
                label: "open with a new terminal",
                on: self.startup.is_on(),
                detail: match startup::rc_path() {
                    Ok(rc) => short_home(&rc),
                    Err(_) => "unsupported shell".into(),
                },
            },
            SettingRow::Toggle {
                key: "include_ssh_config",
                label: "show hosts from ~/.ssh/config",
                on: self.include_ssh_config,
                note: "",
            },
            SettingRow::Toggle {
                key: "tint_tabs",
                label: "tint the new tab's background",
                on: self.tint_tabs,
                note: inert,
            },
            SettingRow::Toggle {
                key: "tab_emoji",
                label: "coloured circle in the tab title",
                on: self.tab_emoji,
                note: inert,
            },
            SettingRow::Toggle {
                key: "show_hidden",
                label: "reveal hidden hosts",
                on: self.show_hidden,
                note: "",
            },
            SettingRow::Choice {
                key: "open_in",
                label: "open sessions in",
                // The stored value, not the resolved one: the row has to agree
                // with what the arrows are cycling through, and the note is
                // what says the answer lands here regardless.
                value: self.open_in.label().to_string(),
                note: if self.backend.can_spawn() { "" } else { "opens here" },
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
        if self.entries[ei].argv.is_empty() {
            self.say(format!("{} has no command to run", self.entries[ei].label), false);
            return;
        }
        // With folders configured there is a choice to make, so ask rather than
        // guessing which one you meant.
        if !self.entries[ei].folders.is_empty() {
            self.mode = Mode::Folders { entry: ei, sel: 0, force };
            return;
        }
        self.launch(ei, None, force);
    }

    fn launch(&mut self, ei: usize, dir: Option<&str>, force: Option<OpenIn>) {
        let e = &self.entries[ei];
        // The tile asks; the terminal answers. Where new surfaces are not
        // available every destination collapses to "here", so a config written
        // on a Mac still opens the right host on a Linux or Windows box.
        let where_to = self.backend.resolve(force.or(e.open_in).unwrap_or(self.open_in));
        let label = e.label.clone();
        let argv = e.argv_in(dir);
        let cwd = e.local_cwd(dir);
        let tint = self.tint_tabs.then(|| e.tint.hex.clone());
        let emoji = self.tab_emoji.then_some(e.tint.emoji);
        let where_text =
            dir.map_or_else(|| where_to.label().to_string(), |d| format!("{} · {d}", where_to.label()));

        if where_to == OpenIn::Current {
            // Handed to main() after the terminal is restored, so the command
            // inherits a clean screen and this tab.
            self.handoff = Some(argv);
            self.handoff_cwd = cwd;
            self.quit = true;
            return;
        }
        match launch::spawn(where_to, &label, &argv, cwd.as_deref(), tint.as_deref(), emoji) {
            Ok(_) => self.say(format!("opened {where_text} — {label}"), true),
            Err(e) => self.say(e, false),
        }
    }

    fn key_folders(&mut self, key: KeyEvent) {
        let Mode::Folders { entry, sel, force } = self.mode else { return };
        let n = self.entries[entry].folders.len() + 1;
        match key.code {
            KeyCode::Esc => self.mode = Mode::Browse,
            KeyCode::Up | KeyCode::Char('k') => {
                self.mode = Mode::Folders { entry, sel: (sel + n - 1) % n, force }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.mode = Mode::Folders { entry, sel: (sel + 1) % n, force }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < n {
                    self.mode = Mode::Browse;
                    let dir = (i > 0).then(|| self.entries[entry].folders[i - 1].clone());
                    self.launch(entry, dir.as_deref(), force);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.mode = Mode::Browse;
                let dir = (sel > 0).then(|| self.entries[entry].folders[sel - 1].clone());
                self.launch(entry, dir.as_deref(), force);
            }
            _ => {}
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
            Mode::Folders { .. } => self.key_folders(key),
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
            KeyCode::Char('a') => self.mode = Mode::Add(Form::new(Block::Host)),
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

    /// Open a tile in the form. Hosts from `~/.ssh/config` are editable too:
    /// saving writes a `[[host]]` override of the same name into config.toml
    /// and leaves the ssh config alone. Their name is the key that links the
    /// two, so it is the one field that can't be changed.
    fn begin_edit(&mut self, vis: &[usize]) {
        let Some(i) = self.selected_entry(vis) else { return };
        let tile = &self.entries[i];
        let name = tile.label.clone();
        let (cfg, _) = config::load();

        let mut form = if tile.is_local() {
            let Some(l) = cfg.locals.iter().find(|l| l.label == name) else {
                self.say(format!("{name} is not in config.toml"), false);
                return;
            };
            let mut f = Form::new(Block::Local);
            f.set("label", &l.label);
            f.set("command", &l.command);
            f.set("detail", &l.detail);
            f.set("folders", l.folders.join(", "));
            f.color = ColorChoice::from_config(l.color.as_deref());
            f.hidden = l.hidden;
            f.open_in = l.open_in;
            f
        } else {
            let ov = cfg.hosts.iter().find(|h| h.name == name);
            let mut f = Form::new(Block::Host);
            f.set("name", &name);
            // Only what config.toml actually says goes in the fields; anything
            // inherited stays blank so it keeps tracking ~/.ssh/config.
            f.set("hostname", ov.and_then(|h| h.hostname.clone()).unwrap_or_default());
            f.set("user", ov.and_then(|h| h.user.clone()).unwrap_or_default());
            f.set("port", ov.and_then(|h| h.port.map(|p| p.to_string())).unwrap_or_default());
            f.set("jump", ov.and_then(|h| h.jump.clone()).unwrap_or_default());
            f.set("folders", ov.map(|h| h.folders.join(", ")).unwrap_or_default());
            f.set_placeholder("hostname", tile.defaults.hostname.clone());
            f.set_placeholder("user", tile.defaults.user.clone());
            f.set_placeholder("port", tile.defaults.port.clone());
            f.set_placeholder("jump", tile.defaults.jump.clone());
            f.color = ColorChoice::from_config(ov.and_then(|h| h.color.as_deref()));
            f.hidden = ov.is_some_and(|h| h.hidden);
            f.open_in = ov.and_then(|h| h.open_in);
            f.name_locked = tile.origin() != Some(entry::Origin::Custom);
            f
        };
        form.editing = Some(name);
        form.kind_locked = true;
        form.focus = form.first_field() + usize::from(form.name_locked);
        self.mode = Mode::Edit(form);
    }

    fn key_form(&mut self, key: KeyEvent) {
        let (Mode::Add(form) | Mode::Edit(form)) = &mut self.mode else { return };
        let (color_row, hidden_row, open_row) =
            (form.color_row(), form.hidden_row(), form.open_row());
        let first = form.first_field();

        match key.code {
            KeyCode::Esc => self.mode = Mode::Browse,
            KeyCode::Tab | KeyCode::Down => form.move_focus(true),
            KeyCode::BackTab | KeyCode::Up => form.move_focus(false),
            KeyCode::Enter => self.submit_form(),

            KeyCode::Left if form.focus == KIND_ROW => form.switch_block(Block::Host),
            KeyCode::Right if form.focus == KIND_ROW => form.switch_block(Block::Local),
            KeyCode::Char(' ') if form.focus == KIND_ROW => {
                let other =
                    if form.block == Block::Host { Block::Local } else { Block::Host };
                form.switch_block(other);
            }

            // On the toggle rows the arrows change the value; on a text row
            // they would do nothing useful, so they move between fields.
            KeyCode::Left if form.focus == color_row => form.color = form.color.cycle(false),
            KeyCode::Right if form.focus == color_row => form.color = form.color.cycle(true),
            KeyCode::Backspace if form.focus == color_row => {
                form.color = match &form.color {
                    ColorChoice::Custom(s) if s.len() > 1 => {
                        let mut s = s.clone();
                        s.pop();
                        ColorChoice::Custom(s)
                    }
                    _ => ColorChoice::Auto,
                }
            }
            KeyCode::Char(c) if form.focus == color_row => {
                // Typing over a preset starts a fresh hex rather than appending
                // to one the user never wrote.
                form.color = match &form.color {
                    ColorChoice::Custom(s) => ColorChoice::Custom(format!("{s}{c}")),
                    _ => ColorChoice::Custom(if c == '#' { c.into() } else { format!("#{c}") }),
                }
            }

            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if form.focus == hidden_row => {
                form.hidden = !form.hidden
            }
            KeyCode::Left if form.focus == open_row => form.cycle_open_in(false),
            KeyCode::Right | KeyCode::Char(' ') if form.focus == open_row => {
                form.cycle_open_in(true)
            }

            KeyCode::Backspace if form.focus >= first && form.focus < color_row => {
                form.fields[form.focus - first].value.pop();
            }
            KeyCode::Char(c) if form.focus >= first && form.focus < color_row => {
                form.fields[form.focus - first].value.push(c);
            }
            _ => {}
        }
    }

    fn submit_form(&mut self) {
        let (Mode::Add(form) | Mode::Edit(form)) = &mut self.mode else { return };
        let block = form.block;
        let name = form.fields[0].value.trim().to_string();
        let editing = form.editing.clone();
        let color = form.color.to_config();

        // Validate before touching the file -- a rejected form keeps what you
        // typed, so a typo costs one keystroke rather than the whole entry.
        let clashes = self
            .entries
            .iter()
            .any(|e| e.label == name && Some(e.label.as_str()) != editing.as_deref());
        let port = form.field("port").trim().to_string();
        let problem = if name.is_empty() {
            Some(format!("{} is required", if block == Block::Host { "name" } else { "label" }))
        } else if name.split_whitespace().count() > 1 {
            Some("name cannot contain spaces".to_string())
        } else if clashes {
            Some(format!("{name} already exists"))
        } else if block == Block::Local && form.field("command").trim().is_empty() {
            Some("command is required".to_string())
        } else if !port.is_empty() && port.parse::<u16>().is_err() {
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

        let draft = Draft {
            block,
            name: name.clone(),
            hostname: form.field("hostname").trim().into(),
            user: form.field("user").trim().into(),
            port,
            jump: form.field("jump").trim().into(),
            command: form.field("command").trim().into(),
            detail: form.field("detail").trim().into(),
            folders: form.folder_list(),
            color,
            hidden: form.hidden,
            open_in: form.open_in,
        };

        match config::save_block(editing.as_deref(), &draft) {
            Ok(updated) => {
                let what = match (&editing, updated) {
                    (Some(_), true) => "updated",
                    // First edit of an ~/.ssh/config host: no block existed.
                    (Some(_), false) => "customised",
                    (None, _) => "added",
                };
                self.mode = Mode::Browse;
                self.load(false);
                self.focus_named(&name);
                self.say(format!("{what} {name}"), true);
            }
            Err(e) => form.error = Some(format!("write failed: {e}")),
        }
    }

    /// Hide or reveal the selected tile, writing (or creating) its block. A
    /// hidden host stays in ~/.ssh/config and still works as a ProxyJump -- it
    /// just stops taking up a tile.
    fn toggle_hidden(&mut self, vis: &[usize]) {
        let Some(i) = self.selected_entry(vis) else { return };
        let name = self.entries[i].label.clone();
        let (cfg, _) = config::load();
        let mut draft = if self.entries[i].is_local() {
            match cfg.locals.iter().find(|l| l.label == name) {
                Some(l) => Draft::from(l),
                None => {
                    self.say(format!("{name} is not in config.toml"), false);
                    return;
                }
            }
        } else {
            cfg.hosts
                .iter()
                .find(|h| h.name == name)
                .map(Draft::from)
                .unwrap_or_else(|| Draft::host(&name))
        };
        draft.hidden = !draft.hidden;

        match config::save_block(Some(&name), &draft) {
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
        if self.entries[i].has_own_block() || self.entries[i].is_local() {
            self.mode = Mode::ConfirmDelete(i);
        } else {
            let name = &self.entries[i].label;
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
                let block =
                    if self.entries[i].is_local() { Block::Local } else { Block::Host };
                self.mode = Mode::Browse;
                match config::remove_block(block, &name) {
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
            SettingRow::Startup { on, .. } => {
                if matches!(
                    key.code,
                    KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Left | KeyCode::Right
                ) {
                    self.toggle_startup(on);
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

    /// The shell rc, not config.toml -- so it says which file it touched, and
    /// where the pre-install copy went the first time.
    fn toggle_startup(&mut self, on: bool) {
        let said = if on {
            startup::disable().map(|(rc, removed)| {
                if removed {
                    format!("startup off — {} restored", short_home(&rc))
                } else {
                    "startup was already off".to_string()
                }
            })
        } else {
            startup::enable()
                .map(|rc| format!("startup on — {} (backup {})", short_home(&rc), short_home(&startup::backup_path(&rc))))
        };
        match said {
            Ok(msg) => {
                self.startup = startup::state();
                self.say(msg, true);
            }
            Err(e) => self.say(e, false),
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
