//! ctx2img-render — scene graph, themes, and PNG/SVG backends.

pub mod display;
pub mod paint;
pub mod raster;
pub mod scene;
pub mod svg;
pub mod text;
pub mod theme;
pub mod theme_parchment;
pub mod theme_vlm;

use anyhow::Result;
pub use scene::{Scene, SceneConfig};
pub use theme::Theme;
pub use theme_parchment::ParchmentTheme;
pub use theme_vlm::{DarkTheme, VlmTheme, WarmTheme};

/// Render a scene to PNG bytes with the given theme.
pub fn render_png(scene: &Scene, theme: &dyn Theme) -> Result<Vec<u8>> {
    let mut r = raster::Raster::new(scene.width, scene.height, theme.background())?;
    r.execute(&theme.terrain(scene));
    theme.post_raster(scene, &mut r);
    r.execute(&theme.overlay(scene));
    r.png()
}

/// Render a scene to a standalone SVG string with the given theme.
pub fn render_svg(scene: &Scene, theme: &dyn Theme) -> String {
    let mut dl = theme.terrain(scene);
    dl.extend(theme.overlay(scene));
    svg::render(&dl, scene.width, scene.height, theme.background())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx2img_index::Workspace;
    use ctx2img_layout::SavedSites;

    fn demo_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        for (path, content) in [
            ("src/auth/session.rs", "use crate::db::pool;\npub struct Session { pub expires: u64 }\npub fn session_expiry(s: &Session) -> u64 { s.expires }\n"),
            ("src/auth/jwt.rs", "pub fn sign(claims: &str) -> String { claims.to_string() }\n"),
            ("src/db/pool.rs", "pub struct Pool;\npub fn connect() -> Pool { Pool }\n"),
            ("src/main.rs", "use crate::auth::session::Session;\nuse serde::Deserialize;\nfn main() {}\n"),
            ("docs/guide.md", "# Guide\nSessions expire after an hour.\n"),
        ] {
            let full = p.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        dir
    }

    #[test]
    fn scenes_render_deterministically_in_both_themes() {
        let dir = demo_repo();
        let ws = Workspace::open(dir.path()).unwrap();
        let built = ws.build("session expiry", 1_700_000_000, false).unwrap();
        let cfg = SceneConfig {
            width: 560,
            height: 560,
            title: "demo".into(),
            ..Default::default()
        };

        let s1 = scene::build_l1(&built, &mut SavedSites::default(), &cfg);
        let s2 = scene::build_l1(&built, &mut SavedSites::default(), &cfg);
        assert_eq!(s1.cells.len(), s2.cells.len());
        assert!(!s1.cells.is_empty());

        let png_a = render_png(&s1, &VlmTheme).unwrap();
        let png_b = render_png(&s2, &VlmTheme).unwrap();
        assert_eq!(png_a, png_b, "byte-identical renders");
        assert!(png_a.len() > 2000);

        let png_h = render_png(&s1, &ParchmentTheme).unwrap();
        assert!(png_h.len() > 2000);
        assert_ne!(png_a, png_h);

        let svg = render_svg(&s1, &VlmTheme);
        assert!(svg.contains("<svg") && svg.contains("</svg>"));
        assert!(svg.contains("▲"), "band markers in labels");
    }

    #[test]
    fn l2_zoom_scene_builds_with_symbol_cities() {
        let dir = demo_repo();
        let ws = Workspace::open(dir.path()).unwrap();
        let built = ws.build("session expiry", 1_700_000_000, false).unwrap();
        let auth = built
            .analysis
            .tree
            .regions
            .iter()
            .position(|r| r.path.contains("auth"))
            .expect("auth region");
        let mut registry = ctx2img_index::HandleRegistry::default();
        let cfg = SceneConfig {
            width: 560,
            height: 560,
            ..Default::default()
        };
        let s = scene::build_l2(
            &built,
            auth,
            &mut registry,
            &mut SavedSites::default(),
            &cfg,
            None,
        );
        assert!(s.cells.len() >= 2, "session.rs and jwt.rs cells");
        let has_symbol_city = s
            .cells
            .iter()
            .flat_map(|c| &c.cities)
            .any(|c| c.label.starts_with('S'));
        assert!(has_symbol_city, "symbols get S handles at L2");
        let png = render_png(&s, &VlmTheme).unwrap();
        assert!(png.len() > 2000);
    }

    /// Inscribe mode (v0.2): cells carry their actual source text.
    #[test]
    fn inscribe_tile_typesets_source_in_territory() {
        let dir = demo_repo();
        let ws = Workspace::open(dir.path()).unwrap();
        let built = ws.build("session expiry", 1_700_000_000, false).unwrap();
        let auth = built
            .analysis
            .tree
            .regions
            .iter()
            .position(|r| r.path.contains("auth"))
            .expect("auth region");
        let mut registry = ctx2img_index::HandleRegistry::default();
        let cfg = SceneConfig {
            width: 900,
            height: 900,
            text_px: 10.0,
            ..Default::default()
        };
        let root = dir.path().to_path_buf();
        let loader = move |p: &str| std::fs::read_to_string(root.join(p)).ok();
        let s = scene::build_l2(
            &built,
            auth,
            &mut registry,
            &mut SavedSites::default(),
            &cfg,
            Some(&loader),
        );
        assert!(s.cells.iter().any(|c| c.text.is_some()), "cells carry text");
        assert!(
            s.cells.iter().all(|c| c.cities.is_empty()),
            "text replaces cities"
        );
        // the theme must emit the actual source as text ops; packed wrapping
        // may split an identifier across rows, so search the joined stream
        let dl = VlmTheme.overlay(&s);
        let body: String = dl
            .ops
            .iter()
            .filter_map(|op| match op {
                crate::display::Op::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            body.contains("session_expiry"),
            "source text typeset into the tile"
        );
        let png_a = render_png(&s, &VlmTheme).unwrap();
        let png_b = render_png(&s, &VlmTheme).unwrap();
        assert_eq!(png_a, png_b, "inscribe renders deterministically");
    }

    /// Regression (h5i MANUAL shape): a page crowded with tiny sections
    /// beside a giant one must never paint a cell's header, body, or spill
    /// marker outside that cell's rectangle.
    #[test]
    fn doc_boxes_never_paint_text_outside_their_cell() {
        use crate::display::{Op, TextAlign};
        let mut sections = vec![scene::DocSection {
            title: "giant aggregate tail".into(),
            text: "lorem ipsum dolor sit amet consectetur ".repeat(4000),
            band: 2,
        }];
        for i in 0..18 {
            sections.push(scene::DocSection {
                title: format!("tiny section {i} with a fairly long heading"),
                text: format!("[14:0{i}:01] NOTE: one-liner {i}"),
                band: 2,
            });
        }
        let cfg = SceneConfig {
            width: 760,
            height: 760,
            text_px: 9.0,
            boxes: true,
            ..Default::default()
        };
        let s = scene::build_doc(&sections, &cfg);
        let (w, h) = (s.width as f32, s.height as f32);
        for op in &VlmTheme.overlay(&s).ops {
            let Op::Text {
                pos,
                text,
                size_px,
                font,
                align,
                ..
            } = op
            else {
                continue;
            };
            let (tw, _) = theme::label_box(text, *size_px, *font);
            let (x, y) = (pos.0 * w, pos.1 * h);
            let (x0, x1) = match align {
                TextAlign::Center => (x - tw / 2.0, x + tw / 2.0),
                TextAlign::Left => (x, x + tw),
            };
            let (top, bot) = (y - size_px, y + size_px * 0.35);
            let cell = s
                .cells
                .iter()
                .map(|c| theme::poly_bbox_px(&c.poly, w, h))
                .find(|&(bx0, by0, bx1, by1)| x >= bx0 && x <= bx1 && y >= by0 && y <= by1)
                .expect("text op anchored inside some cell");
            let (bx0, by0, bx1, by1) = cell;
            let eps = 2.0;
            assert!(
                x0 >= bx0 - eps && x1 <= bx1 + eps && top >= by0 - eps && bot <= by1 + eps,
                "text {text:?} spans ({x0:.0}..{x1:.0}, {top:.0}..{bot:.0}) leaking outside its cell ({bx0:.0}..{bx1:.0}, {by0:.0}..{by1:.0})"
            );
        }
        let png = render_png(&s, &VlmTheme).unwrap();
        assert!(png.len() > 2000);
    }
}
