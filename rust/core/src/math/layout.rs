use super::eval::{self, Number};
use super::parse::{self, Expr, Function, Op};
use crate::geom::{Pt, Rect};
use crate::text;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug)]
pub(crate) enum Mark {
    Text { text: String, at: Pt, size: f64, accent: bool },
    Line { from: Pt, to: Pt, width: f64, accent: bool },
    Bracket { bounds: Rect, kind: char, right: bool, width: f64, accent: bool },
}

impl Mark {
    fn shift(&mut self, delta: Pt) {
        match self {
            Self::Text { at, .. } => *at = *at + delta,
            Self::Line { from, to, .. } => {
                *from = *from + delta;
                *to = *to + delta;
            }
            Self::Bracket { bounds, .. } => *bounds = bounds.shift(delta),
        }
    }
}

/// The box is positioned on the mathematical axis at y=0 while it is built.
#[derive(Default)]
struct BoxLayout {
    width: f64,
    up: f64,
    down: f64,
    marks: Vec<Mark>,
}

impl BoxLayout {
    fn push(&mut self, mut other: Self, x: f64, y: f64) {
        self.up = self.up.max(other.up - y);
        self.down = self.down.max(other.down + y);
        self.width = self.width.max(x + other.width);
        for mark in &mut other.marks {
            mark.shift(Pt::new(x, y));
        }
        self.marks.append(&mut other.marks);
    }

    fn height(&self) -> f64 {
        self.up + self.down
    }
}

pub(crate) struct FormulaLayout {
    pub width: f64,
    pub height: f64,
    pub marks: Vec<Mark>,
}

#[derive(Clone, Copy)]
struct Style {
    size: f64,
    font: u8,
    accent: bool,
}

impl Style {
    fn smaller(self, factor: f64) -> Self {
        Self { size: (self.size * factor).max(self.size.min(3.0)), ..self }
    }

    fn text(self, text: &str) -> BoxLayout {
        let (width, height) = text::measure(text, self.size, self.font);
        let half = height / 2.0;
        BoxLayout {
            width: width + self.size * 0.045,
            up: half,
            down: half,
            marks: vec![Mark::Text {
                text: text.into(), at: Pt::new(0.0, -half), size: self.size, accent: self.accent,
            }],
        }
    }

    fn line(self, x: f64, y: f64, end_x: f64, end_y: f64) -> Mark {
        Mark::Line {
            from: Pt::new(x, y), to: Pt::new(end_x, end_y),
            width: self.size * 0.052, accent: self.accent,
        }
    }

    fn row(self, children: impl IntoIterator<Item = BoxLayout>, spacing: f64) -> BoxLayout {
        let mut row = BoxLayout::default();
        for child in children {
            let gap = if row.marks.is_empty() { 0.0 } else { self.size * spacing };
            row.push(child, row.width + gap, 0.0);
        }
        row
    }

    fn fraction(self, numerator: BoxLayout, denominator: BoxLayout) -> BoxLayout {
        let gap = self.size * 0.06;
        let width = numerator.width.max(denominator.width) + self.size * 0.27;
        let mut result = BoxLayout { width, ..BoxLayout::default() };
        let (nx, ny) = ((width - numerator.width) / 2.0, -gap - numerator.down);
        let (dx, dy) = ((width - denominator.width) / 2.0, gap + denominator.up);
        result.push(numerator, nx, ny);
        result.push(denominator, dx, dy);
        result.marks.push(self.line(gap, 0.0, width - gap, 0.0));
        result
    }

    fn delimited(self, body: BoxLayout, kind: char) -> BoxLayout {
        let up = body.up.max(self.size * 0.46);
        let down = body.down.max(self.size * 0.46);
        let width = self.size * (if kind == '(' { 0.23 } else { 0.27 });
        let inset = self.size * 0.07;
        let full = body.width + (width + inset) * 2.0;
        let mut result = BoxLayout { width: full, up, down, ..BoxLayout::default() };
        result.push(body, width + inset, 0.0);
        for (left, right) in [(inset, false), (full - width, true)] {
            result.marks.push(Mark::Bracket {
                bounds: Rect::ltrb(left, -up + inset, left + width - inset, down - inset),
                kind, right, width: self.size * 0.06, accent: self.accent,
            });
        }
        result
    }

