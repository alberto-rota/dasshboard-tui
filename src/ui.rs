//! All drawing. See `theme` for why red is rationed the way it is.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::app::{App, Mode};
use crate::config::{Block as CfgBlock, OpenIn};
use crate::entry::{Entry, Origin, PALETTE, Tint};
use crate::theme::Theme;

/// Cell height per tile: a 4-row box plus a 1-row gutter.
const CELL_H: u16 = 5;
/// A section title and the blank line under it.
const TITLE_H: u16 = 2;
/// Preferred cell width, gutter included. Tiles keep this size rather than
/// stretching, so a wide terminal gets whitespace instead of vast tiles.
const CELL_W: u16 = 38;
const MAX_COLS: usize = 4;
/// Rows of chrome around the grid: wordmark, blank, rule, blank, then a blank
/// and two footer rows below it.
const CHROME_H: u16 = 7;
const PAD: u16 = 2;

/// One drawn row of the grid.
///
/// Rows are a list rather than an arithmetic function of the tile count now,
/// because a title takes a row and a section boundary can end a row of tiles
/// early -- neither of which `count / cols` can express.
enum GridRow {
    /// A section's title, above the tiles it names.
    Title(usize),
    /// Positions in the visible list, left to right.
    Tiles(Vec<usize>),
    /// A named section with nothing to draw under its title.
    Empty,
}

impl GridRow {
    fn height(&self) -> u16 {
        match self {
            GridRow::Title(_) | GridRow::Empty => TITLE_H,
            GridRow::Tiles(_) => CELL_H,
        }
    }
}

/// Break the visible tiles into rows, with a title above each named section.
///
/// Driven by the sections rather than by the tiles, because a section with
/// nothing in it still has to appear: a group you have just made is empty by
/// definition, and one you cannot see is one you cannot aim a tile at.
///
/// An untitled section never gets a row of its own -- empty it has nothing to
/// say, and full it is the default screen, which is what keeps a config with no
/// sections in it looking exactly as it did before they existed.
fn grid_rows(app: &App, vis: &[usize], cols: usize) -> Vec<GridRow> {
    let group_of = |s: usize| -> Vec<usize> {
        (0..vis.len()).filter(|&n| app.entries[vis[n]].section == s).collect()
    };

    let mut rows = Vec::new();
    for (s, title) in app.sections.iter().enumerate() {
        let group = group_of(s);
        if group.is_empty() {
            // A filter is a temporary lens, not a rearrangement, so it does not
            // get to leave headings behind over nothing.
            if !title.is_empty() && app.filter.is_empty() {
                rows.push(GridRow::Title(s));
                rows.push(GridRow::Empty);
            }
            continue;
        }
        if !title.is_empty() {
            rows.push(GridRow::Title(s));
        }
        rows.extend(group.chunks(cols).map(|c| GridRow::Tiles(c.to_vec())));
    }

    // `build` gives every tile a real section, so this is empty in practice --
    // but a tile whose group went missing must end up on screen anyway rather
    // than falling out of the loop above unnoticed.
    let orphans: Vec<usize> =
        (0..vis.len()).filter(|&n| app.entries[vis[n]].section >= app.sections.len()).collect();
    rows.extend(orphans.chunks(cols).map(|c| GridRow::Tiles(c.to_vec())));
    rows
}

fn columns(full: Rect, count: usize) -> usize {
    let avail = full.width.saturating_sub(2 * PAD).max(1);
    ((avail / CELL_W) as usize).clamp(1, MAX_COLS).min(count.max(1))
}

/// Where the centred block of content sits this frame.
struct Placement {
    x: u16,
    y: u16,
    width: u16,
    cell_w: u16,
    /// Height given to the grid: as many whole rows as fit.
    grid_h: u16,
}

