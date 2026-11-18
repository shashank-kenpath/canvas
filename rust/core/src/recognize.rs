//! Turns a single hand-drawn pen stroke into the clean shape it resembles:
//! a straight line, an arrow, a rectangle or an ellipse. Marks that look like
//! writing or scribbles return `None` and stay as ink.
//!
//! Ported from `recognizeShape` in the Flutter app's `ink_recognition.dart`;
//! thresholds are kept identical.

use crate::geom::{segment_distance, Pt, Rect};
use crate::shape::{Shape, Tool};
use std::f64::consts::PI;

pub fn recognize_shape(stroke: &Shape) -> Option<Shape> {
    if stroke.tool != Tool::Pen || stroke.points.len() < 2 {
        return None;
    }
    // Recognition is bounded independently of raw sampling rate on touch panels.
    let step = ((stroke.points.len() as f64 / 256.0).ceil() as usize).max(1);
    let mut points: Vec<Pt> = Vec::new();
    let mut i = 0;
    while i < stroke.points.len() {
        let p = stroke.points[i];
        if !p.is_finite() {
            return None;
        }
        if points.last().map_or(true, |l| p.dist(*l) >= 0.5) {
            points.push(p);
        }
        i += step;
    }
    let last_raw = *stroke.points.last().unwrap();
    if *points.last()? != last_raw {
        points.push(last_raw);
    }
    if points.len() < 2 {
        return None;
    }
    let (mut left, mut top) = (points[0].x, points[0].y);
    let (mut right, mut bottom) = (left, top);
    let mut length = 0.0;
    for (i, p) in points.iter().enumerate() {
        left = left.min(p.x);
        right = right.max(p.x);
        top = top.min(p.y);
        bottom = bottom.max(p.y);
        if i > 0 {
            length += p.dist(points[i - 1]);
        }
    }
    let bounds = Rect::ltrb(left, top, right, bottom);
    let (bw, bh) = (bounds.width(), bounds.height());
    let diagonal = (bw * bw + bh * bh).sqrt();
    if diagonal < 24.0 || length < 24.0 {
        return None;
    }
    let (a, b) = (points[0], *points.last().unwrap());
    let chord = a.dist(b);
    let result = |tool: Tool, pts: Vec<Pt>| {
        Some(
            Shape::new(stroke.id.clone(), tool, pts)
                .styled(stroke.color, stroke.width, stroke.filled)
                .with_text(stroke.text.clone(), 24.0),
        )
    };

    // Straight line: little detour and no point far from the chord.
    if chord / length >= 0.90
        && points.iter().all(|p| segment_distance(*p, a, b) <= f64::max(4.0, chord * 0.07))
    {
        return result(Tool::Pen, vec![a, b]);
    }
    if let Some(arrow) = arrow_endpoints(&points, diagonal) {
        return result(Tool::Arrow, arrow);
    }

    // Closed outline: the ends meet, allowing a gap or a small overshoot.
    if chord > f64::max(16.0, diagonal * 0.30) {
        return None;
    }
    if bw < 20.0 || bh < 20.0 {
        return None;
    }
    // Reject retraced scribbles and writing, which cover far more or far less
    // path than one trip around the outline.
    let perimeter = 2.0 * (bw + bh);
    if length < perimeter * 0.55 || length > perimeter * 1.45 {
        return None;
    }

    let center = bounds.center();
    let (rx, ry) = (bw / 2.0, bh / 2.0);
    let (mut area, mut radial_error, mut winding) = (0.0, 0.0, 0.0);
    let mut near_edge = 0usize;
    // How far the outline reaches into each corner of its box, in units of
    // the ellipse radius: about 1 for an ellipse, up to 1.41 for a box.
    let mut reach = [0.0f64; 4];
    let mut previous_angle: Option<f64> = None;
    let edge_band = bw.min(bh) * 0.14 + 2.0;
    let n = points.len();
    for i in 0..n {
        let (p, q) = (points[i], points[(i + 1) % n]);
        area += p.x * q.y - q.x * p.y;
        let edge = f64::min(
            f64::min((p.x - left).abs(), (p.x - right).abs()),
            f64::min((p.y - top).abs(), (p.y - bottom).abs()),
        );
        if edge <= edge_band {
            near_edge += 1;
        }
        let (x, y) = ((p.x - center.x) / rx, (p.y - center.y) / ry);
        let r = (x * x + y * y).sqrt();
        radial_error += (r - 1.0).abs();
        let quadrant = (x >= 0.0) as usize + 2 * (y >= 0.0) as usize;
        reach[quadrant] = reach[quadrant].max(r);
        let angle = y.atan2(x);
        if let Some(prev) = previous_angle {
            let mut delta = angle - prev;
            while delta > PI {
                delta -= 2.0 * PI;
            }
            while delta < -PI {
                delta += 2.0 * PI;
            }
            winding += delta;
        }
        previous_angle = Some(angle);
    }
    // One trip around the centre; figure-eights and spirals are not shapes.
    if winding.abs() < 1.5 * PI || winding.abs() > 2.7 * PI {
        return None;
    }
    // How much of its bounding box the outline fills: a box fills nearly all
    // of it, an ellipse about 79 %, a triangle or diamond only half.
    let fill = area.abs() / 2.0 / (bw * bh);
    let edge_share = near_edge as f64 / n as f64;
    radial_error /= n as f64;
    let corners = vec![bounds.top_left(), bounds.bottom_right()];
    let mut sorted = reach;
    sorted.sort_by(f64::total_cmp);
    // The two smallest: a box reaches into all its corners, a circle into
    // none, and one wobbly corner does not make a circle a box.
    let low_reach = (sorted[0] + sorted[1]) / 2.0;
    let high_reach = (sorted[2] + sorted[3]) / 2.0;
    if low_reach >= 1.18 && fill >= 0.72 && edge_share >= 0.55 {
        return result(Tool::Rectangle, corners);
    }
    if (0.66..=0.86).contains(&fill) && radial_error <= 0.10 && high_reach <= 1.16 {
        return result(Tool::Ellipse, corners);
    }
    if fill >= 0.84 && edge_share >= 0.70 {
        return result(Tool::Rectangle, corners);
    }
    // Neither clearly: leave the ink as drawn.
    None
}

