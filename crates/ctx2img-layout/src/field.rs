//! Elevation field: rasterized band values, smoothed for contours and
//! hillshading, plus a marching-squares isoline extractor.

/// Build a smoothed elevation field at half grid resolution.
/// Sea level is 0; land cells contribute their band (1..=5).
pub fn elevation_field(
    assign: &[u16],
    land: &[bool],
    gw: usize,
    gh: usize,
    bands: &[f32],
) -> (Vec<f32>, usize, usize) {
    let fw = gw / 2;
    let fh = gh / 2;
    let mut field = vec![0f32; fw * fh];
    for fy in 0..fh {
        for fx in 0..fw {
            // average the 2×2 source pixels
            let mut acc = 0f32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let x = fx * 2 + dx;
                    let y = fy * 2 + dy;
                    let p = y * gw + x;
                    if land[p] {
                        let a = assign[p] as usize;
                        acc += bands.get(a).copied().unwrap_or(0.0);
                    }
                }
            }
            field[fy * fw + fx] = acc / 4.0;
        }
    }
    // separable box blur ×3 ≈ gaussian; radius scales with grid
    let radius = (fw / 48).max(2);
    for _ in 0..3 {
        box_blur(&mut field, fw, fh, radius);
    }
    (field, fw, fh)
}

fn box_blur(field: &mut [f32], w: usize, h: usize, r: usize) {
    let mut tmp = vec![0f32; field.len()];
    // horizontal
    for y in 0..h {
        let row = &field[y * w..(y + 1) * w];
        let mut acc: f32 = row[..(r + 1).min(w)].iter().sum::<f32>() + row[0] * r as f32;
        let denom = (2 * r + 1) as f32;
        for x in 0..w {
            tmp[y * w + x] = acc / denom;
            let add = row[(x + r + 1).min(w - 1)];
            let sub = row[x.saturating_sub(r)];
            acc += add - sub;
        }
    }
    // vertical
    for x in 0..w {
        let col = |y: usize| tmp[y * w + x];
        let mut acc: f32 = (0..=r.min(h - 1)).map(col).sum::<f32>() + col(0) * r as f32;
        let denom = (2 * r + 1) as f32;
        for y in 0..h {
            field[y * w + x] = acc / denom;
            let add = col((y + r + 1).min(h - 1));
            let sub = col(y.saturating_sub(r));
            acc += add - sub;
        }
    }
}

/// Marching squares: isolines of `field` at `level`, as polylines in
/// normalized [0,1] coordinates. Segments are stitched into chains.
pub fn marching_squares(field: &[f32], w: usize, h: usize, level: f32) -> Vec<Vec<(f32, f32)>> {
    // collect raw segments
    let mut segments: Vec<((f32, f32), (f32, f32))> = Vec::new();
    let v = |x: usize, y: usize| field[y * w + x];
    let interp = |a: f32, b: f32| -> f32 {
        if (b - a).abs() < 1e-9 {
            0.5
        } else {
            ((level - a) / (b - a)).clamp(0.0, 1.0)
        }
    };
    for y in 0..h.saturating_sub(1) {
        for x in 0..w.saturating_sub(1) {
            let (tl, tr, bl, br) = (v(x, y), v(x + 1, y), v(x, y + 1), v(x + 1, y + 1));
            let mut case = 0u8;
            if tl > level {
                case |= 1;
            }
            if tr > level {
                case |= 2;
            }
            if br > level {
                case |= 4;
            }
            if bl > level {
                case |= 8;
            }
            if case == 0 || case == 15 {
                continue;
            }
            let (xf, yf) = (x as f32, y as f32);
            let top = (xf + interp(tl, tr), yf);
            let bottom = (xf + interp(bl, br), yf + 1.0);
            let left = (xf, yf + interp(tl, bl));
            let right = (xf + 1.0, yf + interp(tr, br));
            let mut push = |a: (f32, f32), b: (f32, f32)| segments.push((a, b));
            match case {
                1 | 14 => push(left, top),
                2 | 13 => push(top, right),
                3 | 12 => push(left, right),
                4 | 11 => push(right, bottom),
                6 | 9 => push(top, bottom),
                7 | 8 => push(left, bottom),
                5 => {
                    push(left, top);
                    push(right, bottom);
                }
                10 => {
                    push(top, right);
                    push(left, bottom);
                }
                _ => {}
            }
        }
    }

    // stitch segments into chains — orientation-agnostic: a segment can be
    // consumed from either endpoint (marching-squares emission order is not
    // consistent around a contour, especially on binary masks)
    let key = |p: (f32, f32)| ((p.0 * 8.0).round() as i64, (p.1 * 8.0).round() as i64);
    let mut by_end: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, &(a, b)) in segments.iter().enumerate() {
        by_end.entry(key(a)).or_default().push(i);
        by_end.entry(key(b)).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut chains = Vec::new();
    for i in 0..segments.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (a, b) = segments[i];
        let mut chain = vec![a, b];
        // grow at the tail, flipping segments as needed
        loop {
            let tail = *chain.last().unwrap();
            let k = key(tail);
            let next = by_end
                .get(&k)
                .and_then(|cands| cands.iter().find(|&&j| !used[j]).copied());
            let Some(j) = next else { break };
            used[j] = true;
            let (sa, sb) = segments[j];
            chain.push(if key(sa) == k { sb } else { sa });
        }
        // grow at the head the same way
        loop {
            let head = chain[0];
            let k = key(head);
            let next = by_end
                .get(&k)
                .and_then(|cands| cands.iter().find(|&&j| !used[j]).copied());
            let Some(j) = next else { break };
            used[j] = true;
            let (sa, sb) = segments[j];
            chain.insert(0, if key(sa) == k { sb } else { sa });
        }
        if chain.len() >= 4 {
            chains.push(
                chain
                    .into_iter()
                    .map(|(x, y)| (x / w as f32, y / h as f32))
                    .collect::<Vec<_>>(),
            );
        }
    }
    chains
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isoline_circles_a_bump() {
        let (w, h) = (32usize, 32usize);
        let mut field = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 - 16.0).powi(2) + (y as f32 - 16.0).powi(2)).sqrt();
                field[y * w + x] = (10.0 - d).max(0.0);
            }
        }
        let lines = marching_squares(&field, w, h, 5.0);
        assert!(!lines.is_empty());
        let total_pts: usize = lines.iter().map(|c| c.len()).sum();
        assert!(
            total_pts > 12,
            "should trace a ring, got {total_pts} points"
        );
    }
}
