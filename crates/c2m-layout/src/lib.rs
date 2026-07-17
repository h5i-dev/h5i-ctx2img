//! c2m-layout — stable cartographic geography.
//!
//! A raster power-diagram treemap: territories (regions at L1, files at L2)
//! become organic Voronoi-style cells inside a noise-shaped continent, with
//! cell areas adapted to code mass (Nocaj–Brandes weight adaptation,
//! implemented on a grid for robustness). Geography is *stable*: sites
//! persist across runs keyed by territory path, so the map only shifts
//! where code shifts.
//!
//! Everything is deterministic given (territories, persisted sites, seed).

pub mod field;
pub mod grid;
pub mod noise;
pub mod rect;
pub mod trace;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One thing to place on the map (a region at L1, a file at L2).
#[derive(Debug, Clone)]
pub struct Territory {
    /// Stable persistence key (region path or file path).
    pub key: String,
    /// Target area weight (arbitrary positive scale, e.g. log LOC).
    pub area: f32,
    /// Elevation band 1..=5.
    pub band: u8,
}

/// Normalized output geometry; coordinates are in [0,1] × [0,1].
#[derive(Debug, Clone)]
pub struct CellShape {
    /// Outer boundary polygon (closed implicitly).
    pub poly: Vec<(f32, f32)>,
    pub centroid: (f32, f32),
    /// Pole of inaccessibility: best label anchor.
    pub anchor: (f32, f32),
    /// Clearance radius at the anchor (normalized units).
    pub anchor_radius: f32,
}

#[derive(Debug)]
pub struct Layout {
    pub cells: Vec<CellShape>,
    /// Isolines between elevation bands: (level, polylines).
    pub contours: Vec<ContourLevel>,
    /// Coastline polygons (the continent outline, possibly multiple parts).
    pub coast: Vec<Vec<(f32, f32)>>,
    /// Adjacent cell pairs (a < b).
    pub adjacency: Vec<(usize, usize)>,
    /// Offshore island positions for external deps: (x, y, radius).
    pub islands: Vec<(f32, f32, f32)>,
    /// Smoothed elevation field for hillshading, row-major `field_w × field_h`.
    pub elevation: Vec<f32>,
    pub field_w: usize,
    pub field_h: usize,
}

/// (x, y, power weight) for one persisted site, [0,1] coordinates.
pub type SiteRecord = (f32, f32, f32);

/// One elevation level's isolines: (level, polylines in [0,1] coords).
pub type ContourLevel = (f32, Vec<Vec<(f32, f32)>>);

/// Persisted geography: territory key -> (x, y, weight) in [0,1] coords.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SavedSites {
    pub sites: BTreeMap<String, SiteRecord>,
}

impl SavedSites {
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string(self)?)?;
        Ok(())
    }
}

pub struct LayoutOptions {
    /// Working grid width in pixels (height follows aspect).
    pub grid: usize,
    /// Canvas aspect ratio (width / height).
    pub aspect: f32,
    /// Deterministic seed (vary per repo for distinct coastlines).
    pub seed: u64,
    /// Sea margin as a fraction of the canvas.
    pub margin: f32,
    /// Coastline waviness: fraction of the margin the coast may wander.
    /// High = scenic; low = dense (more canvas is land).
    pub coast_amp: f32,
    /// How many external-dep islands to place.
    pub n_islands: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions {
            grid: 448,
            aspect: 1.0,
            seed: 0xC2A9,
            margin: 0.055,
            coast_amp: 0.45,
            n_islands: 12,
        }
    }
}