/// Ramer-Douglas-Peucker simplification.
fn simplify(points: &[Pt], tolerance: f64) -> Vec<Pt> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let (mut greatest, mut index) = (0.0, 0);
    for (i, p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = segment_distance(*p, first, last);
        if d > greatest {
            greatest = d;
            index = i;
        }
    }
    if greatest <= tolerance {
        return vec![first, last];
    }
    let mut a = simplify(&points[..=index], tolerance);
    let b = simplify(&points[index..], tolerance);
    a.pop();
    a.extend(b);
    a
}

fn arrow_endpoints(points: &[Pt], diagonal: f64) -> Option<Vec<Pt>> {
    let simple = simplify(points, diagonal * 0.05);
    // Shaft start -> tip -> wing -> (back to tip) -> other wing. Some people
    // go straight from one wing to the other, which leaves four points.
    if simple.len() != 5 && simple.len() != 4 {
        return None;
    }
    let (start, tip, wing_a) = (simple[0], simple[1], simple[2]);
    let wing_b = *simple.last().unwrap();
    if simple.len() == 5 && tip.dist(simple[3]) > diagonal * 0.14 {
        return None;
    }
    let shaft = tip - start;
    let distance = shaft.len();
    if distance < diagonal * 0.60 {
        return None;
    }
    let unit = shaft / distance;
    let (da, db) = (wing_a - tip, wing_b - tip);
    if da.len() < distance * 0.10
        || db.len() < distance * 0.10
        || da.len() > distance * 0.55
        || db.len() > distance * 0.55
    {
        return None;
    }
    let projection_a = -(da.x * unit.x + da.y * unit.y);
    let projection_b = -(db.x * unit.x + db.y * unit.y);
    let cross_a = da.x * unit.y - da.y * unit.x;
    let cross_b = db.x * unit.y - db.y * unit.x;
    if projection_a < distance * 0.05
        || projection_b < distance * 0.05
        || cross_a * cross_b >= 0.0
        || cross_a.abs() < distance * 0.05
        || cross_b.abs() < distance * 0.05
    {
        return None;
    }
    Some(vec![start, tip])
}
