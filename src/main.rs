//! dasshboard -- a home screen for Ghostty.
//!
//! Draws every host in `~/.ssh/config` plus anything in `config.toml` as a
//! clickable tile. SSH hosts open in a new Ghostty tab tinted with the host's
//! colour; local tiles take over the tab you are already in. Quitting drops you
//! into the shell that launched it.

mod app;
mod config;
mod entry;
mod ghostty;
mod ssh;
mod theme;
mod ui;

use std::io::{self, Stdout};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use app::App;

fn main() -> io::Result<()> {
    if let Some(code) = run_cli()? {
        std::process::exit(code);
    }

    let mut app = App::new();
    let mut terminal = setup()?;
    let res = run(&mut terminal, &mut app);
    restore()?;
    res?;

    // Only reached once the terminal is back to normal, so the handed-off
    // program starts on a clean screen and owns this tab from here on.
    if let Some(argv) = app.handoff {
        let err = Command::new(&argv[0]).args(&argv[1..]).exec();
        eprintln!("dasshboard: could not exec {}: {err}", argv[0]);
        std::process::exit(1);
    }
    Ok(())
}

/// Non-TUI entry points. Returns `Some(exit_code)` when one of them ran.
fn run_cli() -> io::Result<Option<i32>> {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else { return Ok(None) };

    match first.as_str() {
        "--list" => {
            let (cfg, err) = config::load();
            if let Some(e) = err {
                eprintln!("{e}");
            }
            for e in entry::build(&cfg, &ssh::default_config_path()) {
                let kind = if e.is_local() { "local" } else { "ssh" };
                let mark = if e.hidden() { "hidden" } else { "" };
                let t = e.tint();
                println!(
                    "{kind:<6} {} {:<8} {:<16} {:<28} {mark}",
                    t.emoji,
                    t.hex,
                    e.label(),
                    e.detail()
                );
            }
            Ok(Some(0))
        }
        "--open" => {
            let Some(name) = args.next() else {
                eprintln!("usage: dasshboard --open <host>");
                return Ok(Some(2));
            };
            let (cfg, _) = config::load();
            let entries = entry::build(&cfg, &ssh::default_config_path());
            // Prefer a configured tile so its args and colour are used; fall
            // back to treating the argument as a bare ssh target.
            let found = entries.iter().find(|e| e.label == name && !e.is_local());
            let (argv, tint) = match found {
                Some(t) => (t.argv.clone(), t.tint.clone()),
                None => (vec!["ssh".to_string(), name.clone()], entry::Tint::for_name(&name)),
            };
            let emoji = cfg.options.tab_emoji.then_some(tint.emoji);
            let hex = cfg.options.tint_tabs.then_some(tint.hex);
            let where_to = found.and_then(|t| t.open_in).unwrap_or(cfg.options.open_in);
            // --open is a spawn, so "current" would mean this short-lived CLI
            // process; a tab is the only sensible reading.
            let where_to = if where_to == config::OpenIn::Current {
                config::OpenIn::Tab
            } else {
                where_to
            };
            match ghostty::open(where_to, &name, &argv, hex.as_deref(), emoji) {
                Ok(id) => {
                    println!("{id}");
                    Ok(Some(0))
                }
                Err(e) => {
                    eprintln!("{e}");
                    Ok(Some(1))
                }
            }
        }
        "--config" => {
            println!("{}", config::path().display());
            Ok(Some(0))
        }
        "--help" | "-h" => {
            println!(
                "dasshboard [--list | --open <host> | --config | --help]\n\n\
                 With no arguments, launches the TUI."
            );
            Ok(Some(0))
        }
        other => {
            eprintln!("unknown argument: {other}");
            Ok(Some(2))
        }
    }
}

