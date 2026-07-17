//! Raster backend: display list → tiny-skia pixmap → PNG bytes.

use crate::display::{DisplayList, FontKind, Op, Rgba, TextAlign};
use crate::text;
use anyhow::{Context, Result};
use tiny_skia::{
    FillRule, LineCap, Mask, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

pub struct Raster {
    pub pixmap: Pixmap,
}

impl Raster {
    pub fn new(width: u32, height: u32, background: Rgba) -> Result<Raster> {
        let mut pixmap = Pixmap::new(width, height).context("pixmap alloc")?;
        pixmap.fill(to_color(background));
        Ok(Raster { pixmap })
    }

    pub fn png(&self) -> Result<Vec<u8>> {
        self.pixmap.encode_png().context("png encode")
    }

    fn w(&self) -> f32 {
        self.pixmap.width() as f32
    }
    fn h(&self) -> f32 {
        self.pixmap.height() as f32
    }

    pub fn execute(&mut self, list: &DisplayList) {
        for op in &list.ops {
            self.execute_op(op);
        }
    }

    fn execute_op(&mut self, op: &Op) {
        match op {
            Op::Fill { poly, color } => {
                if let Some(path) = self.poly_path(poly, true) {
                    let paint = paint(*color);
                    self.pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            Op::Stroke {
                path,
                color,
                width_px,
                closed,
                dash,
            } => {
                if let Some(p) = self.poly_path(path, *closed) {
                    let paint = paint(*color);
                    let stroke = make_stroke(*width_px, *dash);
                    self.pixmap
                        .stroke_path(&p, &paint, &stroke, Transform::identity(), None);
                }
            }
            Op::Curve {
                a,
                b,
                c,
                color,
                width_px,
                dash,
                arrow,
            } => {
                let (ax, ay) = self.abs(*a);
                let (bx, by) = self.abs(*b);
                let (cx, cy) = self.abs(*c);
                let mut pb = PathBuilder::new();
                pb.move_to(ax, ay);
                pb.quad_to(cx, cy, bx, by);
                if let Some(p) = pb.finish() {
                    let paint = paint(*color);
                    let stroke = make_stroke(*width_px, *dash);
                    self.pixmap
                        .stroke_path(&p, &paint, &stroke, Transform::identity(), None);
                }
                if *arrow {
                    // small triangle at b, oriented along c→b
                    let (dx, dy) = (bx - cx, by - cy);
                    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
                    let (ux, uy) = (dx / len, dy / len);
                    let (px, py) = (-uy, ux);
                    let s = (width_px * 3.0).max(5.0);
                    let mut pb = PathBuilder::new();
                    pb.move_to(bx, by);
                    pb.line_to(bx - ux * s + px * s * 0.55, by - uy * s + py * s * 0.55);
                    pb.line_to(bx - ux * s - px * s * 0.55, by - uy * s - py * s * 0.55);
                    pb.close();
                    if let Some(p) = pb.finish() {
                        self.pixmap.fill_path(
                            &p,
                            &paint(*color),
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            Op::Circle {
                center,
                r_px,
                fill,
                stroke,
            } => {
                let (cx, cy) = self.abs(*center);
                if let Some(path) = PathBuilder::from_circle(cx, cy, r_px.max(0.5)) {
                    if let Some(fc) = fill {
                        self.pixmap.fill_path(
                            &path,
                            &paint(*fc),
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                    if let Some((sc, sw)) = stroke {
                        self.pixmap.stroke_path(
                            &path,
                            &paint(*sc),
                            &make_stroke(*sw, None),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            Op::Text {
                pos,
                text: s,
                size_px,
                color,
                font,
                align,
                halo,
            } => {
                let (mut x, y) = self.abs(*pos);
                if *align == TextAlign::Center {
                    x -= text::measure(s, *size_px, *font) / 2.0;
                }
                if let Some(hc) = halo {
                    for (dx, dy) in [
                        (-1i32, 0i32),
                        (1, 0),
                        (0, -1),
                        (0, 1),
                        (-1, -1),
                        (1, 1),
                        (-1, 1),
                        (1, -1),
                    ] {
                        self.blit_text(s, *size_px, *font, x + dx as f32, y + dy as f32, *hc);
                    }
                }
                self.blit_text(s, *size_px, *font, x, y, *color);
            }
            Op::Hatch {
                poly,
                color,
                spacing_px,
                width_px,
            } => {
                let Some(path) = self.poly_path(poly, true) else {
                    return;
                };
                let mut mask = Mask::new(self.pixmap.width(), self.pixmap.height()).unwrap();
                mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
                // diagonal lines across the whole canvas, clipped by mask
                let paint_ = paint(*color);
                let stroke = make_stroke(*width_px, None);
                let (w, h) = (self.w(), self.h());
                let step = spacing_px.max(3.0);
                let mut t = -h;
                while t < w {
                    let mut pb = PathBuilder::new();
                    pb.move_to(t, 0.0);
                    pb.line_to(t + h, h);
                    if let Some(p) = pb.finish() {
                        self.pixmap.stroke_path(
                            &p,
                            &paint_,
                            &stroke,
                            Transform::identity(),
                            Some(&mask),
                        );
                    }
                    t += step;
                }
            }
        }
    }

    fn abs(&self, p: (f32, f32)) -> (f32, f32) {
        (p.0 * self.w(), p.1 * self.h())
    }

    fn poly_path(&self, pts: &[(f32, f32)], closed: bool) -> Option<tiny_skia::Path> {
        if pts.len() < 2 {
            return None;
        }
        let mut pb = PathBuilder::new();
        let (x0, y0) = self.abs(pts[0]);
        pb.move_to(x0, y0);
        for &p in &pts[1..] {
            let (x, y) = self.abs(p);
            pb.line_to(x, y);
        }
        if closed {
            pb.close();
        }
        pb.finish()
    }

    fn blit_text(&mut self, s: &str, size_px: f32, font: FontKind, x: f32, y: f32, color: Rgba) {
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let data = self.pixmap.data_mut();
        text::rasterize(s, size_px, font, |gx, gy, cov| {
            let px = x as i32 + gx;
            let py = y as i32 + gy;
            if px < 0 || py < 0 || px >= w || py >= h {
                return;
            }
            let idx = ((py * w + px) * 4) as usize;
            let a = cov as f32 / 255.0 * color.3 as f32 / 255.0;
            // src-over onto premultiplied RGBA
            let (sr, sg, sb) = (
                color.0 as f32 / 255.0 * a,
                color.1 as f32 / 255.0 * a,
                color.2 as f32 / 255.0 * a,
            );
            let inv = 1.0 - a;
            data[idx] = ((sr + data[idx] as f32 / 255.0 * inv) * 255.0) as u8;
            data[idx + 1] = ((sg + data[idx + 1] as f32 / 255.0 * inv) * 255.0) as u8;
            data[idx + 2] = ((sb + data[idx + 2] as f32 / 255.0 * inv) * 255.0) as u8;
            data[idx + 3] = ((a + data[idx + 3] as f32 / 255.0 * inv) * 255.0) as u8;
        });
    }

    /// Multiply the RGB of every pixel by a per-pixel factor (hillshade,
    /// paper grain). `f(nx, ny)` gets normalized coordinates.
    pub fn multiply(&mut self, f: impl Fn(f32, f32) -> f32) {
        let (w, h) = (self.pixmap.width() as usize, self.pixmap.height() as usize);
        let data = self.pixmap.data_mut();
        for y in 0..h {
            let ny = (y as f32 + 0.5) / h as f32;
            for x in 0..w {
                let nx = (x as f32 + 0.5) / w as f32;
                let m = f(nx, ny).clamp(0.0, 2.0);
                let idx = (y * w + x) * 4;
                for c in 0..3 {
                    data[idx + c] = ((data[idx + c] as f32) * m).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

fn to_color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.0, c.1, c.2, c.3)
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(to_color(c));
    p.anti_alias = true;
    p
}

fn make_stroke(width: f32, dash: Option<(f32, f32)>) -> Stroke {
    Stroke {
        width: width.max(0.4),
        line_cap: LineCap::Round,
        dash: dash.and_then(|(on, off)| StrokeDash::new(vec![on, off], 0.0)),
        ..Default::default()
    }
}
