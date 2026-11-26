//! Recursive-descent quick-math evaluator for the launcher search bar.

#[derive(Debug, Clone, PartialEq)]
pub struct MathResult {
    pub expression: String,
    pub formatted: String,
    pub value: f64,
}

/// Attempts to evaluate a user search query as a mathematical expression.
///
/// To avoid hijacking normal app searches like `"2026"` or `"7zip"`, an expression
/// is only evaluated if it starts with `=` or contains at least one explicit
/// operator/function/parenthesis.
pub fn try_eval(input: &str) -> Option<MathResult> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (forced, expr_str) = if let Some(rest) = trimmed.strip_prefix('=') {
        (true, rest.trim())
    } else {
        (false, trimmed)
    };

    if expr_str.is_empty() {
        return None;
    }

    let tokens = tokenize(expr_str)?;
    if tokens.is_empty() {
        return None;
    }

    if !forced && !has_operator_or_func(&tokens) {
        return None;
    }

    let mut parser = Parser::new(&tokens);
    let value = parser.parse_expr()?;
    if !parser.is_at_end() || !value.is_finite() {
        return None;
    }

    let formatted = format_number(value);
    Some(MathResult {
        expression: expr_str.to_string(),
        formatted,
        value,
    })
}

fn format_number(val: f64) -> String {
    // Normalize -0.0
    let val = if val.abs() < 1e-12 { 0.0 } else { val };

    // Exact integer check up to 1e15
    if val.abs() < 1e15 && (val - val.round()).abs() < 1e-10 {
        return format!("{}", val.round() as i64);
    }

    // Format up to 8 decimal places and strip trailing zeros
    let s = format!("{:.8}", val);
    if s.contains('.') {
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    } else {
        s
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
}

fn has_operator_or_func(tokens: &[Token]) -> bool {
    // If there's only a single number (or unary +/- number), skip unless forced with `=`
    let mut has_op = false;
    for (idx, tok) in tokens.iter().enumerate() {
        match tok {
            Token::Plus | Token::Minus => {
                if idx > 0 {
                    has_op = true;
                }
            }
            Token::Star | Token::Slash | Token::Percent | Token::Caret | Token::LParen => {
                has_op = true;
            }
            Token::Ident(_) if tokens.len() > 1 => {
                has_op = true;
            }
            _ => {}
        }
    }
    has_op
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        match ch {
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '0' if i + 1 < bytes.len()
                && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') =>
            {
                i += 2;
                let start = i;
                while i < bytes.len()
                    && ((bytes[i] as char).is_ascii_hexdigit() || bytes[i] == b'_')
                {
                    i += 1;
                }
                let hex_str: String = input[start..i].chars().filter(|&c| c != '_').collect();
                if hex_str.is_empty() {
                    return None;
                }
                let num = u64::from_str_radix(&hex_str, 16).ok()? as f64;
                tokens.push(Token::Number(num));
            }
            '0'..='9' | '.' => {
                let start = i;
                let mut seen_dot = ch == '.';
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i] as char;
                    if c.is_ascii_digit() || c == '_' || c == ',' {
                        i += 1;
                    } else if c == '.' && !seen_dot {
                        seen_dot = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                // Optional scientific notation: e.g. 1e5, 2.5e-3
                if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                    let save = i;
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                        i += 1;
                    }
                    let exp_start = i;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                        i += 1;
                    }
                    if i == exp_start {
                        i = save; // backtrack if not an exponent (e.g. `2e` -> `2` then `e` ident)
                    }
                }
                let num_str: String = input[start..i]
                    .chars()
                    .filter(|&c| c != '_' && c != ',')
                    .collect();
                let num: f64 = num_str.parse().ok()?;
                tokens.push(Token::Number(num));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                tokens.push(Token::Ident(input[start..i].to_ascii_lowercase()));
            }
            _ => return None,
        }
    }

    Some(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    fn parse_expr(&mut self) -> Option<f64> {
        self.parse_add_sub()
    }

    fn parse_add_sub(&mut self) -> Option<f64> {
        let mut lhs = self.parse_mul_div()?;
        while let Some(tok) = self.peek() {
            match tok {
                Token::Plus => {
                    self.next();
                    lhs += self.parse_mul_div()?;
                }
                Token::Minus => {
                    self.next();
                    lhs -= self.parse_mul_div()?;
                }
                _ => break,
            }
        }
        Some(lhs)
    }

    fn parse_mul_div(&mut self) -> Option<f64> {
        let mut lhs = self.parse_unary()?;
        while let Some(tok) = self.peek() {
            match tok {
                Token::Star => {
                    self.next();
                    lhs *= self.parse_unary()?;
                }
                Token::Slash => {
                    self.next();
                    let rhs = self.parse_unary()?;
                    if rhs == 0.0 {
                        return None;
                    }
                    lhs /= rhs;
                }
                Token::Percent => {
                    self.next();
                    let rhs = self.parse_unary()?;
                    if rhs == 0.0 {
                        return None;
                    }
                    lhs %= rhs;
                }
                _ => break,
            }
        }
        Some(lhs)
    }

    fn parse_unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Token::Plus) => {
                self.next();
                self.parse_unary()
            }
            Some(Token::Minus) => {
                self.next();
                Some(-self.parse_unary()?)
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Option<f64> {
        let base = self.parse_primary()?;
        if matches!(self.peek(), Some(Token::Caret)) {
            self.next();
            // Right-associative exponentiation
            let exp = self.parse_unary()?;
            Some(base.powf(exp))
        } else {
            Some(base)
        }
    }

    fn parse_primary(&mut self) -> Option<f64> {
        match self.next()?.clone() {
            Token::Number(n) => Some(n),
            Token::LParen => {
                let val = self.parse_expr()?;
                if !matches!(self.next(), Some(Token::RParen)) {
                    return None;
                }
                Some(val)
            }
            Token::Ident(name) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.next(); // consume '('
                    let arg = self.parse_expr()?;
                    if !matches!(self.next(), Some(Token::RParen)) {
                        return None;
                    }
                    eval_func(&name, arg)
                } else {
                    match name.as_str() {
                        "pi" => Some(std::f64::consts::PI),
                        "e" => Some(std::f64::consts::E),
                        _ => None,
                    }
                }
            }
            _ => None,
        }
    }
}

