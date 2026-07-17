//! Scene assembly: turn analysis + geography into a typed scene graph.
//! Themes style scenes; backends draw them. Tests assert on scenes.

use c2m_core::graph::EdgeKind;
use c2m_core::types::Lang;
use c2m_index::handles::HandleRegistry;
use c2m_index::workspace::Built;
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
        }
    }
}

/// Inscribe-mode file loader: repo-relative path -> file contents.
pub type ContentLoader<'a> = dyn Fn(&str) -> Option<String> + 'a;

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

    let mut subtitle = format!("zoom: {}", region.display_name());
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
    }
}
