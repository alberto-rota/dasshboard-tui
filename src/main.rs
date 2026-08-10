//! dasshboard -- a home screen for your terminal.
//!
//! Draws every host in `~/.ssh/config` plus anything in `config.toml` as a
//! clickable tile. Activating one connects: under Ghostty on macOS that is a
//! new tab tinted with the host's colour, and everywhere else it takes over the
//! terminal you are already in. Quitting drops you into the shell that launched
//! it.
//!
//! Nothing outside `ghostty.rs` knows Ghostty exists -- see `launch.rs`.

mod app;
mod config;
mod entry;
mod ghostty;
mod launch;
mod platform;
mod ssh;
mod startup;
mod theme;
mod ui;

use std::io::{self, Stdout};
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
    // program starts on a clean screen and owns this terminal from here on.
    if let Some(argv) = app.handoff {
        hand_off(&argv, app.handoff_cwd.as_deref());
    }
    Ok(())
}

/// Give this terminal to `argv` and never come back.
///
/// The session dasshboard was asked for *replaces* the home screen rather than
/// running under it, which is what makes the same launcher work in a terminal
/// that cannot open tabs for us: there is no nesting, no wrapper process left
/// holding the tty, and quitting the session falls through to the shell that
/// started dasshboard.
///
/// `SKIP_VAR` goes into the environment because a local tile usually runs a
/// shell, and that shell reads the same rc that started us -- without it, the
/// first thing it would do is draw a second home screen inside the first.
fn hand_off(argv: &[String], cwd: Option<&str>) -> ! {
    if let Some(dir) = cwd {
        if let Err(e) = std::env::set_current_dir(dir) {
            eprintln!("dasshboard: could not enter {dir}: {e}");
            std::process::exit(1);
        }
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).env(launch::SKIP_VAR, "1");

    // Unix replaces this process outright, so nothing of dasshboard survives to
    // be waited on.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        eprintln!("dasshboard: could not run {}: {err}", argv[0]);
        std::process::exit(1);
    }

    // Windows has no exec, so the nearest thing is to run the session as a
    // child on the same console and then leave with its exit code. The console,
    // the keyboard and the scrollback are all still the session's alone; the
    // only difference is a parent process asleep behind it.
    #[cfg(not(unix))]
    match cmd.status() {
        Ok(st) => std::process::exit(st.code().unwrap_or(0)),
        Err(e) => {
            eprintln!("dasshboard: could not run {}: {e}", argv[0]);
            std::process::exit(1);
        }
    }
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
            // Without a terminal that opens tabs for us there is nowhere else to
            // put it, so `--open` becomes what it means in a shell: connect,
            // here, in place of this command.
            if !launch::backend().can_spawn() {
                hand_off(&argv, None);
            }
            match launch::spawn(where_to, &name, &argv, None, hex.as_deref(), emoji) {
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
        // Installing the package does not touch your shell; this is how the
        // home screen gets to open with a terminal, and how it stops.
        "--startup" => Ok(Some(startup_cli(args.next().as_deref()))),
        "--help" | "-h" => {
            let where_to = match launch::backend() {
                launch::Backend::Ghostty => "new Ghostty tabs",
                launch::Backend::InPlace => "this terminal (no tabs: needs Ghostty on macOS)",
            };
            println!(
                "dasshboard [--list | --open <host> | --config | --startup [on|off] | --help]\n\n\
                 With no arguments, launches the TUI.\n\n\
                 --list               print every tile, colours included, and exit\n\
                 --open <host>        connect to one host without the TUI\n\
                 --config             print the path of config.toml\n\
                 --startup            report whether the home screen opens with a terminal\n\
                 --startup on         hook it into your shell rc (opt in; nothing else does this)\n\
                 --startup off        unhook it, restoring whatever owned that slot before\n\
                 --startup print      print the hook instead of writing it\n\n\
                 Sessions open in: {where_to}.\n\
                 Set {} to ghostty or inplace to override that.",
                launch::BACKEND_VAR,
            );
            Ok(Some(0))
        }
        other => {
            eprintln!("unknown argument: {other}");
            Ok(Some(2))
        }
    }
}

