//! Shape extraction from the assignment grid: mask outlines via marching
//! squares (robust against single-pixel spurs, unlike pixel-following
//! traces), Ramer–Douglas–Peucker simplification, Chaikin smoothing, and
//! pole-of-inaccessibility label anchors.

use crate::field::marching_squares;
use crate::CellShape;

/// RDP tolerance in *grid pixels* (converted to normalized units per call).
/// Kept tight: adjacent cells simplify independently, so every unit of
/// tolerance widens the visible channel between them.
const RDP_EPS_PX: f32 = 0.8;

/// (polygon, centroid, anchor, anchor radius) in normalized coordinates.
type CellParts = (Vec<(f32, f32)>, (f32, f32), (f32, f32), f32);

/// Outline(s) of a mask as normalized closed polylines, longest first.
fn mask_outlines(mask: &[bool], gw: usize, gh: usize, max_parts: usize) -> Vec<Vec<(f32, f32)>> {
    let field: Vec<f32> = mask.iter().map(|&m| if m { 1.0 } else { 0.0 }).collect();
    let mut chains = marching_squares(&field, gw, gh, 0.5);
    chains.sort_by_key(|c| std::cmp::Reverse(c.len()));
    chains.truncate(max_parts);
    let eps = RDP_EPS_PX / gw as f32;
    chains
        .into_iter()
        .map(|c| chaikin(&rdp_closed(&c, eps), 1))
        .filter(|c| c.len() >= 3)
        .collect()
}

/// Extract the (largest component of the) cell with the given id as a
/// simplified, lightly smoothed polygon in normalized coordinates.
pub fn extract_cell(assign: &[u16], gw: usize, gh: usize, id: u16) -> CellShape {
    let mask: Vec<bool> = assign.iter().map(|&a| a == id).collect();
    let (poly, centroid, anchor, anchor_radius) = extract_from_mask(&mask, gw, gh);
    CellShape {
        poly,
        centroid,
        anchor,
        anchor_radius,
    }
}

/// Outline(s) of an arbitrary mask (used for the coastline). Returns the
/// largest parts first, up to 4.
pub fn extract_mask_outline(mask: &[bool], gw: usize, gh: usize) -> Vec<Vec<(f32, f32)>> {
    mask_outlines(mask, gw, gh, 4)
}

fn extract_from_mask(mask: &[bool], gw: usize, gh: usize) -> CellParts {
    let count = mask.iter().filter(|&&m| m).count();
    if count == 0 {
        return (Vec::new(), (0.5, 0.5), (0.5, 0.5), 0.0);
    }
    // largest connected component
    let mut visited = vec![false; mask.len()];
    let mut best: Vec<usize> = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || visited[start] {
            continue;
        }
        let mut stack = vec![start];
        visited[start] = true;
        let mut comp = Vec::new();
        while let Some(p) = stack.pop() {
            comp.push(p);
            let (x, y) = (p % gw, p / gw);
            for (nx, ny) in neighbors4(x, y, gw, gh) {
                let q = ny * gw + nx;
                if mask[q] && !visited[q] {
                    visited[q] = true;
                    stack.push(q);
                }
            }
        }
        if comp.len() > best.len() {
            best = comp;
        }
    }
    let comp_mask = {
        let mut m = vec![false; mask.len()];
        for &p in &best {
            m[p] = true;
        }
        m
    };

    // centroid
    let (mut cx, mut cy) = (0f64, 0f64);
    for &p in &best {
        cx += (p % gw) as f64 + 0.5;
        cy += (p / gw) as f64 + 0.5;
    }
    let centroid = (
        (cx / best.len() as f64 / gw as f64) as f32,
        (cy / best.len() as f64 / gh as f64) as f32,
    );

    // pole of inaccessibility: BFS distance from boundary
    let (anchor, radius_px) = pole_of_inaccessibility(&comp_mask, gw, gh);

    let poly = mask_outlines(&comp_mask, gw, gh, 1)
        .into_iter()
        .next()
        .unwrap_or_default();
    (
        poly,
        centroid,
        ((anchor.0 + 0.5) / gw as f32, (anchor.1 + 0.5) / gh as f32),
        radius_px / gw as f32,
    )
}

fn neighbors4(x: usize, y: usize, gw: usize, gh: usize) -> impl Iterator<Item = (usize, usize)> {
    let mut v: Vec<(usize, usize)> = Vec::with_capacity(4);
    if x + 1 < gw {
        v.push((x + 1, y));
    }
    if x > 0 {
        v.push((x - 1, y));
    }
    if y + 1 < gh {
        v.push((x, y + 1));
    }
    if y > 0 {
        v.push((x, y - 1));
    }
    v.into_iter()
}

