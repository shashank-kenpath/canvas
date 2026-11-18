use super::eval::{real_label, Pole, RealExpr};
use super::parse::{self, Expr, Op};
use crate::geom::Pt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_FUNCTIONS: usize = 8;
const MAX_SPAN: f64 = 1e9;
const MIN_SPAN: f64 = 1e-8;

/// Graph coordinates are independent of the board camera and shape bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphRange {
    x: [f64; 2],
    y: [f64; 2],
}

impl Default for GraphRange {
    fn default() -> Self {
        Self { x: [-10.0, 10.0], y: [-10.0, 10.0] }
    }
}

impl GraphRange {
    pub fn new(x: [f64; 2], y: [f64; 2]) -> Option<Self> {
        [x, y].iter().all(|axis| {
            let span = axis[1] - axis[0];
            (MIN_SPAN * 0.99..=MAX_SPAN * 1.01).contains(&span)
                && axis.iter().all(|v| v.abs() <= (span * 1e7).min(1e12))
        }).then_some(Self { x, y })
    }

    pub fn x(self) -> [f64; 2] { self.x }
    pub fn y(self) -> [f64; 2] { self.y }

    pub(crate) fn adjusted(self, pan: Pt, scale: f64) -> Self {
        let adjust = |[low, high]: [f64; 2], pan: f64| {
            let span = high - low;
            let next = (span * scale).clamp(MIN_SPAN, MAX_SPAN);
            let limit = (next * 1e7).min(1e12) - next / 2.0;
            let center = (low + span * (0.5 + pan)).clamp(-limit, limit);
            [center - next * 0.5, center + next * 0.5]
        };
        Self { x: adjust(self.x, pan.x), y: adjust(self.y, pan.y) }
    }
}

fn function(expr: &Expr, requires_equation: bool) -> Option<RealExpr> {
    match expr {
        Expr::Binary(Op::Eq, left, right)
            if matches!(left.ungrouped(), Expr::Symbol(name) if name == "y") => RealExpr::compile(right),
        _ if requires_equation => None,
        _ => RealExpr::compile(expr),
    }
}

pub(crate) fn graphable_equation(source: &str) -> bool {
    parse::formula(source).is_ok_and(|(expr, trailing)| !trailing && function(&expr, true).is_some())
}

fn functions(source: &str) -> Result<Vec<RealExpr>, &'static str> {
    if source.len() > 4096 {
        return Err("This graph source is too long.");
    }
    let expressions = source.split([';', '\n', ',']).map(str::trim).filter(|part| !part.is_empty());
    let mut compiled = Vec::new();
    for part in expressions {
        if compiled.len() == MAX_FUNCTIONS {
            return Err("A graph can plot up to eight functions.");
        }
        let (expr, trailing) = parse::formula(part)?;
        if trailing {
            return Err("Enter a function of x, for example y = x^2.");
        }
        compiled.push(function(&expr, false).ok_or("Enter a function of x, for example y = x^2.")?);
    }
    if compiled.is_empty() {
        return Err("Enter a function of x, for example y = x^2.");
    }
    Ok(compiled)
}

pub(crate) fn validate_graph(source: &str) -> Result<(), &'static str> {
    functions(source).map(|_| ())
}

/// Tick positions use 1, 2 or 5 times a power of ten. Indexing from zero
/// avoids drift and ensures that grid lines and axis labels use the same values.
pub(crate) fn ticks([low, high]: [f64; 2], target: f64) -> Vec<f64> {
    let rough = (high - low) / target;
    let decade = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().find(|n| n * decade >= rough).unwrap_or(10.0) * decade;
    let first = (low / step - 1e-9).ceil() as i64;
    let last = (high / step + 1e-9).floor() as i64;
    (first..=last).take(24).map(|n| n as f64 * step).collect()
}

pub(crate) fn tick_label(value: f64) -> String {
    real_label(value)
}

type Segment = [Pt; 2];
type Curves = Vec<Vec<Segment>>;

struct Sampler<'a> {
    expr: &'a RealExpr,
    poles: Vec<Pole<'a>>,
    range: GraphRange,
    segments: Vec<Segment>,
}

impl Sampler<'_> {
    fn world_x(&self, x: f64) -> f64 {
        let [xmin, xmax] = self.range.x;
        xmin + x * (xmax - xmin)
    }

    fn at(&self, x: f64) -> Pt {
        let [ymin, ymax] = self.range.y;
        let world_y = self.expr.at(self.world_x(x));
        let y = (ymax - world_y) / (ymax - ymin);
        Pt::new(x, if y.is_finite() { y.clamp(-1e9, 1e9) } else { f64::NAN })
    }

    fn visit(&mut self, a: Pt, b: Pt, depth: u8) {
        let mid = self.at(a.x + (b.x - a.x) / 2.0);
        let finite = [a, mid, b].into_iter().filter(|p| p.y.is_finite()).count();
        if finite == 0 {
            return;
        }
        let depth_limit = depth == 7;
        let bend = (mid.y - (a.y + b.y) / 2.0).abs();
        let tolerance = if depth_limit { 0.007 } else { 0.0012 };
        let jump = (a.y - b.y).abs();
        let offscreen = [a, mid, b].iter().all(|p| p.y < -0.1)
            || [a, mid, b].iter().all(|p| p.y > 1.1);
        if offscreen && (depth_limit || bend <= tolerance) {
            return;
        }
        if finite == 3 && bend <= tolerance && jump < 0.5
            && !self.poles.iter().any(|pole| pole.crosses([a.x, mid.x, b.x].map(|x| self.world_x(x))))
        {
            self.segment(a, b);
        } else if !depth_limit {
            self.visit(a, mid, depth + 1);
            self.visit(mid, b, depth + 1);
        }
    }

    fn segment(&mut self, a: Pt, b: Pt) {
        if (a.y < 0.0 && b.y < 0.0) || (a.y > 1.0 && b.y > 1.0) {
            return;
        }
        let clip = |p: Pt, q: Pt| {
            if (0.0..=1.0).contains(&p.y) {
                p
            } else {
                let y = p.y.clamp(0.0, 1.0);
                let x = p.x + (q.x - p.x) * ((y - p.y) / (q.y - p.y));
                Pt::new(x, y)
            }
        };
        self.segments.push([clip(a, b), clip(b, a)]);
    }
}

