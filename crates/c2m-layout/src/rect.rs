//! Squarified treemap (Bruls–Huizing–van Wijk): rectangular territories
//! for text-bearing maps. Rectangles tile exactly — no corner waste, no
//! inter-cell channels — which is what pxpipe-level glyph fill requires.
//! Deterministic: ties broken by input index.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Lay out `weights` inside `bounds`; result is in input order.
pub fn squarify(weights: &[f32], bounds: RectBox) -> Vec<RectBox> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let total: f32 = weights.iter().map(|w| w.max(1e-6)).sum();
    let scale = bounds.w * bounds.h / total;

    // process in descending-weight order, restore input order at the end
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| weights[b].total_cmp(&weights[a]).then(a.cmp(&b)));
    let areas: Vec<f32> = order
        .iter()
        .map(|&i| weights[i].max(1e-6) * scale)
        .collect();

    let mut out = vec![
        RectBox {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0
        };
        n
    ];
    let mut free = bounds;
    let mut row: Vec<usize> = Vec::new(); // indices into `areas`
    let mut i = 0usize;

    let worst = |row: &[usize], areas: &[f32], side: f32| -> f32 {
        let sum: f32 = row.iter().map(|&k| areas[k]).sum();
        if sum <= 0.0 || side <= 0.0 {
            return f32::INFINITY;
        }
        let s2 = sum * sum;
        let mut w = 0f32;
        for &k in row {
            let a = areas[k];
            let r = (side * side * a / s2).max(s2 / (side * side * a));
            w = w.max(r);
        }
        w
    };

    let lay = |row: &[usize], free: &mut RectBox, out: &mut Vec<RectBox>| {
        let sum: f32 = row.iter().map(|&k| areas[k]).sum();
        if sum <= 0.0 {
            return;
        }
        let horizontal = free.w >= free.h; // slice along the shorter side
        if horizontal {
            let col_w = sum / free.h;
            let mut y = free.y;
            for &k in row {
                let h = areas[k] / col_w;
                out[order[k]] = RectBox {
                    x: free.x,
                    y,
                    w: col_w,
                    h,
                };
                y += h;
            }
            free.x += col_w;
            free.w -= col_w;
        } else {
            let row_h = sum / free.w;
            let mut x = free.x;
            for &k in row {
                let w = areas[k] / row_h;
                out[order[k]] = RectBox {
                    x,
                    y: free.y,
                    w,
                    h: row_h,
                };
                x += w;
            }
            free.y += row_h;
            free.h -= row_h;
        }
    };

    while i < areas.len() {
        let side = free.w.min(free.h);
        let mut candidate = row.clone();
        candidate.push(i);
        if row.is_empty() || worst(&candidate, &areas, side) <= worst(&row, &areas, side) {
            row = candidate;
            i += 1;
        } else {
            lay(&row, &mut free, &mut out);
            row.clear();
        }
    }
    if !row.is_empty() {
        lay(&row, &mut free, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_exactly_and_preserves_order() {
        let weights = [8.0, 1.0, 3.0, 2.0];
        let bounds = RectBox {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        };
        let rects = squarify(&weights, bounds);
        let total: f32 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((total - 1.0).abs() < 1e-3, "areas sum to bounds ({total})");
        // proportionality: rect 0 is ~8x rect 1
        let ratio = (rects[0].w * rects[0].h) / (rects[1].w * rects[1].h);
        assert!((ratio - 8.0).abs() < 0.5, "ratio {ratio}");
        // all inside bounds
        for r in &rects {
            assert!(
                r.x >= -1e-4 && r.y >= -1e-4 && r.x + r.w <= 1.0 + 1e-3 && r.y + r.h <= 1.0 + 1e-3
            );
        }
    }

    #[test]
    fn deterministic() {
        let w = [5.0, 5.0, 2.0, 2.0, 1.0];
        let b = RectBox {
            x: 0.0,
            y: 0.0,
            w: 2.0,
            h: 1.0,
        };
        assert_eq!(squarify(&w, b), squarify(&w, b));
    }
}
