//! Scene assembly: turn analysis + geography into a typed scene graph.
//! Themes style scenes; backends draw them. Tests assert on scenes.

use c2m_core::graph::EdgeKind;
use c2m_core::types::Lang;
use c2m_index::handles::HandleRegistry;
use c2m_index::workspace::Built;
use c2m_layout::rect::{squarify, RectBox};
use c2m_layout::{layout, LayoutOptions, SavedSites, Territory};

#[derive(Debug, Clone)]
pub struct CityVis {
    pub pos: (f32, f32),
    pub r_px: f32,
    /// e.g. "F103 session.rs" (L1) or "S12 check_expiry" (L2).
    pub label: String,
    pub band: u8,
}

#[derive(Debug, Clone)]
pub struct CellVis {
    pub handle: String,
    pub name: String,
    pub band: u8,
    pub hazards: u8,
    pub lang: Lang,
    pub loc: u64,
    pub poly: Vec<(f32, f32)>,
    pub anchor: (f32, f32),
    /// Geometric center — used for edge endpoints so roads don't crowd the
    /// label anchor.
    pub centroid: (f32, f32),
    pub anchor_radius: f32,
    pub cities: Vec<CityVis>,
    /// Inscribe mode (v0.2): the cell's actual content, typeset in-territory.
    pub text: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct EdgeVis {
    pub a: (f32, f32),
    pub b: (f32, f32),
    pub kind: EdgeKind,
    pub weight: f32,
}

#[derive(Debug, Clone)]
pub struct IslandVis {
    pub pos: (f32, f32),
    pub r: f32,
    pub label: String,
}

#[derive(Debug)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub subtitle: String,
    pub cells: Vec<CellVis>,
    pub coast: Vec<Vec<(f32, f32)>>,
    pub contours: Vec<c2m_layout::ContourLevel>,
    pub edges: Vec<EdgeVis>,
    pub islands: Vec<IslandVis>,
    pub elevation: Vec<f32>,
    pub field_w: usize,
    pub field_h: usize,
    /// Total LOC represented (for the scale bar).
    pub total_loc: u64,
    /// Inscribe text size (px); themes use it when cells carry text.
    pub text_px: f32,
    /// Box layout (v0.3): rectangular territories, packed text (↵ reflow).
    pub boxes: bool,
}

pub struct SceneConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub seed: u64,
    /// Max dependency curves drawn.
    pub max_edges: usize,
    /// Inscribe mode: mono size for in-territory text (px at final raster).
    pub text_px: f32,
    /// Rectangular (squarified-treemap) territories for text-bearing maps —
    /// pxpipe-density packing. Organic Voronoi remains for index/human maps.
    pub boxes: bool,
}

impl Default for SceneConfig {
    fn default() -> Self {
        SceneConfig {
            width: 1092,
            height: 1092,
            title: String::new(),
            seed: 0,
            max_edges: 22,
            text_px: 10.0,
            boxes: true,
        }
    }
}

/// Inscribe-mode file loader: repo-relative path -> file contents.
pub type ContentLoader<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Squarify with capacity feedback: areas proportional to characters are
/// not enough, because a box's true capacity depends on its shape (the
/// header strip costs width × 2 rows, so wide boxes hold fewer chars per
/// px² than tall ones). Iterate: layout → measure capacity at real font
/// metrics → reweight by need/capacity → re-layout. Deterministic.
fn balanced_squarify(
    chars: &[usize],
    bounds: RectBox,
    canvas_w: f32,
    canvas_h: f32,
    text_px: f32,
) -> Vec<RectBox> {
    let advance = crate::text::measure("M", text_px, crate::display::FontKind::Mono).max(1.0);
    let line_h = text_px * 1.22;
    let pad = text_px * 0.35;
    let hsize = (text_px * 1.2).max(12.0);
    let header_px = hsize * 2.2;

    let capacity = |r: &RectBox| -> f32 {
        let w_px = r.w * canvas_w - 2.0 * pad;
        let h_px = r.h * canvas_h - header_px - pad;
        let cols = (w_px / advance).floor().max(0.0);
        let rows = (h_px / line_h).floor().max(0.0);
        cols * rows
    };

    let mut weights: Vec<f32> = chars.iter().map(|&c| c.max(40) as f32).collect();
    let mut rects = squarify(&weights, bounds);
    for _ in 0..8 {
        let mut worst: f32 = 1.0;
        for (i, r) in rects.iter().enumerate() {
            let cap = capacity(r).max(1.0);
            let need = chars[i].max(40) as f32 * 1.03; // breathing room
            let ratio = (need / cap).clamp(0.5, 2.0);
            worst = worst.max(ratio.max(1.0 / ratio));
            weights[i] *= ratio.powf(0.85);
        }
        rects = squarify(&weights, bounds);
        if worst < 1.05 {
            break;
        }
    }
    rects
}