fn place(full: Rect, cols: usize, rows: &[GridRow]) -> Placement {
    let avail = full.width.saturating_sub(2 * PAD).max(1);
    let cell_w = (avail / cols as u16).min(CELL_W).max(1);
    // The last tile in a row has no gutter to its right.
    let width = (cols as u16 * cell_w).saturating_sub(2).max(1);

    // Whole rows only, and always at least one row of tiles: a terminal too
    // short for even one is better overflowing than blank.
    let for_grid = full.height.saturating_sub(CHROME_H);
    let mut grid_h = 0;
    let mut any_tiles = false;
    for r in rows {
        if any_tiles && grid_h + r.height() > for_grid {
            break;
        }
        grid_h += r.height();
        any_tiles |= matches!(r, GridRow::Tiles(_));
    }
    // At least one row of tiles, and never more than there is room for: a board
    // of nothing but empty groups must not push the grid off the bottom.
    let grid_h = grid_h.clamp(1, for_grid.max(CELL_H)).max(CELL_H.min(for_grid.max(1)));

    // Vertically centre the whole block; fall back to the top when it overflows.
    // The grid's last row is a gutter, which doubles as the blank above the
    // footer, so it is not part of the visible height.
    let content_h = grid_h - 1 + CHROME_H;
    let y = full.y + full.height.saturating_sub(content_h) / 2;

    Placement { x: full.x + full.width.saturating_sub(width) / 2, y, width, cell_w, grid_h }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let vis = app.visible();
    let full = f.area();
    let th = app.theme.clone();
    let th = &th;
    let cols = columns(full, vis.len());
    app.cols = cols;
    let rows = grid_rows(app, &vis, cols);
    let p = place(full, cols, &rows);

    let row = |dy: u16, h: u16| Rect { x: p.x, y: p.y + dy, width: p.width, height: h };

    draw_header(f, app, row(0, 1), vis.len(), th);
    draw_rule(f, row(2, 1), th);
    let grid = row(4, p.grid_h);
    draw_grid(f, app, grid, &vis, &rows, &p, th);
    draw_footer(f, app, row(4 + grid.height, 2), th);

    match &app.mode {
        Mode::Add(form) | Mode::Edit(form) => {
            dim_behind(f, full, th);
            draw_form(f, full, form, app.backend.can_spawn(), th);
        }
        Mode::ConfirmDelete(i) => {
            dim_behind(f, full, th);
            let reverting = app.entries[*i].origin() == Some(Origin::SshOverridden);
            draw_confirm(f, full, app.entries[*i].label(), reverting, th);
        }
        Mode::Folders { entry, sel, .. } => {
            dim_behind(f, full, th);
            let e = &app.entries[*entry];
            draw_folders(f, full, &e.label, &e.folders, *sel, th);
        }
        Mode::Settings { focus, buf } => {
            dim_behind(f, full, th);
            draw_settings(f, full, app, *focus, buf, th);
        }
        Mode::Sections { focus, buf } => {
            dim_behind(f, full, th);
            draw_sections(f, full, app, *focus, buf.as_deref(), th);
        }
        // Move mode is not a modal: the grid *is* the thing being edited, so it
        // stays lit and the grabbed tile marks itself.
        _ => {}
    }
}

/// Flatten everything already drawn to the hairline colour, so a modal reads as
/// in front of the screen rather than tangled up in it.
fn dim_behind(f: &mut Frame, area: Rect, th: &Theme) {
    let buf = f.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            buf[(x, y)].set_fg(th.faint);
        }
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect, shown: usize, th: &Theme) {
    let wordmark = Line::from(vec![
        Span::styled("◆", Style::default().fg(th.accent)),
        Span::styled("  DASSH", Style::default().fg(th.bright).add_modifier(Modifier::BOLD)),
        Span::styled("BOARD", Style::default().fg(th.primary).add_modifier(Modifier::BOLD)),
    ]);

    let hosts = app.entries.iter().filter(|e| !e.is_local()).count();
    let locals = app.entries.len() - hosts;
    let right = if shown == app.entries.len() {
        format!("{hosts} ssh · {locals} local")
    } else {
        format!("{shown} of {} shown", app.entries.len())
    };

    f.render_widget(Paragraph::new(wordmark), area);
    f.render_widget(
        Paragraph::new(Line::styled(right, Style::default().fg(th.muted)).right_aligned()),
        area,
    );
}

/// A short accent stroke that runs into a long hairline -- the one piece of
/// pure decoration, and the only place red appears without meaning "here".
fn draw_rule(f: &mut Frame, area: Rect, th: &Theme) {
    let lead = (area.width / 6).clamp(3, 14) as usize;
    let rest = (area.width as usize).saturating_sub(lead);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("━".repeat(lead), Style::default().fg(th.accent)),
            Span::styled("─".repeat(rest), Style::default().fg(th.faint)),
        ])),
        area,
    );
}