/// Compute the map geography. `saved` carries positions from previous runs
/// (updated in place with the new result).
pub fn layout(territories: &[Territory], opts: &LayoutOptions, saved: &mut SavedSites) -> Layout {
    let gw = opts.grid;
    let gh = ((opts.grid as f32 / opts.aspect).round() as usize).max(64);
    let n = territories.len();

    let land = grid::land_mask(gw, gh, opts.margin, opts.coast_amp, opts.seed);
    let land_count = land.iter().filter(|&&l| l).count().max(1);

    // --- sites: persisted or hash-seeded, snapped into land ---
    let mut sites: Vec<(f32, f32)> = Vec::with_capacity(n);
    let mut weights: Vec<f32> = Vec::with_capacity(n);
    for t in territories {
        if let Some(&(x, y, w)) = saved.sites.get(&t.key) {
            let (x, y) = grid::snap_to_land(&land, gw, gh, x, y);
            sites.push((x, y));
            weights.push(w);
        } else {
            let h = noise::hash64(t.key.as_bytes(), opts.seed);
            let x = 0.15 + 0.7 * ((h & 0xFFFF) as f32 / 65535.0);
            let y = 0.15 + 0.7 * (((h >> 16) & 0xFFFF) as f32 / 65535.0);
            let (x, y) = grid::snap_to_land(&land, gw, gh, x, y);
            sites.push((x, y));
            weights.push(0.0);
        }
    }

    // --- target areas (in land pixels) ---
    let total_area: f32 = territories.iter().map(|t| t.area.max(0.01)).sum();
    let targets: Vec<f32> = territories
        .iter()
        .map(|t| t.area.max(0.01) / total_area * land_count as f32)
        .collect();

    // --- weight adaptation (grid power diagram) ---
    let mut assign = vec![u16::MAX; gw * gh];
    if n > 0 {
        let scale = land_count as f32 / n as f32; // ~mean cell area = distance² scale
        for iter in 0..60usize {
            grid::assign_cells(&land, gw, gh, &sites, &weights, &mut assign);
            let areas = grid::cell_areas(&assign, n);

            // rescue empty cells: park the site inside the biggest cell
            let mut moved = false;
            for i in 0..n {
                if areas[i] == 0 {
                    let big = (0..n).max_by_key(|&j| areas[j]).unwrap_or(0);
                    if big != i && areas[big] > 4 {
                        let h = noise::hash64(territories[i].key.as_bytes(), iter as u64 + 1);
                        if let Some(p) = grid::pick_pixel_in_cell(&assign, gw, big as u16, h) {
                            sites[i] = p;
                            weights[i] = weights[big] * 0.5;
                            moved = true;
                        }
                    }
                }
            }
            if moved {
                continue;
            }

            let mut max_err = 0f32;
            for i in 0..n {
                let err = (targets[i] - areas[i] as f32) / targets[i].max(1.0);
                max_err = max_err.max(err.abs());
                weights[i] += 0.45 * scale * err.clamp(-1.5, 1.5);
            }
            // Lloyd recentering keeps cells round; every 3rd iteration so
            // area adaptation gets time to act between moves.
            if iter % 3 == 2 {
                let centroids = grid::cell_centroids(&assign, gw, gh, n);
                for i in 0..n {
                    if let Some(c) = centroids[i] {
                        sites[i] = (sites[i].0 * 0.4 + c.0 * 0.6, sites[i].1 * 0.4 + c.1 * 0.6);
                    }
                }
            }
            if max_err < 0.06 && iter > 6 {
                break;
            }
        }
        grid::assign_cells(&land, gw, gh, &sites, &weights, &mut assign);
    }

    // --- persist geography ---
    saved.sites.clear();
    for (i, t) in territories.iter().enumerate() {
        saved
            .sites
            .insert(t.key.clone(), (sites[i].0, sites[i].1, weights[i]));
    }

    // --- extract shapes ---
    let cells: Vec<CellShape> = (0..n)
        .map(|i| trace::extract_cell(&assign, gw, gh, i as u16))
        .collect();
    let coast = trace::extract_mask_outline(&land, gw, gh);
    let adjacency = grid::adjacency(&assign, gw, gh, n);

    // --- elevation field + contours ---
    let bands: Vec<f32> = territories.iter().map(|t| t.band as f32).collect();
    let (elevation, fw, fh) = field::elevation_field(&assign, &land, gw, gh, &bands);
    let contours = [1.5f32, 2.5, 3.5, 4.5]
        .iter()
        .map(|&lvl| (lvl, field::marching_squares(&elevation, fw, fh, lvl)))
        .collect();

    let islands = grid::place_islands(&land, gw, gh, opts.n_islands, opts.seed);

    Layout {
        cells,
        contours,
        coast,
        adjacency,
        islands,
        elevation,
        field_w: fw,
        field_h: fh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terr(n: usize) -> Vec<Territory> {
        (0..n)
            .map(|i| Territory {
                key: format!("src/mod{i}"),
                area: (i + 1) as f32,
                band: (i % 5 + 1) as u8,
            })
            .collect()
    }

    fn shoelace(p: &[(f32, f32)]) -> f32 {
        if p.len() < 3 {
            return 0.0;
        }
        let mut s = 0f32;
        for i in 0..p.len() {
            let (x1, y1) = p[i];
            let (x2, y2) = p[(i + 1) % p.len()];
            s += x1 * y2 - x2 * y1;
        }
        s.abs() / 2.0
    }

    #[test]
    fn deterministic() {
        let t = terr(8);
        let opts = LayoutOptions {
            grid: 160,
            ..Default::default()
        };
        let a = layout(&t, &opts, &mut SavedSites::default());
        let b = layout(&t, &opts, &mut SavedSites::default());
        assert_eq!(a.cells.len(), b.cells.len());
        for (ca, cb) in a.cells.iter().zip(&b.cells) {
            assert_eq!(ca.poly, cb.poly);
        }
    }

    #[test]
    fn areas_roughly_proportional() {
        let t = vec![
            Territory {
                key: "big".into(),
                area: 8.0,
                band: 3,
            },
            Territory {
                key: "small".into(),
                area: 1.0,
                band: 2,
            },
        ];
        let opts = LayoutOptions {
            grid: 160,
            ..Default::default()
        };
        let l = layout(&t, &opts, &mut SavedSites::default());
        let (a_big, a_small) = (shoelace(&l.cells[0].poly), shoelace(&l.cells[1].poly));
        assert!(
            a_big > a_small * 2.5,
            "8:1 targets should yield a clearly bigger cell ({a_big} vs {a_small})"
        );
    }

    #[test]
    fn stability_under_addition() {
        let mut t = terr(6);
        let opts = LayoutOptions {
            grid: 160,
            ..Default::default()
        };
        let mut saved = SavedSites::default();
        let before = layout(&t, &opts, &mut saved);
        // add a new territory; existing anchors should not teleport
        t.push(Territory {
            key: "src/newcomer".into(),
            area: 2.0,
            band: 1,
        });
        let after = layout(&t, &opts, &mut saved);
        for i in 0..6 {
            let (ax, ay) = before.cells[i].anchor;
            let (bx, by) = after.cells[i].anchor;
            let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
            assert!(d < 0.35, "cell {i} anchor moved too far: {d}");
        }
    }

    #[test]
    fn contours_and_coast_exist() {
        let l = layout(
            &terr(5),
            &LayoutOptions {
                grid: 160,
                ..Default::default()
            },
            &mut SavedSites::default(),
        );
        assert!(!l.coast.is_empty(), "coastline");
        assert!(
            l.contours.iter().any(|(_, lines)| !lines.is_empty()),
            "some contour lines"
        );
        assert!(!l.islands.is_empty());
    }
}
