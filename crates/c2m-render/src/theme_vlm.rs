//! VLM theme: engineered for machine legibility. Discrete band fills,
//! high-contrast borders, redundant encoding (color + contour + printed
//! band number), hard label floors, no decoration that costs attention.

use crate::display::{DisplayList, FontKind, Op, Rgba, TextAlign};
use crate::scene::Scene;
use crate::theme::{flow_text_ops, label_box, poly_bbox_px, LabelPlacer, Theme};
use c2m_core::graph::EdgeKind;
use c2m_core::hazard;

pub struct VlmTheme;

const SEA: Rgba = Rgba::opaque(255, 255, 255);
const INK: Rgba = Rgba::opaque(20, 20, 20);
const HALO: Rgba = Rgba::opaque(255, 255, 255);
const CONTOUR: Rgba = Rgba::opaque(120, 120, 120);
const HAZARD: Rgba = Rgba::opaque(200, 40, 40);
const ISLAND: Rgba = Rgba::opaque(200, 200, 200);

/// Sequential, colorblind-safe band ramp (light sand → deep teal).
pub fn band_color(band: u8) -> Rgba {
    match band {
        1 => Rgba::opaque(0xF2, 0xF0, 0xE6),
        2 => Rgba::opaque(0xCB, 0xE5, 0xC9),
        3 => Rgba::opaque(0x96, 0xD1, 0xB4),
        4 => Rgba::opaque(0x57, 0xB0, 0xA2),
        _ => Rgba::opaque(0x2B, 0x86, 0x89),
    }
}

/// Inscribe mode: band tint light enough that dark mono text stays readable —
/// the elevation wash sits *under* the text.
pub fn band_tint(band: u8) -> Rgba {
    let c = band_color(band);
    let mix = |v: u8| (v as f32 * 0.35 + 255.0 * 0.65) as u8;
    Rgba::opaque(mix(c.0), mix(c.1), mix(c.2))
}

/// Minimum label height in raster px; below this, drop the label (the
/// legend roster still carries it) rather than render unreadable text.
const MIN_LABEL_PX: f32 = 12.0;

impl Theme for VlmTheme {
    fn background(&self) -> Rgba {
        SEA
    }

