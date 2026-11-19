//! One shape on the board, and its JSON form (identical to the Flutter app's).

use crate::geom::{segment_distance, Pt, Rect};
use crate::math::{self, GraphRange};
use crate::import::ImageAsset;
use crate::table::Table;
use crate::text;
use serde_json::{json, Map, Value};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tool {
    Select,
    Hand,
    Pen,
    Rectangle,
    Ellipse,
    Arrow,
    Text,
    Eraser,
    Line,
    Triangle,
    Diamond,
    Star,
    Hexagon,
    Table,
    MindNode,
    Image,
    Formula,
    Graph,
}

impl Tool {
    pub const ALL: [Tool; 18] = [
        Tool::Select,
        Tool::Hand,
        Tool::Pen,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Text,
        Tool::Eraser,
        Tool::Line,
        Tool::Triangle,
        Tool::Diamond,
        Tool::Star,
        Tool::Hexagon,
        Tool::Table,
        Tool::MindNode,
        Tool::Image,
        Tool::Formula,
        Tool::Graph,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Hand => "hand",
            Tool::Pen => "pen",
            Tool::Rectangle => "rectangle",
            Tool::Ellipse => "ellipse",
            Tool::Arrow => "arrow",
            Tool::Text => "text",
            Tool::Eraser => "eraser",
            Tool::Line => "line",
            Tool::Triangle => "triangle",
            Tool::Diamond => "diamond",
            Tool::Star => "star",
            Tool::Hexagon => "hexagon",
            Tool::Table => "table",
            Tool::MindNode => "mindnode",
            Tool::Image => "image",
            Tool::Formula => "formula",
            Tool::Graph => "graph",
        }
    }
    pub fn by_name(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }
    pub fn index(self) -> i32 {
        Tool::ALL.iter().position(|t| *t == self).unwrap() as i32
    }
    pub fn from_index(i: i32) -> Option<Tool> {
        Tool::ALL.get(i as usize).copied()
    }
}

pub const DEFAULT_COLOR: u32 = 0xff1d1d1d;

fn normalized_star() -> &'static [(f64, f64); 10] {
    static VERTICES: OnceLock<[(f64, f64); 10]> = OnceLock::new();
    VERTICES.get_or_init(|| {
        use std::f64::consts::PI;

        // Fit the outer tips to the box with one tip pointing up.
        let half_width = (PI / 10.0).cos();
        let height = 1.0 + (PI / 5.0).cos();
        std::array::from_fn(|i| {
            let angle = i as f64 * PI / 5.0;
            let radius = if i % 2 == 0 { 1.0 } else { 0.45 };
            let x = 0.5 + radius * angle.sin() / (2.0 * half_width);
            let y = (1.0 - radius * angle.cos()) / height;
            (x, y)
        })
    })
}

/// Immutable once built; edits make a new shape with the same id.
/// A straight line can also be represented by a pen shape with two points.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub id: String,
    pub tool: Tool,
    pub points: Vec<Pt>,
    pub color: u32,
    pub width: f64,
    pub filled: bool,
    pub text: String,
    pub font_size: f64,
    /// Index into [`text::FONTS`].
    pub font: u8,
    pub table: Option<Box<Table>>,
    pub parent_id: Option<String>,
    pub graph_range: GraphRange,
    pub(crate) image: Option<Arc<ImageAsset>>,
    pub(crate) page_background: bool,
    bounds: Rect,
}

impl Shape {
    pub fn new(id: impl Into<String>, tool: Tool, points: Vec<Pt>) -> Shape {
        let mut s = Shape {
            id: id.into(),
            tool,
            points,
            color: DEFAULT_COLOR,
            width: 3.0,
            filled: false,
            text: String::new(),
            font_size: 24.0,
            font: 0,
            table: (tool == Tool::Table).then(Box::default),
            parent_id: None,
            graph_range: GraphRange::default(),
            image: None,
            page_background: false,
            bounds: Rect::ZERO,
        };
        s.bounds = s.compute_bounds();
        s
    }

    pub fn styled(mut self, color: u32, width: f64, filled: bool) -> Shape {
        self.color = color;
        self.width = width;
        self.filled = filled;
        self
    }

