//! Machine-facing themes: engineered for VLM legibility. Discrete band
//! fills, high-contrast borders, redundant encoding (color + contour +
//! printed band number), hard label floors, no decoration that costs
//! attention. Themes differ in **palette only** — the encoding grammar
//! (bands, handles, hatches, swatch legend) is shared, so palettes can be
//! A/B'd fairly by the calibration harness.

use crate::display::{DisplayList, FontKind, Op, Rgba, TextAlign};
use crate::scene::Scene;
use crate::theme::{flow_text_ops, label_box, poly_bbox_px, LabelPlacer, Theme};
use ctx2img_core::graph::EdgeKind;
use ctx2img_core::hazard;

/// A machine-legibility palette. Every color must survive the calibration
/// probes; prettiness is welcome, accuracy is gating.
pub struct MachinePalette {
    pub sea: Rgba,
    pub ink: Rgba,
    pub halo: Rgba,
    pub contour: Rgba,
    pub hazard: Rgba,
    pub island: Rgba,
    /// Sequential elevation ramp, band 1..=5 (light -> dark).
    pub bands: [Rgba; 5],
    /// What band tints mix toward under inscribe text (paper color).
    pub tint_base: Rgba,
    /// Multiplier taking a band color to its border shade (<1 darkens for
    /// light papers, ≥1 keeps/brightens for dark papers).
    pub border_shade: f32,
}

/// Stark: maximum-contrast near-black on white — the production default
/// (matches what pxpipe field-validated for dense text reading).
pub const STARK: MachinePalette = MachinePalette {
    sea: Rgba::opaque(255, 255, 255),
    ink: Rgba::opaque(20, 20, 20),
    halo: Rgba::opaque(255, 255, 255),
    contour: Rgba::opaque(120, 120, 120),
    hazard: Rgba::opaque(200, 40, 40),
    island: Rgba::opaque(200, 200, 200),
    bands: [
        Rgba::opaque(0xF2, 0xF0, 0xE6),
        Rgba::opaque(0xCB, 0xE5, 0xC9),
        Rgba::opaque(0x96, 0xD1, 0xB4),
        Rgba::opaque(0x57, 0xB0, 0xA2),
        Rgba::opaque(0x2B, 0x86, 0x89),
    ],
    tint_base: Rgba::opaque(255, 255, 255),
    border_shade: 0.68,
};

/// Warm: parchment-inspired **calibration candidate** — the cute theme at
/// machine-safe contrast: flat fills, no grain, no hillshade, near-black
/// warm ink on light paper. Promote to default only if
/// `ctx2img calibrate --theme warm` matches stark's probe scores.
pub const WARM: MachinePalette = MachinePalette {
    sea: Rgba::opaque(0xF8, 0xF2, 0xE4),
    ink: Rgba::opaque(0x2A, 0x1F, 0x12),
    halo: Rgba::opaque(0xFC, 0xF8, 0xEE),
    contour: Rgba::opaque(0xA8, 0x8F, 0x6A),
    hazard: Rgba::opaque(0xB5, 0x35, 0x25),
    island: Rgba::opaque(0xDD, 0xD0, 0xB4),
    bands: [
        Rgba::opaque(0xF6, 0xEE, 0xDC),
        Rgba::opaque(0xEC, 0xD9, 0xB2),
        Rgba::opaque(0xDD, 0xBB, 0x87),
        Rgba::opaque(0xC4, 0x92, 0x59),
        Rgba::opaque(0x93, 0x5F, 0x33),
    ],
    tint_base: Rgba::opaque(0xFA, 0xF5, 0xE8),
    border_shade: 0.68,
};

/// Dark: white-on-black **calibration candidate**. Contrast ratio is
/// symmetric, but training distributions skew light-background (documents)
/// — while IDE/code screenshots skew dark. Genuinely undecided by the
/// literature ("inverted colors" was one of the variants in the 47pp font
/// swing), so: measure via `ctx2img calibrate --theme dark`, never assume.
pub const DARK: MachinePalette = MachinePalette {
    sea: Rgba::opaque(0x14, 0x14, 0x14),
    ink: Rgba::opaque(0xEC, 0xEC, 0xEC),
    halo: Rgba::opaque(0x14, 0x14, 0x14),
    contour: Rgba::opaque(0x66, 0x66, 0x66),
    hazard: Rgba::opaque(0xFF, 0x7A, 0x66),
    island: Rgba::opaque(0x3C, 0x3C, 0x3C),
    bands: [
        Rgba::opaque(0x4A, 0x48, 0x40),
        Rgba::opaque(0x3E, 0x6B, 0x4E),
        Rgba::opaque(0x2F, 0x8D, 0x77),
        Rgba::opaque(0x26, 0xB3, 0xA6),
        Rgba::opaque(0x2E, 0xD9, 0xDE),
    ],
    tint_base: Rgba::opaque(0x19, 0x19, 0x19),
    border_shade: 1.0,
};

