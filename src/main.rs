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
        hand_off(
            &argv,
            app.handoff_cwd.as_deref(),
            app.handoff_title.as_deref(),
            app.handoff_shell_after,
        );
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
///
/// `title` renames the surface we are handing over. A tab this program *opens*
/// is titled by the command it runs there; a tab it takes over would otherwise
/// keep the name of the terminal that opened the home screen, so the one tile
/// that lands here would be the one tile the tab bar could not tell you about.
/// It is written after the TUI is torn down and before the exec, which is the
/// only moment this process owns a plain terminal.
///
/// `shell_after` is the one path that does not `exec`: a tile whose command
/// exits -- `echo hi`, a script, anything that is not something you sit in --
/// would otherwise print into a terminal it hands straight back, which reads as
/// a home screen that blinked and vanished. There the command is run, waited
/// for, and the shell that follows it is what this terminal becomes.
fn hand_off(argv: &[String], cwd: Option<&str>, title: Option<&str>, shell_after: bool) -> ! {
    if let Some(t) = title {
        launch::rename_current_tab(t);
    }
    if let Some(dir) = cwd {
        if let Err(e) = std::env::set_current_dir(dir) {
            eprintln!("dasshboard: could not enter {dir}: {e}");
            std::process::exit(1);
        }
    }

    if shell_after {
        // A session that never started has nothing to outlive, so a command
        // that cannot be run is still the same error it always was.
        if let Err(e) = session(argv).status() {
            eprintln!("dasshboard: could not run {}: {e}", argv[0]);
            std::process::exit(1);
        }
        let shell = platform::login_shell();
        let mut after = session(std::slice::from_ref(&shell));
        after.env(launch::NO_WORKSPACE_VAR, "1");
        become_process(&mut after, &shell);
    }
    become_process(&mut session(argv), &argv[0]);
}

/// The command for a session, marked so the shell it may start doesn't draw a
/// second home screen inside this one.
fn session(argv: &[String]) -> Command {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).env(launch::SKIP_VAR, "1");
    cmd
}