fn rect_poly(r: &RectBox) -> Vec<(f32, f32)> {
    vec![
        (r.x, r.y),
        (r.x + r.w, r.y),
        (r.x + r.w, r.y + r.h),
        (r.x, r.y + r.h),
    ]
}

/// Canvas region for box layouts: full-bleed minus a hairline gutter
/// (no title strip — machine renders carry no in-image chrome).
const BOX_BOUNDS: RectBox = RectBox {
    x: 0.004,
    y: 0.004,
    w: 0.992,
    h: 0.992,
};

fn area_weight(loc: u64, n_files: usize) -> f32 {
    (loc as f32 + 30.0 * n_files as f32).max(20.0).powf(0.8)
}

/// L1: whole-repo scene, one cell per display region.
pub fn build_l1(built: &Built, saved: &mut SavedSites, cfg: &SceneConfig) -> Scene {
    let a = &built.analysis;
    let sums = built.region_summaries();

    let territories: Vec<Territory> = a
        .tree
        .regions
        .iter()
        .enumerate()
        .map(|(ri, r)| Territory {
            key: r.display_name().to_string(),
            area: area_weight(r.loc, r.files.len()),
            band: sums[ri].band,
        })
        .collect();
    let lopts = LayoutOptions {
        aspect: cfg.width as f32 / cfg.height as f32,
        seed: cfg.seed,
        n_islands: a.graph.external.len().min(12),
        ..Default::default()
    };
    let l = layout(&territories, &lopts, saved);

    // global label budget for cities: only the most important files get
    // named; the rest render as dots.
    let mut city_label_budget: Vec<(usize, usize, f32)> = Vec::new(); // (region, rank, score)
    for (ri, s) in sums.iter().enumerate() {
        for (rank, &(fi, score)) in s.ranked_files.iter().take(3).enumerate() {
            let _ = fi;
            city_label_budget.push((ri, rank, score));
        }
    }
    city_label_budget.sort_by(|x, y| y.2.total_cmp(&x.2));
    let labeled: std::collections::HashSet<(usize, usize)> = city_label_budget
        .iter()
        .take(14)
        .map(|&(ri, rank, _)| (ri, rank))
        .collect();

    let cells: Vec<CellVis> = a
        .tree
        .regions
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let shape = &l.cells[ri];
            let s = &sums[ri];
            let cities = s
                .ranked_files
                .iter()
                .take(3)
                .enumerate()
                .map(|(rank, &(fi, _))| {
                    // rank 0 sits *below* the anchor: the region label owns
                    // the anchor itself, and a colliding label for the top
                    // file would be dropped — the one name that must show
                    let offsets = [(0.0f32, 0.40f32), (-0.5, -0.30), (0.5, -0.12)];
                    let (ox, oy) = offsets[rank];
                    let rr = shape.anchor_radius;
                    let label = if labeled.contains(&(ri, rank)) {
                        let name = a.files[fi].path.rsplit('/').next().unwrap_or("");
                        format!("{} {}", built.file_handles[fi], name)
                    } else {
                        String::new()
                    };
                    CityVis {
                        pos: (shape.anchor.0 + ox * rr, shape.anchor.1 + oy * rr),
                        r_px: [3.4f32, 2.6, 2.2][rank],
                        label,
                        band: a.relevance.bands[fi],
                    }
                })
                .collect();
            CellVis {
                handle: built.region_handles[ri].clone(),
                name: r.display_name().to_string(),
                band: s.band,
                hazards: s.hazards,
                lang: r.dominant_lang,
                loc: r.loc,
                poly: shape.poly.clone(),
                anchor: shape.anchor,
                centroid: shape.centroid,
                anchor_radius: shape.anchor_radius,
                cities,
                text: None,
            }
        })
        .collect();

    // dependency curves between region centroids, strongest first
    let mut edges: Vec<EdgeVis> = Vec::new();
    let mut per_region = vec![0usize; cells.len()];
    'outer: for (ri, s) in sums.iter().enumerate() {
        for &(to, kind, w) in s.out_edges.iter().take(2) {
            if per_region[ri] >= 2 || edges.len() >= cfg.max_edges {
                if edges.len() >= cfg.max_edges {
                    break 'outer;
                }
                continue;
            }
            per_region[ri] += 1;
            edges.push(EdgeVis {
                a: cells[ri].centroid,
                b: cells[to].centroid,
                kind,
                weight: w,
            });
        }
    }

    let islands: Vec<IslandVis> = l
        .islands
        .iter()
        .zip(a.graph.external.iter().zip(&built.ext_handles))
        .map(|(&(x, y, r), ((name, _), h))| IslandVis {
            pos: (x, y),
            r,
            label: format!("{h} {name}"),
        })
        .collect();

    let total_loc: u64 = a.files.iter().map(|f| f.loc as u64).sum();
    Scene {
        width: cfg.width,
        height: cfg.height,
        title: cfg.title.clone(),
        subtitle: a.relevance.terms.join(" "),
        cells,
        coast: l.coast,
        contours: l.contours,
        edges,
        islands,
        elevation: l.elevation,
        field_w: l.field_w,
        field_h: l.field_h,
        total_loc,
        text_px: cfg.text_px,
        boxes: false,
    }
}