/// Multi-source BFS from all boundary pixels; the last-reached pixel is the
/// most interior point, its BFS depth the clearance radius.
fn pole_of_inaccessibility(mask: &[bool], gw: usize, gh: usize) -> ((f32, f32), f32) {
    let mut dist = vec![u32::MAX; mask.len()];
    let mut queue = std::collections::VecDeque::new();
    for y in 0..gh {
        for x in 0..gw {
            let p = y * gw + x;
            if !mask[p] {
                continue;
            }
            let on_edge = x == 0
                || y == 0
                || x == gw - 1
                || y == gh - 1
                || neighbors4(x, y, gw, gh).any(|(nx, ny)| !mask[ny * gw + nx]);
            if on_edge {
                dist[p] = 0;
                queue.push_back(p);
            }
        }
    }
    let mut best = (0usize, 0u32);
    while let Some(p) = queue.pop_front() {
        let (x, y) = (p % gw, p / gw);
        for (nx, ny) in neighbors4(x, y, gw, gh) {
            let q = ny * gw + nx;
            if mask[q] && dist[q] == u32::MAX {
                dist[q] = dist[p] + 1;
                if dist[q] > best.1 || (dist[q] == best.1 && q < best.0) {
                    best = (q, dist[q]);
                }
                queue.push_back(q);
            }
        }
    }
    let (x, y) = (best.0 % gw, best.0 / gw);
    ((x as f32, y as f32), best.1 as f32)
}

/// RDP for a *closed* boundary: a zero-length chord (first == last point)
/// makes plain RDP collapse everything, so split at the point farthest from
/// the start and simplify the two halves independently.
pub fn rdp_closed(points: &[(f32, f32)], epsilon: f32) -> Vec<(f32, f32)> {
    let mut pts = points.to_vec();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 4 {
        return pts;
    }
    let (sx, sy) = pts[0];
    let far = pts
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let da = (a.0 - sx).powi(2) + (a.1 - sy).powi(2);
            let db = (b.0 - sx).powi(2) + (b.1 - sy).powi(2);
            da.total_cmp(&db)
        })
        .map(|(i, _)| i)
        .unwrap_or(pts.len() / 2)
        .max(1);
    let mut first_half = rdp(&pts[..=far], epsilon);
    let mut second_half = {
        let mut back: Vec<(f32, f32)> = pts[far..].to_vec();
        back.push(pts[0]);
        rdp(&back, epsilon)
    };
    first_half.pop(); // shared vertex at `far`
    second_half.pop(); // shared vertex at start
    first_half.append(&mut second_half);
    first_half
}

/// Ramer–Douglas–Peucker polyline simplification (open polyline).
pub fn rdp(points: &[(f32, f32)], epsilon: f32) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (ax, ay) = points[a];
        let (bx, by) = points[b];
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let mut worst = (a, 0f32);
        for (i, &(px, py)) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = ((px - ax) * dy - (py - ay) * dx).abs() / len;
            if d > worst.1 {
                worst = (i, d);
            }
        }
        if worst.1 > epsilon {
            keep[worst.0] = true;
            stack.push((a, worst.0));
            stack.push((worst.0, b));
        }
    }
    points
        .iter()
        .zip(&keep)
        .filter(|(_, &k)| k)
        .map(|(&p, _)| p)
        .collect()
}

/// Chaikin corner cutting, `rounds` iterations, treating the polygon as closed.
pub fn chaikin(points: &[(f32, f32)], rounds: usize) -> Vec<(f32, f32)> {
    let mut pts = points.to_vec();
    for _ in 0..rounds {
        if pts.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(pts.len() * 2);
        for i in 0..pts.len() {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % pts.len()];
            next.push((ax * 0.75 + bx * 0.25, ay * 0.75 + by * 0.25));
            next.push((ax * 0.25 + bx * 0.75, ay * 0.25 + by * 0.75));
        }
        pts = next;
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_a_square() {
        let (gw, gh) = (10usize, 10usize);
        let mut mask = vec![false; gw * gh];
        for y in 2..8 {
            for x in 2..8 {
                mask[y * gw + x] = true;
            }
        }
        let (poly, centroid, anchor, radius) = extract_from_mask(&mask, gw, gh);
        assert!(poly.len() >= 4);
        assert!((centroid.0 - 0.5).abs() < 0.05);
        assert!((anchor.0 - 0.5).abs() < 0.15);
        assert!(radius > 0.0);
    }

    #[test]
    fn rdp_collapses_collinear() {
        let line: Vec<(f32, f32)> = (0..50).map(|i| (i as f32, 0.0)).collect();
        assert_eq!(rdp(&line, 0.5).len(), 2);
    }
}
