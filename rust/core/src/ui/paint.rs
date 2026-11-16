//! Drawing helpers for the app's own controls: rounded panels, soft shadows,
//! text, Material icons, pictures and the brand mark.

use super::font::{self, Face};
use crate::geom::IRect;
use tiny_skia::{
    FillRule, Paint, Path, PathBuilder, PixmapMut, PixmapPaint, PixmapRef, Rect, Transform,
};

/// A rectangle in screen pixels.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rf {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rf {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rf {
        Rf { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn inset(&self, d: f32) -> Rf {
        Rf::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
    /// Whole pixels covering the rectangle grown by `pad`.
    pub fn covering(&self, pad: f32) -> IRect {
        IRect::new(
            (self.x - pad).floor() as i32,
            (self.y - pad).floor() as i32,
            (self.right() + pad).ceil() as i32,
            (self.bottom() + pad).ceil() as i32,
        )
    }
}

/// Paints into a pixel buffer that covers the screen area starting at
/// (`ox`, `oy`). Callers use screen coordinates.
pub struct Painter<'a> {
    pub pm: PixmapMut<'a>,
    pub ox: f32,
    pub oy: f32,
}

fn paint(color: u32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(
        (color >> 16) as u8,
        (color >> 8) as u8,
        color as u8,
        (color >> 24) as u8,
    );
    p.anti_alias = true;
    p
}

fn rrect_path(r: Rf, radius: f32) -> Option<Path> {
    if r.w <= 0.0 || r.h <= 0.0 {
        return None;
    }
    let rad = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    if rad <= 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(r.x, r.y, r.w, r.h)?));
    }
    // Circular corners from cubic curves.
    let k = rad * 0.447_715;
    let (l, t, rt, b) = (r.x, r.y, r.right(), r.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(l + rad, t);
    pb.line_to(rt - rad, t);
    pb.cubic_to(rt - k, t, rt, t + k, rt, t + rad);
    pb.line_to(rt, b - rad);
    pb.cubic_to(rt, b - k, rt - k, b, rt - rad, b);
    pb.line_to(l + rad, b);
    pb.cubic_to(l + k, b, l, b - k, l, b - rad);
    pb.line_to(l, t + rad);
    pb.cubic_to(l, t + k, l + k, t, l + rad, t);
    pb.close();
    pb.finish()
}

impl<'a> Painter<'a> {
    fn tf(&self) -> Transform {
        Transform::from_translate(-self.ox, -self.oy)
    }

    fn clip(&self) -> IRect {
        IRect::new(0, 0, self.pm.width() as i32, self.pm.height() as i32)
    }

    pub fn fill(&mut self, color: u32) {
        let (r, g, b, a) = (
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
            (color >> 24) as u8,
        );
        self.pm.fill(tiny_skia::Color::from_rgba8(r, g, b, a));
    }

    pub fn rrect(&mut self, r: Rf, radius: f32, color: u32) {
        if color >> 24 == 0 {
            return;
        }
        if let Some(path) = rrect_path(r, radius) {
            let tf = self.tf();
            self.pm
                .fill_path(&path, &paint(color), FillRule::Winding, tf, None);
        }
    }

    /// A rounded outline drawn just inside `r`.
    pub fn rrect_line(&mut self, r: Rf, radius: f32, color: u32, width: f32) {
        let h = width / 2.0;
        if let Some(path) = rrect_path(r.inset(h), (radius - h).max(0.0)) {
            let stroke = tiny_skia::Stroke {
                width,
                ..Default::default()
            };
            let tf = self.tf();
            self.pm.stroke_path(&path, &paint(color), &stroke, tf, None);
        }
    }