    fn radical(self, body: BoxLayout) -> BoxLayout {
        let x = self.size * 0.57;
        let pad = self.size * 0.09;
        let top = -body.up - pad;
        let bottom = body.down - pad * 2.0;
        let mut radical = BoxLayout { width: body.width + x + pad, up: -top + pad, ..BoxLayout::default() };
        radical.push(body, x, 0.0);
        radical.marks.extend([
            self.line(0.04 * self.size, bottom - self.size * 0.24, 0.16 * self.size, bottom - self.size * 0.29),
            self.line(0.16 * self.size, bottom - self.size * 0.29, 0.27 * self.size, bottom),
            self.line(0.27 * self.size, bottom, x - pad, top),
            self.line(x - pad, top, radical.width - pad * 0.5, top),
        ]);
        radical
    }

    fn scripts(self, expr: &Expr) -> BoxLayout {
        let mut base = expr;
        let (mut upper, mut lower) = (None, None);
        while let Expr::Binary(op, left, right) = base {
            match op {
                Op::Pow if upper.is_none() => upper = Some(right.ungrouped()),
                Op::Subscript if lower.is_none() => lower = Some(right.ungrouped()),
                _ => break,
            }
            base = left;
        }
        let large = matches!(base, Expr::Symbol(word) if matches!(parse::symbol(word), "∑" | "∫"));
        let mut body = self.expr(base);
        let script = self.smaller(0.69);
        let upper = upper.map(|e| script.expr(e));
        let lower = lower.map(|e| script.expr(e));
        let gap = self.size * 0.07;
        let up = body.up;
        let down = body.down;
        if large {
            let width = body.width
                .max(upper.as_ref().map_or(0.0, |b| b.width))
                .max(lower.as_ref().map_or(0.0, |b| b.width));
            let mut result = BoxLayout::default();
            let x = (width - body.width) / 2.0;
            result.push(body, x, 0.0);
            if let Some(b) = upper {
                let (x, y) = ((width - b.width) / 2.0, -up - b.down);
                result.push(b, x, y);
            }
            if let Some(b) = lower {
                let (x, y) = ((width - b.width) / 2.0, down + b.up);
                result.push(b, x, y);
            }
            return result;
        }
        let x = body.width;
        if let Some(b) = upper {
            let y = -(up * 0.72).max(b.down + gap);
            body.push(b, x, y);
        }
        if let Some(b) = lower {
            let y = (down * 0.75).max(b.up + gap);
            body.push(b, x, y);
        }
        body
    }

    fn expr(self, expr: &Expr) -> BoxLayout {
        match expr {
            Expr::Number(number) => self.text(number),
            Expr::Symbol(word) => {
                let symbol = parse::symbol(word);
                if matches!(symbol, "∑" | "∫") {
                    Self { size: self.size * 1.35, ..self }.text(symbol)
                } else {
                    self.text(symbol)
                }
            }
            Expr::Group(kind, inner) => self.delimited(self.expr(inner), *kind),
            Expr::Call(Function::Sqrt, inner) => self.radical(self.expr(inner.ungrouped())),
            Expr::Call(function, inner) => {
                self.row([self.text(function.label()), self.delimited(self.expr(inner), '(')], 0.02)
            }
            Expr::Signed(negative, inner) => {
                self.row([self.text(if *negative { "−" } else { "+" }), self.expr(inner)], 0.0)
            }
            Expr::Percent(inner) => self.row([self.expr(inner), self.text("%")], 0.0),
            Expr::Binary(Op::Pow | Op::Subscript, _, _) => self.scripts(expr),
            Expr::Binary(Op::Div, a, b) => {
                let part = self.smaller(0.87);
                self.fraction(part.expr(a.ungrouped()), part.expr(b.ungrouped()))
            }
            Expr::Binary(Op::Juxtapose, a, b) => self.row([self.expr(a), self.expr(b)], 0.045),
            Expr::Binary(op, a, b) => {
                let glyph = match op {
                    Op::Eq => "=", Op::Add => "+", Op::Sub => "−", Op::Mul => "×",
                    Op::DivideSign => "÷", _ => unreachable!(),
                };
                self.row([self.expr(a), self.text(glyph), self.expr(b)], 0.13)
            }
        }
    }