/// Give this terminal to `cmd`, for good.
fn become_process(cmd: &mut Command, what: &str) -> ! {
    // Unix replaces this process outright, so nothing of dasshboard survives to
    // be waited on.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        eprintln!("dasshboard: could not run {what}: {err}");
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
            eprintln!("dasshboard: could not run {what}: {e}");
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
            // Hidden tiles are printed, marked: this exists to show what is
            // configured, and a hidden tile is configured. A deleted one is not
            // built at all, so it cannot appear here either way.
            for e in entry::build(&cfg, &ssh::default_config_path()).entries {
                let kind = if e.is_local() { "local" } else { "ssh" };
                let mark = if e.hidden() { "hidden" } else { "" };
                let t = e.tint();
                println!(
                    "{kind:<6} {} {:<8} {:<16} {:<28} {:<26} {mark}",
                    t.emoji,
                    t.hex,
                    e.label(),
                    e.detail(),
                    e.note().unwrap_or_default(),
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
            let entries = entry::build(&cfg, &ssh::default_config_path()).entries;
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
            // here, in place of this command -- naming the tab on the way, as
            // the tab it would have opened would have been named.
            if !launch::backend().can_spawn() {
                let title = launch::tab_title(&name, emoji);
                // Only ssh tiles are reachable here, and an ssh session carries
                // its own follow-on shell in the remote command.
                hand_off(&argv, None, Some(&title), false);
            }
            match launch::spawn(where_to, &name, &argv, None, hex.as_deref(), emoji, false) {
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
        let (_, placed) = frame(120, 32, |a| {
            for (n, i) in a.visible().into_iter().enumerate() {
                match n {
                    0 => a.entries[i].folder = Some("/home/v120bb18/thesis".into()),
                    1 => a.entries[i].command = Some("tmux attach".into()),
                    2 => {
                        a.entries[i].folder = Some("~/dasshboard-tui".into());
                        a.entries[i].command = Some("nvim".into());
                    }
                    _ => {}
                }
            }
        });
        println!("=== folders and commands on the tiles ===\n{placed}");
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
        let (_, revealed) = frame(120, 32, |a| {
            a.show_hidden = true;
            if let Some(e) = a.entries.first_mut() {
                e.hidden = true;
            }
        });
        println!("=== hidden tiles, revealed by X ===\n{revealed}");
        let (_, set) = frame(120, 30, |a| a.mode = Mode::Settings { focus: 5, buf: "#aaaaaa".into() });
        println!("=== settings ===\n{set}");
        let (_, filt) = frame(120, 30, |a| {
            a.mode = Mode::Filter;
            a.filter = "c".into();
        });
        println!("=== filter ===\n{filt}");
        let (_, grouped) = frame(120, 40, |a| {
            a.sections = vec!["work".into(), "home".into(), "lab".into()];
            let vis = a.visible();
            for (n, &i) in vis.iter().enumerate() {
                a.entries[i].section = usize::from(n > 1);
            }
        });
        println!("=== sections ===\n{grouped}");
        let (_, moving) = frame(120, 40, |a| {
            a.sections = vec!["work".into(), "home".into()];
            let vis = a.visible();
            for (n, &i) in vis.iter().enumerate() {
                a.entries[i].section = usize::from(n > 1);
            }
            a.sel = 1;
            a.mode = Mode::Move;
        });
        println!("=== moving a tile ===\n{moving}");
        let (_, groups) = frame(120, 40, |a| {
            a.sections = vec!["work".into(), "home".into(), String::new()];
            a.mode = Mode::Sections { focus: 1, buf: None };
        });
        println!("=== groups panel ===\n{groups}");
    }

    #[test]
    fn columns_shrink_with_width() {
        // Tiles are a fixed 38 wide, so these are floor((w - 2*PAD) / 38), capped at 4.
        for (w, want) in [(200u16, 4usize), (156, 4), (150, 3), (120, 3), (80, 2), (44, 1)] {
            let (app, _) = frame(w, 30, |_| {});
            assert_eq!(app.cols, want, "width {w}");
        }
    }

    /// What you can see, you can click -- one hitbox per tile the grid actually
    /// drew. Not per *visible* tile: a board taller than the terminal scrolls,
    /// and the tiles below the fold have no place on screen to be clicked.
    #[test]
    fn every_tile_is_clickable_and_none_overlap() {
        let (app, _) = frame(120, 30, |_| {});
        let drawn: usize = app.tile_rows.iter().map(|r| r.len()).sum();
        assert_eq!(app.hitboxes.len(), drawn, "one hitbox per drawn tile");
        assert!(drawn <= app.visible().len());

        // ...and on a screen with room for the whole board, that is all of them.
        let (big, _) = frame(180, 60, |_| {});
        assert_eq!(big.hitboxes.len(), big.visible().len(), "nothing is left unclickable");

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
    /// over the footer. And the footer, now that it lives on the bottom edge
    /// rather than under the last row of tiles, must not bleed the other way:
    /// it gives up the bottom edge rather than print the keys over a tile.
    ///
    /// Below 30x10 a single tile is taller than the room there is for one, and
    /// the layout says outright that it would rather overflow than draw
    /// nothing -- so that is not a size this asserts anything about.
    #[test]
    fn tiles_never_escape_the_viewport() {
        for (w, h) in [(150u16, 40u16), (120, 30), (80, 24), (44, 14), (30, 10)] {
            let (app, out) = frame(w, h, |_| {});
            for (r, _) in &app.hitboxes {
                assert!(r.x + r.width <= w, "{w}x{h}: {r:?} past right edge");
                assert!(r.y + r.height <= h, "{w}x{h}: {r:?} past bottom edge");
            }
            // Nothing is drawn twice into one cell, which is what an overlap of
            // the two would look like: a tile border with letters through it.
            let rows: Vec<&str> = out.lines().collect();
            let bottom_of_grid =
                app.hitboxes.iter().map(|(r, _)| r.y + r.height - 1).max().unwrap_or(0);
            for (y, row) in rows.iter().enumerate().take(bottom_of_grid as usize) {
                assert!(
                    !row.contains(" open ") || y as u16 >= bottom_of_grid,
                    "{w}x{h}: the footer landed on row {y}, inside the grid"
                );
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
        if app.entries.is_empty() {
            return;
        }
        let i = 0;
        app.entries[i].open_in = Some(OpenIn::Current);
        let vis = app.visible();
        app.sel = vis.iter().position(|&e| e == i).unwrap();
        let argv = app.entries[i].argv.clone();
        let label = app.entries[i].label().to_string();
        // Loading the config may have had something to say; what this is about
        // is what *activating* says.
        app.status = None;
        app.activate(&vis);
        assert!(app.quit, "must leave the TUI");
        assert_eq!(app.handoff.as_deref(), Some(argv.as_slice()));
        assert!(app.status.is_none(), "handoff should not report a spawned tab");
        // The tab we are handing over has to be renamed, or it keeps the name
        // of whatever opened the home screen.
        let title = app.handoff_title.expect("a taken-over tab is still a tab");
        assert!(title.ends_with(&label), "the tile names the tab: {title}");
    }

    /// A one-off key beats the tile's own setting, which beats the global.
    /// Only `current` can be asserted -- the others would spawn real tabs.
    #[test]
    fn the_destination_is_resolved_most_specific_first() {
        use config::OpenIn;
        let pick = |a: &App| (!a.entries.is_empty()).then_some(0);

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
        let Some(i) = app.entries.iter().position(|e| !e.argv.is_empty()) else {
            return;
        };
        app.entries[i].open_in = Some(OpenIn::Window);
        let vis = app.visible();
        app.sel = vis.iter().position(|&x| x == i).unwrap();
        let argv = app.entries[i].argv.clone();

        // Both the tile's own setting and a one-off `t` ask for a new surface.
        app.status = None;
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

    /// `D` is one verb on every tile now: a host from ~/.ssh/config has no
    /// block of ours to remove, but it is still deletable -- that is the whole
    /// change, and it must not fall back to the old "nothing to remove".
    #[test]
    fn delete_offers_itself_on_a_plain_ssh_config_host() {
        use entry::Origin;
        let mut app = App::new();
        if !select(&mut app, |e| e.origin() == Some(Origin::Ssh)) {
            return;
        }
        app.status = None;
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('D'));
        assert!(matches!(app.mode, Mode::ConfirmDelete(_)), "must ask, not decline");
        assert!(app.status.is_none(), "and say nothing until it is answered");

        // n keeps it, and takes the dialog away.
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('n'));
        assert!(matches!(app.mode, Mode::Browse));
    }

    /// `d` duplicates and `D` deletes, which puts the destructive half behind
    /// shift. Pressing `d` must not open the confirm dialog it used to, and the
    /// old duplicate key must be inert rather than quietly still bound -- `y` is
    /// the confirm dialog's "yes" now and nothing else. Neither half is pressed
    /// here: duplicating writes to config.toml, so this asserts the dispatch and
    /// the footer that advertises it, and `delete_offers_itself_on_a_plain_ssh_
    /// config_host` is what presses `D`.
    #[test]
    fn duplicate_is_d_and_delete_is_shift_d() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = App::new();
        if app.visible().is_empty() {
            return;
        }
        app.status = None;
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(app.mode, Mode::Browse), "y opens nothing on the board");
        assert!(app.status.is_none(), "and writes nothing, so it says nothing");

        let (_, out) = frame(120, 30, |_| {});
        assert!(out.contains("d dup"), "the footer offers d for duplicate");
        assert!(out.contains("D delete"), "and shift-d for delete");
    }

    /// A filter is the only thing that takes a tile off the grid now, and the
    /// clamp has to count what is left rather than the whole list.
    #[test]
    fn filtering_clamps_the_selection_to_what_is_visible() {
        let mut app = App::new();
        if app.visible().is_empty() {
            return;
        }
        app.sel = app.visible().len() - 1;
        app.filter = "zzzz-matches-nothing".into();
        app.clamp_selection();
        let vis = app.visible();
        assert!(vis.is_empty(), "nothing matches that");
        assert_eq!(app.sel, 0, "the cursor cannot be left past the end");
    }

    /// The two ways off the screen are not the same way. A hidden tile is still
    /// a tile -- it leaves the grid, keeps its place in `entries`, and one key
    /// brings every one of them back. A deleted tile was never built.
    #[test]
    fn hiding_takes_a_tile_off_the_grid_and_x_shows_it_again() {
        let mut app = App::new();
        app.show_hidden = false;
        let total = app.entries.len();
        // Counted rather than assumed: this runs against whatever the machine
        // actually has configured, which may already hide some of it.
        let Some(shown) = app.entries.iter().position(|e| !e.hidden()) else { return };
        let before = app.visible().len();

        // What `x` writes, without touching the real config file.
        app.entries[shown].hidden = true;
        assert_eq!(app.visible().len(), before - 1, "it leaves the grid");
        assert_eq!(app.entries.len(), total, "but not the board");

        press(&mut app, ratatui::crossterm::event::KeyCode::Char('X'));
        assert!(app.show_hidden, "X reveals");
        assert_eq!(app.visible().len(), total, "all of them, in their own places");
        assert!(app.status.as_ref().is_some_and(|s| s.good), "and says what it did");

        press(&mut app, ratatui::crossterm::event::KeyCode::Char('X'));
        assert!(!app.show_hidden, "and puts them back");
        assert_eq!(app.visible().len(), before - 1);
    }

    /// Revealing is a look, not an edit: `X` must not write to config.toml, so
    /// a reload -- which every write does -- must not undo the look either.
    #[test]
    fn revealing_survives_a_reload_and_writes_nothing() {
        let mut app = App::new();
        let before = std::fs::read(config::path()).ok();
        app.show_hidden = false;
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('X'));
        assert!(app.show_hidden);
        app.reload();
        assert!(app.show_hidden, "a reload must not put the hidden tiles back");
        assert_eq!(std::fs::read(config::path()).ok(), before, "the file is untouched");
    }

    /// Hiding the selected last tile must not leave the cursor past the end:
    /// the clamp has to count visible tiles, not all of them.
    #[test]
    fn hiding_clamps_the_selection_to_what_is_visible() {
        let mut app = App::new();
        if app.visible().is_empty() {
            return;
        }
        app.sel = app.visible().len() - 1;
        for e in app.entries.iter_mut() {
            e.hidden = true;
        }
        app.clamp_selection();
        let vis = app.visible();
        assert!(vis.is_empty() || app.sel < vis.len(), "sel {} of {}", app.sel, vis.len());
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

    /// The board is centred in both axes -- in what is left of the screen once
    /// the footer has taken the bottom edge, since the keys are a fixed
    /// reference rather than part of the board.
    #[test]
    fn content_is_centred_above_a_footer_on_the_bottom_edge() {
        use ratatui::crossterm::event::KeyCode;
        let _ = KeyCode::Enter;
        for (w, h) in [(160u16, 50u16), (120, 40), (100, 30)] {
            let (app, out) = frame(w, h, |_| {});
            let rows: Vec<&str> = out.lines().collect();
            let ink = |r: &str| r.trim_matches('"').trim_end().len() > 0;

            // The keys are on the last row that is not the status line, however
            // few tiles there are to sit above them.
            assert!(ink(rows[rows.len() - 2]), "{w}x{h}: no keys on the bottom edge");

            let board = &rows[..rows.len() - 2];
            let top = board.iter().position(|r| ink(r)).unwrap();
            let bottom = board.len() - 1 - board.iter().rev().position(|r| ink(r)).unwrap();
            let above = top;
            let below = board.len() - 1 - bottom;
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

    // ------------------------------------------------------------- sections

    /// Put every visible tile after the first into a second group, so a board
    /// with two named sections can be drawn whatever the machine's own config
    /// happens to hold.
    fn split_into_two_groups(a: &mut App) {
        a.sections = vec!["alpha".into(), "beta".into()];
        let vis = a.visible();
        for (n, &i) in vis.iter().enumerate() {
            a.entries[i].section = usize::from(n > 0);
        }
    }

    /// A named group announces itself above its tiles, and the group after it
    /// starts a fresh row rather than filling out the one before.
    #[test]
    fn a_named_group_gets_a_title_row_of_its_own() {
        if App::new().visible().len() < 3 {
            return;
        }
        let (app, out) = frame(120, 44, split_into_two_groups);

        assert!(out.contains("alpha"), "the first group is titled");
        assert!(out.contains("beta"));
        assert_eq!(
            app.tile_rows.first().map(|r| r.len()),
            Some(1),
            "the one tile in alpha keeps its row to itself"
        );
        let first = app.hitboxes[0].0.y;
        assert!(
            app.hitboxes[1..].iter().all(|(r, _)| r.y > first),
            "beta's tiles are all below alpha's"
        );
    }

    /// A group you have just made is empty by definition, so an empty one has to
    /// be on screen: one you cannot see is one you cannot aim a tile at.
    #[test]
    fn an_empty_named_group_still_shows_its_heading() {
        let (app, out) = frame(120, 44, |a| {
            a.sections = vec![String::new(), "lab".into()];
            for e in a.entries.iter_mut() {
                e.section = 0;
            }
        });
        assert!(out.contains("lab"), "the empty group names itself");
        assert!(out.contains("empty — press m"), "and says how to fill it");
        assert_eq!(app.hitboxes.len(), app.visible().len(), "without inventing a tile");
    }

    /// A filter is a temporary lens, not a rearrangement, so it does not get to
    /// leave headings standing over nothing.
    #[test]
    fn a_filter_drops_the_groups_it_empties() {
        let (_, out) = frame(120, 44, |a| {
            a.sections = vec![String::new(), "lab".into()];
            for e in a.entries.iter_mut() {
                e.section = 0;
            }
            a.mode = Mode::Filter;
            a.filter = "zzzz-matches-nothing".into();
        });
        assert!(!out.contains("lab"), "no heading over an emptied group");
    }

    /// An untitled group is what everything starts in, so it has to cost
    /// nothing: no heading, and no row given up to one.
    #[test]
    fn an_untitled_group_draws_no_heading() {
        let one_group = |title: &str| {
            let title = title.to_string();
            move |a: &mut App| {
                a.sections = vec![title.clone()];
                for e in a.entries.iter_mut() {
                    e.section = 0;
                }
            }
        };
        let (plain, before) = frame(120, 44, one_group(""));
        let (named, after) = frame(120, 44, one_group("work"));

        // How many rows sit between the rule under the wordmark and the tiles.
        let gap = |out: &str| {
            let lines: Vec<&str> = out.lines().collect();
            let tile = |l: &&&str| l.contains('┏') || l.contains('╭');
            let rule = lines.iter().position(|l| l.contains('━') && !l.contains('┏')).unwrap();
            lines.iter().position(|l| tile(&l)).unwrap() - rule - 1
        };
        assert_eq!(gap(&before), 1, "an untitled group costs no rows at all");
        assert_eq!(gap(&after), 3, "a named one costs its heading and the blank under it");
        assert!(!before.contains("work") && after.contains("work"));
        assert_eq!(plain.hitboxes.len(), named.hitboxes.len(), "same tiles either way");
    }

    /// `j` follows the grid as drawn. A group that ends a row early must not
    /// send the cursor to wherever a fixed stride of `cols` would have put it.
    #[test]
    fn moving_down_lands_on_the_row_below_a_short_row() {
        let (mut app, _) = frame(120, 44, |a| {
            a.sections = vec!["alpha".into(), "beta".into()];
            let vis = a.visible();
            for (n, &i) in vis.iter().enumerate() {
                a.entries[i].section = usize::from(n > 1);
            }
        });
        if app.cols != 3 || app.visible().len() < 5 {
            return;
        }
        // alpha holds two tiles, so its row is one short of full.
        assert_eq!(app.tile_rows[0].len(), 2);
        app.sel = 0;
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('j'));
        assert_eq!(app.sel, 2, "the row below, not three tiles along");
    }

    /// What a move writes is the arrangement read back off the screen, so it has
    /// to account for every tile: a layout that omitted one would delete it from
    /// the order the moment anything else moved.
    #[test]
    fn the_layout_read_off_the_screen_accounts_for_every_tile() {
        let app = App::new();
        let layout = app.layout();
        assert_eq!(layout.len(), app.sections.len(), "one entry per group");
        let placed: usize = layout.iter().map(|s| s.items.len()).sum();
        assert_eq!(placed, app.entries.len(), "every tile, hidden ones included");
        for (s, sec) in layout.iter().enumerate() {
            assert_eq!(sec.title, app.sections[s]);
        }
    }

    /// `m` grabs the tile under the cursor rather than opening anything, and the
    /// footer is the only chrome that changes -- the grid is what is being
    /// edited, so it stays lit.
    #[test]
    fn m_grabs_the_selected_tile() {
        let mut app = App::new();
        if app.visible().is_empty() {
            return;
        }
        press(&mut app, ratatui::crossterm::event::KeyCode::Char('m'));
        assert!(matches!(app.mode, Mode::Move));
        press(&mut app, ratatui::crossterm::event::KeyCode::Enter);
        assert!(matches!(app.mode, Mode::Browse), "⏎ drops it");
        assert!(app.status.is_none(), "and takes the hint away with it");
    }

    /// The arrows move the tile, not the cursor: a step yields a new arrangement
    /// with the tile one place along, and a step off the end of a group carries
    /// it into the next one rather than stopping at the boundary.
    #[test]
    fn stepping_a_grabbed_tile_rewrites_the_arrangement() {
        let mut app = App::new();
        if app.visible().len() < 3 {
            return;
        }
        // One tile alone in the first group, everything else -- hidden tiles
        // included, since they hold places too -- in the second.
        app.sections = vec!["alpha".into(), "beta".into()];
        for e in app.entries.iter_mut() {
            e.section = 1;
        }
        let vis = app.visible();
        app.entries[vis[0]].section = 0;
        app.sel = 0;
        let label = app.entries[vis[0]].label().to_string();

        let forward = app.shifted(&vis, true, 1).expect("it can leave its group");
        assert!(forward[0].items.is_empty(), "alpha is empty now");
        assert_eq!(forward[1].items.first(), Some(&label), "at the head of beta");
        assert_eq!(
            forward.iter().map(|s| s.items.len()).sum::<usize>(),
            app.entries.len(),
            "and no tile was lost on the way"
        );

        // The first slot on screen has nothing before it: the ends are walls.
        assert!(app.shifted(&vis, false, 1).is_none());
    }

    /// A title is typed before anything is written, which is what lets `d` be a
    /// letter on that row and a verb everywhere else in the panel.
    #[test]
    fn the_group_panel_drafts_a_title_before_saving_it() {
        use ratatui::crossterm::event::KeyCode;
        let mut app = App::new();
        press(&mut app, KeyCode::Char('S'));
        assert!(matches!(app.mode, Mode::Sections { focus: 0, buf: None }));

        // The last row is the one that makes a group, so it starts empty.
        for _ in 0..app.section_rows() - 1 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Sections { buf: Some(b), .. } if b.is_empty()));

        for c in ['d', 'a', 'y'] {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Backspace);
        assert!(matches!(&app.mode, Mode::Sections { buf: Some(b), .. } if b == "da"));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.mode, Mode::Sections { buf: None, .. }), "the draft is dropped");
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.mode, Mode::Browse));
        assert_eq!(app.sections, App::new().sections, "nothing was written");
    }

    /// Two tiles for one host are the same host: where each of them lands is the
    /// whole of the difference, so the folder has to be on the tile. The command
    /// deliberately is not -- one short line per tile, spent on the destination.
    #[test]
    fn a_tile_says_where_it_lands_and_not_what_it_runs() {
        if App::new().visible().len() < 2 {
            return;
        }
        let (_, out) = frame(120, 30, |a| {
            let vis = a.visible();
            a.entries[vis[0]].folder = Some("~/thesis".into());
            a.entries[vis[1]].command = Some("tmux attach".into());
        });
        assert!(out.contains("~/thesis"), "the folder is on the tile");
        assert!(!out.contains("tmux attach"), "the command is not");
    }

    /// Both kinds of tile have a folder and a command now, and switching between
    /// them mid-form must not throw away what has been typed into either.
    #[test]
    fn folder_and_command_belong_to_both_kinds_of_tile() {
        use config::Block;
        use ratatui::crossterm::event::KeyCode;
        assert!(ui::HOST_FIELDS.contains(&"folder") && ui::HOST_FIELDS.contains(&"command"));
        assert!(ui::LOCAL_FIELDS.contains(&"folder") && ui::LOCAL_FIELDS.contains(&"command"));

        let mut app = App::new();
        let mut form = Form::new(Block::Host);
        form.set("folder", "~/thesis");
        form.set("command", "tmux attach");
        app.mode = Mode::Add(form);
        press(&mut app, KeyCode::Right);
        let Mode::Add(f) = &app.mode else { panic!("left the form") };
        assert_eq!(f.block, Block::Local, "the kind row switched");
        assert_eq!(f.field("folder"), "~/thesis", "carried across");
        assert_eq!(f.field("command"), "tmux attach");
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