    /// A soft shadow under a raised surface, like a Material elevation.
    pub fn shadow(&mut self, r: Rf, radius: f32, d: f32, level: f32) {
        for (grow, dy, alpha) in [
            (0.0, 1.0, 0x16u32),
            (1.0, 1.5, 0x0d),
            (2.2, 2.2, 0x07),
            (3.6, 3.0, 0x04),
        ] {
            let g = grow * d * level;
            let a = ((alpha as f32) * level.min(2.0)).min(255.0) as u32;
            let s = Rf::new(
                r.x - g,
                r.y - g + dy * d * level,
                r.w + 2.0 * g,
                r.h + 2.0 * g,
            );
            self.rrect(s, radius + g, a << 24);
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: u32) {
        if let Some(path) = PathBuilder::from_circle(cx, cy, radius) {
            let tf = self.tf();
            self.pm
                .fill_path(&path, &paint(color), FillRule::Winding, tf, None);
        }
    }

    pub fn circle_line(&mut self, cx: f32, cy: f32, radius: f32, color: u32, width: f32) {
        if let Some(path) = PathBuilder::from_circle(cx, cy, radius - width / 2.0) {
            let stroke = tiny_skia::Stroke {
                width,
                ..Default::default()
            };
            let tf = self.tf();
            self.pm.stroke_path(&path, &paint(color), &stroke, tf, None);
        }
    }

    /// Draws one line of text; returns its advance.
    pub fn text(
        &mut self,
        face: Face,
        size: f32,
        text: &str,
        x: f32,
        baseline: f32,
        color: u32,
    ) -> f32 {
        let clip = self.clip();
        font::draw(
            &mut self.pm,
            face,
            size,
            text,
            x - self.ox,
            baseline - self.oy,
            color,
            clip,
        )
    }

    /// Draws one line of text cut with "…" to fit `max_width`.
    pub fn text_fit(
        &mut self,
        face: Face,
        size: f32,
        text: &str,
        x: f32,
        baseline: f32,
        max_width: f32,
        color: u32,
    ) -> f32 {
        let clip = self.clip();
        font::draw_ellipsized(
            &mut self.pm,
            face,
            size,
            text,
            x - self.ox,
            baseline - self.oy,
            max_width,
            color,
            clip,
        )
    }

    pub fn icon(&mut self, code: u32, size: f32, cx: f32, cy: f32, color: u32) {
        let clip = self.clip();
        font::draw_icon(
            &mut self.pm,
            code,
            size,
            cx - self.ox,
            cy - self.oy,
            color,
            clip,
        );
    }

    /// Draws a premultiplied RGBA picture with its top left at (x, y).
    pub fn image(&mut self, rgba: &[u8], w: u32, h: u32, x: f32, y: f32) {
        if let Some(src) = PixmapRef::from_bytes(rgba, w, h) {
            let (px, py) = ((x - self.ox).round() as i32, (y - self.oy).round() as i32);
            self.pm.draw_pixmap(
                px,
                py,
                src,
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }

    /// Canvas Studio's paper-and-pencil mark (brand_logo.dart), filling `r`.
    pub fn logo(&mut self, r: Rf) {
        let s = r.w / 108.0;
        let tf = Transform::from_row(s, 0.0, 0.0, s, r.x - self.ox, r.y - self.oy);
        let fill = |pm: &mut PixmapMut, path: Option<Path>, color: u32| {
            if let Some(path) = path {
                pm.fill_path(&path, &paint(color), FillRule::Winding, tf, None);
            }
        };
        let poly = |pts: &[(f32, f32)]| {
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].0, pts[0].1);
            for p in &pts[1..] {
                pb.line_to(p.0, p.1);
            }
            pb.close();
            pb.finish()
        };
        fill(
            &mut self.pm,
            rrect_path(Rf::new(0.0, 0.0, 108.0, 108.0), 25.0),
            0xff6750d8,
        );
        fill(
            &mut self.pm,
            rrect_path(Rf::new(28.0, 23.0, 52.0, 64.0), 8.0),
            0xffffffff,
        );
        fill(
            &mut self.pm,
            rrect_path(Rf::new(38.0, 36.0, 23.0, 4.0), 2.0),
            0xffe7e0ff,
        );
        fill(
            &mut self.pm,
            rrect_path(Rf::new(38.0, 46.0, 13.0, 4.0), 2.0),
            0xffe7e0ff,
        );
        fill(
            &mut self.pm,
            poly(&[(40.0, 61.0), (64.0, 37.0), (76.0, 49.0), (52.0, 73.0)]),
            0xffff8966,
        );
        let cap = {
            let mut pb = PathBuilder::new();
            pb.move_to(64.0, 37.0);
            pb.line_to(68.0, 33.0);
            pb.quad_to(72.0, 29.0, 76.0, 33.0);
            pb.line_to(80.0, 37.0);
            pb.quad_to(84.0, 41.0, 80.0, 45.0);
            pb.line_to(76.0, 49.0);
            pb.close();
            pb.finish()
        };
        fill(&mut self.pm, cap, 0xffffb39b);
        fill(
            &mut self.pm,
            poly(&[(44.0, 65.0), (68.0, 41.0), (72.0, 45.0), (48.0, 69.0)]),
            0xffffa184,
        );
        fill(
            &mut self.pm,
            poly(&[(36.0, 77.0), (40.0, 61.0), (52.0, 73.0)]),
            0xffffd8aa,
        );
        fill(
            &mut self.pm,
            poly(&[(36.0, 77.0), (38.0, 69.0), (44.0, 75.0)]),
            0xff453587,
        );
    }
}

/// Splits `text` into lines no wider than `width`, breaking at spaces.
pub fn wrap(face: Face, size: f32, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && font::measure(face, size, &candidate).0 > width {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}
