//! Two colours, and the shades derived from them.
//!
//! The user picks a primary and an accent; everything else is computed, so a
//! new pair restyles the whole UI without anyone hand-picking six values. The
//! discipline the defaults were built on survives the change: accent means
//! selection, the brand mark and failure, and nothing else is allowed to use
//! it.

use ratatui::style::Color;

pub const DEFAULT_PRIMARY: &str = "#aaaaaa";
pub const DEFAULT_ACCENT: &str = "#ff0000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub primary_hex: String,
    pub accent_hex: String,
    /// Body text.
    pub primary: Color,
    /// Selection, the brand mark, errors.
    pub accent: Color,
    /// The one word that should out-shout its neighbours.
    pub bright: Color,
    /// Secondary text: hostnames, hints.
    pub muted: Color,
    /// Hairlines and idle borders.
    pub faint: Color,
    /// Tags and hover, so true accent stays exclusive to selection.
    pub accent_dim: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::new(DEFAULT_PRIMARY, DEFAULT_ACCENT)
    }
}

impl Theme {
    /// Invalid hex falls back to the default for that slot rather than
    /// blanking the UI -- a half-typed colour must still be readable.
    pub fn new(primary: &str, accent: &str) -> Theme {
        let p = rgb(primary).unwrap_or_else(|| rgb(DEFAULT_PRIMARY).unwrap());
        let a = rgb(accent).unwrap_or_else(|| rgb(DEFAULT_ACCENT).unwrap());

        // Ratios taken from the hand-tuned #aaaaaa palette, so the defaults
        // come out byte-identical to what they were before this was runtime.
        Theme {
            primary_hex: hex(p),
            accent_hex: hex(a),
            primary: col(p),
            accent: col(a),
            bright: col(lighten(p, 0.73)),
            muted: col(scale(p, 0.62)),
            faint: col(scale(p, 0.34)),
            accent_dim: col(scale(mix(a, p, 0.25), 0.70)),
        }
    }
}

pub fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((b(0)?, b(2)?, b(4)?))
}

pub fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn col((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

/// Toward black.
fn scale((r, g, b): (u8, u8, u8), f: f32) -> (u8, u8, u8) {
    let s = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    (s(r), s(g), s(b))
}

/// Toward white.
fn lighten((r, g, b): (u8, u8, u8), f: f32) -> (u8, u8, u8) {
    let s = |v: u8| (v as f32 + (255.0 - v as f32) * f).round().clamp(0.0, 255.0) as u8;
    (s(r), s(g), s(b))
}

/// `t` of the way from `a` to `b`.
fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round().clamp(0.0, 255.0) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derived ratios were reverse-engineered from the original hand-picked
    /// palette; this pins them so a refactor can't quietly restyle the app.
    #[test]
    fn the_default_theme_reproduces_the_original_shades() {
        let t = Theme::default();
        assert_eq!(t.primary, Color::Rgb(0xaa, 0xaa, 0xaa));
        assert_eq!(t.accent, Color::Rgb(0xff, 0x00, 0x00));
        assert_eq!(t.bright, Color::Rgb(0xe8, 0xe8, 0xe8));
        assert_eq!(t.muted, Color::Rgb(0x69, 0x69, 0x69));
        assert_eq!(t.faint, Color::Rgb(0x3a, 0x3a, 0x3a));
    }

    #[test]
    fn a_custom_pair_derives_its_own_shades() {
        let t = Theme::new("#7fd1b9", "#ffb703");
        assert_eq!(t.primary_hex, "#7fd1b9");
        assert_eq!(t.accent_hex, "#ffb703");
        // Derived shades must straddle the primary, not collapse onto it.
        assert_ne!(t.bright, t.primary);
        assert_ne!(t.muted, t.primary);
        assert_ne!(t.faint, t.muted);
    }

    #[test]
    fn nonsense_falls_back_instead_of_blanking_the_ui() {
        let t = Theme::new("not-a-colour", "");
        assert_eq!(t, Theme::default());
    }

    #[test]
    fn shades_stay_ordered_from_faint_to_bright() {
        for (p, a) in [("#aaaaaa", "#ff0000"), ("#7fd1b9", "#ffb703"), ("#333333", "#00ffcc")] {
            let t = Theme::new(p, a);
            let lum = |c: Color| match c {
                Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
                _ => unreachable!(),
            };
            assert!(lum(t.faint) <= lum(t.muted), "{p}: faint must not outshine muted");
            assert!(lum(t.muted) <= lum(t.primary), "{p}: muted must not outshine primary");
            assert!(lum(t.primary) <= lum(t.bright), "{p}: primary must not outshine bright");
        }
    }
}