fn draw_grid(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    vis: &[usize],
    rows: &[GridRow],
    p: &Placement,
    th: &Theme,
) {
    app.hitboxes.clear();
    app.tile_rows.clear();
    if area.width == 0 || area.height == 0 {
        return;
    }

    if vis.is_empty() {
        let msg = if app.entries.is_empty() {
            "no hosts yet — press a to add one"
        } else {
            "nothing matches"
        };
        f.render_widget(
            Paragraph::new(Line::styled(msg, Style::default().fg(th.muted)).centered()),
            area,
        );
        return;
    }

    let height_from = |start: usize| -> u16 {
        rows[start..].iter().map(|r| r.height()).sum()
    };
    app.scroll = app.scroll.min(rows.len().saturating_sub(1));

    // Keep the selected tile in view, and its title with it -- a group you have
    // scrolled into head-first should still say which group it is.
    if let Some(sel_row) =
        rows.iter().position(|r| matches!(r, GridRow::Tiles(t) if t.contains(&app.sel)))
    {
        let top = if sel_row > 0 && matches!(rows[sel_row - 1], GridRow::Title(_)) {
            sel_row - 1
        } else {
            sel_row
        };
        app.scroll = app.scroll.min(top);
        while app.scroll < sel_row
            && rows[app.scroll..=sel_row].iter().map(|r| r.height()).sum::<u16>() > area.height
        {
            app.scroll += 1;
        }
    }
    // Don't leave a gap at the bottom while there is still something above.
    while app.scroll > 0 && height_from(app.scroll - 1) <= area.height {
        app.scroll -= 1;
    }

    let mut y = area.y;
    let mut drawn = 0;
    for r in &rows[app.scroll..] {
        if y + r.height() > area.y + area.height {
            break;
        }
        match r {
            GridRow::Title(s) => draw_section_title(
                f,
                Rect { x: area.x, y, width: area.width, height: 1 },
                &app.sections[*s],
                // What is on screen, not what the group holds: a heading that
                // counted hidden or filtered-out tiles would contradict the
                // tiles under it.
                vis.iter().filter(|&&i| app.entries[i].section == *s).count(),
                th,
            ),
            GridRow::Tiles(items) => {
                for (c, &i) in items.iter().enumerate() {
                    let cell = Rect {
                        x: area.x + c as u16 * p.cell_w,
                        y,
                        width: p.cell_w,
                        height: CELL_H,
                    };
                    // Hit-test the whole cell, gutter included -- a near miss
                    // should still land on the tile you aimed at.
                    app.hitboxes.push((cell, i));
                    draw_tile(
                        f,
                        &app.entries[vis[i]],
                        cell,
                        i,
                        i == app.sel,
                        app.hover == Some(i),
                        matches!(app.mode, Mode::Move) && i == app.sel,
                        th,
                    );
                }
                app.tile_rows.push(items.clone());
            }
            // Says what to do about it, right where the tiles would be: this is
            // the one place a group's own emptiness is visible, and the gesture
            // that fills it is not one you would guess.
            GridRow::Empty => f.render_widget(
                Paragraph::new(Line::styled(
                    "    empty — press m on a tile and move it in here",
                    Style::default().fg(th.faint),
                )),
                Rect { x: area.x, y, width: area.width, height: 1 },
            ),
        }
        y += r.height();
        drawn += 1;
    }

    if drawn < rows.len() {
        let x = area.x + area.width + 1;
        if x < f.area().width {
            draw_scrollbar(
                f,
                Rect { x, y: area.y, width: 1, height: area.height.saturating_sub(1) },
                app.scroll,
                drawn,
                rows.len(),
                th,
            );
        }
    }
}

/// A group's title, then a hairline out to the edge -- the same shape as the
/// rule under the wordmark, one level quieter, so the page reads as a hierarchy
/// rather than as two competing headers.
fn draw_section_title(f: &mut Frame, area: Rect, title: &str, count: usize, th: &Theme) {
    let name = fit(title, area.width.saturating_sub(10) as usize);
    let tally = format!("  {count}  ");
    let rest = (area.width as usize).saturating_sub(width_of(&name) + width_of(&tally));
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(name, Style::default().fg(th.bright).add_modifier(Modifier::BOLD)),
            Span::styled(tally, Style::default().fg(th.muted)),
            Span::styled("─".repeat(rest), Style::default().fg(th.faint)),
        ])),
        area,
    );
}