/// Minimum label height in raster px; below this, drop the label (the
/// legend roster still carries it) rather than render unreadable text.
const MIN_LABEL_PX: f32 = 12.0;

impl MachinePalette {
    fn band(&self, band: u8) -> Rgba {
        self.bands[(band.clamp(1, 5) - 1) as usize]
    }

    /// Border ramp for text cells: the band colors darkened enough that
    /// even band 1 is visible against paper.
    fn band_border(&self, band: u8) -> Rgba {
        self.band(band).shade(self.border_shade)
    }

    fn terrain(&self, scene: &Scene) -> DisplayList {
        let mut dl = DisplayList::default();
        // continent base under the cells, so simplification gaps between
        // cell polygons read as land, not sea
        for part in &scene.coast {
            dl.push(Op::Fill {
                poly: part.clone(),
                color: self.band(1),
            });
        }
        let inscribe = scene.cells.iter().any(|c| c.text.is_some());
        // land fills, low bands first so summits paint over shared borders.
        // Text cells stay PURE PAPER — maximum glyph contrast; their band is
        // carried by the border (color + weight) and the ▲n in the header.
        let mut order: Vec<usize> = (0..scene.cells.len()).collect();
        order.sort_by_key(|&i| scene.cells[i].band);
        for &i in &order {
            let c = &scene.cells[i];
            if c.poly.len() >= 3 {
                let color = if c.text.is_some() {
                    self.tint_base
                } else {
                    self.band(c.band)
                };
                dl.push(Op::Fill {
                    poly: c.poly.clone(),
                    color,
                });
            }
        }
        // contours (between-band isolines) — noise under body text, skip in inscribe
        for (_, lines) in if inscribe {
            &[][..]
        } else {
            &scene.contours[..]
        } {
            for line in lines {
                dl.push(Op::Stroke {
                    path: line.clone(),
                    color: self.contour,
                    width_px: 0.9,
                    closed: false,
                    dash: None,
                });
            }
        }
        // region borders + hazard hatch. Text cells: the border IS the
        // elevation channel — band-colored and band-weighted (1.6px valley
        // → 4.6px summit), doubly redundant with the header's ▲n. Summit
        // borders stroke last so they win shared edges.
        for &i in &order {
            let c = &scene.cells[i];
            if c.poly.len() < 3 {
                continue;
            }
            let (bcolor, bwidth) = if c.text.is_some() {
                (self.band_border(c.band), 1.0 + c.band as f32 * 0.72)
            } else {
                (self.ink, 1.8)
            };
            dl.push(Op::Stroke {
                path: c.poly.clone(),
                color: bcolor,
                width_px: bwidth,
                closed: true,
                dash: None,
            });
            if c.hazards != 0 && c.text.is_some() {
                // hazard: thin dashed red inner line, distinct from the
                // band border (header also carries ⚠tags)
                dl.push(Op::Stroke {
                    path: inset_poly(&c.poly, 3.0 / scene.width as f32),
                    color: self.hazard,
                    width_px: 1.2,
                    closed: true,
                    dash: Some((5.0, 3.0)),
                });
            }
            if c.hazards != 0 && c.text.is_none() {
                dl.push(Op::Hatch {
                    poly: c.poly.clone(),
                    color: Rgba(self.hazard.0, self.hazard.1, self.hazard.2, 70),
                    spacing_px: 13.0,
                    width_px: 1.0,
                });
                dl.push(Op::Stroke {
                    path: c.poly.clone(),
                    color: self.hazard,
                    width_px: 1.4,
                    closed: true,
                    dash: Some((6.0, 3.0)),
                });
            }
        }
        // coastline
        for part in &scene.coast {
            dl.push(Op::Stroke {
                path: part.clone(),
                color: self.ink,
                width_px: 2.4,
                closed: true,
                dash: None,
            });
        }
        dl
    }

