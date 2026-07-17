//! Parchment theme: the human-facing fantasy-cartography render. Language
//! biomes, hillshaded terrain, sepia contours, dependency roads, dep
//! islands off the coast, compass rose. Same scene as the VLM theme —
//! only the stylesheet changes.

use crate::display::{DisplayList, FontKind, Op, Rgba, TextAlign};
use crate::raster::Raster;
use crate::scene::Scene;
use crate::theme::{label_box, sample_elevation, LabelPlacer, Theme};
use ctx2img_core::graph::EdgeKind;
use ctx2img_core::types::Lang;
use ctx2img_layout::noise;

pub struct ParchmentTheme;

const PAPER: Rgba = Rgba::opaque(0xE9, 0xDE, 0xC4);
const SEA: Rgba = Rgba::opaque(0xDD, 0xD2, 0xB2);
const INK: Rgba = Rgba::opaque(0x4A, 0x38, 0x22);
const SEPIA: Rgba = Rgba::opaque(0x8A, 0x6F, 0x47);
const ROAD: Rgba = Rgba::opaque(0x7A, 0x5A, 0x38);

fn biome(lang: Lang) -> Rgba {
    match lang {
        Lang::Rust => Rgba::opaque(0xC6, 0x8A, 0x62),
        Lang::Python => Rgba::opaque(0xA9, 0xAC, 0x62),
        Lang::JavaScript => Rgba::opaque(0xCE, 0xA9, 0x52),
        Lang::TypeScript => Rgba::opaque(0xB8, 0xA2, 0x54),
        Lang::Go => Rgba::opaque(0x83, 0xAC, 0x9E),
        Lang::Java => Rgba::opaque(0xB0, 0x84, 0x58),
        Lang::Markdown => Rgba::opaque(0xC9, 0xBE, 0x9A),
        Lang::Config => Rgba::opaque(0xB5, 0xAD, 0x92),
        Lang::Shell => Rgba::opaque(0xA8, 0x9B, 0x7C),
        Lang::Other => Rgba::opaque(0xBD, 0xA9, 0x86),
    }
}

impl Theme for ParchmentTheme {
    fn background(&self) -> Rgba {
        SEA
    }

    fn terrain(&self, scene: &Scene) -> DisplayList {
        let mut dl = DisplayList::default();
        // continent base so cell gaps read as land
        for part in &scene.coast {
            dl.push(Op::Fill {
                poly: part.clone(),
                color: PAPER.shade(0.97),
            });
        }
        // land: biome fill, hue-jittered per province (monoculture repos
        // would otherwise render as one flat color), lifted by elevation
        for c in &scene.cells {
            if c.poly.len() < 3 {
                continue;
            }
            let base = biome(c.lang);
            let jitter = 0.90 + 0.18 * hash01(&c.name);
            let lift = 1.0 + 0.06 * (c.band as f32 - 1.0);
            dl.push(Op::Fill {
                poly: c.poly.clone(),
                color: base.shade(jitter * lift),
            });
        }
        // sepia contours
        for (_, lines) in &scene.contours {
            for line in lines {
                dl.push(Op::Stroke {
                    path: line.clone(),
                    color: Rgba(SEPIA.0, SEPIA.1, SEPIA.2, 150),
                    width_px: 0.8,
                    closed: false,
                    dash: None,
                });
            }
        }
        // inner borders: faint dashed boundaries between provinces
        for c in &scene.cells {
            if c.poly.len() >= 3 {
                dl.push(Op::Stroke {
                    path: c.poly.clone(),
                    color: Rgba(INK.0, INK.1, INK.2, 90),
                    width_px: 0.9,
                    closed: true,
                    dash: Some((5.0, 4.0)),
                });
            }
        }
        // coastline: classic double stroke
        for part in &scene.coast {
            dl.push(Op::Stroke {
                path: part.clone(),
                color: INK,
                width_px: 2.2,
                closed: true,
                dash: None,
            });
            dl.push(Op::Stroke {
                path: part.iter().map(|&(x, y)| (x, y)).collect(),
                color: Rgba(INK.0, INK.1, INK.2, 70),
                width_px: 5.5,
                closed: true,
                dash: None,
            });
        }
        dl
    }