fn draw_scrollbar(f: &mut Frame, area: Rect, scroll: usize, fit: usize, total: usize, th: &Theme) {
    let h = area.height as usize;
    if h == 0 || total == 0 {
        return;
    }
    let thumb = ((fit * h) / total).clamp(1, h);
    let top = ((scroll * h) / total).min(h - thumb);
    let lines: Vec<Line> = (0..h)
        .map(|i| {
            let inside = i >= top && i < top + thumb;
            let (glyph, color) = if inside { ("┃", th.accent) } else { ("│", th.faint) };
            Line::styled(glyph, Style::default().fg(color))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

#[allow(clippy::too_many_arguments)]
fn draw_tile(
    f: &mut Frame,
    entry: &Entry,
    cell: Rect,
    idx: usize,
    sel: bool,
    hover: bool,
    moving: bool,
    th: &Theme,
) {
    // Shrink out of the cell to leave a gutter between tiles.
    let area = Rect {
        width: cell.width.saturating_sub(2).max(1),
        height: cell.height.saturating_sub(1).max(1),
        ..cell
    };

    // Four states, four weights: a grabbed tile doubles its border so it reads
    // as lifted off the grid, selection gets thick red, hover lifts the border
    // to primary, idle stays a hairline.
    let (border_style, border_color, mut label_color) = match (moving, sel, hover) {
        (true, _, _) => (BorderType::Double, th.accent, th.bright),
        (false, true, _) => (BorderType::Thick, th.accent, th.bright),
        (false, false, true) => (BorderType::Rounded, th.primary, th.bright),
        (false, false, false) => (BorderType::Rounded, th.faint, th.primary),
    };
    if entry.hidden() && !sel {
        label_color = th.muted;
    }

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_style)
        .border_style(Style::default().fg(border_color));

    if idx < 9 {
        block = block.title_top(
            Line::styled(
                format!(" {} ", idx + 1),
                Style::default()
                    .fg(if sel { th.accent } else { th.muted })
                    .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
            )
            .left_aligned(),
        );
    }
    if moving {
        block = block
            .title_top(Line::styled(" moving ", Style::default().fg(th.accent)).right_aligned());
    } else if entry.is_local() {
        block = block
            .title_top(Line::styled(" local ", Style::default().fg(th.accent_dim)).right_aligned());
    } else if entry.hidden() {
        // Only ever seen with show_hidden on, which is how you get one back.
        block = block
            .title_top(Line::styled(" hidden ", Style::default().fg(th.accent_dim)).right_aligned());
    }

    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    // A solid bar rather than an arrow: it reads as a state, not a cursor. The
    // dot beside it is the host's own colour -- identity, not state, which is
    // why the two never share a glyph.
    let marker = if sel { "▌" } else { " " };
    const INDENT: usize = 4;
    let avail = (inner.width as usize).saturating_sub(INDENT);

    let name = Line::from(vec![
        Span::styled(marker, Style::default().fg(th.accent)),
        Span::styled(" ● ", Style::default().fg(entry.tint().color)),
        Span::styled(
            fit(entry.label(), avail),
            Style::default().fg(label_color).add_modifier(Modifier::BOLD),
        ),
    ]);

    let detail_color = if sel { th.primary } else { th.muted };
    let target = entry.detail();
    let mut detail = vec![Span::raw("    ")];
    match entry.jump() {
        // Spell out the jump when there is room, otherwise keep just the arrow
        // as a hint -- losing the domain matters more than losing the name.
        Some(jump) if width_of(target) + width_of(jump) + 5 <= avail => {
            detail.push(Span::styled(target, Style::default().fg(detail_color)));
            detail.push(Span::styled(format!("  ⤳ {jump}"), Style::default().fg(th.accent_dim)));
        }
        Some(_) if width_of(target) + 2 <= avail => {
            detail.push(Span::styled(target, Style::default().fg(detail_color)));
            detail.push(Span::styled(" ⤳", Style::default().fg(th.accent_dim)));
        }
        _ => detail.push(Span::styled(fit(target, avail), Style::default().fg(detail_color))),
    }

    f.render_widget(Paragraph::new(vec![name, Line::from(detail)]), inner);
}

fn key_hints(th: &Theme, pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, what)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), Style::default().fg(th.primary)));
        spans.push(Span::styled(format!(" {what}"), Style::default().fg(th.faint)));
    }
    Line::from(spans)
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let keys = match &app.mode {
        Mode::Filter => Line::from(vec![
            Span::styled("find ", Style::default().fg(th.faint)),
            Span::styled(
                app.filter.clone(),
                Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▌", Style::default().fg(th.accent)),
            Span::styled("    ⏎ open   esc cancel", Style::default().fg(th.faint)),
        ]),
        Mode::Browse => {
            // The destination keys are only advertised where there is a choice
            // to make: with nothing able to open a tab, t/w/c all mean ⏎.
            let mut hints = vec![("⏎", "open")];
            if app.backend.can_spawn() {
                hints.push(("t/w/c", "tab·win·here"));
            }
            hints.extend([
                ("/", "find"),
                ("a", "add"),
                ("e", "edit"),
                ("m", "move"),
                ("S", "groups"),
                ("x", "hide"),
                ("s", "settings"),
                ("q", "shell"),
            ]);
            key_hints(th, &hints)
        }
        // The one mode with no modal in front of it, so the footer is the only
        // place that can say the arrows have stopped moving the cursor.
        Mode::Move => key_hints(
            th,
            &[("←→", "reorder"), ("↑↓", "by a row"), ("g/G", "to an end"), ("⏎", "drop")],
        ),
        _ => Line::raw(""),
    };

    let status = match &app.status {
        Some(s) if s.good => Line::from(vec![
            Span::styled("✓ ", Style::default().fg(th.primary)),
            Span::styled(s.text.clone(), Style::default().fg(th.muted)),
        ]),
        Some(s) => Line::from(vec![
            Span::styled("✗ ", Style::default().fg(th.accent)),
            Span::styled(s.text.clone(), Style::default().fg(th.accent)),
        ]),
        None => Line::raw(""),
    };

    f.render_widget(Paragraph::new(vec![keys, status]), area);
}

/// Centre a box of the given size inside `area`.
fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// Clear a slightly larger region than the modal itself, so it sits in its own
/// gutter instead of touching whatever it covers.
fn modal_area(f: &mut Frame, area: Rect, w: u16, h: u16) -> Rect {
    f.render_widget(Clear, centered(area, w + 4, h + 2));
    centered(area, w, h)
}

fn modal_block(title: &str, th: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(th.accent))
        .title_top(Line::styled(
            format!(" {title} "),
            Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
        ))
}

// ------------------------------------------------------------------- form

pub const HOST_FIELDS: [&str; 6] =
    ["name", "hostname", "user", "port", "jump", "folders"];
pub const LOCAL_FIELDS: [&str; 4] = ["label", "command", "detail", "folders"];

pub struct Field {
    pub key: &'static str,
    pub value: String,
    /// What this field inherits if left blank.
    pub placeholder: String,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ColorChoice {
    /// Let the palette decide from the name.
    Auto,
    Preset(usize),
    /// A hand-typed hex, valid or not yet.
    Custom(String),
}

impl ColorChoice {
    pub fn from_config(hex: Option<&str>) -> ColorChoice {
        match hex {
            None => ColorChoice::Auto,
            Some(h) => match PALETTE.iter().position(|s| s.hex.eq_ignore_ascii_case(h)) {
                Some(i) => ColorChoice::Preset(i),
                None => ColorChoice::Custom(h.to_string()),
            },
        }
    }

