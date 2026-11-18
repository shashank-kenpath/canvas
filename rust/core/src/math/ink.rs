use super::parse::{self, Expr, Op};
use crate::{Rect, Shape};
use std::sync::Arc;

fn digit_script(ch: char) -> Option<char> {
    match ch {
        '⁰' => Some('0'), '¹' => Some('1'), '²' => Some('2'), '³' => Some('3'),
        '⁴' => Some('4'), '⁵' => Some('5'), '⁶' => Some('6'), '⁷' => Some('7'),
        '⁸' => Some('8'), '⁹' => Some('9'), '⁻' => Some('-'), '⁺' => Some('+'),
        _ => None,
    }
}

pub(super) fn normalize(input: &str) -> String {
    let mut input = input.trim().chars().peekable();
    let mut output = String::new();
    while let Some(ch) = input.next() {
        if let Some(digit) = digit_script(ch) {
            output.push_str("^(");
            output.push(digit);
            while let Some(digit) = input.peek().and_then(|&ch| digit_script(ch)) {
                output.push(digit);
                input.next();
            }
            output.push(')');
            continue;
        }
        let left = output.trim_end().chars().next_back();
        let right = if matches!(ch, 'x' | 'X') {
            input.clone().find(|c| !c.is_whitespace())
        } else {
            None
        };
        output.push(match ch {
            'x' | 'X' if left.is_some_and(|c| c.is_ascii_digit() || c == ')')
                && right.is_some_and(|c| c.is_ascii_digit() || c == '.') => '×',
            '−' | '–' | '—' => '-',
            '∕' | '⁄' => '/',
            '˄' | 'ˆ' => '^',
            '∙' => '·',
            '＝' => '=',
            c => c,
        });
    }
    output
}

fn mathematical_words(expr: &Expr) -> bool {
    match expr {
        Expr::Symbol(name) => name.chars().count() == 1 || name == "infty" || parse::symbol(name) != name,
        Expr::Number(_) => true,
        Expr::Group(_, inner) | Expr::Call(_, inner) | Expr::Signed(_, inner) | Expr::Percent(inner) => {
            mathematical_words(inner)
        }
        Expr::Binary(_, a, b) => mathematical_words(a) && mathematical_words(b),
    }
}

fn has_operation(expr: &Expr) -> bool {
    match expr {
        Expr::Group(_, inner) => has_operation(inner),
        Expr::Binary(Op::Juxtapose, a, b) => has_operation(a) || has_operation(b),
        Expr::Number(_) | Expr::Symbol(_) => false,
        _ => true,
    }
}

/// Detect a standalone fraction only if a long stroke separates two aligned
/// groups and the candidate supplies exactly two plausible numeric values.
fn stacked(candidate: &str, strokes: &[Arc<Shape>]) -> Option<String> {
    if !(3..=40).contains(&strokes.len()) {
        return None;
    }
    let mut values = candidate.split(|c: char| c.is_whitespace() || matches!(c, '−' | '-' | '/' | '÷'))
        .filter(|word| !word.is_empty());
    let (above, below) = (values.next()?, values.next()?);
    let is_unsigned_number = |value| {
        matches!(parse::formula(value), Ok((Expr::Number(_), false)))
    };
    if values.next().is_some() || !is_unsigned_number(above) || !is_unsigned_number(below) {
        return None;
    }
    for (i, stroke) in strokes.iter().enumerate() {
        let bar = stroke.bounds();
        let width = bar.width();
        if width < 8.0 || bar.height() > width * 0.12 {
            continue;
        }
        let mut first = None;
        let (mut top, mut bottom) = (None::<Rect>, None::<Rect>);
        let mut valid = true;
        for (j, item) in strokes.iter().enumerate().filter(|(j, _)| *j != i) {
            let b = item.bounds();
            let is_above = b.b < bar.t;
            if (!is_above && b.t <= bar.b)
                || b.center().x < bar.l || b.center().x > bar.r
                || b.width() > width * 1.25 || (b.center().y - bar.center().y).abs() > width
            {
                valid = false;
                break;
            }
            first.get_or_insert((j, is_above));
            let group = if is_above { &mut top } else { &mut bottom };
            *group = Some(group.map_or(b, |g| g.union(&b)));
        }
        if valid && top.zip(bottom).is_some_and(|(a, b)| {
            a.height() > 2.0 && b.height() > 2.0 && (a.center().x - b.center().x).abs() <= width * 0.3
        }) {
            let (a, b) = if first.is_some_and(|(_, is_above)| is_above) { (above, below) } else { (below, above) };
            return Some(format!("{a}/{b}"));
        }
    }
    None
}

