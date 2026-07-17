//! Theme trait + shared styling helpers (label collision, field sampling).

use crate::display::{DisplayList, FontKind, Rgba};
use crate::raster::Raster;
use crate::scene::Scene;
use crate::text;

pub trait Theme {
    fn background(&self) -> Rgba;
    /// Base terrain: sea, land fills, coast, contours (pure vector).
    fn terrain(&self, scene: &Scene) -> DisplayList;
    /// Per-pixel pass between terrain and overlay (hillshade, paper grain).
    fn post_raster(&self, _scene: &Scene, _raster: &mut Raster) {}
    /// Everything above terrain: edges, cities, labels, decorations.
    fn overlay(&self, scene: &Scene) -> DisplayList;
}

/// Greedy label placement with collision rejection. Coordinates in px.
pub struct LabelPlacer {
    placed: Vec<(f32, f32, f32, f32)>, // x, y, w, h (top-left)
    width: f32,
    height: f32,
}

impl LabelPlacer {
    pub fn new(width: f32, height: f32) -> LabelPlacer {
        LabelPlacer {
            placed: Vec::new(),
            width,
            height,
        }
    }

    /// Try to claim a centered box at (cx, cy). Returns false on collision
    /// or out-of-canvas.
    pub fn try_place(&mut self, cx: f32, cy: f32, w: f32, h: f32) -> bool {
        let (x, y) = (cx - w / 2.0, cy - h / 2.0);
        if x < 1.0 || y < 1.0 || x + w > self.width - 1.0 || y + h > self.height - 1.0 {
            return false;
        }
        let pad = 3.0;
        for &(px, py, pw, ph) in &self.placed {
            if x < px + pw + pad && px < x + w + pad && y < py + ph + pad && py < y + h + pad {
                return false;
            }
        }
        self.placed.push((x, y, w, h));
        true
    }
}

/// Measure a label box for placement (single line).
pub fn label_box(s: &str, size_px: f32, font: FontKind) -> (f32, f32) {
    (text::measure(s, size_px, font), size_px * 1.15)
}