/// A titled chunk of a structured document (markdown section, chapter, …).
pub struct DocSection {
    pub title: String,
    pub text: String,
    pub band: u8,
}

/// Document map: sections become territories carrying their full text —
/// the `paint` path for structured text (no repo analysis involved).
pub fn build_doc(sections: &[DocSection], cfg: &SceneConfig) -> Scene {
    if cfg.boxes {
        return build_doc_boxes(sections, cfg);
    }
    let territories: Vec<Territory> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| Territory {
            key: format!("§{}:{}", i + 1, s.title),
            area: (s.text.len() as f32).max(80.0).powf(0.8),
            band: s.band,
        })
        .collect();
    let lopts = LayoutOptions {
        aspect: cfg.width as f32 / cfg.height as f32,
        seed: cfg.seed,
        n_islands: 0,
        // text-bearing maps: the sea is a sliver and the coast is calm —
        // pixels carry text
        margin: 0.018,
        coast_amp: 0.18,
        ..Default::default()
    };
    let mut saved = SavedSites::default();
    let l = layout(&territories, &lopts, &mut saved);

    let cells: Vec<CellVis> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let shape = &l.cells[i];
            CellVis {
                handle: format!("§{}", i + 1),
                name: s.title.clone(),
                band: s.band,
                hazards: 0,
                lang: Lang::Markdown,
                loc: s.text.lines().count() as u64,
                poly: shape.poly.clone(),
                anchor: shape.anchor,
                centroid: shape.centroid,
                anchor_radius: shape.anchor_radius,
                cities: Vec::new(),
                text: Some(s.text.lines().take(1200).map(str::to_string).collect()),
            }
        })
        .collect();

    let total_loc: u64 = cells.iter().map(|c| c.loc).sum();
    Scene {
        width: cfg.width,
        height: cfg.height,
        title: cfg.title.clone(),
        subtitle: String::new(),
        cells,
        coast: l.coast,
        contours: l.contours,
        edges: Vec::new(),
        islands: Vec::new(),
        elevation: l.elevation,
        field_w: l.field_w,
        field_h: l.field_h,
        total_loc,
        text_px: cfg.text_px,
        boxes: false,
    }
}

