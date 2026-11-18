//! Canvas drawing for formula layouts and graph shapes.

use super::{draw_label, f, line_path, paint_for, round_rect_path, round_stroke, Target};
use crate::geom::{Pt, Rect};
use crate::math::{self, Mark};
use crate::{text, Shape};
use tiny_skia::{FillRule, Path, PathBuilder, Transform};

fn bracket_path(bounds: Rect, kind: char, right: bool) -> Option<Path> {
    let mut path = PathBuilder::new();
    let (y, h) = (bounds.t as f32, bounds.height() as f32);
    let (outer, inner) = if right { (bounds.r, bounds.l) } else { (bounds.l, bounds.r) };
    let (outer, inner) = (outer as f32, inner as f32);
    path.move_to(inner, y);
    match kind {
        '[' => {
            path.line_to(outer, y);
            path.line_to(outer, y + h);
            path.line_to(inner, y + h);
        }
        '{' => {
            path.cubic_to(outer, y, inner, y + h * 0.43, outer, y + h * 0.5);
            path.cubic_to(inner, y + h * 0.57, outer, y + h, inner, y + h);
        }
        _ => path.cubic_to(outer, y + h * 0.2, outer, y + h * 0.8, inner, y + h),
    }
    path.finish()
}

pub(super) fn draw_formula(target: &mut Target, shape: &Shape, alpha: f32, transform: Transform) {
    let origin = shape.points[0];
    let layout = math::formula_layout(&shape.text, shape.font_size, shape.font);
    let color = |accent| if accent { math::ACCENT } else { shape.color };
    for mark in &layout.marks {
        let (path, width, accent) = match mark {
            Mark::Text { text, at, size, accent } => {
                draw_label(target, (text, origin + *at, *size), (shape.font, color(*accent)), alpha, transform, false);
                continue;
            }
            Mark::Line { from, to, width, accent } => (line_path(origin + *from, origin + *to), *width, *accent),
            Mark::Bracket { bounds, kind, right, width, accent } => {
                (bracket_path(bounds.shift(origin), *kind, *right), *width, *accent)
            }
        };
        if let Some(path) = path {
            target.pm.stroke_path(&path, &paint_for(color(accent), alpha), &round_stroke(width), transform, None);
        }
    }
}

fn line(path: &mut PathBuilder, a: Pt, b: Pt) {
    let (x, y) = f(a);
    path.move_to(x, y);
    let (x, y) = f(b);
    path.line_to(x, y);
}

fn draw_path(target: &mut Target, path: PathBuilder, style: (u32, f64), alpha: f32, transform: Transform) {
    if let Some(path) = path.finish() {
        target.pm.stroke_path(&path, &paint_for(style.0, alpha), &round_stroke(style.1), transform, None);
    }
}

const CURVE_COLORS: [u32; 7] = [0xffc62828, 0xff00856f, 0xff872dbc, 0xff9c510c, 0xff077ba3, 0xff245817, 0xff5d547c];