fn eval_func(name: &str, arg: f64) -> Option<f64> {
    match name {
        "sqrt" => Some(arg.sqrt()),
        "abs" => Some(arg.abs()),
        "round" => Some(arg.round()),
        "floor" => Some(arg.floor()),
        "ceil" => Some(arg.ceil()),
        "sin" => Some(arg.sin()),
        "cos" => Some(arg.cos()),
        "tan" => Some(arg.tan()),
        "ln" => Some(arg.ln()),
        "log" | "log10" => Some(arg.log10()),
        "log2" => Some(arg.log2()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_arithmetic_and_precedence() {
        assert_eq!(try_eval("24*12").unwrap().formatted, "288");
        assert_eq!(try_eval("2 + 3 * 4").unwrap().formatted, "14");
        assert_eq!(try_eval("(2 + 3) * 4").unwrap().formatted, "20");
        assert_eq!(try_eval("= 2 + 2").unwrap().formatted, "4");
        assert_eq!(try_eval("-5 + 12").unwrap().formatted, "7");
        assert_eq!(try_eval("2^3^2").unwrap().formatted, "512");
        assert_eq!(try_eval("10 % 3").unwrap().formatted, "1");
    }

    #[test]
    fn functions_constants_and_hex() {
        assert_eq!(try_eval("sqrt(144)").unwrap().formatted, "12");
        assert_eq!(try_eval("abs(-42.5)").unwrap().formatted, "42.5");
        assert_eq!(try_eval("0xff + 1").unwrap().formatted, "256");
        assert_eq!(try_eval("=0x10").unwrap().formatted, "16");
        assert_eq!(try_eval("=pi").unwrap().formatted, "3.14159265");
        assert_eq!(try_eval("round(3.6)").unwrap().formatted, "4");
        assert_eq!(try_eval("1_000 * 2").unwrap().formatted, "2000");
    }

    #[test]
    fn ignores_non_math_and_bare_numbers() {
        assert!(try_eval("2026").is_none());
        assert!(try_eval("7zip").is_none());
        assert!(try_eval("firefox").is_none());
        assert!(try_eval("1 / 0").is_none());
        assert!(try_eval("=").is_none());
        assert!(try_eval("").is_none());
    }
}
