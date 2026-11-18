use super::{draw_text_at, paint_for, round_rect_path, round_stroke, Target};
use crate::geom::{Pt, Rect};
use crate::mindmap;
use crate::shape::{Shape, Tool};
use crate::table::Cell;
use crate::text;
use tiny_skia::{FillRule, PathBuilder, Transform};

fn draw_box_text(t: &mut Target, s: &Shape, content: &str, area: Rect, alpha: f32, ts: Transform) {
    if content.is_empty() || area.shortest_side() <= 0.0 {
        return;
    }
    let (w, h) = text::measure(content, s.font_size, s.font);
    let fit = (area.width() / w.max(1.0)).min(area.height() / h.max(1.0)).min(1.0);
    let origin = if s.tool == Tool::MindNode {
        area.center() - Pt::new(w, h) * (fit / 2.0)
    } else {
        area.top_left()
    };
    draw_text_at(t, s, (content, origin, s.font_size * fit), alpha, ts, false);
}

pub(super) fn draw_table(t: &mut Target, s: &Shape, alpha: f32, ts: Transform) {
    let Some(table) = s.table.as_deref() else { return };
    let b = s.bounds();
    if b.shortest_side() <= 0.0 {
        return;
    }
    let Some(outline) = round_rect_path(&b, 0.0) else { return };
    t.pm.fill_path(&outline, &paint_for(0xffffffff, alpha), FillRule::Winding, ts, None);
    if s.filled {
        t.pm.fill_path(&outline, &paint_for(s.color, alpha * 0.12), FillRule::Winding, ts, None);
    }
    let size = table.cell_size(b);
    let mut grid = PathBuilder::new();
    grid.push_path(&outline);
    for column in 1..table.columns() {
        let x = (b.l + column as f64 * size.x) as f32;
        grid.move_to(x, b.t as f32);
        grid.line_to(x, b.b as f32);
    }
    for row in 1..table.rows() {
        let y = (b.t + row as f64 * size.y) as f32;
        grid.move_to(b.l as f32, y);
        grid.line_to(b.r as f32, y);
    }
    if let Some(path) = grid.finish() {
        t.pm.stroke_path(&path, &paint_for(s.color, alpha), &round_stroke(s.width), ts, None);
    }
    let inset = 8.0_f64.min(size.x.min(size.y) / 5.0);
    for (i, content) in table.cells().iter().enumerate().filter(|(_, text)| !text.is_empty()) {
        let cell = Cell { row: i / table.columns(), column: i % table.columns() };
        let area = table.cell_bounds(b, cell).inflate(-inset);
        draw_box_text(t, s, content, area, alpha, ts);
    }
}

/// Draws the connector underneath both nodes. Connectors have no separate
/// selection or persistence state.
pub(crate) fn draw_connection(t: &mut Target, parent: Rect, child: Rect, color: u32) {
    let (a, b) = mindmap::endpoints(parent, child);
    let x = ((a.x + b.x) / 2.0) as f32;
    let mut curve = PathBuilder::new();
    curve.move_to(a.x as f32, a.y as f32);
    curve.cubic_to(x, a.y as f32, x, b.y as f32, b.x as f32, b.y as f32);
    if let Some(curve) = curve.finish() {
        let ts = t.world();
        t.pm.stroke_path(&curve, &paint_for(color, 0.7), &round_stroke(3.0), ts, None);
    }
}

pub(super) fn draw_mind_node(t: &mut Target, s: &Shape, alpha: f32, ts: Transform) {
    let b = s.bounds();
    let radius = 18.0_f64.min(b.shortest_side() / 3.0);
    let Some(path) = round_rect_path(&b, radius) else { return };
    t.pm.fill_path(&path, &paint_for(0xffffffff, alpha), FillRule::Winding, ts, None);
    if s.filled {
        t.pm.fill_path(&path, &paint_for(s.color, alpha * 0.12), FillRule::Winding, ts, None);
    }
    t.pm.stroke_path(&path, &paint_for(s.color, alpha), &round_stroke(s.width), ts, None);
    draw_box_text(t, s, &s.text, b.inflate(-radius), alpha, ts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::thumbnail;
    use std::sync::Arc;

    #[test]
    fn thumbnail_draws_the_table_grid_and_the_mind_map_connector() {
        let (w, h) = (520, 320);
        let center = (h as usize / 2 * w as usize + w as usize / 2) * 4;
        let table = Arc::new(crate::table::centered("table".into(), Pt::ZERO, 2, 2));
        let image = thumbnail(&[table], w, h);
        assert_eq!(&image[center..center + 4], &[0x4a, 0x51, 0x60, 0xff]);

        let root = Arc::new(mindmap::root("root".into(), Pt::ZERO));
        let child = Arc::new(mindmap::child(&[], &root, "child".into()));
        let image = thumbnail(&[root, child], w, h);
        assert!(image[center] < 0x80);
        assert!(image[center + 2] > 0xd0);
    }
}
