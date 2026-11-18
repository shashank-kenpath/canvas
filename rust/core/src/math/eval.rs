use super::parse::{Expr, Function, Op};

#[derive(Clone, Copy)]
pub(super) struct Ratio {
    pub numerator: i128,
    pub denominator: i128,
    decimal: bool,
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Ratio {
    fn new(n: i128, d: i128, decimal: bool) -> Option<Self> {
        if d == 0 || n == i128::MIN || d == i128::MIN {
            return None;
        }
        let factor = gcd(n, d) * d.signum();
        Some(Self { numerator: n / factor, denominator: d / factor, decimal })
    }

    fn parse(source: &str) -> Option<Self> {
        let (mantissa, exponent) = match source.split_once(['e', 'E']) {
            Some((number, power)) => (number, power.parse::<i32>().ok()?),
            None => (source, 0),
        };
        let digits = mantissa.replace('.', "").parse::<i128>().ok()?;
        let places = mantissa.split_once('.').map_or(0, |(_, tail)| tail.len() as i32);
        let scale = places.checked_sub(exponent)?;
        let power = 10_i128.checked_pow(scale.unsigned_abs())?;
        let (n, d) = if scale < 0 { (digits.checked_mul(power)?, 1) } else { (digits, power) };
        Self::new(n, d, scale > 0 || mantissa.contains('.'))
    }

    fn apply(self, op: Op, other: Self) -> Option<Self> {
        let (a, b, c, d) = (self.numerator, self.denominator, other.numerator, other.denominator);
        let decimal = self.decimal || other.decimal;
        match op {
            Op::Add | Op::Sub => {
                let factor = gcd(b, d);
                let left = a.checked_mul(d / factor)?;
                let right = c.checked_mul(b / factor)?;
                let total = if op == Op::Add { left.checked_add(right)? } else { left.checked_sub(right)? };
                let reduced = Self::new(total, factor, decimal)?;
                Self::new(reduced.numerator, (b / factor).checked_mul(d / factor)?.checked_mul(reduced.denominator)?, decimal)
            }
            Op::Mul | Op::Juxtapose => {
                let left = gcd(a, d);
                let right = gcd(c, b);
                Self::new((a / left).checked_mul(c / right)?, (b / right).checked_mul(d / left)?, decimal)
            }
            Op::Div | Op::DivideSign => self.apply(Op::Mul, Self::new(d, c, decimal)?),
            Op::Pow if d == 1 => {
                let power = u32::try_from(c.unsigned_abs()).ok()?;
                let (n, d) = (a.checked_pow(power)?, b.checked_pow(power)?);
                if c < 0 { Self::new(d, n, decimal) } else { Self::new(n, d, decimal) }
            }
            _ => None,
        }
    }

    fn sqrt(self) -> Option<Self> {
        let n = u128::try_from(self.numerator).ok()?;
        let d = self.denominator as u128;
        let (a, b) = (n.isqrt(), d.isqrt());
        (a * a == n && b * b == d).then(|| Self::new(a as i128, b as i128, self.decimal)).flatten()
    }

    fn real(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    pub(super) fn decimal_string(self) -> Option<String> {
        if self.denominator == 1 {
            return Some(self.numerator.to_string());
        }
        if !self.decimal {
            return None;
        }
        let mut rest = self.denominator;
        for factor in [2, 5] {
            while rest % factor == 0 {
                rest /= factor;
            }
        }
        if rest != 1 {
            return None;
        }
        let n = self.numerator.unsigned_abs();
        let d = self.denominator as u128;
        let mut remaining = n % d;
        let sign = if self.numerator < 0 { "-" } else { "" };
        let mut label = format!("{sign}{}.", n / d);
        while remaining != 0 {
            // Long division without overflowing when the denominator is large.
            let previous = remaining;
            remaining = 0;
            let mut digit = 0;
            for _ in 0..10 {
                remaining += previous;
                if remaining >= d {
                    remaining -= d;
                    digit += 1;
                }
            }
            label.push(char::from(b'0' + digit));
        }
        Some(label)
    }
}

pub(super) enum Number {
    Exact(Ratio),
    Real(f64),
}

impl Number {
    fn real(&self) -> f64 {
        match self {
            Self::Exact(ratio) => ratio.real(),
            Self::Real(value) => *value,
        }
    }

    fn apply(self, op: Op, right: Self) -> Self {
        if let (Self::Exact(a), Self::Exact(b)) = (&self, &right) {
            if let Some(exact) = a.apply(op, *b) {
                return Self::Exact(exact);
            }
        }
        Self::Real(binary(op, self.real(), right.real()))
    }

    fn call(self, function: Function) -> Self {
        if let Self::Exact(ratio) = &self {
            match function {
                Function::Abs => return Self::Exact(Ratio { numerator: ratio.numerator.abs(), ..*ratio }),
                Function::Sqrt => {
                    if let Some(exact) = ratio.sqrt() {
                        return Self::Exact(exact);
                    }
                }
                _ => {}
            }
        }
        let argument = self.real();
        let mut result = call(function, argument, true);
        if matches!(function, Function::Sin | Function::Cos | Function::Tan) {
            if result.abs() < 1e-15 {
                result = 0.0;
            } else if (result.abs() - 1.0).abs() < 1e-15 {
                result = result.signum();
            }
        }
        Self::Real(result)
    }
}

fn binary(op: Op, a: f64, b: f64) -> f64 {
    if !a.is_finite() || !b.is_finite() {
        return f64::NAN;
    }
    match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul | Op::Juxtapose => a * b,
        Op::Div | Op::DivideSign => a / b,
        Op::Pow => a.powf(b),
        _ => f64::NAN,
    }
}

fn call(function: Function, argument: f64, degrees: bool) -> f64 {
    if !argument.is_finite() {
        return f64::NAN;
    }
    let angle = if degrees { argument.rem_euclid(360.0).to_radians() } else { argument };
    match function {
        Function::Sqrt => argument.sqrt(),
        Function::Sin => angle.sin(),
        Function::Cos => angle.cos(),
        Function::Tan if degrees && angle.cos().abs() <= f64::EPSILON => f64::NAN,
        Function::Tan => angle.tan(),
        Function::Abs => argument.abs(),
        Function::Ln => argument.ln(),
        Function::Log => argument.log10(),
        Function::Exp => argument.exp(),
    }
}

fn constant(word: &str) -> Option<f64> {
    match word {
        "pi" | "π" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        _ => None,
    }
}

pub(super) fn numeric(expr: &Expr, allow_x: bool) -> bool {
    match expr {
        Expr::Number(_) => true,
        Expr::Symbol(word) => constant(word).is_some() || (allow_x && word == "x"),
        Expr::Group(_, inner) | Expr::Call(_, inner) | Expr::Signed(_, inner) | Expr::Percent(inner) => numeric(inner, allow_x),
        Expr::Binary(Op::Eq | Op::Subscript, _, _) => false,
        Expr::Binary(_, left, right) => numeric(left, allow_x) && numeric(right, allow_x),
    }
}

fn evaluate(expr: &Expr) -> Number {
    match expr {
        Expr::Number(source) => Ratio::parse(source).map_or_else(
            || Number::Real(source.parse().unwrap_or(f64::NAN)), Number::Exact),
        Expr::Symbol(word) => Number::Real(constant(word).unwrap_or(f64::NAN)),
        Expr::Group(_, inner) | Expr::Signed(false, inner) => evaluate(inner),
        Expr::Signed(true, inner) => match evaluate(inner) {
            Number::Exact(ratio) => Number::Exact(Ratio { numerator: -ratio.numerator, ..ratio }),
            Number::Real(value) => Number::Real(-value),
        },
        Expr::Percent(inner) => evaluate(inner).apply(
            Op::Mul, Number::Exact(Ratio { numerator: 1, denominator: 100, decimal: true })),
        Expr::Call(function, inner) => evaluate(inner).call(*function),
        Expr::Binary(op, a, b) => evaluate(a).apply(*op, evaluate(b)),
    }
}

pub(super) fn calculate(expr: &Expr) -> Option<Number> {
    if !numeric(expr, false) {
        return None;
    }
    let value = evaluate(expr);
    value.real().is_finite().then_some(value)
}

pub(super) fn real_label(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let scientific = format!("{value:.11e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let power = exponent.parse::<i32>().unwrap();
    if (-6..12).contains(&power) {
        let decimal = format!("{value:.digits$}", digits = (11 - power).max(0) as usize);
        if let Some((_, fraction)) = decimal.split_once('.') {
            if !fraction.is_empty() {
                return decimal.trim_end_matches('0').trim_end_matches('.').into();
            }
        }
        return decimal;
    }
    format!("{}e{power}", mantissa.trim_end_matches('0').trim_end_matches('.'))
}

/// Compiled floating-point expression used when sampling many x values.
pub(super) enum RealExpr {
    Constant(f64),
    X,
    Call(Function, Box<Self>),
    Binary(Op, Box<Self>, Box<Self>),
}

pub(super) enum Pole<'a> {
    Zero(&'a RealExpr),
    Tangent(&'a RealExpr),
}

impl Pole<'_> {
    pub(super) fn crosses(&self, x: [f64; 3]) -> bool {
        let values = match self {
            Self::Zero(expr) => x.map(|x| expr.at(x)),
            Self::Tangent(expr) => x.map(|x| expr.at(x).cos()),
        };
        values.contains(&0.0) || values.windows(2).any(|pair| {
            pair[0].is_finite() && pair[1].is_finite()
                && pair[0].is_sign_negative() != pair[1].is_sign_negative()
        })
    }
}

impl RealExpr {
    pub(super) fn compile(expr: &Expr) -> Option<Self> {
        let compile = |e| Self::compile(e).map(Box::new);
        let constant_node = |value| Box::new(Self::Constant(value));
        Some(match expr {
            Expr::Number(source) => Self::Constant(source.parse::<f64>().ok().filter(|n| n.is_finite())?),
            Expr::Symbol(variable) if variable == "x" => Self::X,
            Expr::Symbol(word) => Self::Constant(constant(word)?),
            Expr::Group(_, inner) | Expr::Signed(false, inner) => Self::compile(inner)?,
            Expr::Signed(true, inner) => match Self::compile(inner)? {
                Self::Constant(value) => Self::Constant(-value),
                inner => Self::Binary(Op::Sub, constant_node(0.0), Box::new(inner)),
            },
            Expr::Percent(inner) => Self::Binary(Op::Mul, compile(inner)?, constant_node(0.01)),
            Expr::Call(func, inner) => Self::Call(*func, compile(inner)?),
            Expr::Binary(Op::Eq | Op::Subscript, _, _) => return None,
            Expr::Binary(op, a, b) => Self::Binary(*op, compile(a)?, compile(b)?),
        })
    }

    pub(super) fn at(&self, x: f64) -> f64 {
        match self {
            Self::Constant(value) => *value,
            Self::X => x,
            Self::Call(function, arg) => call(*function, arg.at(x), false),
            Self::Binary(op, a, b) => binary(*op, a.at(x), b.at(x)),
        }
    }

    pub(super) fn poles<'a>(&'a self, poles: &mut Vec<Pole<'a>>) {
        match self {
            Self::Call(function, argument) => {
                if *function == Function::Tan {
                    poles.push(Pole::Tangent(argument));
                }
                argument.poles(poles);
            }
            Self::Binary(op, a, b) => {
                match op {
                    Op::Div | Op::DivideSign => b.zeroes(poles),
                    Op::Pow if matches!(**b, Self::Constant(exp) if exp < 0.0) => a.zeroes(poles),
                    _ => {}
                }
                a.poles(poles);
                b.poles(poles);
            }
            _ => {}
        }
    }

    fn zeroes<'a>(&'a self, poles: &mut Vec<Pole<'a>>) {
        match self {
            Self::Constant(_) => {}
            Self::Call(Function::Abs, inner) => inner.zeroes(poles),
            Self::Binary(Op::Pow, inner, exp) if matches!(**exp, Self::Constant(exp) if exp > 0.0) => inner.zeroes(poles),
            Self::Binary(Op::Mul | Op::Juxtapose, a, b) => {
                a.zeroes(poles);
                b.zeroes(poles);
            }
            _ => poles.push(Pole::Zero(self)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::parse::formula;

    fn result(source: &str) -> Option<String> {
        match calculate(&formula(source).unwrap().0)? {
            Number::Exact(ratio) => Some(ratio.decimal_string()
                .unwrap_or_else(|| format!("{}/{}", ratio.numerator, ratio.denominator))),
            Number::Real(value) => Some(real_label(value)),
        }
    }

    #[test]
    fn precedence_and_exact_arithmetic() {
        for (source, value) in [
            ("12 × 7 + 3 =", "87"), ("-2^2", "-4"), ("(-2)^2", "4"),
            ("2^3^2", "512"), ("2^-3", "1/8"), ("2^3%", "0.08"),
            ("1/6 + 1/3", "1/2"), ("1/(-2)", "-1/2"), ("(3/7)/(9/14)", "2/3"),
            ("0.1 + .2", "0.3"), ("3e-4 + 2.1e-4", "0.00051"), ("0.01/3", "1/300"),
            ("25%*80", "20"), ("sqrt(81/100)", "9/10"), ("sqrt(.0625)", "0.25"),
            ("10000000000000000001 + 1", "10000000000000000002"),
            ("1/(2+3)", "1/5"), ("2(3+4)", "14"), ("20 ÷ 5 − 3", "1"),
            ("1/99999999999999999999 - 1/99999999999999999999", "0"),
        ] {
            assert_eq!(result(source).as_deref(), Some(value), "{source}");
        }
    }

    #[test]
    fn degree_trigonometry_and_domain_checks() {
        for (source, value) in [
            ("sin(30)", "0.5"), ("cos(360)", "1"), ("tan(45)", "1"),
            ("sin(-270)", "1"), ("cos(90)", "0"), ("2pi", "6.28318530718"),
        ] {
            assert_eq!(result(source).as_deref(), Some(value));
        }
        for source in ["tan(90)", "tan(270)", "sqrt(-1)", "1/0", "x+2", "3=3"] {
            assert!(result(source).is_none(), "{source}");
        }
    }
}
