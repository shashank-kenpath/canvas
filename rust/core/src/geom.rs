//! Points and rectangles in board ("world") or screen coordinates.
//! Semantics follow Flutter's `Offset` / `Rect` so ported logic behaves the same.

use std::ops::{Add, Div, Mul, Sub};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const ZERO: Pt = Pt { x: 0.0, y: 0.0 };
    pub fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }
    pub fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    pub fn len2(self) -> f64 {
        self.x * self.x + self.y * self.y
    }
    pub fn dist(self, o: Pt) -> f64 {
        (self - o).len()
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Pt {
    type Output = Pt;
    fn mul(self, k: f64) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}
impl Div<f64> for Pt {
    type Output = Pt;
    fn div(self, k: f64) -> Pt {
        Pt::new(self.x / k, self.y / k)
    }
}

/// Distance from `p` to the segment `a`-`b`.
pub fn segment_distance(p: Pt, a: Pt, b: Pt) -> f64 {
    let d = b - a;
    let l2 = d.len2();
    if l2 == 0.0 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0);
    p.dist(a + d * t)
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

impl Rect {
    pub const ZERO: Rect = Rect { l: 0.0, t: 0.0, r: 0.0, b: 0.0 };
    pub fn ltrb(l: f64, t: f64, r: f64, b: f64) -> Rect {
        Rect { l, t, r, b }
    }
    pub fn from_points(a: Pt, b: Pt) -> Rect {
        Rect { l: a.x.min(b.x), t: a.y.min(b.y), r: a.x.max(b.x), b: a.y.max(b.y) }
    }
    pub fn width(&self) -> f64 {
        self.r - self.l
    }
    pub fn height(&self) -> f64 {
        self.b - self.t
    }
    pub fn center(&self) -> Pt {
        Pt::new((self.l + self.r) / 2.0, (self.t + self.b) / 2.0)
    }
    pub fn top_left(&self) -> Pt {
        Pt::new(self.l, self.t)
    }
    pub fn top_right(&self) -> Pt {
        Pt::new(self.r, self.t)
    }
    pub fn bottom_left(&self) -> Pt {
        Pt::new(self.l, self.b)
    }
    pub fn bottom_right(&self) -> Pt {
        Pt::new(self.r, self.b)
    }
    pub fn inflate(&self, d: f64) -> Rect {
        Rect::ltrb(self.l - d, self.t - d, self.r + d, self.b + d)
    }
    pub fn shift(&self, d: Pt) -> Rect {
        Rect::ltrb(self.l + d.x, self.t + d.y, self.r + d.x, self.b + d.y)
    }
    /// Half-open like Flutter: the right and bottom edges are outside.
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.l && p.x < self.r && p.y >= self.t && p.y < self.b
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        !(self.r <= o.l || o.r <= self.l || self.b <= o.t || o.b <= self.t)
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect::ltrb(self.l.min(o.l), self.t.min(o.t), self.r.max(o.r), self.b.max(o.b))
    }
    pub fn longest_side(&self) -> f64 {
        self.width().abs().max(self.height().abs())
    }
    pub fn shortest_side(&self) -> f64 {
        self.width().abs().min(self.height().abs())
    }
}

/// Integer pixel rectangle, half-open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
        IRect { x0, y0, x1, y1 }
    }
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        IRect::new(self.x0.min(o.x0), self.y0.min(o.y0), self.x1.max(o.x1), self.y1.max(o.y1))
    }
    pub fn intersect(&self, o: &IRect) -> IRect {
        IRect::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1))
    }
    pub fn intersects(&self, o: &IRect) -> bool {
        !self.intersect(o).is_empty()
    }
    /// Smallest pixel rectangle covering `r`, grown by `pad` pixels.
    pub fn covering(r: &Rect, pad: f64) -> IRect {
        let clamp = |v: f64| v.clamp(-1.0e9, 1.0e9) as i32;
        IRect::new(
            clamp((r.l - pad).floor()),
            clamp((r.t - pad).floor()),
            clamp((r.r + pad).ceil()),
            clamp((r.b + pad).ceil()),
        )
    }
}
