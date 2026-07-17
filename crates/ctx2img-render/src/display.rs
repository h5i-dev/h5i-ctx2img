//! Backend-neutral display list. Themes emit ops; the raster and SVG
//! backends execute them, which is what keeps PNG and SVG output in
//! lockstep. All coordinates are normalized [0,1] × [0,1].

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn opaque(r: u8, g: u8, b: u8) -> Rgba {
        Rgba(r, g, b, 255)
    }
    pub fn hex(&self) -> String {
        if self.3 == 255 {
            format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.0, self.1, self.2, self.3)
        }
    }
    /// Multiply RGB by `f` (clamped), keep alpha.
    pub fn shade(&self, f: f32) -> Rgba {
        let m = |c: u8| ((c as f32 * f).clamp(0.0, 255.0)) as u8;
        Rgba(m(self.0), m(self.1), m(self.2), self.3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    Sans,
    SansBold,
    Serif,
    SerifBold,
    Mono,
}

#[derive(Debug, Clone)]
pub enum Op {
    /// Filled polygon (closed).
    Fill { poly: Vec<(f32, f32)>, color: Rgba },
    /// Stroked path; `closed` joins last→first. `dash` in canvas px.
    Stroke {
        path: Vec<(f32, f32)>,
        color: Rgba,
        width_px: f32,
        closed: bool,
        dash: Option<(f32, f32)>,
    },
    /// Quadratic bezier a → b with control c, optional arrowhead at b.
    Curve {
        a: (f32, f32),
        b: (f32, f32),
        c: (f32, f32),
        color: Rgba,
        width_px: f32,
        dash: Option<(f32, f32)>,
        arrow: bool,
    },
    Circle {
        center: (f32, f32),
        r_px: f32,
        fill: Option<Rgba>,
        stroke: Option<(Rgba, f32)>,
    },
    /// Single-line text. `size_px` is the cap height target in canvas px.
    Text {
        pos: (f32, f32),
        text: String,
        size_px: f32,
        color: Rgba,
        font: FontKind,
        align: TextAlign,
        halo: Option<Rgba>,
    },
    /// Diagonal hatch lines clipped to a polygon (hazard overlay).
    Hatch {
        poly: Vec<(f32, f32)>,
        color: Rgba,
        spacing_px: f32,
        width_px: f32,
    },
}

#[derive(Debug, Default, Clone)]
pub struct DisplayList {
    pub ops: Vec<Op>,
}

impl DisplayList {
    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }
    pub fn extend(&mut self, other: DisplayList) {
        self.ops.extend(other.ops);
    }
}