    fn overlay(&self, scene: &Scene) -> DisplayList {
        let mut dl = DisplayList::default();
        let (w, h) = (scene.width as f32, scene.height as f32);
        let mut placer = LabelPlacer::new(w, h);

        // dependency curves under labels
        for e in &scene.edges {
            let mid = curve_control(e.a, e.b);
            let (color, dash) = match e.kind {
                EdgeKind::Import => (Rgba::opaque(90, 90, 90), Some((7.0, 4.0))),
                EdgeKind::Reference => (Rgba::opaque(50, 50, 50), None),
                EdgeKind::CoChange => (Rgba::opaque(140, 140, 140), Some((2.0, 4.0))),
            };
            dl.push(Op::Curve {
                a: e.a,
                b: e.b,
                c: mid,
                color,
                width_px: (1.0 + e.weight.sqrt() * 0.4).min(3.0),
                dash,
                arrow: true,
            });
        }

        // region labels, summit-first so the most relevant always win space
        let mut order: Vec<usize> = (0..scene.cells.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse((scene.cells[i].band, scene.cells[i].loc)));
        for &i in &order {
            let c = &scene.cells[i];
            if c.poly.len() < 3 {
                continue;
            }
            // inscribe cell: header pinned to the cell top-left, the text
            // below it; ellipsized to the box so it never crosses neighbors
            if let Some(lines) = &c.text {
                let hsize = (scene.text_px * 1.2).max(MIN_LABEL_PX);
                let (bx0, by0, bx1, _) = poly_bbox_px(&c.poly, w, h);
                let hy = by0 + hsize * 1.5;
                let mut header = format!("{} {} ▲{}", c.handle, c.name, c.band);
                let tags = hazard::tags(c.hazards);
                if !tags.is_empty() {
                    header.push_str(&format!(" ⚠{}", tags.join(",")));
                }
                let max_w = (bx1 - bx0) - hsize * 1.2;
                while header.chars().count() > 4
                    && label_box(&header, hsize, FontKind::SansBold).0 > max_w
                {
                    header = header.chars().take(header.chars().count() - 2).collect();
                    header.push('…');
                }
                dl.push(Op::Text {
                    pos: ((bx0 + hsize * 0.6) / w, hy / h),
                    text: header,
                    size_px: hsize,
                    color: self.ink,
                    font: FontKind::SansBold,
                    align: TextAlign::Left,
                    halo: Some(self.halo),
                });
                let spill_note = if c.handle.starts_with('F') {
                    format!("ctx2img read {}", c.handle)
                } else {
                    "truncated — see source".to_string()
                };
                let (ops, _) = flow_text_ops(
                    &c.poly,
                    w,
                    h,
                    lines,
                    scene.text_px,
                    hy + hsize * 0.5,
                    self.ink,
                    Rgba::opaque(128, 128, 128),
                    &spill_note,
                    scene.boxes,
                );
                for op in ops {
                    dl.push(op);
                }
                continue;
            }
            let size = (h * 0.017).clamp(MIN_LABEL_PX + 3.0, 24.0);
            let label = format!("{} {} ▲{}", c.handle, c.name, c.band);
            let (bw, bh) = label_box(&label, size, FontKind::SansBold);
            let (cx, cy) = (c.anchor.0 * w, c.anchor.1 * h);
            if placer.try_place(cx, cy, bw, bh) {
                dl.push(Op::Text {
                    pos: (c.anchor.0, c.anchor.1),
                    text: label,
                    size_px: size,
                    color: self.ink,
                    font: FontKind::SansBold,
                    align: TextAlign::Center,
                    halo: Some(self.halo),
                });
                let tags = hazard::tags(c.hazards);
                if !tags.is_empty() {
                    let tag = format!("⚠{}", tags.join(","));
                    let tsize = (size * 0.72).max(MIN_LABEL_PX);
                    let (tw, th) = label_box(&tag, tsize, FontKind::Sans);
                    if placer.try_place(cx, cy + bh, tw, th) {
                        dl.push(Op::Text {
                            pos: (c.anchor.0, c.anchor.1 + bh / h),
                            text: tag,
                            size_px: tsize,
                            color: self.hazard,
                            font: FontKind::Sans,
                            align: TextAlign::Center,
                            halo: Some(self.halo),
                        });
                    }
                }
            }
        }

        // cities: dots always, labels only if they fit legibly
        for c in &scene.cells {
            for city in &c.cities {
                dl.push(Op::Circle {
                    center: city.pos,
                    r_px: city.r_px,
                    fill: Some(self.halo),
                    stroke: Some((self.ink, 1.4)),
                });
                if city.label.is_empty() {
                    continue;
                }
                let size = (h * 0.0125).max(MIN_LABEL_PX);
                let (bw, bh) = label_box(&city.label, size, FontKind::Sans);
                let (cx, cy) = (city.pos.0 * w, city.pos.1 * h + bh * 0.75);
                if placer.try_place(cx, cy, bw, bh) {
                    dl.push(Op::Text {
                        pos: (city.pos.0, city.pos.1 + (bh * 0.75 + city.r_px) / h),
                        text: city.label.clone(),
                        size_px: size,
                        color: self.ink,
                        font: FontKind::Sans,
                        align: TextAlign::Center,
                        halo: Some(self.halo),
                    });
                }
            }
        }

        // islands (external deps)
        for isl in &scene.islands {
            dl.push(Op::Circle {
                center: isl.pos,
                r_px: isl.r * w,
                fill: Some(self.island),
                stroke: Some((self.ink, 1.0)),
            });
            let size = (h * 0.011).max(MIN_LABEL_PX);
            let (bw, bh) = label_box(&isl.label, size, FontKind::Sans);
            let (cx, cy) = (isl.pos.0 * w, isl.pos.1 * h + isl.r * w + bh * 0.7);
            if placer.try_place(cx, cy, bw, bh) {
                dl.push(Op::Text {
                    pos: (isl.pos.0, isl.pos.1 + (isl.r * w + bh * 0.7) / h),
                    text: isl.label.clone(),
                    size_px: size,
                    color: Rgba::opaque(80, 80, 80),
                    font: FontKind::Sans,
                    align: TextAlign::Center,
                    halo: Some(self.halo),
                });
            }
        }

        // No in-image title/query/swatch: that information always rides as
        // adjacent text (legend/roster), and pixels are for content. Human
        // themes keep their decorations.
        dl
    }
}