/// Bilinear sample of the scene's elevation field at normalized (x, y).
pub fn sample_elevation(scene: &Scene, x: f32, y: f32) -> f32 {
    let (w, h) = (scene.field_w, scene.field_h);
    if w == 0 || h == 0 {
        return 0.0;
    }
    let fx = (x * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
    let fy = (y * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (fx as usize, fy as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let v = |xx: usize, yy: usize| scene.elevation[yy * w + xx];
    let a = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * tx;
    let b = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * tx;
    a + (b - a) * ty
}

/// Language tag suffix shown in region labels ("src/auth · rs").
pub fn lang_suffix(lang: ctx2img_core::Lang) -> String {
    format!(" · {}", lang.tag())
}

/// Bounding box of a normalized polygon in canvas px: (x0, y0, x1, y1).
pub fn poly_bbox_px(poly: &[(f32, f32)], w: f32, h: f32) -> (f32, f32, f32, f32) {
    let mut bb = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in poly {
        bb.0 = bb.0.min(x * w);
        bb.1 = bb.1.min(y * h);
        bb.2 = bb.2.max(x * w);
        bb.3 = bb.3.max(y * h);
    }
    bb
}

/// Widest horizontal interval of the polygon at height `y` (canvas px).
fn row_interval(poly: &[(f32, f32)], w: f32, h: f32, y: f32) -> Option<(f32, f32)> {
    let mut xs: Vec<f32> = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let (x1, y1) = (poly[i].0 * w, poly[i].1 * h);
        let (x2, y2) = (poly[(i + 1) % n].0 * w, poly[(i + 1) % n].1 * h);
        if (y1 <= y && y2 > y) || (y2 <= y && y1 > y) {
            xs.push(x1 + (y - y1) / (y2 - y1) * (x2 - x1));
        }
    }
    xs.sort_by(f32::total_cmp);
    xs.chunks_exact(2)
        .map(|c| (c[0], c[1]))
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
}

/// The v0.2 text-flow engine: typeset `lines` inside a territory polygon.
///
/// Mono metrics, hard-wrap with `↪` continuation, comment lines dimmed,
/// explicit spill marker when content doesn't fit. Emits plain `Op::Text`
/// rows, so both backends (raster + SVG) render it with no new machinery.
/// Returns (ops, source_lines_consumed).
#[allow(clippy::too_many_arguments)]
pub fn flow_text_ops(
    poly: &[(f32, f32)],
    canvas_w: f32,
    canvas_h: f32,
    lines: &[String],
    size_px: f32,
    start_y_px: f32,
    ink: Rgba,
    dim: Rgba,
    spill_note: &str,
    packed: bool,
) -> (Vec<crate::display::Op>, usize) {
    use crate::display::{Op, TextAlign};
    let font = FontKind::Mono;
    let advance = text::measure("M", size_px, font).max(1.0);
    let line_h = size_px * 1.22;
    let pad = size_px * 0.35;
    let (bx0, _, bx1, by1) = poly_bbox_px(poly, canvas_w, canvas_h);

    let mut ops = Vec::new();
    let mut consumed = 0usize;
    // pending: remainder of a hard-wrapped source line
    let mut pending: Option<(String, bool)> = None;
    let mut y = start_y_px;

    // the bottom row is reserved for the spill marker, so it never
    // overprints the last body line
    while y + 2.0 * line_h < by1 - pad && (consumed < lines.len() || pending.is_some()) {
        y += line_h;
        // the row must be inside the polygon over the text's full height
        let top = row_interval(poly, canvas_w, canvas_h, y - size_px);
        let bot = row_interval(poly, canvas_w, canvas_h, y);
        let Some(((tx0, tx1), (bx0, bx1))) = top.zip(bot) else {
            continue;
        };
        let (x0, x1) = (tx0.max(bx0) + pad, tx1.min(bx1) - pad);
        let capacity = ((x1 - x0) / advance) as usize;
        if capacity < 6 {
            continue;
        }

        let (raw, was_wrapped) = match pending.take() {
            Some(p) => p,
            None => {
                let l = lines[consumed].replace('\t', "    ");
                consumed += 1;
                (l, false)
            }
        };
        let trimmed = raw.trim_start();
        let is_comment = trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with('*')
            || trimmed.starts_with("/*");

        // packed mode: content is one ↵-reflowed stream; wraps are not
        // marked (↵ already marks the real newlines)
        let budget = capacity.saturating_sub(if was_wrapped && !packed { 2 } else { 0 });
        let mut shown: String = raw.chars().take(budget).collect();
        let rest: String = raw.chars().skip(budget).collect();
        if !rest.is_empty() {
            pending = Some((rest, true));
        }
        if was_wrapped && !packed {
            shown = format!("↪ {shown}");
        }
        if shown.trim().is_empty() {
            continue;
        }
        ops.push(Op::Text {
            pos: (x0 / canvas_w, y / canvas_h),
            text: shown,
            size_px,
            color: if is_comment { dim } else { ink },
            font,
            align: TextAlign::Left,
            halo: None,
        });
    }

    if consumed < lines.len() || pending.is_some() {
        // the marker must stay inside the cell: fall back to a compact
        // form in narrow boxes, drop it entirely when even that spills
        // (the legend still carries the cell)
        let msize = size_px.max(10.0);
        let inner_w = (bx1 - bx0) - 2.0 * pad;
        let full = format!("⋯ +{} lines · {spill_note}", lines.len() - consumed);
        let text = if label_box(&full, msize, FontKind::Sans).0 <= inner_w {
            Some(full)
        } else {
            let short = format!("⋯+{}", lines.len() - consumed);
            (label_box(&short, msize, FontKind::Sans).0 <= inner_w).then_some(short)
        };
        let fits_below_body = by1 - pad - start_y_px >= line_h;
        if let (Some(text), true) = (text, fits_below_body) {
            ops.push(Op::Text {
                pos: ((bx0 + bx1) / 2.0 / canvas_w, (by1 - pad) / canvas_h),
                text,
                size_px: msize,
                color: dim,
                font: FontKind::Sans,
                align: TextAlign::Center,
                halo: None,
            });
        }
    }
    (ops, consumed)
}
