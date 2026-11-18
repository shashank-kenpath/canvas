//! Bounded Pratt parser shared by the formula, calculator and graph.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Eq,
    Add,
    Sub,
    Mul,
    Juxtapose,
    Div,
    DivideSign,
    Pow,
    Subscript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Function {
    Sqrt,
    Sin,
    Cos,
    Tan,
    Abs,
    Ln,
    Log,
    Exp,
}

impl Function {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "sqrt" => Self::Sqrt,
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            "abs" => Self::Abs,
            "ln" => Self::Ln,
            "log" => Self::Log,
            "exp" => Self::Exp,
            _ => return None,
        })
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Sqrt => "sqrt",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Abs => "abs",
            Self::Ln => "ln",
            Self::Log => "log",
            Self::Exp => "exp",
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Expr {
    Number(String),
    Symbol(String),
    Group(char, Box<Self>),
    Call(Function, Box<Self>),
    Signed(bool, Box<Self>),
    Percent(Box<Self>),
    Binary(Op, Box<Self>, Box<Self>),
}

impl Expr {
    pub(super) fn ungrouped(&self) -> &Self {
        match self {
            Self::Group(_, inner) => inner,
            _ => self,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Number(String),
    Word(String),
    Op(Op),
    Root,
    Left(char),
    Right(char),
    Percent,
}

fn lex(source: &str) -> Result<Vec<Token>, &'static str> {
    if source.len() > 2048 {
        return Err("This expression is too long.");
    }
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        let token = match ch {
            c if c.is_whitespace() => continue,
            '=' => Token::Op(Op::Eq),
            '+' => Token::Op(Op::Add),
            '-' | '−' | '–' => Token::Op(Op::Sub),
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                Token::Op(Op::Pow)
            }
            '*' | '×' | '·' | '⋅' => Token::Op(Op::Mul),
            '/' | '⁄' => Token::Op(Op::Div),
            '÷' => Token::Op(Op::DivideSign),
            '^' => Token::Op(Op::Pow),
            '_' => Token::Op(Op::Subscript),
            '%' => Token::Percent,
            '(' | '[' | '{' => Token::Left(ch),
            ')' | ']' | '}' => Token::Right(ch),
            '√' => Token::Root,
            '∑' => Token::Word("sum".into()),
            '∫' => Token::Word("int".into()),
            '∞' => Token::Word("infty".into()),
            '.' | '0'..='9' => {
                let mut number = String::from(ch);
                let mut dot = ch == '.';
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || (c == '.' && !dot) {
                        dot |= c == '.';
                        number.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                if number == "." || chars.peek() == Some(&'.') {
                    return Err("Check the decimal point.");
                }
                if matches!(chars.peek(), Some('e' | 'E')) {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    if matches!(lookahead.peek(), Some('+' | '-')) {
                        lookahead.next();
                    }
                    if lookahead.peek().is_some_and(char::is_ascii_digit) {
                        number.push(chars.next().unwrap());
                        if matches!(chars.peek(), Some('+' | '-')) {
                            number.push(chars.next().unwrap());
                        }
                        while chars.peek().is_some_and(char::is_ascii_digit) {
                            number.push(chars.next().unwrap());
                        }
                    }
                }
                Token::Number(number)
            }
            c if c.is_alphabetic() => {
                let mut word = String::from(c);
                while chars.peek().is_some_and(|c| c.is_alphabetic()) {
                    word.push(chars.next().unwrap());
                }
                Token::Word(word)
            }
            _ => return Err("This expression contains an unknown symbol."),
        };
        tokens.push(token);
        if tokens.len() > 240 {
            return Err("This expression is too long.");
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: std::iter::Peekable<std::vec::IntoIter<Token>>,
}

impl Parser {
    fn group(&mut self, open: char) -> Result<Expr, &'static str> {
        let inner = self.expression(0)?;
        let close = match open {
            '(' => ')',
            '[' => ']',
            _ => '}',
        };
        if self.tokens.next() == Some(Token::Right(close)) {
            Ok(inner)
        } else {
            Err("Check the matching parentheses.")
        }
    }

    fn atom(&mut self) -> Result<Expr, &'static str> {
        Ok(match self.tokens.next() {
            Some(Token::Number(number)) => Expr::Number(number),
            Some(Token::Word(word)) => match (Function::named(&word), self.tokens.peek()) {
                (Some(function), Some(Token::Left(open))) => {
                    let open = *open;
                    self.tokens.next();
                    Expr::Call(function, Box::new(self.group(open)?))
                }
                _ => Expr::Symbol(word),
            },
            Some(Token::Left(open)) => Expr::Group(open, Box::new(self.group(open)?)),
            Some(Token::Root) => Expr::Call(Function::Sqrt, Box::new(self.atom()?)),
            Some(Token::Op(op @ (Op::Add | Op::Sub))) => {
                Expr::Signed(op == Op::Sub, Box::new(self.expression(46)?))
            }
            _ => return Err("Expected a number, variable or expression."),
        })
    }

