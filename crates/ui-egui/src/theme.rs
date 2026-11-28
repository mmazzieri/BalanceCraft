//! Colours of the light and dark themes. egui's own error, warning, link and
//! weak-text colours fall short of 4.5:1 on the light panel, so both themes
//! get colours that reach it; everything else stays egui's.

use egui::{Color32, Visuals};

/// Colours that carry meaning: deadlines, area silence, the status bar.
struct Tokens {
    error: Color32,
    warn: Color32,
    ok: Color32,
    link: Color32,
    weak: Color32,
}

const LIGHT: Tokens = Tokens {
    error: Color32::from_rgb(0xB0, 0x1E, 0x1E),
    warn: Color32::from_rgb(0x8A, 0x4B, 0x00),
    ok: Color32::from_rgb(0x1E, 0x6B, 0x34),
    link: Color32::from_rgb(0x0B, 0x5C, 0xAD),
    weak: Color32::from_gray(0x5E),
};

const DARK: Tokens = Tokens {
    error: Color32::from_rgb(0xFF, 0x8A, 0x80),
    warn: Color32::from_rgb(0xFF, 0xC2, 0x66),
    ok: Color32::from_rgb(0x7B, 0xD8, 0x8F),
    link: Color32::from_rgb(0x8A, 0xB4, 0xF8),
    weak: Color32::from_gray(0xA8),
};

fn tokens(v: &Visuals) -> &'static Tokens {
    if v.dark_mode { &DARK } else { &LIGHT }
}

/// Puts the theme's colours into egui's visuals, for both themes.
pub fn install(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| tune(&mut style.visuals));
    }
}

fn tune(v: &mut Visuals) {
    let t = tokens(v);
    v.error_fg_color = t.error;
    v.warn_fg_color = t.warn;
    v.hyperlink_color = t.link;
    v.weak_text_color = Some(t.weak);
}

/// The colour of a deadline that is far enough away.
pub fn ok_color(v: &Visuals) -> Color32 {
    tokens(v).ok
}

#[cfg(test)]
mod tests {
    use super::{ok_color, tune};
    use egui::{Color32, Visuals};

    fn luminance(c: Color32) -> f64 {
        let lin = |u: u8| {
            let x = f64::from(u) / 255.0;
            if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    /// WCAG contrast of `fg` (premultiplied, as egui keeps it) drawn on `bg`.
    fn contrast(fg: Color32, bg: Color32) -> f64 {
        let a = 1.0 - f64::from(fg.a()) / 255.0;
        let over = |f: u8, b: u8| (f64::from(f) + f64::from(b) * a).round().clamp(0.0, 255.0) as u8;
        let seen = Color32::from_rgb(over(fg.r(), bg.r()), over(fg.g(), bg.g()), over(fg.b(), bg.b()));
        let (l1, l2) = (luminance(seen), luminance(bg));
        (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
    }

    #[test]
    fn meaningful_text_is_readable_in_both_themes() {
        for mut v in [Visuals::light(), Visuals::dark()] {
            tune(&mut v);
            let colours = [v.error_fg_color, v.warn_fg_color, v.hyperlink_color, v.weak_text_color(), ok_color(&v)];
            for bg in [v.panel_fill, v.window_fill] {
                for fg in colours {
                    let c = contrast(fg, bg);
                    assert!(c >= 4.5, "{fg:?} on {bg:?}: {c:.2}:1 (dark: {})", v.dark_mode);
                }
            }
        }
    }
}