/// Box layout for documents: each section is a rectangle sized by its
/// text, packed edge-to-edge (squarified treemap), content ↵-reflowed.
fn build_doc_boxes(sections: &[DocSection], cfg: &SceneConfig) -> Scene {
    let chars: Vec<usize> = sections.iter().map(|s| s.text.len()).collect();
    let rects = balanced_squarify(
        &chars,
        BOX_BOUNDS,
        cfg.width as f32,
        cfg.height as f32,
        cfg.text_px,
    );
    let cells: Vec<CellVis> = sections
        .iter()
        .zip(&rects)
        .enumerate()
        .map(|(i, (s, r))| CellVis {
            handle: format!("§{}", i + 1),
            name: s.title.clone(),
            band: s.band,
            hazards: 0,
            lang: Lang::Markdown,
            loc: s.text.lines().count() as u64,
            poly: rect_poly(r),
            anchor: (r.x + r.w / 2.0, r.y + r.h / 2.0),
            centroid: (r.x + r.w / 2.0, r.y + r.h / 2.0),
            anchor_radius: r.w.min(r.h) / 2.0,
            cities: Vec::new(),
            text: Some(vec![crate::paint::reflow(&s.text)]),
        })
        .collect();
    let total_loc: u64 = cells.iter().map(|c| c.loc).sum();
    Scene {
        width: cfg.width,
        height: cfg.height,
        title: cfg.title.clone(),
        subtitle: String::new(),
        cells,
        coast: Vec::new(),
        contours: Vec::new(),
        edges: Vec::new(),
        islands: Vec::new(),
        elevation: vec![0.0; 4],
        field_w: 2,
        field_h: 2,
        total_loc,
        text_px: cfg.text_px,
        boxes: true,
    }
}

