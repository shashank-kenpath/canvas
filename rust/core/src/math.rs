//! Formula notation, calculation, graphing and handwriting normalization.

mod eval;
mod graph;
mod ink;
mod layout;
mod parse;

pub use graph::GraphRange;
pub(crate) use graph::{curves, graphable_equation, tick_label, ticks, validate_graph};
pub(crate) use ink::recognize;
pub(crate) use layout::{formula_layout, Mark};

pub const ACCENT: u32 = 0xff2748e8;

pub(crate) fn validate_formula(source: &str) -> Result<(), &'static str> {
    parse::formula(source).map(|_| ())
}

/// Whether the source requests a calculation of a valid numeric expression.
pub(crate) fn is_calculation(source: &str) -> bool {
    parse::formula(source).is_ok_and(|(expr, equals)| equals && eval::numeric(&expr, false))
}

pub(crate) fn calculation_source(source: &str) -> Option<String> {
    if !source.trim_end().ends_with(['=', '＝']) {
        return None;
    }
    let source = ink::normalize(source);
    is_calculation(&source).then_some(source)
}