    pub fn with_text(mut self, text: impl Into<String>, font_size: f64) -> Shape {
        self.text = text.into();
        self.font_size = font_size;
        self.bounds = self.compute_bounds();
        self
    }

    pub fn with_font(mut self, font: u8) -> Shape {
        self.font = if (font as usize) < text::FONTS.len() { font } else { 0 };
        self.bounds = self.compute_bounds();
        self
    }

    pub fn with_points(&self, points: Vec<Pt>) -> Shape {
        let mut s = self.clone();
        s.points = points;
        s.bounds = s.compute_bounds();
        s
    }

    pub(crate) fn with_table(mut self, table: Table) -> Shape {
        self.table = Some(Box::new(table));
        self
    }

    pub(crate) fn with_image(mut self, image: Arc<ImageAsset>, page_background: bool) -> Shape {
        self.image = Some(image);
        self.page_background = page_background;
        self
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Clockwise vertices of the box-based polygons, shared by drawing and hit testing.
    pub(crate) fn polygon_vertices(&self) -> impl Iterator<Item = Pt> + '_ {
        let vertices: &[(f64, f64)] = match self.tool {
            Tool::Triangle => &[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)],
            Tool::Diamond => &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)],
            Tool::Star => normalized_star(),
            Tool::Hexagon => &[
                (0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5),
            ],
            _ => &[],
        };
        let b = self.bounds;
        vertices.iter().map(move |&(x, y)| Pt::new(b.l + x * b.width(), b.t + y * b.height()))
    }

    pub fn is_dot(&self) -> bool {
        let a = self.points[0];
        self.points.iter().all(|p| (*p - a).len2() <= 0.01)
    }

    fn compute_bounds(&self) -> Rect {
        let Some(first) = self.points.first() else {
            return Rect::ZERO;
        };
        if matches!(self.tool, Tool::Text | Tool::Formula) {
            let (w, h) = if self.tool == Tool::Formula {
                let layout = math::formula_layout(&self.text, self.font_size, self.font);
                (layout.width, layout.height)
            } else {
                text::measure(&self.text, self.font_size, self.font)
            };
            return Rect::ltrb(
                first.x,
                first.y,
                first.x + w.max(self.font_size),
                first.y + h.max(self.font_size * text::LINE_HEIGHT),
            );
        }
        if self.tool == Tool::Line {
            return Rect::from_points(*first, *self.points.last().unwrap());
        }
        let mut r = Rect::ltrb(first.x, first.y, first.x, first.y);
        for p in &self.points[1..] {
            r.l = r.l.min(p.x);
            r.r = r.r.max(p.x);
            r.t = r.t.min(p.y);
            r.b = r.b.max(p.y);
        }
        r
    }

    pub fn to_json(&self) -> Value {
        self.to_json_with_image_ref(None)
    }

    pub(crate) fn to_json_with_image_ref(&self, reference: Option<String>) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("tool".into(), json!(self.tool.name()));
        m.insert(
            "points".into(),
            Value::Array(self.points.iter().map(|p| json!([p.x, p.y])).collect()),
        );
        m.insert("color".into(), json!(self.color));
        m.insert("width".into(), json!(self.width));
        m.insert("filled".into(), json!(self.filled));
        m.insert("text".into(), json!(self.text));
        if self.font_size != 24.0 {
            m.insert("fontSize".into(), json!(self.font_size));
        }
        if self.font != 0 {
            m.insert("font".into(), json!(text::FONTS[self.font as usize]));
        }
        if let Some(table) = &self.table {
            m.insert("rows".into(), json!(table.rows()));
            m.insert("columns".into(), json!(table.columns()));
            m.insert("cells".into(), json!(table.cells()));
        }
        if let Some(parent) = &self.parent_id {
            m.insert("parentId".into(), json!(parent));
        }
        if self.tool == Tool::Graph {
            m.insert("graphRange".into(), json!({"x": self.graph_range.x(), "y": self.graph_range.y()}));
        }
        if let Some(image) = &self.image {
            let (key, value) = match reference {
                Some(reference) => ("imageRef", reference),
                None => ("imageData", image.base64()),
            };
            m.insert(key.into(), Value::String(value));
            if self.page_background {
                m.insert("pageBackground".into(), Value::Bool(true));
            }
        }
        Value::Object(m)
    }

    pub fn from_json(v: &Value) -> Result<Shape, String> {
        Self::from_json_with_images(v, &|_| None)
    }

    /// Like [`Shape::from_json`], finding `imageRef` images through `images`.
    pub(crate) fn from_json_with_images(v: &Value, images: &dyn Fn(&str) -> Option<Arc<ImageAsset>>) -> Result<Shape, String> {
        let o = v.as_object().ok_or("Shape must be an object.")?;
        let id = o.get("id").and_then(Value::as_str).ok_or("Shape id missing.")?;
        let tool = o
            .get("tool")
            .and_then(Value::as_str)
            .and_then(Tool::by_name)
            .ok_or("Unknown shape tool.")?;
        let mut points = Vec::new();
        for p in o.get("points").and_then(Value::as_array).ok_or("Shape points missing.")? {
            let pair = p.as_array().filter(|a| a.len() >= 2).ok_or("Bad point.")?;
            let (x, y) = (pair[0].as_f64(), pair[1].as_f64());
            match (x, y) {
                (Some(x), Some(y)) if x.is_finite() && y.is_finite() => points.push(Pt::new(x, y)),
                _ => return Err("Shape coordinates must be finite.".into()),
            }
        }
        let num = |key: &str, default: f64| -> Result<f64, String> {
            match o.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v.as_f64().ok_or(format!("{key} must be a number.")),
            }
        };
        let width = num("width", 3.0)?;
        if !width.is_finite() || width <= 0.0 {
            return Err("Stroke width must be positive and finite.".into());
        }
        let font_size = num("fontSize", 24.0)?;
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err("Font size must be positive and finite.".into());
        }
        let color = num("color", DEFAULT_COLOR as f64)? as i64 as u32;
        let filled = o.get("filled").and_then(Value::as_bool).unwrap_or(false);
        let text = o.get("text").and_then(Value::as_str).unwrap_or("");
        let font = o.get("font").and_then(Value::as_str).map(text::font_index).unwrap_or(0);
        let mut shape = Shape::new(id, tool, points).styled(color, width, filled);
        if matches!(tool, Tool::Table | Tool::MindNode) {
            let bounds = shape.bounds();
            if shape.points.len() != 2 || bounds.shortest_side() <= 0.0
                || !bounds.longest_side().is_finite()
            {
                return Err("Tables and mind-map nodes need two opposite box corners.".into());
            }
        }
        if tool == Tool::Table {
            shape = shape.with_table(Table::from_json(o));
        }
        if tool == Tool::Image {
            let bounds = shape.bounds();
            if shape.points.len() != 2 || bounds.shortest_side() <= 0.0 || !bounds.longest_side().is_finite() {
                return Err("Images need two opposite box corners.".into());
            }
            let background = o.get("pageBackground").and_then(Value::as_bool).unwrap_or(false);
            let image = match o.get("imageData").and_then(Value::as_str) {
                Some(data) => Arc::new(ImageAsset::from_base64(data)?),
                None => o.get("imageRef").and_then(Value::as_str).and_then(images).ok_or("Image data missing.")?,
            };
            shape = shape.with_image(image, background);
        }
        if tool == Tool::MindNode {
            shape.parent_id = o.get("parentId").and_then(Value::as_str)
                .filter(|parent| !parent.is_empty() && *parent != id).map(str::to_string);
        }
        if tool == Tool::Formula {
            if shape.points.len() != 1 {
                return Err("Formulas need one origin point.".into());
            }
            math::validate_formula(text)?;
        }
        if tool == Tool::Graph {
            let bounds = shape.bounds();
            if shape.points.len() != 2 || bounds.shortest_side() <= 0.0 || !bounds.longest_side().is_finite() {
                return Err("Graphs need two opposite box corners.".into());
            }
            math::validate_graph(text)?;
            if let Some(range) = o.get("graphRange").filter(|value| !value.is_null()) {
                let axis = |key: &str| -> Option<[f64; 2]> {
                    let pair = range.get(key)?.as_array()?;
                    let [low, high] = pair.as_slice() else { return None };
                    Some([low.as_f64()?, high.as_f64()?])
                };
                shape.graph_range = axis("x").zip(axis("y")).and_then(|(x, y)| GraphRange::new(x, y))
                    .ok_or("Graph range needs increasing, finite x and y ends.")?;
            }
        }
        shape.font = font;
        Ok(shape.with_text(text, font_size))
    }

    /// True when `point` is within `tolerance` of the visible shape.
    pub fn hits(&self, point: Pt, tolerance: f64) -> bool {
        if self.points.is_empty() {
            return false;
        }
        let radius = tolerance + self.width / 2.0;
        let bounds = self.bounds;
        let reach = bounds.inflate(radius);
        if !(reach.l..=reach.r).contains(&point.x) || !(reach.t..=reach.b).contains(&point.y) {
            return false;
        }
        match self.tool {
            Tool::Text | Tool::Formula | Tool::Graph => bounds.inflate(tolerance).contains(point),
            Tool::Image => !self.page_background && bounds.inflate(tolerance).contains(point),
            Tool::Table | Tool::MindNode => reach.contains(point),
            Tool::Rectangle => {
                if self.filled && bounds.contains(point) {
                    return true;
                }
                let c = [
                    bounds.top_left(),
                    bounds.top_right(),
                    bounds.bottom_right(),
                    bounds.bottom_left(),
                ];
                (0..4).any(|i| segment_distance(point, c[i], c[(i + 1) % 4]) <= radius)
            }
            Tool::Ellipse => {
                let (rx, ry) = (bounds.width() / 2.0, bounds.height() / 2.0);
                if rx < 0.001 || ry < 0.001 {
                    return segment_distance(point, bounds.top_left(), bounds.bottom_right())
                        <= radius;
                }
                let c = bounds.center();
                let (dx, dy) = (point.x - c.x, point.y - c.y);
                let normalized = (dx * dx / (rx * rx) + dy * dy / (ry * ry)).sqrt();
                if self.filled && normalized <= 1.0 {
                    return true;
                }
                if normalized < 0.000001 {
                    return rx.min(ry) <= radius;
                }
                let on_outline = c + Pt::new(dx / normalized, dy / normalized);
                on_outline.dist(point) <= radius
            }
            Tool::Triangle | Tool::Diamond | Tool::Star | Tool::Hexagon => {
                let mut vertices = self.polygon_vertices();
                let first = vertices.next().unwrap();
                let mut a = first;
                let mut inside = false;
                for b in vertices.chain([first]) {
                    if segment_distance(point, a, b) <= radius {
                        return true;
                    }
                    // Count crossings of the ray extending to the right of the point.
                    if self.filled && (a.y > point.y) != (b.y > point.y) {
                        let crossing_x = a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x);
                        if point.x < crossing_x {
                            inside = !inside;
                        }
                    }
                    a = b;
                }
                inside
            }
            Tool::Line => {
                segment_distance(point, self.points[0], *self.points.last().unwrap()) <= radius
            }
            Tool::Pen | Tool::Arrow => {
                if self.points.len() == 1 {
                    return self.points[0].dist(point) <= radius;
                }
                self.points.windows(2).any(|w| segment_distance(point, w[0], w[1]) <= radius)
            }
            Tool::Select | Tool::Hand | Tool::Eraser => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tools_round_trip_with_stable_indices() {
        for (tool, name, index) in [
            (Tool::Line, "line", 8),
            (Tool::Triangle, "triangle", 9),
            (Tool::Diamond, "diamond", 10),
            (Tool::Star, "star", 11),
            (Tool::Hexagon, "hexagon", 12),
        ] {
            let shape = Shape::new(name, tool, vec![Pt::new(90.0, 10.0), Pt::new(0.0, 60.0)])
                .styled(0xffabcdef, 5.0, true);
            let json = shape.to_json();
            assert_eq!(json["tool"], name);
            assert_eq!(Shape::from_json(&json).unwrap(), shape);
            assert_eq!(tool.index(), index);
            assert_eq!(Tool::from_index(index), Some(tool));
        }
    }
}
