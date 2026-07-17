//! Embedded fonts (DejaVu — see assets/fonts/LICENSE-DejaVu) and simple
//! single-line shaping: advance + kerning, no bidi/complex scripts. Map
//! labels are short identifiers; this is deliberate scope.

use crate::display::FontKind;
use fontdue::{Font, FontSettings};
use std::sync::OnceLock;

static SANS: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");
static SANS_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans-Bold.ttf");
static SERIF: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSerif.ttf");
static SERIF_BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSerif-Bold.ttf");
static MONO: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");

pub fn font(kind: FontKind) -> &'static Font {
    static CELL: OnceLock<Vec<Font>> = OnceLock::new();
    let fonts = CELL.get_or_init(|| {
        [SANS, SANS_BOLD, SERIF, SERIF_BOLD, MONO]
            .iter()
            .map(|bytes| {
                Font::from_bytes(*bytes, FontSettings::default()).expect("embedded font must parse")
            })
            .collect()
    });
    match kind {
        FontKind::Sans => &fonts[0],
        FontKind::SansBold => &fonts[1],
        FontKind::Serif => &fonts[2],
        FontKind::SerifBold => &fonts[3],
        FontKind::Mono => &fonts[4],
    }
}

/// Width of `text` at `size_px`, in px.
pub fn measure(text: &str, size_px: f32, kind: FontKind) -> f32 {
    let f = font(kind);
    let mut w = 0f32;
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        let m = f.metrics(ch, size_px);
        w += m.advance_width;
        if let Some(p) = prev {
            w += f.horizontal_kern(p, ch, size_px).unwrap_or(0.0);
        }
        prev = Some(ch);
    }
    w
}

/// Rasterize a line into per-pixel coverage callbacks: `set(x, y, coverage)`
/// with (x, y) relative to the text origin (left baseline).
pub fn rasterize(text: &str, size_px: f32, kind: FontKind, mut set: impl FnMut(i32, i32, u8)) {
    let f = font(kind);
    let mut pen_x = 0f32;
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        if let Some(p) = prev {
            pen_x += f.horizontal_kern(p, ch, size_px).unwrap_or(0.0);
        }
        let (m, bitmap) = f.rasterize(ch, size_px);
        for gy in 0..m.height {
            for gx in 0..m.width {
                let cov = bitmap[gy * m.width + gx];
                if cov > 8 {
                    let x = (pen_x + m.xmin as f32) as i32 + gx as i32;
                    let y = -(m.ymin + m.height as i32) + gy as i32;
                    set(x, y, cov);
                }
            }
        }
        pen_x += m.advance_width;
        prev = Some(ch);
    }
}

/// Approximate line height (ascender above baseline) at `size_px`.
pub fn ascent(size_px: f32, kind: FontKind) -> f32 {
    let f = font(kind);
    f.horizontal_line_metrics(size_px)
        .map(|m| m.ascent)
        .unwrap_or(size_px * 0.8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_and_measure() {
        let w = measure("R3 src/auth", 16.0, FontKind::Sans);
        assert!(w > 30.0 && w < 200.0, "unexpected width {w}");
        let bold = measure("R3 src/auth", 16.0, FontKind::SansBold);
        assert!(bold > w * 0.9);
    }

    #[test]
    fn rasterizes_something() {
        let mut hits = 0usize;
        rasterize("Fx", 14.0, FontKind::Sans, |_, _, _| hits += 1);
        assert!(hits > 20);
    }
}