/// L2: one region's interior — cells are files, cities are symbols.
/// `content` (inscribe mode): loads a file's text by repo-relative path; when
/// given, cells carry their source for in-territory typesetting and symbol
/// cities are omitted (the text replaces them).
pub fn build_l2(
    built: &Built,
    region_idx: usize,
    registry: &mut HandleRegistry,
    saved: &mut SavedSites,
    cfg: &SceneConfig,
    content: Option<&ContentLoader<'_>>,
) -> Scene {
    let a = &built.analysis;
    let region = &a.tree.regions[region_idx];
    const MAX_FILES: usize = 48;

    // rank member files by importance+relevance, cap for readability
    let mut members: Vec<usize> = region.files.iter().map(|f| f.idx()).collect();
    members.sort_by(|&x, &y| {
        let sx = a.relevance.scores[x] + a.importance[x] * 2.0;
        let sy = a.relevance.scores[y] + a.importance[y] * 2.0;
        sy.total_cmp(&sx).then(x.cmp(&y))
    });
    let shown: Vec<usize> = members.iter().copied().take(MAX_FILES).collect();
    let hidden = members.len().saturating_sub(shown.len());

    // Box layout (default for inscribe): rectangles sized by source length,
    // packed edge-to-edge, ↵-reflowed text — pxpipe-density with structure.
    if cfg.boxes {
        if let Some(loader) = content {
            let sources: Vec<String> = shown
                .iter()
                .map(|&fi| {
                    loader(&a.files[fi].path)
                        .map(|s| s.lines().take(1200).collect::<Vec<_>>().join("\n"))
                        .unwrap_or_default()
                })
                .collect();
            let chars: Vec<usize> = sources.iter().map(|s| s.len()).collect();
            let rects = balanced_squarify(
                &chars,
                BOX_BOUNDS,
                cfg.width as f32,
                cfg.height as f32,
                cfg.text_px,
            );
            let cells: Vec<CellVis> = shown
                .iter()
                .zip(&rects)
                .zip(&sources)
                .map(|((&fi, r), src)| {
                    let f = &a.files[fi];
                    CellVis {
                        handle: built.file_handles[fi].clone(),
                        name: f.path.rsplit('/').next().unwrap_or(&f.path).to_string(),
                        band: a.relevance.bands[fi],
                        hazards: a.parsed[fi].hazards,
                        lang: f.lang,
                        loc: f.loc as u64,
                        poly: rect_poly(r),
                        anchor: (r.x + r.w / 2.0, r.y + r.h / 2.0),
                        centroid: (r.x + r.w / 2.0, r.y + r.h / 2.0),
                        anchor_radius: r.w.min(r.h) / 2.0,
                        cities: Vec::new(),
                        text: Some(vec![crate::paint::reflow(src)]),
                    }
                })
                .collect();
            let mut subtitle = format!("region: {}", region.display_name());
            if hidden > 0 {
                subtitle.push_str(&format!(
                    " (+{hidden} smaller files not shown — see legend)"
                ));
            }
            return Scene {
                width: cfg.width,
                height: cfg.height,
                title: cfg.title.clone(),
                subtitle,
                cells,
                coast: Vec::new(),
                contours: Vec::new(),
                edges: Vec::new(),
                islands: Vec::new(),
                elevation: vec![0.0; 4],
                field_w: 2,
                field_h: 2,
                total_loc: region.loc,
                text_px: cfg.text_px,
                boxes: true,
            };
        }
    }

    let territories: Vec<Territory> = shown
        .iter()
        .map(|&fi| Territory {
            key: a.files[fi].path.clone(),
            area: area_weight(a.files[fi].loc as u64, 1),
            band: a.relevance.bands[fi],
        })
        .collect();
    let lopts = LayoutOptions {
        aspect: cfg.width as f32 / cfg.height as f32,
        seed: cfg.seed ^ 0x2200,
        n_islands: 0,
        // zoom tiles exist to carry content; minimal, calm sea
        margin: if content.is_some() { 0.018 } else { 0.045 },
        coast_amp: if content.is_some() { 0.18 } else { 0.40 },
        ..Default::default()
    };
    let l = layout(&territories, &lopts, saved);

    let cells: Vec<CellVis> = shown
        .iter()
        .enumerate()
        .map(|(ci, &fi)| {
            let shape = &l.cells[ci];
            let f = &a.files[fi];
            let text: Option<Vec<String>> = content.and_then(|loader| {
                loader(&f.path).map(|src| {
                    src.lines()
                        .take(1200)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
            });
            // top symbols become labeled cities (inscribe mode: text replaces them)
            let mut syms: Vec<&c2m_core::Symbol> = a.parsed[fi].symbols.iter().collect();
            syms.sort_by_key(|s| std::cmp::Reverse(s.line_end.saturating_sub(s.line)));
            let cities = if text.is_some() {
                Vec::new()
            } else {
                syms.iter()
                    .take(2)
                    .enumerate()
                    .map(|(rank, sym)| {
                        let h = registry.assign_symbol(
                            &f.path,
                            &sym.name,
                            sym.kind.tag(),
                            (sym.line, sym.line_end),
                        );
                        let offsets = [(0.0f32, 0.28f32), (0.0, 0.62)];
                        let (ox, oy) = offsets[rank];
                        CityVis {
                            pos: (
                                shape.anchor.0 + ox,
                                shape.anchor.1 + oy * shape.anchor_radius,
                            ),
                            r_px: 2.4,
                            label: format!("{h} {}", sym.name),
                            band: a.relevance.bands[fi],
                        }
                    })
                    .collect()
            };
            CellVis {
                handle: built.file_handles[fi].clone(),
                name: f.path.rsplit('/').next().unwrap_or(&f.path).to_string(),
                band: a.relevance.bands[fi],
                hazards: a.parsed[fi].hazards,
                lang: f.lang,
                loc: f.loc as u64,
                poly: shape.poly.clone(),
                anchor: shape.anchor,
                centroid: shape.centroid,
                anchor_radius: shape.anchor_radius,
                cities,
                text,
            }
        })
        .collect();

    // intra-region file edges
    let local: std::collections::HashMap<usize, usize> =
        shown.iter().enumerate().map(|(ci, &fi)| (fi, ci)).collect();
    let mut edges: Vec<EdgeVis> = Vec::new();
    for e in &a.graph.edges {
        if edges.len() >= cfg.max_edges {
            break;
        }
        if let (Some(&ca), Some(&cb)) = (local.get(&e.from.idx()), local.get(&e.to.idx())) {
            if ca != cb && e.kind == EdgeKind::Import {
                edges.push(EdgeVis {
                    a: cells[ca].anchor,
                    b: cells[cb].anchor,
                    kind: e.kind,
                    weight: e.weight,
                });
            }
        }
    }

    let mut subtitle = format!("region: {}", region.display_name());
    if hidden > 0 {
        subtitle.push_str(&format!(
            " (+{hidden} smaller files not shown — see legend)"
        ));
    }
    let total_loc = region.loc;
    Scene {
        width: cfg.width,
        height: cfg.height,
        title: cfg.title.clone(),
        subtitle,
        cells,
        coast: l.coast,
        contours: l.contours,
        edges,
        islands: Vec::new(),
        elevation: l.elevation,
        field_w: l.field_w,
        field_h: l.field_h,
        total_loc,
        text_px: cfg.text_px,
        boxes: false,
    }
}