    fn expression(&mut self, min: u8) -> Result<Expr, &'static str> {
        let mut left = self.atom()?;
        while let Some(token) = self.tokens.peek() {
            let (op, binding) = match token {
                Token::Op(Op::Eq) => (Op::Eq, 10),
                Token::Op(op @ (Op::Add | Op::Sub)) => (*op, 20),
                Token::Op(op @ (Op::Mul | Op::Div | Op::DivideSign)) => (*op, 30),
                Token::Number(_) | Token::Word(_) | Token::Left(_) | Token::Root => {
                    if matches!((&left, token), (Expr::Number(_), Token::Number(_))) {
                        return Err("Separate adjacent numbers with an operator.");
                    }
                    (Op::Juxtapose, 35)
                }
                Token::Percent if min <= 45 => {
                    self.tokens.next();
                    left = Expr::Percent(Box::new(left));
                    continue;
                }
                Token::Op(Op::Subscript) => (Op::Subscript, 49),
                Token::Op(Op::Pow) => (Op::Pow, 50),
                _ => break,
            };
            if binding < min {
                break;
            }
            if op != Op::Juxtapose {
                self.tokens.next();
            }
            let right = match op {
                Op::Subscript => self.atom()?,
                Op::Pow => self.expression(binding)?,
                _ => self.expression(binding + 1)?,
            };
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
}

pub(super) fn formula(source: &str) -> Result<(Expr, bool), &'static str> {
    let mut tokens = lex(source)?;
    let equals = tokens.last() == Some(&Token::Op(Op::Eq));
    if equals {
        tokens.pop();
    }
    let mut parser = Parser { tokens: tokens.into_iter().peekable() };
    let expression = parser.expression(0)?;
    if parser.tokens.peek().is_some() {
        return Err("Check the expression or parentheses.");
    }
    Ok((expression, equals))
}

pub(super) fn symbol(name: &str) -> &str {
    match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ",
        "epsilon" => "ε", "zeta" => "ζ", "eta" => "η", "theta" => "θ",
        "iota" => "ι", "kappa" => "κ", "lambda" => "λ", "mu" => "μ",
        "nu" => "ν", "xi" => "ξ", "omicron" => "ο", "pi" => "π",
        "rho" => "ρ", "sigma" => "σ", "tau" => "τ", "upsilon" => "υ",
        "phi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Alpha" => "Α", "Beta" => "Β", "Gamma" => "Γ", "Delta" => "Δ",
        "Epsilon" => "Ε", "Zeta" => "Ζ", "Eta" => "Η", "Theta" => "Θ",
        "Iota" => "Ι", "Kappa" => "Κ", "Lambda" => "Λ", "Mu" => "Μ",
        "Nu" => "Ν", "Xi" => "Ξ", "Omicron" => "Ο", "Pi" => "Π",
        "Rho" => "Ρ", "Sigma" => "Σ", "Tau" => "Τ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Chi" => "Χ", "Psi" => "Ψ", "Omega" => "Ω",
        "sum" => "∑", "int" | "integral" => "∫", "infty" => "∞",
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symbolic_equations_and_both_limit_orders() {
        let (expr, equals) = formula("x^2 + 1/2 = sqrt(y)").unwrap();
        assert!(!equals);
        assert!(matches!(expr, Expr::Binary(Op::Eq, _, right) if
            matches!(*right, Expr::Call(Function::Sqrt, _))));
        for source in ["sum_(i=1)^n i", "∫^1_0 x dx", "alpha_2 + θ"] {
            assert!(formula(source).is_ok(), "{source}");
        }
        assert_eq!(symbol("Theta"), "Θ");
    }

    #[test]
    fn rejects_incomplete_malformed_and_oversized_sources() {
        for source in ["", "=", "2..3", "3 4", "2^^5", "sqrt()", "x = (2}", "f(2", "2+", "5==", "2 # 3"] {
            assert!(formula(source).is_err(), "{source}");
        }
        assert!(formula(&"√".repeat(300)).is_err());
        assert!(formula(&"(".repeat(240)).is_err());
    }
}
