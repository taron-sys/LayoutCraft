//! Small numeric evaluator for cached ShapeSheet formulas used by geometry.
//!
//! Evidence: the function names and operators are those of the public ShapeSheet formula language ([MS-VSDX] cell formulas); the formulas evaluated are the ones
//! found in master geometry of real files; the unit tests below pin the semantics, and the cell oracle (run-cell-oracle.ps1: COM versus importer cells) compares the evaluated cells with
//! the values Visio itself reports.
//!
//! This is deliberately a small evaluator, not a full formula engine: it covers the arithmetic,
//! comparisons, cell references and functions that master-body geometry uses
//! (`GUARD`, `IF`, `MIN`, `MAX`, `ABS`, `SQRT`, `ROUND`, `INT`, trigonometry).
use std::f64::consts::PI;

pub type Lookup<'a> = dyn FnMut(&str) -> Option<f64> + 'a;

pub fn evaluate(text: &str, lookup: &mut Lookup<'_>) -> Option<f64> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        index: 0,
        skip: false,
        lookup,
    };
    let value = parser.expression()?;
    parser.whitespace();
    if parser.index == parser.chars.len() {
        Some(value)
    } else {
        None
    }
}

enum StringOperand {
    Literal(String),
    TextLength(f64),
}

struct Parser<'a> {
    chars: Vec<char>,
    index: usize,
    /// True while parsing an untaken branch: nothing is looked up and errors
    /// (division by zero, unknown functions) are ignored, as in a lazy `IF`.
    skip: bool,
    lookup: &'a mut Lookup<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.index += 1;
        }
    }

    fn eat(&mut self, value: &str) -> bool {
        self.whitespace();
        let mut length = 0;
        for (offset, expected) in value.chars().enumerate() {
            if self.chars.get(self.index + offset) != Some(&expected) {
                return false;
            }
            length += 1;
        }
        self.index += length;
        true
    }

    fn expression(&mut self) -> Option<f64> {
        self.comparison()
    }

    fn comparison(&mut self) -> Option<f64> {
        let mut left = self.additive()?;
        loop {
            self.whitespace();
            let op = if self.eat("<>") {
                "<>"
            } else if self.eat("<=") {
                "<="
            } else if self.eat(">=") {
                ">="
            } else if self.eat("=") {
                "="
            } else if self.eat("<") {
                "<"
            } else if self.eat(">") {
                ">"
            } else {
                break;
            };
            let right = self.additive()?;
            left = match op {
                "=" => (left == right) as u8 as f64,
                "<>" => (left != right) as u8 as f64,
                "<" => (left < right) as u8 as f64,
                ">" => (left > right) as u8 as f64,
                "<=" => (left <= right) as u8 as f64,
                _ => (left >= right) as u8 as f64,
            };
        }
        Some(left)
    }

    fn additive(&mut self) -> Option<f64> {
        let mut value = self.multiplicative()?;
        loop {
            self.whitespace();
            if self.eat("+") {
                value += self.multiplicative()?;
            } else if self.eat("-") {
                value -= self.multiplicative()?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn multiplicative(&mut self) -> Option<f64> {
        let mut value = self.unary()?;
        loop {
            self.whitespace();
            if self.eat("*") {
                value *= self.unary()?;
            } else if self.eat("/") {
                let divisor = self.unary()?;
                if divisor == 0.0 {
                    if self.skip {
                        value = 0.0;
                        continue;
                    }
                    return None;
                }
                value /= divisor;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn unary(&mut self) -> Option<f64> {
        self.power()
    }

    /// Operand with optional sign. Verified in Visio 2021: unary minus binds
    /// tighter than `^` (`-2^2` = 4).
    fn signed(&mut self) -> Option<f64> {
        self.whitespace();
        if self.eat("-") {
            return Some(-self.signed()?);
        }
        if self.eat("+") {
            return self.signed();
        }
        self.primary()
    }

    /// `a ^ b ^ c`, left-associative in Visio (`2^3^2` = 64).
    fn power(&mut self) -> Option<f64> {
        let mut value = self.signed()?;
        loop {
            self.whitespace();
            if !self.eat("^") {
                return Some(value);
            }
            let exponent = self.signed()?;
            value = value.powf(exponent);
            if !value.is_finite() {
                if self.skip {
                    value = 0.0;
                } else {
                    return None;
                }
            }
        }
    }

    fn primary(&mut self) -> Option<f64> {
        self.whitespace();
        if self.eat("(") {
            let value = self.expression()?;
            if !self.eat(")") {
                return None;
            }
            return Some(value);
        }
        if self.peek() == Some('"') {
            return None;
        }
        if self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            return self.number();
        }
        let name = self.identifier()?;
        self.whitespace();
        if self.eat("(") {
            return self.call(&name);
        }
        match name.to_ascii_uppercase().as_str() {
            "TRUE" => Some(1.0),
            "FALSE" => Some(0.0),
            "PI" => Some(PI),
            _ if self.skip => Some(0.0),
            _ => (self.lookup)(&name),
        }
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.index;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.index += 1;
        }
        if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
            let saved = self.index;
            self.index += 1;
            if self.peek().is_some_and(|c| c == '+' || c == '-') {
                self.index += 1;
            }
            let digits = self.index;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.index += 1;
            }
            if self.index == digits {
                self.index = saved;
            }
        }
        let text: String = self.chars[start..self.index].iter().collect();
        let mut value = text.parse::<f64>().ok()?;
        let unit_start = self.index;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '%')
        {
            self.index += 1;
        }
        let unit: String = self.chars[unit_start..self.index].iter().collect();
        if !unit.is_empty() {
            value *= match unit.to_ascii_uppercase().as_str() {
                "IN" | "IN_F" => 1.0,
                "PT" => 1.0 / 72.0,
                "CM" => 1.0 / 2.54,
                "MM" => 1.0 / 25.4,
                "FT" => 12.0,
                "PX" => 1.0 / 96.0,
                "%" => 0.01,
                "DEG" | "DEGREE" => PI / 180.0,
                _ => 1.0,
            };
        }
        Some(value)
    }

    fn identifier(&mut self) -> Option<String> {
        self.whitespace();
        let start = self.index;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '!')
        {
            self.index += 1;
        }
        if self.index == start {
            None
        } else {
            Some(self.chars[start..self.index].iter().collect())
        }
    }

    /// Parse one argument; with `skipped`, lookups and errors are suppressed.
    fn argument(&mut self, skipped: bool) -> Option<f64> {
        let saved = self.skip;
        self.skip = saved || skipped;
        let value = self.expression();
        self.skip = saved;
        value
    }

    /// `IF`, `CHOOSE` and `SWITCH` only evaluate the branch they select.
    /// Outer `None` = not a lazy function; inner `None` = evaluation failed.
    fn lazy_call(&mut self, name: &str) -> Option<Option<f64>> {
        match name {
            "IF" => {
                let condition = self.argument(false)?;
                if !self.eat(",") {
                    return Some(None);
                }
                let taken = condition != 0.0;
                let first = self.argument(!taken)?;
                let second = if self.eat(",") {
                    self.argument(taken)?
                } else {
                    0.0
                };
                Some(self.eat(")").then_some(if taken { first } else { second }))
            }
            "CHOOSE" => {
                let index = self.argument(false)?.trunc() as i64;
                let mut chosen = None;
                let mut position = 1;
                while self.eat(",") {
                    let value = self.argument(position != index)?;
                    if position == index {
                        chosen = Some(value);
                    }
                    position += 1;
                }
                Some(if self.eat(")") { chosen } else { None })
            }
            "SWITCH" => {
                let subject = self.argument(false)?;
                let mut result = None;
                let mut default = None;
                while self.eat(",") {
                    let case = self.argument(result.is_some())?;
                    if !self.eat(",") {
                        default = Some(case);
                        break;
                    }
                    let matched = result.is_none() && case == subject;
                    let value = self.argument(!matched)?;
                    if matched {
                        result = Some(value);
                    }
                }
                Some(if self.eat(")") {
                    result.or(default)
                } else {
                    None
                })
            }
            _ => None,
        }
    }

    /// A string operand of `STRSAME`: a literal, or `SHAPETEXT(TheText)` which is only known by its
    /// length (the lookup answers `TheText.len`), enough to tell whether the shape has text.
    fn string_operand(&mut self) -> Option<StringOperand> {
        self.whitespace();
        if self.peek() == Some('"') {
            self.index += 1;
            let mut text = String::new();
            loop {
                match self.peek()? {
                    '"' if self.chars.get(self.index + 1) == Some(&'"') => {
                        text.push('"');
                        self.index += 2;
                    }
                    '"' => {
                        self.index += 1;
                        break;
                    }
                    c => {
                        text.push(c);
                        self.index += 1;
                    }
                }
            }
            return Some(StringOperand::Literal(text));
        }
        let name = self.identifier()?;
        if name.eq_ignore_ascii_case("SHAPETEXT") && self.eat("(") {
            let _cell = self.identifier()?;
            if !self.eat(")") {
                return None;
            }
            let length = if self.skip { 0.0 } else { (self.lookup)("TheText.len")? };
            return Some(StringOperand::TextLength(length));
        }
        None
    }

    fn call(&mut self, name: &str) -> Option<f64> {
        if name.eq_ignore_ascii_case("STRSAME") {
            let a = self.string_operand()?;
            if !self.eat(",") {
                return None;
            }
            let b = self.string_operand()?;
            if !self.eat(")") {
                return None;
            }
            return match (a, b) {
                (StringOperand::Literal(x), StringOperand::Literal(y)) => Some((x == y) as u8 as f64),
                (StringOperand::TextLength(n), StringOperand::Literal(y))
                | (StringOperand::Literal(y), StringOperand::TextLength(n)) => {
                    // Only emptiness is decidable from the length.
                    if y.is_empty() {
                        Some((n == 0.0) as u8 as f64)
                    } else if n == 0.0 {
                        Some(0.0)
                    } else if self.skip {
                        Some(0.0)
                    } else {
                        None
                    }
                }
                _ => None,
            };
        }
        if name.eq_ignore_ascii_case("INDEX") {
            // `INDEX(n, "a;b;c")`: the n-th (zero-based) item of a literal list, read as a number.
            let saved = self.index;
            let position = self.expression();
            if let (Some(position), true) = (position, self.eat(",")) {
                if let Some(StringOperand::Literal(list)) = self.string_operand() {
                    if self.eat(")") {
                        return list
                            .split(';')
                            .nth(position.trunc().max(0.0) as usize)
                            .and_then(|item| item.trim().parse::<f64>().ok());
                    }
                    return None;
                }
            }
            self.index = saved;
        }
        if let Some(result) = self.lazy_call(&name.to_ascii_uppercase()) {
            return result;
        }
        let mut args = Vec::new();
        self.whitespace();
        if !self.eat(")") {
            loop {
                let value = self.expression()?;
                args.push(value);
                self.whitespace();
                if self.eat(")") {
                    break;
                }
                if !self.eat(",") {
                    return None;
                }
            }
        }
        match name.to_ascii_uppercase().as_str() {
            "GUARD" | "EVAL" | "THEMEGUARD" | "SETATREFEXPR" => args.first().copied(),
            "OR" => Some(if args.iter().any(|value| *value != 0.0) {
                1.0
            } else {
                0.0
            }),
            "BITAND" => Some((*args.first()? as i64 & *args.get(1)? as i64) as f64),
            "BITOR" => Some((*args.first()? as i64 | *args.get(1)? as i64) as f64),
            "BITXOR" => Some(
                args.iter()
                    .fold(0i64, |acc, value| acc ^ (*value as i64)) as f64,
            ),
            "AND" => Some(if args.iter().all(|value| *value != 0.0) {
                1.0
            } else {
                0.0
            }),
            "NOT" => Some(if args.first().copied().unwrap_or(0.0) == 0.0 {
                1.0
            } else {
                0.0
            }),
            "MIN" => args.into_iter().reduce(f64::min),
            "MAX" => args.into_iter().reduce(f64::max),
            "ABS" => args.first().map(|value| value.abs()),
            "SQRT" => args.first().map(|value| value.sqrt()),
            "ROUND" => {
                let value = *args.first()?;
                let scale = 10f64.powi(args.get(1).copied().unwrap_or(0.0) as i32);
                Some((value * scale).round() / scale)
            }
            "INT" => args.first().map(|value| value.floor()),
            // Visio FLOOR rounds toward zero (FLOOR(-1.2) = -1).
            "FLOOR" => {
                let value = *args.first()?;
                match args.get(1).copied().filter(|multiple| *multiple != 0.0) {
                    Some(multiple) => Some((value / multiple).trunc() * multiple),
                    None => Some(value.trunc()),
                }
            }
            "TRUNC" => args.first().map(|value| value.trunc()),
            // `CEILING(x, significance)` rounds up to a multiple (calendar rows grow in 1/16 in steps).
            "CEILING" => {
                let value = *args.first()?;
                match args.get(1).copied().filter(|multiple| *multiple != 0.0) {
                    Some(multiple) => Some(((value / multiple) - 1e-9).ceil() * multiple),
                    None => Some(value.ceil()),
                }
            }
            "SIGN" => args.first().map(|value| {
                if *value == 0.0 {
                    0.0
                } else {
                    value.signum()
                }
            }),
            "MOD" => {
                let (a, b) = (*args.first()?, *args.get(1)?);
                if b == 0.0 {
                    return if self.skip { Some(0.0) } else { None };
                }
                // Visio: |a| mod |b| carrying the divisor's sign
                // (MOD(-7,3) = 1, MOD(7,-3) = -1).
                let magnitude = a.abs() % b.abs();
                Some(if b < 0.0 { -magnitude } else { magnitude })
            }
            "POW" => Some(args.first()?.powf(*args.get(1)?)),
            "EXP" => args.first().map(|value| value.exp()),
            "LN" => args.first().filter(|v| **v > 0.0).map(|v| v.ln()),
            "LOG" => args.first().filter(|v| **v > 0.0).map(|v| v.log10()),
            "ASIN" => args.first().map(|value| value.asin()),
            "ACOS" => args.first().map(|value| value.acos()),
            "ATAN" => args.first().map(|value| value.atan()),
            "DEG" => args.first().map(|value| value.to_degrees()),
            "RAD" => args.first().map(|value| value.to_radians()),
            "SUM" => Some(args.iter().sum()),
            "AVG" | "AVERAGE" if !args.is_empty() => {
                Some(args.iter().sum::<f64>() / args.len() as f64)
            }
            "SIN" => args.first().map(|value| value.sin()),
            "COS" => args.first().map(|value| value.cos()),
            "TAN" => args.first().map(|value| value.tan()),
            "ATAN2" => Some(args.first()?.atan2(*args.get(1)?)),
            "PI" => Some(PI),
            _ if self.skip => Some(0.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str) -> Option<f64> {
        evaluate(text, &mut |name| match name {
            "Width" => Some(4.0),
            "Zero" => Some(0.0),
            _ => None,
        })
    }

    #[test]
    fn ceiling_and_floor_take_a_significance() {
        // CALNDR Month sizes its rows with CEILING(text height, 1/16 in).
        assert_eq!(eval("CEILING(0.2778,0.0625)"), Some(0.3125));
        assert_eq!(eval("CEILING(0.3125,0.0625)"), Some(0.3125));
        assert_eq!(eval("CEILING(1.2)"), Some(2.0));
        assert_eq!(eval("FLOOR(0.2778,0.0625)"), Some(0.25));
        assert_eq!(eval("FLOOR(-1.2)"), Some(-1.0));
    }

    #[test]
    fn if_is_lazy() {
        assert_eq!(eval("IF(Zero=0,1,Width/Zero)"), Some(1.0));
        assert_eq!(eval("IF(Zero,Missing(1),2)"), Some(2.0));
        assert_eq!(eval("IF(Width>3,Width*2,UnknownCell)"), Some(8.0));
        assert_eq!(eval("1/Zero"), None);
    }

    #[test]
    fn nested_and_short_forms() {
        assert_eq!(eval("IF(0,1,IF(1,5,1/Zero))"), Some(5.0));
        assert_eq!(eval("CHOOSE(2,10,20,1/Zero)"), Some(20.0));
        assert_eq!(eval("SWITCH(3,1,10,3,30,99)"), Some(30.0));
        assert_eq!(eval("SWITCH(7,1,10,3,30,99)"), Some(99.0));
    }

    #[test]
    fn operators_and_functions() {
        // Expected values below were read back from Visio 16.0.14332.
        assert_eq!(eval("2^3^2"), Some(64.0));
        assert_eq!(eval("-2^2"), Some(4.0));
        assert_eq!(eval("MOD(-7,3)"), Some(1.0));
        assert_eq!(eval("MOD(7,-3)"), Some(-1.0));
        assert_eq!(eval("ROUND(3.14159,2)"), Some(3.14));
        assert_eq!(eval("ROUND(2.5,0)"), Some(3.0));
        assert_eq!(eval("ROUND(-2.5,0)"), Some(-3.0));
        assert_eq!(eval("INT(-1.5)"), Some(-2.0));
        assert_eq!(eval("FLOOR(-1.2)"), Some(-1.0));
        assert_eq!(eval("CEILING(1.2)"), Some(2.0));
        assert_eq!(eval("SUM(1,2,3)"), Some(6.0));
        assert_eq!(eval("SIGN(-9)"), Some(-1.0));
    }
}
