//! Color palettes that drive every UI style.
//!
//! At build time `build.rs` parses `themes/*.toml` and emits a static
//! `BUNDLED` table — see `bundled_palettes.rs` in `OUT_DIR`. At runtime the
//! TUI client looks up the active Omarchy theme name and resolves it against
//! that table; on non-Omarchy systems it falls back to [`Palette::legacy`],
//! whose values reproduce the original hardcoded look of kvn-tui.

use ratatui::style::Color;

/// A self-contained color palette. Matches the schema used by Omarchy's
/// per-theme `colors.toml`: six semantic colors plus a 16-entry ANSI ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub accent: Color,
    pub cursor: Color,
    pub foreground: Color,
    pub background: Color,
    pub selection_foreground: Color,
    pub selection_background: Color,
    pub ansi: [Color; 16],
}

include!(concat!(env!("OUT_DIR"), "/bundled_palettes.rs"));

impl Palette {
    /// Look up a bundled palette by Omarchy theme name (snake-case slug from
    /// `~/.config/omarchy/current/theme.name`).
    pub fn lookup(name: &str) -> Option<Palette> {
        BUNDLED.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
    }

    /// Names of every bundled palette, sorted alphabetically. Used by the
    /// in-TUI theme picker to build its menu.
    pub fn bundled_names() -> Vec<&'static str> {
        BUNDLED.iter().map(|(n, _)| *n).collect()
    }

    /// Fallback palette that reproduces the pre-theming look: cyan accents,
    /// gray text, named ANSI colors throughout.
    pub const fn legacy() -> Palette {
        Palette {
            accent: Color::Cyan,
            cursor: Color::White,
            foreground: Color::Gray,
            background: Color::Black,
            selection_foreground: Color::Cyan,
            selection_background: Color::Rgb(45, 45, 85),
            ansi: [
                Color::Black,
                Color::Red,
                Color::Green,
                Color::Yellow,
                Color::Blue,
                Color::Magenta,
                Color::Cyan,
                Color::White,
                Color::DarkGray,
                Color::LightRed,
                Color::LightGreen,
                Color::LightYellow,
                Color::LightBlue,
                Color::LightMagenta,
                Color::LightCyan,
                Color::White,
            ],
        }
    }
}

/// Linear interpolation between two colors in sRGB space. Used to derive a
/// "darkened" highlight (the active-connection row background) from the
/// palette's green and background. `t` is the weight of `b` in the result.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = to_rgb(a);
    let (br, bg, bb) = to_rgb(b);
    let lerp = |x: u8, y: u8| {
        let v = f32::from(x) * (1.0 - t) + f32::from(y) * t;
        v.round().clamp(0.0, 255.0) as u8
    };
    Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

/// Best-effort RGB resolution for a `ratatui` `Color`. Named ANSI colors use
/// the conventional VGA-ish triples; non-RGB exotic variants fall through to
/// black, which is fine because [`mix`] and the OSC 11 emitter only operate
/// on palette colors that are either RGB or familiar ANSI names.
pub fn to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (170, 0, 0),
        Color::Green => (0, 170, 0),
        Color::Yellow => (170, 85, 0),
        Color::Blue => (0, 0, 170),
        Color::Magenta => (170, 0, 170),
        Color::Cyan => (0, 170, 170),
        Color::Gray => (170, 170, 170),
        Color::DarkGray => (85, 85, 85),
        Color::LightRed => (255, 85, 85),
        Color::LightGreen => (85, 255, 85),
        Color::LightYellow => (255, 255, 85),
        Color::LightBlue => (85, 85, 255),
        Color::LightMagenta => (255, 85, 255),
        Color::LightCyan => (85, 255, 255),
        Color::White => (255, 255, 255),
        _ => (0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_all_19_omarchy_themes() {
        let names = Palette::bundled_names();
        let expected = [
            "catppuccin",
            "catppuccin-latte",
            "ethereal",
            "everforest",
            "flexoki-light",
            "gruvbox",
            "hackerman",
            "kanagawa",
            "lumon",
            "matte-black",
            "miasma",
            "nord",
            "osaka-jade",
            "retro-82",
            "ristretto",
            "rose-pine",
            "tokyo-night",
            "vantablack",
            "white",
        ];
        for name in expected {
            assert!(names.contains(&name), "missing palette: {name}");
        }
    }

    #[test]
    fn lookup_returns_known_gruvbox_palette() {
        let p = Palette::lookup("gruvbox").expect("gruvbox");
        // Spot-check accent — matches themes/gruvbox.toml.
        assert_eq!(p.accent, Color::Rgb(0x7d, 0xae, 0xa3));
        // color1 (red) used by error().
        assert_eq!(p.ansi[1], Color::Rgb(0xea, 0x69, 0x62));
    }

    #[test]
    fn lookup_unknown_theme_returns_none() {
        assert!(Palette::lookup("does-not-exist").is_none());
    }

    #[test]
    fn legacy_palette_preserves_pre_theming_look() {
        let p = Palette::legacy();
        assert_eq!(p.accent, Color::Cyan);
        assert_eq!(p.foreground, Color::Gray);
        assert_eq!(p.ansi[1], Color::Red);
        assert_eq!(p.ansi[2], Color::Green);
        assert_eq!(p.ansi[3], Color::Yellow);
        assert_eq!(p.ansi[8], Color::DarkGray);
        assert_eq!(p.selection_background, Color::Rgb(45, 45, 85));
    }

    #[test]
    fn mix_clamps_endpoints() {
        let a = Color::Rgb(100, 200, 0);
        let b = Color::Rgb(0, 0, 100);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
    }

    #[test]
    fn mix_blends_named_colors_via_rgb_table() {
        // Green=(0,170,0), Black=(0,0,0); 75% toward black ≈ (0, 43, 0).
        let blended = mix(Color::Green, Color::Black, 0.75);
        assert_eq!(blended, Color::Rgb(0, 43, 0));
    }
}