/// `--startup [on|off|print]`. With no argument it only reports, since the
/// whole point is that nothing changes your shell unless you say so.
fn startup_cli(arg: Option<&str>) -> i32 {
    let say_state = |rc: &std::path::Path| {
        let st = startup::state_at(rc);
        println!("startup: {} ({})", st.label(), rc.display());
        if st == startup::State::Partial {
            println!("run `dasshboard --startup on` to repair it, or `off` to remove it");
        }
    };

    match arg {
        None | Some("status") => match startup::rc_path() {
            Ok(rc) => {
                say_state(&rc);
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("on" | "enable") => match startup::enable() {
            Ok(rc) => {
                println!("startup: on ({})", rc.display());
                println!("a new Ghostty window opens the home screen; q drops you into the shell");
                println!("pre-install copy: {}", startup::backup_path(&rc).display());
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("off" | "disable") => match startup::disable() {
            Ok((rc, true)) => {
                println!("startup: off ({} restored)", rc.display());
                println!("whatever owned the terminal-open slot before gets it back");
                0
            }
            Ok((rc, false)) => {
                println!("startup was already off ({})", rc.display());
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("print") => {
            print!("{}", startup::script());
            0
        }
        Some(other) => {
            eprintln!("usage: dasshboard --startup [status|on|off|print], not {other}");
            2
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
        let (_, add) = frame(120, 30, |a| a.mode = Mode::Add(Form::new(config::Block::Host)));
        println!("=== add form ===\n{add}");
        let (_, del) = frame(120, 30, |a| a.mode = Mode::ConfirmDelete(0));
        println!("=== confirm ===\n{del}");
        let (_, pick) = frame(120, 32, |a| {
            if let Some(i) = a.entries.iter().position(|e| !e.hidden()) {
                a.entries[i].folders =
                    vec!["/scratch/atlas".into(), "/home/v120bb18/thesis".into()];
                let vis = a.visible();
                a.sel = vis.iter().position(|&x| x == i).unwrap();
                a.on_key(
                    ratatui::crossterm::event::KeyEvent::from(
                        ratatui::crossterm::event::KeyCode::Enter,
                    ),
                    &vis,
                );
            }
        });
        println!("=== folder picker ===\n{pick}");
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
        // These tests run against whatever you have configured, and a tile with
        // folders asks which one first -- a path with its own test. Destination
        // resolution is what this is about, so take the question away.
        app.entries[i].folders.clear();
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

        // A tile with folders asks which one before it launches, which is a
        // different question from where it launches.
        let no_folders = |a: &mut App| a.entries[i].folders.clear();

        // Global says tab, tile says current: the tile wins, so this hands off.
        app.sel = sel;
        app.open_in = OpenIn::Tab;
        app.entries[i].open_in = Some(OpenIn::Current);
        no_folders(&mut app);
        app.activate(&vis);
        assert!(app.handoff.is_some(), "tile setting beats the global");

        // Global says current, tile silent: the global applies.
        let mut app = App::new();
        app.sel = sel;
        app.open_in = OpenIn::Current;
        app.entries[i].open_in = None;
        no_folders(&mut app);
        app.activate(&vis);
        assert!(app.handoff.is_some(), "global applies when the tile is silent");

        // And a one-off beats both.
        let mut app = App::new();
        app.sel = sel;
        app.open_in = OpenIn::Tab;
        app.entries[i].open_in = Some(OpenIn::Tab);
        no_folders(&mut app);
        app.activate_in(&vis, Some(OpenIn::Current));
        assert!(app.handoff.is_some(), "the one-off key wins");
    }

    /// The whole of the portability promise, in one assertion: on a machine
    /// with nothing to open a tab *with*, a tile that asks for one lands in
    /// this terminal instead. Without this, `t` on Linux would reach the
    /// AppleScript backend and fail there rather than doing the obvious thing.
    #[test]
    fn without_a_spawner_every_destination_becomes_a_handoff() {
        use config::OpenIn;
        use launch::Backend;
        let mut app = App::new();
        app.backend = Backend::InPlace;
        let Some(i) = app.entries.iter().position(|e| !e.hidden() && !e.argv.is_empty()) else {
            return;
        };
        // A tile with folders asks which one first, which is a different
        // question from where it opens.
        app.entries[i].folders.clear();
        app.entries[i].open_in = Some(OpenIn::Window);
        let vis = app.visible();
        app.sel = vis.iter().position(|&x| x == i).unwrap();
        let argv = app.entries[i].argv.clone();

        // Both the tile's own setting and a one-off `t` ask for a new surface.
        app.activate_in(&vis, Some(OpenIn::Tab));
        assert!(app.quit, "must leave the TUI rather than report a failure");
        assert_eq!(app.handoff.as_deref(), Some(argv.as_slice()));
        assert!(app.status.is_none(), "a handoff is not a spawned tab");
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
                assert_eq!(f.fields[0].value, name);
                assert_ne!(f.focus, f.first_field(), "focus must skip the locked field");
                // Blank fields track ~/.ssh/config, shown as placeholders.
                assert!(
                    f.fields[1..].iter().all(|x| x.value.is_empty()),
                    "no override yet"
                );
                assert!(
                    f.fields.iter().any(|x| !x.placeholder.is_empty()),
                    "inherits something"
                );
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

    /// Local tiles are editable now, with their own field set rather than the
    /// host one.
    #[test]
    fn local_tiles_open_in_the_form_with_local_fields() {
        use config::Block;
        let mut app = App::new();
        if !select(&mut app, |e| e.is_local()) {
            return;
        }
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('e'));
        match &app.mode {
            Mode::Edit(f) => {
                assert_eq!(f.block, Block::Local);
                assert!(f.kind_locked, "editing must not change a tile's kind");
                let keys: Vec<&str> = f.fields.iter().map(|x| x.key).collect();
                assert_eq!(keys, ui::LOCAL_FIELDS, "local fields, not host ones");
                assert!(!f.field("command").is_empty(), "prefilled from config");
            }
            _ => panic!("local tiles must be editable"),
        }
    }

    /// Adding starts on the kind row so a local tile is one keystroke away.
    #[test]
    fn the_add_form_can_switch_to_a_local_tile() {
        use config::Block;
        use ratatui::crossterm::event::KeyCode;
        let mut app = App::new();
        press(&mut app, KeyCode::Char('a'));
        let Mode::Add(f) = &app.mode else { panic!("a must open the form") };
        assert_eq!(f.block, Block::Host);
        assert_eq!(f.focus, ui::KIND_ROW);

        press(&mut app, KeyCode::Right);
        let Mode::Add(f) = &app.mode else { panic!() };
        assert_eq!(f.block, Block::Local);
        let keys: Vec<&str> = f.fields.iter().map(|x| x.key).collect();
        assert_eq!(keys, ui::LOCAL_FIELDS);
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
        use config::Block;
        use ui::ColorChoice;
        let mut app = App::new();
        let form = ui::Form::new(Block::Host);
        let color_row = form.color_row();
        app.mode = Mode::Add(form);
        while !matches!(&app.mode, Mode::Add(f) if f.focus == color_row) {
            press(&mut app, ratatui::crossterm::event::KeyCode::Tab);
        }
        let Mode::Add(f) = &app.mode else { panic!("left the form") };
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

    /// The one row that edits something outside config.toml leads the panel,
    /// and it must not be reachable through the option writer -- `startup` is
    /// not a key in `[options]`, and writing one there would do nothing at all.
    #[test]
    fn the_startup_row_leads_the_settings_and_is_not_an_option_key() {
        use ui::SettingRow;
        let app = App::new();
        let rows = app.setting_rows();
        assert!(matches!(rows[0], SettingRow::Startup { .. }), "startup comes first");
        assert!(
            rows[1..].iter().all(|r| !matches!(r, SettingRow::Startup { .. })),
            "one switch, not two"
        );
        let keys: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                SettingRow::Toggle { key, .. } | SettingRow::Choice { key, .. } => Some(*key),
                _ => None,
            })
            .collect();
        assert!(!keys.contains(&"startup"), "it lives in the shell rc, not the config");
    }

    #[test]
    fn a_bad_hex_is_rejected_without_writing() {
        use ui::ColorChoice;
        let mut app = App::new();
        let mut form = Form::new(config::Block::Host);
        form.set("name", "probe-host");
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