/// Use the most likely candidate that looks like math. A plausible first
/// choice in ordinary prose keeps precedence over lower-ranked alternatives.
pub(crate) fn recognize(candidates: &[&str], strokes: &[Arc<Shape>]) -> Option<String> {
    let first = candidates.first()?;
    let first_has_math_marks = first.chars().any(|c| c.is_ascii_digit()
        || matches!(c, '=' | '^' | '÷' | '×' | '√' | '±' | 'π' | '%' | '+' | '/'));
    for (i, raw) in candidates.iter().take(5).enumerate() {
        if i > 0 && !first_has_math_marks {
            break;
        }
        let source = normalize(raw);
        if let Some(source) = stacked(&source, strokes) {
            return Some(source);
        }
        let words: Vec<_> = source.split_whitespace().collect();
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        if words.windows(3).any(|w| digits(w[0]) && digits(w[2])
            && w[1].len() == 1 && w[1].chars().all(char::is_alphabetic))
        {
            continue;
        }
        if let Ok((expr, equals)) = parse::formula(&source) {
            if (equals || has_operation(&expr)) && mathematical_words(&expr) {
                return Some(source);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Pt, Tool};

    #[test]
    fn recognizes_math_and_keeps_prose() {
        assert_eq!(recognize(&["12 x 7 + 3 ＝"], &[]).as_deref(), Some("12 × 7 + 3 ="));
        assert_eq!(recognize(&["20 ÷ 5 − 3 ="], &[]).as_deref(), Some("20 ÷ 5 - 3 ="));
        assert_eq!(recognize(&["x² + y = 3"], &[]).as_deref(), Some("x^(2) + y = 3"));
        assert_eq!(recognize(&["y = x˄2"], &[]).as_deref(), Some("y = x^2"));
        assert_eq!(recognize(&["12 l 3 =", "12 / 3 ="], &[]).as_deref(), Some("12 / 3 ="));
        assert_eq!(recognize(&["meet tomorrow", "2 + 3"], &[]), None);
        assert_eq!(recognize(&["I take 3 notes"], &[]), None);
        assert_eq!(recognize(&["12"], &[]), None);
    }

    #[test]
    fn detects_unambiguous_stacked_digits_but_not_inline_digits() {
        let stroke = |id, a: Pt, b: Pt| Arc::new(Shape::new(id, Tool::Pen, vec![a, b]));
        let top = stroke("a", Pt::new(14.0, 2.0), Pt::new(18.0, 10.0));
        let bottom = stroke("b", Pt::new(14.0, 22.0), Pt::new(18.0, 30.0));
        let bar = stroke("bar", Pt::new(5.0, 16.0), Pt::new(27.0, 16.0));
        assert_eq!(recognize(&["3 4"], &[top.clone(), bar.clone(), bottom.clone()]).as_deref(), Some("3/4"));
        assert_eq!(recognize(&["4 3"], &[bottom.clone(), bar.clone(), top.clone()]).as_deref(), Some("3/4"));
        let inline = stroke("c", Pt::new(40.0, 9.0), Pt::new(46.0, 21.0));
        assert_eq!(recognize(&["3 4"], &[top, bar, bottom, inline]), None);
    }
}