pub(super) fn draw_graph(target: &mut Target, shape: &Shape, alpha: f32, transform: Transform) {
    let bounds = shape.bounds();
    if bounds.shortest_side() <= 0.0 {
        return;
    }
    let [xmin, xmax] = shape.graph_range.x();
    let [ymin, ymax] = shape.graph_range.y();
    let size = bounds.shortest_side() * 0.055;
    let label_style = (shape.font, 0xff4a5160);
    let yticks = math::ticks([ymin, ymax], 5.0);
    let ylabels: Vec<_> = yticks.iter().map(|&y| math::tick_label(y)).collect();
    let left = ylabels.iter().map(|label| text::measure(label, size, shape.font).0).fold(0.0, f64::max)
        .min(bounds.width() * 0.28) + size * 0.72;
    let plot = Rect::ltrb(bounds.l + left, bounds.t + size * 0.75, bounds.r - size * 0.9, bounds.b - size * 1.7);
    let place = |p: Pt| Pt::new(plot.l + p.x * plot.width(), plot.t + p.y * plot.height());
    let px = |x: f64| place(Pt::new((x - xmin) / (xmax - xmin), 0.0)).x;
    let py = |y: f64| place(Pt::new(0.0, (ymax - y) / (ymax - ymin))).y;
    let region_stroke = size * 0.075;
    let Some(paper) = round_rect_path(&bounds, size * 0.3) else { return };
    target.pm.fill_path(&paper, &paint_for(0xffffffff, alpha), FillRule::Winding, transform, None);
    target.pm.stroke_path(&paper, &paint_for(0xffe1e4ea, alpha), &round_stroke(region_stroke), transform, None);
    let (mut grid, mut axes) = (PathBuilder::new(), PathBuilder::new());
    let mut previous_right = bounds.l;
    let x_target = (plot.width() / (size * 3.8)).clamp(3.0, 8.0);
    for x in math::ticks([xmin, xmax], x_target) {
        let cx = px(x).clamp(plot.l, plot.r);
        line(if x == 0.0 { &mut axes } else { &mut grid }, Pt::new(cx, plot.t), Pt::new(cx, plot.b));
        let label = math::tick_label(x);
        let width = text::measure(&label, size, shape.font).0;
        let left = (cx - width * 0.5).max(bounds.l + size * 0.25);
        if left >= previous_right && left + width + size * 0.25 < bounds.r {
            draw_label(target, (&label, Pt::new(left, plot.b + size * 0.23), size), label_style, alpha, transform, false);
            previous_right = left + width + size * 0.44;
        }
    }
    for (y, label) in yticks.into_iter().zip(ylabels) {
        let cy = py(y).clamp(plot.t, plot.b);
        line(if y == 0.0 { &mut axes } else { &mut grid }, Pt::new(plot.l, cy), Pt::new(plot.r, cy));
        let (width, height) = text::measure(&label, size, shape.font);
        let x = plot.l - size * 0.55 - width;
        if x >= bounds.l + size * 0.07 {
            draw_label(target, (&label, Pt::new(x, cy - height / 2.0), size), label_style, alpha, transform, false);
        }
    }
    if let Some(rect) = round_rect_path(&plot, 0.0) {
        grid.push_path(&rect);
    }
    draw_path(target, grid, (0xffe1e4ea, region_stroke), alpha, transform);
    draw_path(target, axes, (0xff9fa8b2, region_stroke * 1.4), alpha, transform);
    let curves = math::curves(&shape.text, shape.graph_range);
    for (i, segments) in curves.iter().enumerate() {
        let mut path = PathBuilder::new();
        let mut previous = None;
        for &[a, b] in segments {
            if previous != Some(a) {
                let (x, y) = f(place(a));
                path.move_to(x, y);
            }
            let (x, y) = f(place(b));
            path.line_to(x, y);
            previous = Some(b);
        }
        let color = if i == 0 { shape.color } else { CURVE_COLORS[i - 1] };
        draw_path(target, path, (color, size * 0.15), alpha, transform);
    }
}

#[cfg(test)]
mod tests {
    use crate::render::thumbnail;
    use crate::{Pt, Shape, Tool};
    use std::sync::Arc;

    #[test]
    fn thumbnail_draws_graph_curves_and_accent_result_rules() {
        let formula = Arc::new(Shape::new("formula", Tool::Formula, vec![Pt::new(0.0, 0.0)])
            .with_text("1/4 + 1/4 =", 55.0));
        let graph = Arc::new(Shape::new("graph", Tool::Graph, vec![Pt::new(0.0, 150.0), Pt::new(300.0, 350.0)])
            .styled(crate::math::ACCENT, 3.0, false).with_text("y = x", 24.0));
        for shape in [formula, graph] {
            let pixels = thumbnail(&[shape], 400, 300);
            assert!(pixels.chunks_exact(4).any(|rgba| rgba[2] > 200 && rgba[0] < 80 && rgba[1] < 110));
        }
    }
}