    fn terrain(&self, scene: &Scene) -> DisplayList {
        let mut dl = DisplayList::default();
        // continent base under the cells, so simplification gaps between
        // cell polygons read as land, not sea
        for part in &scene.coast {
            dl.push(Op::Fill {
                poly: part.clone(),
                color: band_color(1),
            });
        }
        let inscribe = scene.cells.iter().any(|c| c.text.is_some());
        // land fills, low bands first so summits paint over shared borders;
        // inscribe cells get the light tint so text stays dark-on-light
        let mut order: Vec<usize> = (0..scene.cells.len()).collect();
        order.sort_by_key(|&i| scene.cells[i].band);
        for &i in &order {
            let c = &scene.cells[i];
            if c.poly.len() >= 3 {
                let color = if c.text.is_some() {
                    band_tint(c.band)
                } else {
                    band_color(c.band)
                };
                dl.push(Op::Fill {
                    poly: c.poly.clone(),
                    color,
                });
            }
        }
        // contours (between-band isolines) — noise under body text, skip in inscribe
        for (_, lines) in if inscribe { &[][..] } else { &scene.contours[..] } {
            for line in lines {
                dl.push(Op::Stroke {
                    path: line.clone(),
                    color: CONTOUR,
                    width_px: 0.9,
                    closed: false,
                    dash: None,
                });
            }
        }
        // region borders + hazard hatch
        for c in &scene.cells {
            if c.poly.len() < 3 {
                continue;
            }
            dl.push(Op::Stroke {
                path: c.poly.clone(),
                color: INK,
                width_px: 1.8,
                closed: true,
                dash: None,
            });
            if c.hazards != 0 && c.text.is_none() {
                dl.push(Op::Hatch {
                    poly: c.poly.clone(),
                    color: Rgba(HAZARD.0, HAZARD.1, HAZARD.2, 70),
                    spacing_px: 13.0,
                    width_px: 1.0,
                });
                dl.push(Op::Stroke {
                    path: c.poly.clone(),
                    color: HAZARD,
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
                color: INK,
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
            // inscribe cell: header pinned to the cell top, the text below it
            if let Some(lines) = &c.text {
                let hsize = (scene.text_px * 1.2).max(MIN_LABEL_PX);
                let (bx0, by0, bx1, _) = poly_bbox_px(&c.poly, w, h);
                let cxm = (bx0 + bx1) / 2.0;
                let hy = by0 + hsize * 1.7;
                let mut header = format!("{} {} ▲{}", c.handle, c.name, c.band);
                let tags = hazard::tags(c.hazards);
                if !tags.is_empty() {
                    header.push_str(&format!(" ⚠{}", tags.join(",")));
                }
                dl.push(Op::Text {
                    pos: (cxm / w, hy / h),
                    text: header,
                    size_px: hsize,
                    color: INK,
                    font: FontKind::SansBold,
                    align: TextAlign::Center,
                    halo: Some(HALO),
                });
                let (ops, _) = flow_text_ops(
                    &c.poly,
                    w,
                    h,
                    lines,
                    scene.text_px,
                    hy + hsize * 0.5,
                    INK,
                    Rgba::opaque(128, 128, 128),
                    &format!("c2m read {}", c.handle),
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
                    color: INK,
                    font: FontKind::SansBold,
                    align: TextAlign::Center,
                    halo: Some(HALO),
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
                            color: HAZARD,
                            font: FontKind::Sans,
                            align: TextAlign::Center,
                            halo: Some(HALO),
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
                    fill: Some(HALO),
                    stroke: Some((INK, 1.4)),
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
                        color: INK,
                        font: FontKind::Sans,
                        align: TextAlign::Center,
                        halo: Some(HALO),
                    });
                }
            }
        }

        // islands (external deps)
        for isl in &scene.islands {
            dl.push(Op::Circle {
                center: isl.pos,
                r_px: isl.r * w,
                fill: Some(ISLAND),
                stroke: Some((INK, 1.0)),
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
                    halo: Some(HALO),
                });
            }
        }

        // header: title + query (mono, top-left)
        let hsize = (h * 0.014).max(MIN_LABEL_PX);
        if !scene.title.is_empty() {
            dl.push(Op::Text {
                pos: (0.012, 0.028),
                text: scene.title.clone(),
                size_px: hsize * 1.15,
                color: INK,
                font: FontKind::Mono,
                align: TextAlign::Left,
                halo: Some(HALO),
            });
        }
        if !scene.subtitle.is_empty() {
            dl.push(Op::Text {
                pos: (0.012, 0.028 + hsize * 1.6 / h),
                text: format!("query: {}", scene.subtitle),
                size_px: hsize,
                color: Rgba::opaque(70, 70, 70),
                font: FontKind::Mono,
                align: TextAlign::Left,
                halo: Some(HALO),
            });
        }

        // band swatch bar (bottom-right): redundant legend inside the image
        let sw = 0.028f32;
        let sh = 0.018f32;
        for band in 1..=5u8 {
            let x = 1.0 - 0.02 - sw * (5 - band as i32 + 1) as f32;
            let y = 1.0 - 0.03;
            let poly = vec![(x, y), (x + sw, y), (x + sw, y + sh), (x, y + sh)];
            dl.push(Op::Fill {
                poly: poly.clone(),
                color: band_color(band),
            });
            dl.push(Op::Stroke {
                path: poly,
                color: INK,
                width_px: 1.0,
                closed: true,
                dash: None,
            });
            dl.push(Op::Text {
                pos: (x + sw / 2.0, y + sh * 0.78),
                text: format!("{band}"),
                size_px: (h * sh * 0.62).max(9.0),
                color: INK,
                font: FontKind::Sans,
                align: TextAlign::Center,
                halo: None,
            });
        }
        dl.push(Op::Text {
            pos: (1.0 - 0.02 - sw * 5.0, 1.0 - 0.035),
            text: "relevance ▲".to_string(),
            size_px: (h * 0.011).max(9.0),
            color: INK,
            font: FontKind::Sans,
            align: TextAlign::Left,
            halo: Some(HALO),
        });
        dl
    }
}

/// Control point: midpoint pushed perpendicular for a gentle arc.
pub fn curve_control(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    (mx - dy * 0.18, my + dx * 0.18)
}