    /// What to write to config.toml; empty means omit the key.
    pub fn to_config(&self) -> String {
        match self {
            ColorChoice::Auto => String::new(),
            ColorChoice::Preset(i) => PALETTE[i % PALETTE.len()].hex.to_string(),
            ColorChoice::Custom(s) => s.clone(),
        }
    }

    /// Cycle Auto -> each preset -> Auto. A typed colour rejoins at the start.
    pub fn cycle(&self, forward: bool) -> ColorChoice {
        let n = PALETTE.len() as i32;
        let current = match self {
            ColorChoice::Auto | ColorChoice::Custom(_) => 0,
            ColorChoice::Preset(i) => *i as i32 + 1,
        };
        let next = (current + if forward { 1 } else { -1 }).rem_euclid(n + 1);
        if next == 0 { ColorChoice::Auto } else { ColorChoice::Preset((next - 1) as usize) }
    }

    fn preview(&self, name: &str) -> Option<Tint> {
        match self {
            ColorChoice::Auto => (!name.is_empty()).then(|| Tint::for_name(name)),
            ColorChoice::Preset(i) => Some(Tint::slot(*i)),
            ColorChoice::Custom(s) => Tint::parse(s),
        }
    }
}

/// The add/edit form. The field list depends on what is being described -- a
/// host and a local tile have almost nothing in common past the name -- so it
/// is a `Vec` rather than a fixed array, and the toggle rows sit after it.
pub struct Form {
    pub block: CfgBlock,
    pub fields: Vec<Field>,
    pub color: ColorChoice,
    pub hidden: bool,
    pub open_in: Option<OpenIn>,
    pub focus: usize,
    pub error: Option<String>,
    /// The original name when editing; `None` when adding.
    pub editing: Option<String>,
    /// An ~/.ssh/config host's name is the key joining the two files.
    pub name_locked: bool,
    /// You can't turn a host into a local tile by editing it.
    pub kind_locked: bool,
}

/// Row 0 picks host or local; the fields follow; the toggles come last.
pub const KIND_ROW: usize = 0;

impl Form {
    fn fields_for(block: CfgBlock) -> Vec<Field> {
        let keys: &[&'static str] = match block {
            CfgBlock::Host => &HOST_FIELDS,
            CfgBlock::Local => &LOCAL_FIELDS,
        };
        keys.iter()
            .map(|k| Field { key: k, value: String::new(), placeholder: String::new() })
            .collect()
    }

    pub fn new(block: CfgBlock) -> Self {
        Self {
            block,
            fields: Form::fields_for(block),
            color: ColorChoice::Auto,
            hidden: false,
            open_in: None,
            focus: KIND_ROW,
            error: None,
            editing: None,
            name_locked: false,
            kind_locked: false,
        }
    }

    pub fn first_field(&self) -> usize {
        KIND_ROW + 1
    }
    pub fn color_row(&self) -> usize {
        self.first_field() + self.fields.len()
    }
    pub fn hidden_row(&self) -> usize {
        self.color_row() + 1
    }
    pub fn open_row(&self) -> usize {
        self.hidden_row() + 1
    }
    pub fn rows(&self) -> usize {
        self.open_row() + 1
    }

    pub fn field(&self, key: &str) -> &str {
        self.fields.iter().find(|f| f.key == key).map_or("", |f| f.value.as_str())
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        if let Some(f) = self.fields.iter_mut().find(|f| f.key == key) {
            f.value = value.into();
        }
    }

    pub fn set_placeholder(&mut self, key: &str, value: impl Into<String>) {
        if let Some(f) = self.fields.iter_mut().find(|f| f.key == key) {
            f.placeholder = value.into();
        }
    }

    /// Swap the field set, carrying across the two things both kinds have.
    pub fn switch_block(&mut self, block: CfgBlock) {
        if self.kind_locked || self.block == block {
            return;
        }
        let (name, folders) = (self.fields[0].value.clone(), self.field("folders").to_string());
        self.block = block;
        self.fields = Form::fields_for(block);
        self.fields[0].value = name;
        self.set("folders", folders);
        self.focus = KIND_ROW;
    }

    /// Move focus, stepping over the name when it is locked.
    pub fn move_focus(&mut self, forward: bool) {
        let n = self.rows();
        let step = |i: usize| if forward { (i + 1) % n } else { (i + n - 1) % n };
        self.focus = step(self.focus);
        if self.name_locked && self.focus == self.first_field() {
            self.focus = step(self.focus);
        }
        if self.kind_locked && self.focus == KIND_ROW {
            self.focus = step(self.focus);
        }
    }

    /// Cycle "use the default" and each explicit destination.
    pub fn cycle_open_in(&mut self, forward: bool) {
        let n = OpenIn::ALL.len() as i32;
        let cur = self.open_in.map_or(0, |o| o.index() as i32 + 1);
        let next = (cur + if forward { 1 } else { -1 }).rem_euclid(n + 1);
        self.open_in = (next > 0).then(|| OpenIn::ALL[(next - 1) as usize]);
    }

