//! One look for everything the edge draws (docs/EDGE_RUST.md, "One look, one set of settings"):
//! the colours from the system (SURFACE_COLOR, TEXT_COLOR, BORDER_ACTIVE) or a scheme built from the
//! accent, the corners, the cards' opacity.

use crate::config::Config;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f64, pub f64, pub f64, pub f64);

impl Rgba {
    pub fn rgb(c: [u8; 3]) -> Rgba {
        Rgba(
            c[0] as f64 / 255.0,
            c[1] as f64 / 255.0,
            c[2] as f64 / 255.0,
            1.0,
        )
    }
    /// Relative luminance (WCAG), from sRGB.
    pub fn luminance(self) -> f64 {
        let f = |c: f64| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(self.0) + 0.7152 * f(self.1) + 0.0722 * f(self.2)
    }
    /// Contrast ratio with `o` (WCAG): 1 to 21.
    pub fn contrast(self, o: Rgba) -> f64 {
        let (a, b) = (self.luminance(), o.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    pub fn alpha(self, a: f64) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }
    pub fn mix(self, o: Rgba, t: f64) -> Rgba {
        Rgba(
            self.0 + (o.0 - self.0) * t,
            self.1 + (o.1 - self.1) * t,
            self.2 + (o.2 - self.2) * t,
            self.3 + (o.3 - self.3) * t,
        )
    }
    fn hsl(h: f64, s: f64, l: f64) -> Rgba {
        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let hp = (h.rem_euclid(360.0)) / 60.0;
        let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = l - c / 2.0;
        Rgba(r + m, g + m, b + m, 1.0)
    }
    fn hue(self) -> f64 {
        let (r, g, b) = (self.0, self.1, self.2);
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        let d = mx - mn;
        if d == 0.0 {
            return 0.0;
        }
        let h = if mx == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h * 60.0
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    /// a card's fill (with its opacity)
    pub card: Rgba,
    /// a card's 1 px edge
    pub edge: Rgba,
    pub text: Rgba,
    /// secondary text: labels, states that are off, times
    pub muted: Rgba,
    /// what is on or active
    pub accent: Rgba,
    /// text and icons on an accent fill
    pub on_accent: Rgba,
    /// the accent where it is text or a thin line on a card: the accent itself when it reads
    /// there (3:1 or more against the card), else moved toward the text colour until it does
    pub accent_text: Rgba,
    /// a thin bar's empty part
    pub track: Rgba,
    pub corners: f64,
    /// SWITCH_LOOK: rows, icons, tiles
    pub switch_look: String,
}

/// `true` when `fg` has at least 3:1 WCAG contrast on opaque `bg` (`Theme`'s readable check).
pub fn is_readable(fg: Rgba, bg: Rgba) -> bool {
    fg.contrast(bg.alpha(1.0)) >= 3.0
}

/// `fg` as it reads on `bg`: itself at 3:1 contrast or more, else mixed toward `toward` in tenths
/// until it does.
fn readable(fg: Rgba, bg: Rgba, toward: Rgba) -> Rgba {
    (0..=10)
        .map(|i| fg.mix(toward, i as f64 / 10.0))
        .find(|c| is_readable(*c, bg))
        .unwrap_or(toward)
}

impl Theme {
    pub fn from_config(c: &Config) -> Theme {
        let surface = Rgba::rgb(c.surface);
        let text = Rgba::rgb(c.text);
        let seed = Rgba::rgb(c.accent);
        let h = seed.hue();
        // per scheme: the card tint's saturation and the accent's (0..1)
        let (card, accent) = match c.scheme.as_str() {
            "monochrome" => (Rgba::hsl(0.0, 0.0, 0.09), Rgba(0.91, 0.91, 0.91, 1.0)),
            "neutral" => (Rgba::hsl(h, 0.06, 0.09), Rgba::hsl(h, 0.22, 0.72)),
            "tonal" => (Rgba::hsl(h, 0.14, 0.09), Rgba::hsl(h, 0.60, 0.72)),
            "vibrant" => (Rgba::hsl(h, 0.28, 0.09), Rgba::hsl(h, 0.88, 0.72)),
            "expressive" => (Rgba::hsl(h, 0.20, 0.09), Rgba::hsl(h, 0.75, 0.72)),
            // "yours" and anything unknown: the system's colours as they are
            _ => (surface, seed),
        };
        let edge = match (c.scheme.as_str(), c.hair) {
            ("yours", Some(hair)) | ("", Some(hair)) => Rgba::rgb(hair).alpha(0.9),
            _ => text.alpha(0.10),
        };
        let lum = 0.2126 * accent.0 + 0.7152 * accent.1 + 0.0722 * accent.2;
        Theme {
            card: card.alpha(c.opacity),
            edge,
            text,
            muted: text.mix(surface, 0.32),
            accent,
            on_accent: if lum > 0.45 { surface } else { text },
            accent_text: readable(accent, card, text),
            track: text.alpha(0.12),
            corners: c.corners,
            switch_look: c.switch_look.clone(),
        }
    }

    /// The resolved look as shell lines (`--print-theme`), for everything else Infynitae draws or
    /// configures (GTK, mako, fuzzel, the lock screen, the C helpers): one place decides the look
    /// (docs/EDGE_RUST.md stage 7). Colours as #rrggbb, their opacity as two hex digits.
    pub fn shell_lines(&self) -> String {
        let hex = |c: Rgba| {
            let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("#{:02x}{:02x}{:02x}", b(c.0), b(c.1), b(c.2))
        };
        let aa = |c: Rgba| format!("{:02x}", (c.3.clamp(0.0, 1.0) * 255.0).round() as u8);
        let mut out = String::new();
        for (name, c) in [
            ("CARD", self.card),
            ("EDGE", self.edge),
            ("TEXT", self.text),
            ("MUTED", self.muted),
            ("ACCENT", self.accent),
            ("ON_ACCENT", self.on_accent),
            ("TRACK", self.track),
        ] {
            out += &format!("LOOK_{name}=\"{}\"\nLOOK_{name}_AA=\"{}\"\n", hex(c), aa(c));
        }
        out += &format!(
            "LOOK_CORNERS=\"{}\"\nLOOK_SMALL_RADIUS=\"{}\"\n",
            self.corners.round() as i64,
            self.small_radius() as i64
        );
        out
    }

    /// a small element's radius (buttons, art), from the cards' corners
    pub fn small_radius(&self) -> f64 {
        (self.corners * 0.55).round()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_look_prints_as_shell_lines() {
        let c = Config::from_text(
            "SURFACE_COLOR=\"#171013\"\nTEXT_COLOR=\"#ffffff\"\nBORDER_ACTIVE=\"#cc70a9\"\nCORNERS=\"12\"\nCARD_OPACITY=\"80\"\n",
        );
        let out = Theme::from_config(&c).shell_lines();
        assert!(
            out.contains("LOOK_CARD=\"#171013\"\nLOOK_CARD_AA=\"cc\"\n"),
            "{out}"
        );
        assert!(out.contains("LOOK_ACCENT=\"#cc70a9\"\n"), "{out}");
        assert!(
            out.contains("LOOK_CORNERS=\"12\"\nLOOK_SMALL_RADIUS=\"7\"\n"),
            "{out}"
        );
        assert_eq!(out.lines().count(), 16);
    }

    #[test]
    fn yours_takes_the_system_colours_as_they_are() {
        let c = Config::from_text(
            "SURFACE_COLOR=\"#171013\"\nBORDER_ACTIVE=\"#cc70a9\"\nBORDER_INACTIVE=\"#452e3c\"\n",
        );
        let t = Theme::from_config(&c);
        assert_eq!(t.accent, Rgba::rgb([0xcc, 0x70, 0xa9]));
        assert_eq!(
            (t.card.0, t.card.1, t.card.2),
            (
                0x17 as f64 / 255.0,
                0x10 as f64 / 255.0,
                0x13 as f64 / 255.0
            )
        );
        assert!((t.card.3 - 0.90).abs() < 1e-9);
    }

    #[test]
    fn monochrome_has_no_hue() {
        let c = Config::from_text("THEME_SCHEME=\"monochrome\"\nBORDER_ACTIVE=\"#cc70a9\"\n");
        let t = Theme::from_config(&c);
        assert!((t.accent.0 - t.accent.1).abs() < 1e-9 && (t.accent.1 - t.accent.2).abs() < 1e-9);
    }
}
