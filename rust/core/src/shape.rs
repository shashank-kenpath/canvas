//! One shape on the board, and its JSON form (identical to the Flutter app's).

use crate::geom::{segment_distance, Pt, Rect};
use crate::text;
use serde_json::{json, Map, Value};

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
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Select,
        Tool::Hand,
        Tool::Pen,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Text,
        Tool::Eraser,
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

/// Immutable once built; edits make a new shape with the same id.
/// A straight line is a pen shape with exactly two points.
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

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn is_dot(&self) -> bool {
        let a = self.points[0];
        self.points.iter().all(|p| (*p - a).len2() <= 0.01)
    }

    fn compute_bounds(&self) -> Rect {
        let Some(first) = self.points.first() else {
            return Rect::ZERO;
        };
        if self.tool == Tool::Text {
            let (w, h) = text::measure(&self.text, self.font_size, self.font);
            return Rect::ltrb(
                first.x,
                first.y,
                first.x + w.max(self.font_size),
                first.y + h.max(self.font_size * text::LINE_HEIGHT),
            );
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
        Value::Object(m)
    }

    pub fn from_json(v: &Value) -> Result<Shape, String> {
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
        if !bounds.inflate(radius).contains(point) {
            return false;
        }
        match self.tool {
            Tool::Text => bounds.inflate(tolerance).contains(point),
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