    /// Folders are typed as one comma-separated line; splitting here keeps the
    /// form a flat list of text rows instead of a nested editor.
    pub fn folder_list(&self) -> Vec<String> {
        self.field("folders")
            .split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect()
    }
}

fn draw_form(f: &mut Frame, area: Rect, form: &Form, spawns: bool, th: &Theme) {
    let title = match &form.editing {
        Some(n) => format!("edit {n}"),
        None => "add".to_string(),
    };
    let area = modal_area(f, area, 62, form.rows() as u16 + 5);
    let block = modal_block(&title, th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let row_mark = |on: bool| {
        Span::styled(if on { " ▌ " } else { "   " }, Style::default().fg(th.accent))
    };
    let label = |text: &str, on: bool| {
        Span::styled(
            format!("{text:<9}"),
            Style::default().fg(if on { th.bright } else { th.muted }),
        )
    };

    let mut lines = vec![Line::raw("")];

    // Kind.
    let on = form.focus == KIND_ROW && !form.kind_locked;
    lines.push(Line::from(vec![
        row_mark(on),
        label("kind", on),
        Span::styled(
            match form.block {
                CfgBlock::Host => "ssh host",
                CfgBlock::Local => "local command",
            },
            Style::default().fg(if form.kind_locked { th.muted } else { th.primary }),
        ),
        Span::styled(
            if form.kind_locked {
                "   fixed".to_string()
            } else if on {
                "   ←/→ switch".to_string()
            } else {
                String::new()
            },
            Style::default().fg(th.faint),
        ),
    ]));

    for (i, field) in form.fields.iter().enumerate() {
        let row = form.first_field() + i;
        let locked = i == 0 && form.name_locked;
        let on = row == form.focus && !locked;
        let hint = if !field.placeholder.is_empty() {
            format!("{}  ~/.ssh/config", field.placeholder)
        } else {
            match field.key {
                "name" | "label" => "required".into(),
                "command" => "required, e.g. /bin/zsh".into(),
                "hostname" => "defaults to name".into(),
                "port" => "22".into(),
                "folders" => "comma separated; more than one asks".into(),
                _ => "optional".into(),
            }
        };
        lines.push(Line::from(vec![
            row_mark(on),
            label(field.key, on),
            Span::styled(
                field.value.clone(),
                Style::default().fg(if locked { th.muted } else { th.primary }),
            ),
            Span::styled(if on { "▌" } else { "" }, Style::default().fg(th.accent)),
            Span::styled(
                if locked {
                    "  from ~/.ssh/config".to_string()
                } else if field.value.is_empty() {
                    format!("  {hint}")
                } else {
                    String::new()
                },
                Style::default().fg(th.faint),
            ),
        ]));
    }

    // Colour, with a live swatch and the circle the tab will carry.
    let on = form.focus == form.color_row();
    let tint = form.color.preview(&form.fields[0].value);
    let shown = match &form.color {
        ColorChoice::Auto => "auto".to_string(),
        ColorChoice::Preset(i) => PALETTE[*i].hex.to_string(),
        ColorChoice::Custom(s) => s.clone(),
    };
    lines.push(Line::from(vec![
        row_mark(on),
        label("color", on),
        Span::styled("● ", Style::default().fg(tint.as_ref().map_or(th.faint, |t| t.color))),
        Span::styled(shown, Style::default().fg(th.primary)),
        Span::styled(if on { "▌" } else { "" }, Style::default().fg(th.accent)),
        Span::styled(
            tint.as_ref().map_or(String::new(), |t| format!("   {} tab", t.emoji)),
            Style::default().fg(th.muted),
        ),
        Span::styled(if on { "   ←/→ pick" } else { "" }, Style::default().fg(th.faint)),
    ]));

    let on = form.focus == form.hidden_row();
    lines.push(Line::from(vec![
        row_mark(on),
        label("hidden", on),
        Span::styled(
            if form.hidden { "[on ]" } else { "[off]" },
            Style::default().fg(if form.hidden { th.primary } else { th.faint }),
        ),
        Span::styled(
            if form.hidden { "   off the home screen" } else { "" },
            Style::default().fg(th.muted),
        ),
        Span::styled(if on { "   space toggle" } else { "" }, Style::default().fg(th.faint)),
    ]));

    let on = form.focus == form.open_row();
    lines.push(Line::from(vec![
        row_mark(on),
        label("open in", on),
        Span::styled(
            form.open_in.map_or("default".to_string(), |o| o.label().to_string()),
            Style::default().fg(th.primary),
        ),
        // The value is still written and still travels with the config; it just
        // cannot be honoured by the terminal reading it right now.
        Span::styled(
            if spawns { "" } else { "  opens here" },
            Style::default().fg(th.faint),
        ),
        Span::styled(if on { "   ←/→ pick" } else { "" }, Style::default().fg(th.faint)),
    ]));

    lines.push(Line::raw(""));
    lines.push(match &form.error {
        Some(e) => Line::from(vec![
            Span::raw("   "),
            Span::styled("✗ ", Style::default().fg(th.accent)),
            Span::styled(e.clone(), Style::default().fg(th.accent)),
        ]),
        None => Line::from(vec![
            Span::raw("   "),
            Span::styled("tab/↑↓ field   ⏎ save   esc cancel", Style::default().fg(th.faint)),
        ]),
    });

    f.render_widget(Paragraph::new(lines), inner);
}

/// The folder picker, shown when a tile has somewhere to start.
fn draw_folders(f: &mut Frame, area: Rect, label: &str, folders: &[String], sel: usize, th: &Theme) {
    let rows = folders.len() + 1;
    let area = modal_area(f, area, 60, rows as u16 + 5);
    let block = modal_block(&format!("open {label} in"), th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = vec![Line::raw("")];
    // Index 0 is always "no folder", so a configured host is never forced
    // somewhere -- home stays one keystroke away.
    for (i, item) in std::iter::once(&"~".to_string()).chain(folders).enumerate() {
        let on = i == sel;
        let (text, dim) = if i == 0 {
            ("home".to_string(), "no cd".to_string())
        } else {
            let leaf = item.rsplit('/').next().unwrap_or(item).to_string();
            (leaf, item.clone())
        };
        lines.push(Line::from(vec![
            Span::styled(if on { " ▌ " } else { "   " }, Style::default().fg(th.accent)),
            Span::styled(
                format!("{} ", if i < 9 { (b'1' + i as u8) as char } else { ' ' }),
                Style::default().fg(th.faint),
            ),
            Span::styled(
                format!("{text:<18}"),
                Style::default().fg(if on { th.bright } else { th.primary }),
            ),
            Span::styled(dim, Style::default().fg(th.muted)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("↑↓ move   1-9 jump   ⏎ open   esc cancel", Style::default().fg(th.faint)),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

// ----------------------------------------------------------- other modals

fn draw_confirm(f: &mut Frame, area: Rect, label: &str, reverting: bool, th: &Theme) {
    let area = modal_area(f, area, 52, 6);
    let block = modal_block(if reverting { "revert" } else { "delete" }, th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    f.render_widget(
        Paragraph::new(vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if reverting { "drop customisation for " } else { "remove " },
                    Style::default().fg(th.primary),
                ),
                Span::styled(
                    label.to_string(),
                    Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if reverting { "?" } else { " from config.toml?" },
                    Style::default().fg(th.primary),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if reverting { "the tile stays, from ~/.ssh/config" } else { "" },
                    Style::default().fg(th.faint),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("y", Style::default().fg(th.accent).add_modifier(Modifier::BOLD)),
                Span::styled(
                    if reverting { " revert    " } else { " delete    " },
                    Style::default().fg(th.faint),
                ),
                Span::styled("n", Style::default().fg(th.primary)),
                Span::styled(" keep", Style::default().fg(th.faint)),
            ]),
        ]),
        inner,
    );
}

/// A row in the settings panel. Toggles write through on the keystroke;
/// colours write as soon as what you have typed is a valid hex, so the whole UI
/// restyles while you type.
///
/// `note` is for a row that is real but inert on this machine -- a tab tint
/// where nothing can open a tab. Those rows stay editable rather than
/// disappearing, since a config is often written on one machine and read on
/// another; the note is how the screen stays honest about it.
pub enum SettingRow {
    Toggle { key: &'static str, label: &'static str, on: bool, note: &'static str },
    /// The one row that is not in config.toml: it writes the shell rc hook that
    /// opens the home screen with a terminal. `detail` names the file being
    /// edited, because that is a bigger thing to do than flipping a flag.
    Startup { label: &'static str, on: bool, detail: String },
    Choice { key: &'static str, label: &'static str, value: String, note: &'static str },
    Color { key: &'static str, label: &'static str, value: String },
}

/// A note is set off from the value it qualifies, and takes no room at all when
/// there is nothing to say.
fn dim_note(note: &str) -> String {
    if note.is_empty() { String::new() } else { format!("  {note}") }
}

/// Cycled by the arrows on a colour row; typing overrides them.
pub const PRIMARY_PRESETS: [&str; 5] =
    ["#aaaaaa", "#c0caf5", "#d4d4d4", "#8b949e", "#e6edf3"];
pub const ACCENT_PRESETS: [&str; 6] =
    ["#ff0000", "#ff7a18", "#f5c211", "#3fb950", "#58a6ff", "#bc8cff"];

fn draw_settings(f: &mut Frame, area: Rect, app: &App, focus: usize, buf: &str, th: &Theme) {
    let rows = app.setting_rows();
    // The same width as the form: a row here can carry a label, a value and a
    // note saying the value is inert on this machine, which is one field more
    // than the panel was originally sized for.
    let area = modal_area(f, area, 62, rows.len() as u16 + 5);
    let block = modal_block("settings", th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = vec![Line::raw("")];
    for (i, row) in rows.iter().enumerate() {
        let on_this = i == focus;
        let mark = Span::styled(
            if on_this { " ▌ " } else { "   " },
            Style::default().fg(th.accent),
        );
        let label_style = Style::default().fg(if on_this { th.bright } else { th.muted });

        let mut spans = vec![mark];
        match row {
            SettingRow::Toggle { label, on, note, .. } => {
                spans.push(Span::styled(
                    if *on { "[on ] " } else { "[off] " },
                    Style::default().fg(if *on { th.primary } else { th.faint }),
                ));
                spans.push(Span::styled(label.to_string(), label_style));
                spans.push(Span::styled(dim_note(note), Style::default().fg(th.faint)));
            }
            SettingRow::Startup { label, on, detail } => {
                spans.push(Span::styled(
                    if *on { "[on ] " } else { "[off] " },
                    Style::default().fg(if *on { th.primary } else { th.faint }),
                ));
                spans.push(Span::styled(label.to_string(), label_style));
                spans.push(Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(th.faint),
                ));
            }
            SettingRow::Choice { label, value, note, .. } => {
                spans.push(Span::styled(format!("{label:<24}"), label_style));
                spans.push(Span::styled(value.clone(), Style::default().fg(th.primary)));
                spans.push(Span::styled(dim_note(note), Style::default().fg(th.faint)));
                if on_this {
                    spans.push(Span::styled("   ←/→", Style::default().fg(th.faint)));
                }
            }
            SettingRow::Color { label, value, .. } => {
                // While focused, show what is being typed rather than what is
                // saved, so a half-finished hex is visible.
                let shown = if on_this { buf } else { value.as_str() };
                let swatch = crate::theme::rgb(shown)
                    .map_or(th.faint, |(r, g, b)| ratatui::style::Color::Rgb(r, g, b));
                spans.push(Span::styled(format!("{label:<24}"), label_style));
                spans.push(Span::styled("● ", Style::default().fg(swatch)));
                spans.push(Span::styled(shown.to_string(), Style::default().fg(th.primary)));
                if on_this {
                    spans.push(Span::styled("▌", Style::default().fg(th.accent)));
                    spans.push(Span::styled("  ←/→ or type", Style::default().fg(th.faint)));
                }
            }
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled("↑↓ move   space/←/→ change   esc close", Style::default().fg(th.faint)),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

/// The group list: what sections exist, in the order they are drawn, and the
/// verbs that change that.
///
/// Membership is not edited here -- a tile is put in a group by being moved into
/// it, which is the same gesture as reordering and needs no list of checkboxes.
/// This panel is only about the groups themselves.
fn draw_sections(f: &mut Frame, area: Rect, app: &App, focus: usize, buf: Option<&str>, th: &Theme) {
    let rows = app.section_rows();
    let area = modal_area(f, area, 62, rows as u16 + 5);
    let block = modal_block("groups", th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = vec![Line::raw("")];
    for i in 0..rows {
        let on = i == focus;
        let typing = on.then_some(buf).flatten();
        let mut spans =
            vec![Span::styled(if on { " ▌ " } else { "   " }, Style::default().fg(th.accent))];

        match app.sections.get(i) {
            Some(title) => {
                let (text, style) = match (typing, title.is_empty()) {
                    (Some(t), _) => (t.to_string(), Style::default().fg(th.bright)),
                    // Every tile starts in an untitled group; it is a real group
                    // that simply draws no heading, so it is named as one here
                    // rather than left as a blank row.
                    (None, true) => ("untitled".to_string(), Style::default().fg(th.faint)),
                    (None, false) => (
                        title.clone(),
                        Style::default().fg(if on { th.bright } else { th.primary }),
                    ),
                };
                match typing {
                    // The caret has to sit against the name, so a row being
                    // typed into is not padded out to the tally column.
                    Some(_) => {
                        spans.push(Span::styled(text, style));
                        spans.push(Span::styled("▌", Style::default().fg(th.accent)));
                    }
                    None => {
                        let n = app.section_count(i);
                        spans.push(Span::styled(format!("{:<28}", fit(&text, 27)), style));
                        spans.push(Span::styled(
                            format!("{n} tile{}", if n == 1 { "" } else { "s" }),
                            Style::default().fg(th.muted),
                        ));
                    }
                }
            }
            None => match typing {
                Some(t) => {
                    spans.push(Span::styled(t.to_string(), Style::default().fg(th.bright)));
                    spans.push(Span::styled("▌", Style::default().fg(th.accent)));
                }
                None => spans.push(Span::styled(
                    "+ new group",
                    Style::default().fg(if on { th.bright } else { th.muted }),
                )),
            },
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled(
            if buf.is_some() {
                "⏎ save   esc cancel"
            } else {
                "↑↓ move  ⏎ rename  a new  d delete  J/K order  esc close"
            },
            Style::default().fg(th.faint),
        ),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

pub fn width_of(s: &str) -> usize {
    s.chars().count()
}

/// Truncate to `max` columns, marking the cut with an ellipsis.
pub fn fit(s: &str, max: usize) -> String {
    if width_of(s) <= max {
        return s.to_string();
    }
    match max {
        0 => String::new(),
        1 => "…".to_string(),
        _ => s.chars().take(max - 1).chain(['…']).collect(),
    }
}