    fn result(self, expression: &Expr) -> BoxLayout {
        let accent = Self { accent: true, ..self };
        match eval::calculate(expression) {
            Some(Number::Exact(ratio)) => {
                if let Some(label) = ratio.decimal_string() {
                    accent.text(&label)
                } else {
                    let part = accent.smaller(0.87);
                    let fraction = accent.fraction(
                        part.text(&ratio.numerator.abs().to_string()), part.text(&ratio.denominator.to_string()));
                    if ratio.numerator < 0 {
                        accent.row([accent.text("−"), fraction], 0.04)
                    } else {
                        fraction
                    }
                }
            }
            Some(Number::Real(value)) => accent.text(&eval::real_label(value)),
            None if eval::numeric(expression, false) => accent.text("undefined"),
            None => BoxLayout::default(),
        }
    }
}

fn build(source: &str, size: f64, font: u8) -> FormulaLayout {
    let style = Style { size, font, accent: false };
    let mut layout = match parse::formula(source) {
        Ok((expr, true)) => {
            let expr_layout = style.expr(&expr);
            let result = style.result(&expr);
            if result.marks.is_empty() {
                style.row([expr_layout, style.text("=")], 0.13)
            } else {
                style.row([expr_layout, style.text("="), result], 0.13)
            }
        }
        Ok((expr, false)) => style.expr(&expr),
        Err(_) => style.text(source),
    };
    let height = layout.height();
    for mark in &mut layout.marks {
        mark.shift(Pt::new(0.0, layout.up));
    }
    FormulaLayout { width: layout.width, height, marks: layout.marks }
}

type Cache = Mutex<HashMap<(String, u64, u8), Arc<FormulaLayout>>>;

pub(crate) fn formula_layout(source: &str, size: f64, font: u8) -> Arc<FormulaLayout> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (source.to_string(), size.to_bits(), font);
    if let Some(layout) = cache.lock().unwrap().get(&key) {
        return layout.clone();
    }
    let layout = Arc::new(build(source, size, font));
    let mut cache = cache.lock().unwrap();
    if cache.len() >= 128 {
        cache.clear();
    }
    cache.insert(key, layout.clone());
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(layout: &FormulaLayout, label: &str) -> (Pt, f64, bool) {
        layout.marks.iter().find_map(|mark| match mark {
            Mark::Text { text, at, size, accent } if text == label => Some((*at, *size, *accent)),
            _ => None,
        }).unwrap()
    }

    #[test]
    fn fraction_root_and_brackets_follow_the_box_height() {
        let stacked = build("(1/2) + sqrt(3/4)", 40.0, 0);
        let simple = build("(1)", 40.0, 0);
        let bracket_height = |layout: &FormulaLayout| layout.marks.iter().find_map(|mark| match mark {
            Mark::Bracket { bounds, .. } => Some(bounds.height()),
            _ => None,
        }).unwrap();
        assert!(bracket_height(&stacked) > bracket_height(&simple) * 1.5);
        assert!(mark(&stacked, "1").0.y < mark(&stacked, "2").0.y);
        assert!(mark(&stacked, "3").0.y < mark(&stacked, "4").0.y);
        assert!(stacked.height > simple.height * 1.7);
        assert_eq!(stacked.marks.iter().filter(|mark| matches!(mark, Mark::Line { .. })).count(), 6);
    }

    #[test]
    fn scripts_and_limits_use_smaller_text_on_separate_rows() {
        let scripts = build("x^2_i", 40.0, 0);
        let (base, _, _) = mark(&scripts, "x");
        let (upper, size, _) = mark(&scripts, "2");
        let (lower, _, _) = mark(&scripts, "i");
        assert!(upper.y < base.y && base.y < lower.y);
        assert!(upper.x > base.x && (upper.x - lower.x).abs() < 1e-9 && size < 40.0);
        let limits = build("sum_(i=1)^n t + int_0^9 t", 40.0, 0);
        assert!(mark(&limits, "n").0.y < mark(&limits, "∑").0.y);
        assert!(mark(&limits, "1").0.y > mark(&limits, "∑").0.y);
        assert!(mark(&limits, "9").0.y < mark(&limits, "∫").0.y);
        assert!(mark(&limits, "0").0.y > mark(&limits, "∫").0.y);
    }

    #[test]
    fn only_calculation_result_uses_accent() {
        let layout = build("1/4 + 1/4 =", 40.0, 0);
        assert!(!mark(&layout, "=").2);
        assert_eq!(layout.marks.iter().filter(|mark| matches!(mark, Mark::Line { accent: true, .. })).count(), 1);
        let layout = build("12 × 7 + 3 =", 40.0, 0);
        assert!(mark(&layout, "87").2);
        assert!(!mark(&layout, "12").2);
        assert_eq!(mark(&build("alpha + pi", 40.0, 0), "α").1, 40.0);
    }
}
