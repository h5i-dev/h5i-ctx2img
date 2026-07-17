//! SVG backend: the same display list, serialized as standalone SVG.
//! Vector output for humans (infinite zoom, README embeds).

use crate::display::{DisplayList, FontKind, Op, Rgba, TextAlign};

pub fn render(list: &DisplayList, width: u32, height: u32, background: Rgba) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    ));
    s.push_str(&format!(
        "<rect width=\"{width}\" height=\"{height}\" fill=\"{}\"/>\n",
        background.hex()
    ));
    let (w, h) = (width as f32, height as f32);
    let mut clip_n = 0usize;
    for op in &list.ops {
        emit(&mut s, op, w, h, &mut clip_n);
    }
    s.push_str("</svg>\n");
    s
}

fn pt(p: (f32, f32), w: f32, h: f32) -> (f32, f32) {
    (p.0 * w, p.1 * h)
}

fn path_d(pts: &[(f32, f32)], closed: bool, w: f32, h: f32) -> String {
    let mut d = String::new();
    for (i, &p) in pts.iter().enumerate() {
        let (x, y) = pt(p, w, h);
        d.push_str(&format!(
            "{}{:.1} {:.1} ",
            if i == 0 { "M" } else { "L" },
            x,
            y
        ));
    }
    if closed {
        d.push('Z');
    }
    d
}

fn dash_attr(dash: Option<(f32, f32)>) -> String {
    dash.map(|(on, off)| format!(" stroke-dasharray=\"{on:.1} {off:.1}\""))
        .unwrap_or_default()
}

fn font_family(kind: FontKind) -> &'static str {
    match kind {
        FontKind::Sans | FontKind::SansBold => "'DejaVu Sans',Verdana,sans-serif",
        FontKind::Serif | FontKind::SerifBold => "'DejaVu Serif',Georgia,serif",
        FontKind::Mono => "'DejaVu Sans Mono',monospace",
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn emit(s: &mut String, op: &Op, w: f32, h: f32, clip_n: &mut usize) {
    match op {
        Op::Fill { poly, color } => {
            s.push_str(&format!(
                "<path d=\"{}\" fill=\"{}\"/>\n",
                path_d(poly, true, w, h),
                color.hex()
            ));
        }
        Op::Stroke {
            path,
            color,
            width_px,
            closed,
            dash,
        } => {
            s.push_str(&format!(
                "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\"{}/>\n",
                path_d(path, *closed, w, h),
                color.hex(),
                width_px,
                dash_attr(*dash)
            ));
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
            let (ax, ay) = pt(*a, w, h);
            let (bx, by) = pt(*b, w, h);
            let (cx, cy) = pt(*c, w, h);
            s.push_str(&format!(
                "<path d=\"M{ax:.1} {ay:.1} Q{cx:.1} {cy:.1} {bx:.1} {by:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{width_px:.1}\"{}/>\n",
                color.hex(),
                dash_attr(*dash)
            ));
            if *arrow {
                let (dx, dy) = (bx - cx, by - cy);
                let len = (dx * dx + dy * dy).sqrt().max(1e-3);
                let (ux, uy) = (dx / len, dy / len);
                let (px, py) = (-uy, ux);
                let sz = (width_px * 3.0).max(5.0);
                s.push_str(&format!(
                    "<path d=\"M{bx:.1} {by:.1} L{:.1} {:.1} L{:.1} {:.1} Z\" fill=\"{}\"/>\n",
                    bx - ux * sz + px * sz * 0.55,
                    by - uy * sz + py * sz * 0.55,
                    bx - ux * sz - px * sz * 0.55,
                    by - uy * sz - py * sz * 0.55,
                    color.hex()
                ));
            }
        }
        Op::Circle {
            center,
            r_px,
            fill,
            stroke,
        } => {
            let (cx, cy) = pt(*center, w, h);
            let fill_attr = fill.map(|f| f.hex()).unwrap_or_else(|| "none".into());
            let stroke_attr = stroke
                .map(|(c, sw)| format!(" stroke=\"{}\" stroke-width=\"{sw:.1}\"", c.hex()))
                .unwrap_or_default();
            s.push_str(&format!(
                "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r_px:.1}\" fill=\"{fill_attr}\"{stroke_attr}/>\n"
            ));
        }
        Op::Text {
            pos,
            text,
            size_px,
            color,
            font,
            align,
            halo,
        } => {
            let (x, y) = pt(*pos, w, h);
            let anchor = match align {
                TextAlign::Left => "start",
                TextAlign::Center => "middle",
            };
            let weight = matches!(font, FontKind::SansBold | FontKind::SerifBold)
                .then_some(" font-weight=\"bold\"")
                .unwrap_or("");
            let halo_attr = halo
                .map(|hc| {
                    format!(
                        " stroke=\"{}\" stroke-width=\"2.5\" paint-order=\"stroke\" stroke-linejoin=\"round\"",
                        hc.hex()
                    )
                })
                .unwrap_or_default();
            s.push_str(&format!(
                "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"{}\" font-size=\"{size_px:.1}\" fill=\"{}\" text-anchor=\"{anchor}\"{weight}{halo_attr}>{}</text>\n",
                font_family(*font),
                color.hex(),
                escape(text)
            ));
        }
        Op::Hatch {
            poly,
            color,
            spacing_px,
            width_px,
        } => {
            let id = format!("hatch{clip_n}");
            *clip_n += 1;
            s.push_str(&format!(
                "<clipPath id=\"{id}\"><path d=\"{}\"/></clipPath>\n<g clip-path=\"url(#{id})\">\n",
                path_d(poly, true, w, h)
            ));
            let step = spacing_px.max(3.0);
            let mut t = -h;
            while t < w {
                s.push_str(&format!(
                    "<line x1=\"{t:.0}\" y1=\"0\" x2=\"{:.0}\" y2=\"{h:.0}\" stroke=\"{}\" stroke-width=\"{width_px:.1}\"/>\n",
                    t + h,
                    color.hex()
                ));
                t += step;
            }
            s.push_str("</g>\n");
        }
    }
}