fn sample(expr: &RealExpr, range: GraphRange) -> Vec<Segment> {
    let mut poles = Vec::new();
    expr.poles(&mut poles);
    let mut sampler = Sampler { expr, poles, range, segments: Vec::new() };
    let mut left = sampler.at(0.0);
    let seeds = 96;
    for i in 1..=seeds {
        let x = i as f64 / seeds as f64;
        // Slightly stagger the interiors so periodic curves cannot line up
        // with every initial probe and appear constant.
        let next = if i == seeds { x } else { x + (x * 53.0).sin() * 0.0019 };
        let right = sampler.at(next);
        sampler.visit(left, right, 0);
        left = right;
    }
    sampler.segments
}

type Cache = Mutex<HashMap<(String, [u64; 4]), Arc<Curves>>>;

pub(crate) fn curves(source: &str, range: GraphRange) -> Arc<Curves> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (source.to_string(), [range.x[0], range.x[1], range.y[0], range.y[1]].map(f64::to_bits));
    if let Some(lines) = cache.lock().unwrap().get(&key) {
        return lines.clone();
    }
    let lines: Arc<Curves> = Arc::new(functions(source).unwrap_or_default().iter().map(|expr| sample(expr, range)).collect());
    let mut cache = cache.lock().unwrap();
    if cache.len() >= 24 {
        cache.clear();
    }
    cache.insert(key, lines.clone());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_multiple_functions_and_checks_equation_and_variables() {
        assert_eq!(functions("y = x^2 - 2\ny = sin(x); cos(x)").unwrap().len(), 3);
        assert!(graphable_equation("y = x^2 - 2"));
        for invalid in ["y = n*x", "y=x=", "x = y", "y = x_2", "sin(x) =", "y ="] {
            assert!(validate_graph(invalid).is_err(), "{invalid}");
            assert!(!graphable_equation(invalid));
        }
        assert!(!graphable_equation("sin(x)"));
    }

    #[test]
    fn adaptive_samples_are_finite_and_inside_the_frame() {
        let range = GraphRange::new([-3.0, 3.0], [-2.0, 2.0]).unwrap();
        let lines = curves("x; sin(60x); x^2 - 2", range);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].len() > lines[0].len() * 2);
        assert!(!lines[2].is_empty());
        for points in lines.iter().flatten().flatten() {
            assert!((0.0..=1.0).contains(&points.x));
            assert!((0.0..=1.0).contains(&points.y));
        }
        let range = GraphRange::new([-std::f64::consts::PI, std::f64::consts::PI], [-1.0, 1.0]).unwrap();
        let line = curves("sin(x)", range);
        assert!(line[0].iter().flatten().any(|p| p.y < 0.005));
        assert!(line[0].iter().flatten().any(|p| p.y > 0.995));
    }

    #[test]
    fn curves_do_not_connect_at_poles_or_outside_the_domain() {
        let range = GraphRange::new([-2.0, 2.0], [-8.0, 8.0]).unwrap();
        for (source, pole) in [
            ("1/(x-0.137)", 0.137), ("tan(x)", std::f64::consts::FRAC_PI_2),
            ("(x-0.137)/(x-0.137)", 0.137), ("(x-0.137)^2/(x-0.137)^2", 0.137),
            ("abs(x-0.137)/(x-0.137)", 0.137), ("(x-0.137)*(x-0.137)^-1", 0.137),
        ] {
            let plots = curves(source, range);
            let x = (pole + 2.0) / 4.0;
            assert!(plots[0].iter().any(|[_, b]| b.x < x));
            assert!(plots[0].iter().any(|[a, _]| a.x > x));
            assert!(plots[0].iter().all(|[a, b]| b.x < x || a.x > x));
        }
        let sqrt = curves("sqrt(x)", range);
        assert!(sqrt[0].iter().flatten().all(|p| p.x >= 0.5));
    }

    #[test]
    fn range_changes_and_ticks_have_stable_positions() {
        let range = GraphRange::default().adjusted(Pt::new(0.25, -0.25), 0.5);
        assert_eq!(range.x(), [0.0, 10.0]);
        assert_eq!(range.y(), [-10.0, 0.0]);
        assert_eq!(ticks(range.x(), 5.0), [0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        assert_eq!(tick_label(0.0), "0");
        assert_eq!(tick_label(-0.001), "-0.001");
        assert!(GraphRange::new([1.0, 0.0], [-1.0, 1.0]).is_none());
    }
}