fn setup() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    // Without this a panic leaves the terminal in raw mode with no echo.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        hook(info);
    }));

    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn restore() -> io::Result<()> {
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(200))? {
            let vis = app.visible();
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k, &vis),
                Event::Mouse(m) => app.on_mouse(m, &vis),
                _ => {}
            }
        }

        app.expire_status();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use app::Mode;
    use entry::Entry;
    use ratatui::backend::TestBackend;
    use ui::Form;

    fn frame(w: u16, h: u16, prep: impl FnOnce(&mut App)) -> (App, String) {
        let mut app = App::new();
        prep(&mut app);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        let out = format!("{}", term.backend());
        (app, out)
    }

    /// Not an assertion -- run with `--nocapture` to eyeball the layout.
    #[test]
    fn layout_dump() {
        for (label, w, h, prep) in [
            ("120x30", 120u16, 30u16, None::<fn(&mut App)>),
            ("120x30 selected 3", 120, 30, Some((|a: &mut App| a.sel = 3) as fn(&mut App))),
            ("84x22", 84, 22, None),
            ("54x18 narrow", 54, 18, None),
        ] {
            let (_, out) = frame(w, h, |a| {
                if let Some(p) = prep {
                    p(a)
                }
            });
            println!("=== {label} ===\n{out}");
        }
        let (_, add) = frame(120, 30, |a| a.mode = Mode::Add(Form::new()));
        println!("=== add form ===\n{add}");
        let (_, del) = frame(120, 30, |a| a.mode = Mode::ConfirmDelete(0));
        println!("=== confirm ===\n{del}");
        let (_, edit) = frame(120, 30, |a| {
            let vis = a.visible();
            if let Some(s) = vis.iter().position(|&i| a.entries[i].origin().is_some()) {
                a.sel = s;
                a.on_key(
                    ratatui::crossterm::event::KeyEvent::from(
                        ratatui::crossterm::event::KeyCode::Char('e'),
                    ),
                    &vis,
                );
            }
        });
        println!("=== edit an ssh-config host ===\n{edit}");
        let (_, set) = frame(120, 30, |a| a.mode = Mode::Settings { focus: 5, buf: "#aaaaaa".into() });
        println!("=== settings ===\n{set}");
        let (_, filt) = frame(120, 30, |a| {
            a.mode = Mode::Filter;
            a.filter = "c".into();
        });
        println!("=== filter ===\n{filt}");
    }

    #[test]
    fn columns_shrink_with_width() {
        // Tiles are a fixed 38 wide, so these are floor((w - 2*PAD) / 38), capped at 4.
        for (w, want) in [(200u16, 4usize), (156, 4), (150, 3), (120, 3), (80, 2), (44, 1)] {
            let (app, _) = frame(w, 30, |_| {});
            assert_eq!(app.cols, want, "width {w}");
        }
    }

    #[test]
    fn every_tile_is_clickable_and_none_overlap() {
        let (app, _) = frame(120, 30, |_| {});
        assert_eq!(app.hitboxes.len(), app.visible().len());
        for (i, (a, _)) in app.hitboxes.iter().enumerate() {
            for (b, _) in &app.hitboxes[i + 1..] {
                let disjoint = a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y;
                assert!(disjoint, "{a:?} overlaps {b:?}");
            }
        }
    }

    /// Tiles must stay inside the drawing area at any size, or the grid bleeds
    /// over the footer.
    #[test]
    fn tiles_never_escape_the_viewport() {
        for (w, h) in [(150u16, 40u16), (120, 30), (80, 24), (44, 14), (30, 10)] {
            let (app, _) = frame(w, h, |_| {});
            for (r, _) in &app.hitboxes {
                assert!(r.x + r.width <= w, "{w}x{h}: {r:?} past right edge");
                assert!(r.y + r.height <= h, "{w}x{h}: {r:?} past bottom edge");
            }
        }
    }

    /// `open_in = current` is the one destination that doesn't call Ghostty:
    /// it execs in this process once the terminal is restored. Every other
    /// destination would spawn a real tab, so this is the only one a test may
    /// exercise.
    #[test]
    fn open_in_current_hands_off_instead_of_spawning() {
        use config::OpenIn;
        let mut app = App::new();
        let Some(i) = app.entries.iter().position(|e| !e.hidden()) else { return };
        app.entries[i].open_in = Some(OpenIn::Current);
        let vis = app.visible();
        app.sel = vis.iter().position(|&e| e == i).unwrap();
        let argv = app.entries[i].argv.clone();
        app.activate(&vis);
        assert!(app.quit, "must leave the TUI");
        assert_eq!(app.handoff.as_deref(), Some(argv.as_slice()));
        assert!(app.status.is_none(), "handoff should not report a spawned tab");
    }

    /// A one-off key beats the tile's own setting, which beats the global.
    /// Only `current` can be asserted -- the others would spawn real tabs.
    #[test]
    fn the_destination_is_resolved_most_specific_first() {
        use config::OpenIn;
        let pick = |a: &App| a.entries.iter().position(|e| !e.hidden());

        let mut app = App::new();
        let Some(i) = pick(&app) else { return };
        let vis = app.visible();
        let sel = vis.iter().position(|&e| e == i).unwrap();

        // Global says tab, tile says current: the tile wins, so this hands off.
        app.sel = sel;
        app.open_in = OpenIn::Tab;
        app.entries[i].open_in = Some(OpenIn::Current);
        app.activate(&vis);
        assert!(app.handoff.is_some(), "tile setting beats the global");

        // Global says current, tile silent: the global applies.
        let mut app = App::new();
        app.sel = sel;
        app.open_in = OpenIn::Current;
        app.entries[i].open_in = None;
        app.activate(&vis);
        assert!(app.handoff.is_some(), "global applies when the tile is silent");

        // And a one-off beats both.
        let mut app = App::new();
        app.sel = sel;
        app.open_in = OpenIn::Tab;
        app.entries[i].open_in = Some(OpenIn::Tab);
        app.activate_in(&vis, Some(OpenIn::Current));
        assert!(app.handoff.is_some(), "the one-off key wins");
    }

    fn press(app: &mut App, code: ratatui::crossterm::event::KeyCode) {
        let vis = app.visible();
        app.on_key(ratatui::crossterm::event::KeyEvent::from(code), &vis);
    }

    fn select(app: &mut App, pred: impl Fn(&Entry) -> bool) -> bool {
        let Some(i) = app.entries.iter().position(pred) else { return false };
        let vis = app.visible();
        match vis.iter().position(|&e| e == i) {
            Some(s) => {
                app.sel = s;
                true
            }
            None => false,
        }
    }

    /// An ~/.ssh/config host is editable -- the override goes to config.toml --
    /// but its name is the key joining the two files, so it stays locked.
    #[test]
    fn ssh_config_hosts_open_for_edit_with_a_locked_name() {
        use entry::Origin;
        let mut app = App::new();
        if !select(&mut app, |e| e.origin() == Some(Origin::Ssh)) {
            return;
        }
        let name = app.entries[app.visible()[app.sel]].label().to_string();
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('e'));
        match &app.mode {
            Mode::Edit(f) => {
                assert!(f.name_locked, "the alias joins the two files");
                assert_eq!(f.values[0], name);
                assert_ne!(f.focus, 0, "focus must skip the locked field");
                // Blank fields track ~/.ssh/config, shown as placeholders.
                assert!(f.values[1..].iter().all(String::is_empty), "no override yet");
                assert!(f.placeholders.iter().any(|p| !p.is_empty()), "inherits something");
            }
            _ => panic!("ssh config hosts must be editable"),
        }
    }

    /// `d` only offers to undo something that exists. A plain ~/.ssh/config
    /// host has no block of ours, so there is nothing to remove.
    #[test]
    fn delete_declines_when_there_is_no_customisation() {
        use entry::Origin;
        let mut app = App::new();
        if !select(&mut app, |e| e.origin() == Some(Origin::Ssh)) {
            return;
        }
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('d'));
        assert!(matches!(app.mode, Mode::Browse), "no dialog for a no-op");
        assert!(app.status.as_ref().is_some_and(|s| !s.good), "must say why");
    }

    /// A hidden host leaves the screen but not the config, and `s` is the way
    /// back to it.
    /// Counts come from the live config rather than being assumed, since these
    /// tests run against whatever the user actually has configured.
    #[test]
    fn hidden_hosts_drop_out_of_view_until_revealed() {
        let mut app = App::new();
        app.show_hidden = false;
        let total = app.entries.len();
        let Some(i) = app.entries.iter().position(|e| !e.is_local() && !e.hidden()) else {
            return;
        };
        let before = app.visible().len();

        // Simulate what `x` writes, without touching the real config file.
        app.entries[i].hidden = true;
        assert_eq!(app.visible().len(), before - 1, "hidden host leaves the grid");
        assert_eq!(app.entries.len(), total, "but stays in the list");

        app.show_hidden = true;
        assert_eq!(app.visible().len(), total, "revealed again by the setting");
    }

    /// Hiding the selected last tile must not leave the cursor past the end.
    /// The clamp has to count visible tiles, not all of them.
    #[test]
    fn hiding_clamps_the_selection_to_what_is_visible() {
        let mut app = App::new();
        app.sel = app.visible().len() - 1;
        for e in app.entries.iter_mut() {
            e.hidden = true;
        }
        app.clamp_selection();
        let vis = app.visible();
        assert!(
            vis.is_empty() || app.sel < vis.len(),
            "sel {} past the {} visible tiles",
            app.sel,
            vis.len()
        );
    }

    #[test]
    fn local_tiles_are_not_editable_from_the_form() {
        use ratatui::crossterm::event::KeyCode;
        for key in [KeyCode::Char('e'), KeyCode::Char('d')] {
            let mut app = App::new();
            if !select(&mut app, |e| e.is_local()) {
                return;
            }
            press(&mut app, key);
            assert!(matches!(app.mode, Mode::Browse), "{key:?} must not open a dialog");
            assert!(app.status.as_ref().is_some_and(|s| !s.good));
        }
    }

    /// The whole block, not just the grid, is centred in both axes.
    #[test]
    fn content_is_centred() {
        use ratatui::crossterm::event::KeyCode;
        let _ = KeyCode::Enter;
        for (w, h) in [(160u16, 50u16), (120, 40), (100, 30)] {
            let (app, out) = frame(w, h, |_| {});
            let rows: Vec<&str> = out.lines().collect();
            let ink = |r: &str| r.trim_matches('"').trim_end().len() > 0;
            let top = rows.iter().position(|r| ink(r)).unwrap();
            let bottom = rows.len() - 1 - rows.iter().rev().position(|r| ink(r)).unwrap();
            let above = top;
            let below = rows.len() - 1 - bottom;
            assert!(
                above.abs_diff(below) <= 2,
                "{w}x{h}: {above} rows above vs {below} below"
            );

            // Horizontal: equal gap either side of the tile block.
            let left = app.hitboxes.iter().map(|(r, _)| r.x).min().unwrap();
            let right = app.hitboxes.iter().map(|(r, _)| r.x + r.width - 2).max().unwrap();
            assert!(
                left.abs_diff(w - right) <= 2,
                "{w}x{h}: {left} cols left vs {} right",
                w - right
            );
        }
    }

    /// Tiles keep their size on a wide screen instead of stretching.
    #[test]
    fn tiles_do_not_stretch_on_a_wide_terminal() {
        let (narrow, _) = frame(120, 30, |_| {});
        let (wide, _) = frame(200, 30, |_| {});
        let w = |a: &App| a.hitboxes[0].0.width;
        assert_eq!(w(&narrow), w(&wide), "tile width must not depend on terminal width");
    }

    #[test]
    fn the_form_colour_row_cycles_and_previews() {
        use ui::ColorChoice;
        let mut app = App::new();
        app.mode = Mode::Add(Form::new());
        for _ in 0..ui::FIELDS.len() {
            press(&mut app, ratatui::crossterm::event::KeyCode::Tab);
        }
        let Mode::Add(f) = &app.mode else { panic!("left the form") };
        assert_eq!(f.focus, ui::COLOR_ROW);
        assert!(matches!(f.color, ColorChoice::Auto));

        press(&mut app, ratatui::crossterm::event::KeyCode::Right);
        let Mode::Add(f) = &app.mode else { panic!() };
        assert!(matches!(f.color, ColorChoice::Preset(0)));

        // Wrapping backwards from Auto lands on the last preset, not off the end.
        press(&mut app, ratatui::crossterm::event::KeyCode::Left);
        press(&mut app, ratatui::crossterm::event::KeyCode::Left);
        let Mode::Add(f) = &app.mode else { panic!() };
        assert!(matches!(f.color, ColorChoice::Preset(i) if i == entry::PALETTE.len() - 1));
    }

    #[test]
    fn a_bad_hex_is_rejected_without_writing() {
        use ui::ColorChoice;
        let mut app = App::new();
        let mut form = Form::new();
        form.values[0] = "probe-host".into();
        form.color = ColorChoice::Custom("#zzz".into());
        app.mode = Mode::Add(form);
        press(&mut app, ratatui::crossterm::event::KeyCode::Enter);
        match &app.mode {
            Mode::Add(f) => assert!(f.error.as_deref().is_some_and(|e| e.contains("#rrggbb"))),
            _ => panic!("must stay in the form"),
        }
        assert!(!app.entries.iter().any(|e| e.label() == "probe-host"), "nothing written");
    }
}