/// Stark machine theme — the default everywhere a model is the reader.
pub struct VlmTheme;
/// Warm machine theme — same grammar, parchment-flavored palette
/// (calibration candidate; see `ctx2img calibrate --theme warm`).
pub struct WarmTheme;

impl Theme for VlmTheme {
    fn background(&self) -> Rgba {
        STARK.sea
    }
    fn terrain(&self, scene: &Scene) -> DisplayList {
        STARK.terrain(scene)
    }
    fn overlay(&self, scene: &Scene) -> DisplayList {
        STARK.overlay(scene)
    }
}

/// Dark machine theme (calibration candidate).
pub struct DarkTheme;

impl Theme for DarkTheme {
    fn background(&self) -> Rgba {
        DARK.sea
    }
    fn terrain(&self, scene: &Scene) -> DisplayList {
        DARK.terrain(scene)
    }
    fn overlay(&self, scene: &Scene) -> DisplayList {
        DARK.overlay(scene)
    }
}

impl Theme for WarmTheme {
    fn background(&self) -> Rgba {
        WARM.sea
    }
    fn terrain(&self, scene: &Scene) -> DisplayList {
        WARM.terrain(scene)
    }
    fn overlay(&self, scene: &Scene) -> DisplayList {
        WARM.overlay(scene)
    }
}

/// Shrink a polygon toward its centroid by roughly `d` (normalized units).
fn inset_poly(poly: &[(f32, f32)], d: f32) -> Vec<(f32, f32)> {
    let n = poly.len().max(1) as f32;
    let (cx, cy) = poly
        .iter()
        .fold((0.0, 0.0), |(ax, ay), &(x, y)| (ax + x / n, ay + y / n));
    poly.iter()
        .map(|&(x, y)| {
            let (dx, dy) = (x - cx, y - cy);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let f = (1.0 - d / len).max(0.0);
            (cx + dx * f, cy + dy * f)
        })
        .collect()
}

/// Control point: midpoint pushed perpendicular for a gentle arc.
pub fn curve_control(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    (mx - dy * 0.18, my + dx * 0.18)
}
