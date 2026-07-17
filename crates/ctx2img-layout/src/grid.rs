//! Raster machinery: land mask, power-diagram assignment, areas,
//! centroids, adjacency, island placement.

use crate::noise;
use rayon::prelude::*;

/// Continent mask: an inset rounded rectangle whose boundary is warped by
/// value noise — enough organic character to read as a landmass, without
/// risking disconnected blobs (the noise only perturbs the edge).
pub fn land_mask(gw: usize, gh: usize, margin: f32, coast_amp: f32, seed: u64) -> Vec<bool> {
    let mut mask = vec![false; gw * gh];
    for y in 0..gh {
        for x in 0..gw {
            let nx = (x as f32 + 0.5) / gw as f32;
            let ny = (y as f32 + 0.5) / gh as f32;
            // distance to canvas edge in normalized units
            let edge = nx.min(1.0 - nx).min(ny).min(1.0 - ny);
            let n = noise::fbm(nx, ny, seed);
            let threshold = margin * (1.0 + coast_amp * (n - 0.5) * 2.0);
            if edge > threshold {
                mask[y * gw + x] = true;
            }
        }
    }
    mask
}

/// Nearest land pixel to (x, y) in normalized coords (spiral search).
pub fn snap_to_land(land: &[bool], gw: usize, gh: usize, x: f32, y: f32) -> (f32, f32) {
    let px = ((x * gw as f32) as isize).clamp(0, gw as isize - 1);
    let py = ((y * gh as f32) as isize).clamp(0, gh as isize - 1);
    if land[py as usize * gw + px as usize] {
        return (x, y);
    }
    for r in 1..(gw.max(gh) as isize) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue; // ring only
                }
                let (qx, qy) = (px + dx, py + dy);
                if qx >= 0
                    && qy >= 0
                    && (qx as usize) < gw
                    && (qy as usize) < gh
                    && land[qy as usize * gw + qx as usize]
                {
                    return ((qx as f32 + 0.5) / gw as f32, (qy as f32 + 0.5) / gh as f32);
                }
            }
        }
    }
    (0.5, 0.5)
}

/// Power-diagram assignment: each land pixel goes to argmin(d² − w).
/// Deterministic; ties break toward the lower site index.
pub fn assign_cells(
    land: &[bool],
    gw: usize,
    gh: usize,
    sites: &[(f32, f32)],
    weights: &[f32],
    out: &mut [u16],
) {
    // pre-scale sites to pixel space
    let sp: Vec<(f32, f32, f32)> = sites
        .iter()
        .zip(weights)
        .map(|(&(x, y), &w)| (x * gw as f32, y * gh as f32, w))
        .collect();
    out.par_chunks_mut(gw).enumerate().for_each(|(y, row)| {
        for (x, slot) in row.iter_mut().enumerate() {
            if !land[y * gw + x] {
                *slot = u16::MAX;
                continue;
            }
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut best = 0u16;
            let mut best_d = f32::INFINITY;
            for (i, &(sx, sy, w)) in sp.iter().enumerate() {
                let d = (px - sx) * (px - sx) + (py - sy) * (py - sy) - w;
                if d < best_d {
                    best_d = d;
                    best = i as u16;
                }
            }
            *slot = best;
        }
    });
}

pub fn cell_areas(assign: &[u16], n: usize) -> Vec<u32> {
    let mut areas = vec![0u32; n];
    for &a in assign {
        if (a as usize) < n {
            areas[a as usize] += 1;
        }
    }
    areas
}

pub fn cell_centroids(assign: &[u16], gw: usize, gh: usize, n: usize) -> Vec<Option<(f32, f32)>> {
    let mut sum = vec![(0f64, 0f64, 0u64); n];
    for y in 0..gh {
        for x in 0..gw {
            let a = assign[y * gw + x] as usize;
            if a < n {
                sum[a].0 += x as f64 + 0.5;
                sum[a].1 += y as f64 + 0.5;
                sum[a].2 += 1;
            }
        }
    }
    sum.into_iter()
        .map(|(sx, sy, c)| {
            if c == 0 {
                None
            } else {
                Some((
                    (sx / c as f64 / gw as f64) as f32,
                    (sy / c as f64 / gh as f64) as f32,
                ))
            }
        })
        .collect()
}

/// Deterministically pick a pixel inside cell `id` (used to rescue
/// vanished cells). `salt` varies the choice between attempts.
pub fn pick_pixel_in_cell(assign: &[u16], gw: usize, id: u16, salt: u64) -> Option<(f32, f32)> {
    let members: Vec<usize> = assign
        .iter()
        .enumerate()
        .filter(|(_, &a)| a == id)
        .map(|(i, _)| i)
        .collect();
    if members.is_empty() {
        return None;
    }
    let pick = members[(salt as usize) % members.len()];
    let gh = assign.len() / gw;
    Some((
        ((pick % gw) as f32 + 0.5) / gw as f32,
        ((pick / gw) as f32 + 0.5) / gh as f32,
    ))
}

/// Neighboring cell pairs (a < b), from 4-connected grid transitions.
pub fn adjacency(assign: &[u16], gw: usize, gh: usize, n: usize) -> Vec<(usize, usize)> {
    let mut pairs = std::collections::BTreeSet::new();
    for y in 0..gh {
        for x in 0..gw {
            let a = assign[y * gw + x] as usize;
            if a >= n {
                continue;
            }
            if x + 1 < gw {
                let b = assign[y * gw + x + 1] as usize;
                if b < n && b != a {
                    pairs.insert((a.min(b), a.max(b)));
                }
            }
            if y + 1 < gh {
                let b = assign[(y + 1) * gw + x] as usize;
                if b < n && b != a {
                    pairs.insert((a.min(b), a.max(b)));
                }
            }
        }
    }
    pairs.into_iter().collect()
}

/// Scatter islands in the sea ring around the continent: deterministic
/// angles, nudged outward until fully in water.
pub fn place_islands(
    land: &[bool],
    gw: usize,
    gh: usize,
    count: usize,
    seed: u64,
) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    for i in 0..count {
        let h = noise::hash64(&(i as u64).to_le_bytes(), seed ^ 0x15AD);
        let angle = (h & 0xFFFF) as f32 / 65535.0 * std::f32::consts::TAU;
        let radius_scale = 0.90 + 0.07 * ((h >> 16) & 0xFF) as f32 / 255.0;
        let r = 0.010 + 0.008 * ((h >> 24) & 0xFF) as f32 / 255.0;
        // walk outward from center until in sea with clearance
        let (dx, dy) = (angle.cos(), angle.sin());
        let mut t = 0.30f32;
        let mut placed = None;
        while t < 0.75 {
            let x = 0.5 + dx * t * radius_scale;
            let y = 0.5 + dy * t * radius_scale;
            if !(0.03..=0.97).contains(&x) || !(0.03..=0.97).contains(&y) {
                break;
            }
            let px = (x * gw as f32) as usize;
            let py = (y * gh as f32) as usize;
            let clear = (r * gw as f32) as isize + 2;
            let mut ok = true;
            'chk: for cy in -clear..=clear {
                for cx in -clear..=clear {
                    let (qx, qy) = (px as isize + cx, py as isize + cy);
                    if qx >= 0
                        && qy >= 0
                        && (qx as usize) < gw
                        && (qy as usize) < gh
                        && land[qy as usize * gw + qx as usize]
                    {
                        ok = false;
                        break 'chk;
                    }
                }
            }
            if ok {
                placed = Some((x, y, r));
                break;
            }
            t += 0.02;
        }
        if let Some(p) = placed {
            out.push(p);
        }
    }
    out
}