    fn post_raster(&self, scene: &Scene, raster: &mut Raster) {
        // hillshade (light from NW) + paper grain, one multiplicative pass
        let d = 1.5 / scene.field_w.max(1) as f32;
        raster.multiply(|x, y| {
            let e_w = sample_elevation(scene, x - d, y);
            let e_e = sample_elevation(scene, x + d, y);
            let e_n = sample_elevation(scene, x, y - d);
            let e_s = sample_elevation(scene, x, y + d);
            let gx = e_e - e_w;
            let gy = e_s - e_n;
            let shade = 1.0 - (gx * 0.9 + gy * 1.1);
            let grain = 1.0 + 0.06 * (noise::fbm(x * 3.0, y * 3.0, 0xBEEF) - 0.5);
            (shade * grain).clamp(0.72, 1.25)
        });
    }

    fn overlay(&self, scene: &Scene) -> DisplayList {
        let mut dl = DisplayList::default();
        let (w, h) = (scene.width as f32, scene.height as f32);
        let mut placer = LabelPlacer::new(w, h);

        // roads
        for e in &scene.edges {
            let mid = crate::theme_vlm::curve_control(e.a, e.b);
            let dash = match e.kind {
                EdgeKind::Import => Some((6.0, 4.0)),
                EdgeKind::Reference => None,
                EdgeKind::CoChange => Some((2.0, 4.0)),
            };
            dl.push(Op::Curve {
                a: e.a,
                b: e.b,
                c: mid,
                color: Rgba(ROAD.0, ROAD.1, ROAD.2, 170),
                width_px: (0.9 + e.weight.sqrt() * 0.35).min(2.6),
                dash,
                arrow: false,
            });
        }

        // toponyms (serif, no handles — this map is for humans)
        let mut order: Vec<usize> = (0..scene.cells.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse((scene.cells[i].band, scene.cells[i].loc)));
        for &i in &order {
            let c = &scene.cells[i];
            if c.poly.len() < 3 {
                continue;
            }
            let size = (h * 0.0165).clamp(11.0, 24.0);
            let (bw, bh) = label_box(&c.name, size, FontKind::SerifBold);
            let (cx, cy) = (c.anchor.0 * w, c.anchor.1 * h);
            if placer.try_place(cx, cy, bw, bh) {
                dl.push(Op::Text {
                    pos: (c.anchor.0, c.anchor.1),
                    text: c.name.clone(),
                    size_px: size,
                    color: INK,
                    font: FontKind::SerifBold,
                    align: TextAlign::Center,
                    halo: Some(Rgba(PAPER.0, PAPER.1, PAPER.2, 200)),
                });
            }
            // summit glyph on the highest band
            if c.band == 5 {
                dl.push(Op::Text {
                    pos: (c.anchor.0, c.anchor.1 - size * 1.3 / h),
                    text: "▲".into(),
                    size_px: size * 0.9,
                    color: SEPIA,
                    font: FontKind::Sans,
                    align: TextAlign::Center,
                    halo: None,
                });
            }
        }

        // cities
        for c in &scene.cells {
            for city in &c.cities {
                dl.push(Op::Circle {
                    center: city.pos,
                    r_px: city.r_px * 0.9,
                    fill: Some(INK),
                    stroke: None,
                });
                if city.label.is_empty() {
                    continue;
                }
                // drop the handle prefix for the human map ("F103 auth.rs" -> "auth.rs")
                let name = city
                    .label
                    .split_once(' ')
                    .map(|(_, n)| n)
                    .unwrap_or(&city.label);
                let size = (h * 0.011).clamp(9.0, 15.0);
                let (bw, bh) = label_box(name, size, FontKind::Serif);
                let (cx, cy) = (city.pos.0 * w, city.pos.1 * h + bh * 0.8);
                if placer.try_place(cx, cy, bw, bh) {
                    dl.push(Op::Text {
                        pos: (city.pos.0, city.pos.1 + (bh * 0.8 + city.r_px) / h),
                        text: name.to_string(),
                        size_px: size,
                        color: INK,
                        font: FontKind::Serif,
                        align: TextAlign::Center,
                        halo: Some(Rgba(PAPER.0, PAPER.1, PAPER.2, 190)),
                    });
                }
            }
        }

        // islands with names
        for isl in &scene.islands {
            dl.push(Op::Circle {
                center: isl.pos,
                r_px: isl.r * w,
                fill: Some(biome(Lang::Other).shade(1.05)),
                stroke: Some((INK, 1.2)),
            });
            let name = isl
                .label
                .split_once(' ')
                .map(|(_, n)| n)
                .unwrap_or(&isl.label);
            let size = (h * 0.0105).clamp(9.0, 14.0);
            let (bw, bh) = label_box(name, size, FontKind::Serif);
            let (cx, cy) = (isl.pos.0 * w, isl.pos.1 * h + isl.r * w + bh * 0.7);
            if placer.try_place(cx, cy, bw, bh) {
                dl.push(Op::Text {
                    pos: (isl.pos.0, isl.pos.1 + (isl.r * w + bh * 0.7) / h),
                    text: name.to_string(),
                    size_px: size,
                    color: Rgba(INK.0, INK.1, INK.2, 210),
                    font: FontKind::Serif,
                    align: TextAlign::Center,
                    halo: None,
                });
            }
        }

        // title cartouche
        if !scene.title.is_empty() {
            let size = (h * 0.026).clamp(16.0, 34.0);
            dl.push(Op::Text {
                pos: (0.5, 0.052),
                text: scene.title.clone(),
                size_px: size,
                color: INK,
                font: FontKind::SerifBold,
                align: TextAlign::Center,
                halo: Some(Rgba(PAPER.0, PAPER.1, PAPER.2, 220)),
            });
        }

        // caption (bottom-left) + compass rose (top-right)
        let files: usize = scene.cells.iter().map(|c| c.cities.len().max(1)).sum();
        let _ = files;
        dl.push(Op::Text {
            pos: (0.02, 0.975),
            text: format!(
                "{} regions · {}",
                scene.cells.len(),
                human_loc(scene.total_loc)
            ),
            size_px: (h * 0.012).max(10.0),
            color: Rgba(INK.0, INK.1, INK.2, 200),
            font: FontKind::Serif,
            align: TextAlign::Left,
            halo: None,
        });
        compass(&mut dl, (0.935, 0.075), 0.032, h);
        dl
    }
}

/// Deterministic per-name value in [0,1] for tint jitter.
fn hash01(name: &str) -> f32 {
    (noise::hash64(name.as_bytes(), 0x71A7) & 0xFFFF) as f32 / 65535.0
}

fn human_loc(loc: u64) -> String {
    if loc >= 1000 {
        format!("{:.1}k lines charted", loc as f64 / 1000.0)
    } else {
        format!("{loc} lines charted")
    }
}

fn compass(dl: &mut DisplayList, center: (f32, f32), r: f32, canvas_h: f32) {
    let (cx, cy) = center;
    // 8-point star: long N/E/S/W spikes, short diagonals
    for (i, &(len, width)) in [
        (1.0f32, 0.22f32),
        (0.55, 0.16),
        (1.0, 0.22),
        (0.55, 0.16),
        (1.0, 0.22),
        (0.55, 0.16),
        (1.0, 0.22),
        (0.55, 0.16),
    ]
    .iter()
    .enumerate()
    {
        let ang = i as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
        let tip = (cx + ang.cos() * r * len, cy + ang.sin() * r * len);
        let l_ang = ang + std::f32::consts::FRAC_PI_2;
        let base_a = (cx + l_ang.cos() * r * width, cy + l_ang.sin() * r * width);
        let base_b = (cx - l_ang.cos() * r * width, cy - l_ang.sin() * r * width);
        dl.push(Op::Fill {
            poly: vec![tip, base_a, base_b],
            color: if len > 0.9 { INK } else { SEPIA },
        });
    }
    dl.push(Op::Circle {
        center,
        r_px: 2.2,
        fill: Some(PAPER),
        stroke: Some((INK, 1.0)),
    });
    dl.push(Op::Text {
        pos: (cx, cy - r * 1.45),
        text: "N".into(),
        size_px: (canvas_h * 0.014).max(10.0),
        color: INK,
        font: FontKind::SerifBold,
        align: TextAlign::Center,
        halo: None,
    });
}
