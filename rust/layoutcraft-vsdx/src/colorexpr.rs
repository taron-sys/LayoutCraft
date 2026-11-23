//! Typed evaluator for ShapeSheet colour expressions.
//!
//! Evidence: the formula text is read from files (stencil and theme shapes carry it as cell formulas); the function set is the subset those files use
//! (`THEMEVAL`, `LUM`, `LUMDIFF`, `TINT` / `MSOTINT`, `SHADE`, `IF`, ...). The unit tests below pin the semantics; `tests/examples_theme.rs` (private corpus)
//! expects the colours Visio itself evaluated for real files (cell oracle, Visio 16.0.14332).
//!
//! Stencils pick text and line colours with formulas such as
//! `IF(ABS(LUMDIFF(THEMEVAL("BackgroundColor"),THEMEVAL()))<40,
//!    IF(LUM(THEMEVAL("BackgroundColor"))>120,User.DarkerColor,THEMEVAL("Light")),
//!    THEMEVAL())`.
//! The plain numeric evaluator in `formula.rs` cannot represent colours, so this
//! module evaluates numbers, colours and strings together. Only the branch an
//! `IF` selects is evaluated, so references in the other branch may be missing.
use crate::styles::{tint, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Num(f64),
    /// `#rrggbb`, lower case.
    Color(String),
    Str(String),
}

impl Val {
    fn num(&self) -> Option<f64> {
        match self {
            Val::Num(value) => Some(*value),
            Val::Color(_) => None,
            Val::Str(text) => text.trim().parse().ok(),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Val::Num(value) => *value != 0.0,
            Val::Color(_) => true,
            Val::Str(text) => !text.is_empty() && !text.eq_ignore_ascii_case("false"),
        }
    }
}

/// Resolves a cell reference (`User.DarkerColor`, `Sheet.5!User.X`, `FillForegnd`).
pub type CellLookup<'a> = dyn FnMut(&str) -> Option<Val> + 'a;

/// Evaluate `formula` to a colour. `bare_themeval` is the colour a `THEMEVAL()`
/// with no arguments resolves to in the cell being evaluated (its theme role).
pub fn evaluate_color(
    formula: &str,
    theme: &Theme,
    bare_themeval: Option<&str>,
    lookup: &mut CellLookup<'_>,
) -> Option<String> {
    let mut parser = Parser {
        chars: formula.chars().collect(),
        index: 0,
        skip: false,
        theme,
        bare: bare_themeval,
        lookup,
    };
    let value = parser.expression()?;
    parser.whitespace();
    if parser.index != parser.chars.len() {
        return None;
    }
    match value {
        Val::Color(hex) => Some(hex),
        _ => None,
    }
}

/// Lightness of `#rrggbb` on Visio's 0-255 HSL scale.
pub fn luminance(hex: &str) -> Option<f64> {
    let (r, g, b) = rgb(hex)?;
    let max = r.max(g).max(b) as f64;
    let min = r.min(g).min(b) as f64;
    Some((max + min) / 2.0)
}

fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

struct Parser<'a, 'b> {
    chars: Vec<char>,
    index: usize,
    skip: bool,
    theme: &'a Theme,
    bare: Option<&'a str>,
    lookup: &'a mut CellLookup<'b>,
}

impl Parser<'_, '_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.index += 1;
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.whitespace();
        let token: Vec<char> = token.chars().collect();
        if self.chars[self.index..].starts_with(&token) {
            self.index += token.len();
            true
        } else {
            false
        }
    }

    /// Placeholder used while parsing a branch that is not taken.
    fn dummy(&self) -> Option<Val> {
        Some(Val::Num(0.0))
    }

    fn expression(&mut self) -> Option<Val> {
        let left = self.additive()?;
        for op in ["<>", "<=", ">=", "=", "<", ">"] {
            if self.eat(op) {
                let right = self.additive()?;
                if self.skip {
                    return self.dummy();
                }
                let result = match (&left, &right) {
                    (Val::Color(a), Val::Color(b)) => match op {
                        "=" => a == b,
                        "<>" => a != b,
                        _ => return None,
                    },
                    _ => {
                        let (a, b) = (left.num()?, right.num()?);
                        match op {
                            "=" => a == b,
                            "<>" => a != b,
                            "<" => a < b,
                            ">" => a > b,
                            "<=" => a <= b,
                            _ => a >= b,
                        }
                    }
                };
                return Some(Val::Num(result as u8 as f64));
            }
        }
        Some(left)
    }

    fn additive(&mut self) -> Option<Val> {
        let mut value = self.multiplicative()?;
        loop {
            if self.eat("+") {
                let right = self.multiplicative()?;
                value = self.arith(&value, &right, |a, b| a + b)?;
            } else if self.peek_minus() {
                self.index += 1;
                let right = self.multiplicative()?;
                value = self.arith(&value, &right, |a, b| a - b)?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn peek_minus(&mut self) -> bool {
        self.whitespace();
        self.peek() == Some('-')
    }

    fn multiplicative(&mut self) -> Option<Val> {
        let mut value = self.unary()?;
        loop {
            if self.eat("*") {
                let right = self.unary()?;
                value = self.arith(&value, &right, |a, b| a * b)?;
            } else if self.eat("/") {
                let right = self.unary()?;
                if right.num() == Some(0.0) {
                    if self.skip {
                        value = Val::Num(0.0);
                        continue;
                    }
                    return None;
                }
                value = self.arith(&value, &right, |a, b| a / b)?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn arith(&self, a: &Val, b: &Val, op: impl Fn(f64, f64) -> f64) -> Option<Val> {
        if self.skip {
            return self.dummy();
        }
        Some(Val::Num(op(a.num()?, b.num()?)))
    }

    fn unary(&mut self) -> Option<Val> {
        if self.eat("-") {
            let value = self.unary()?;
            return Some(if self.skip {
                Val::Num(0.0)
            } else {
                Val::Num(-value.num()?)
            });
        }
        if self.eat("+") {
            return self.unary();
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<Val> {
        self.whitespace();
        if self.eat("(") {
            let value = self.expression()?;
            return self.eat(")").then_some(value);
        }
        match self.peek()? {
            '"' => self.string(),
            c if c.is_ascii_digit() || c == '.' => self.number(),
            '#' => self.hex(),
            _ => self.name_or_call(),
        }
    }

    fn string(&mut self) -> Option<Val> {
        self.index += 1;
        let start = self.index;
        while self.peek()? != '"' {
            self.index += 1;
        }
        let text: String = self.chars[start..self.index].iter().collect();
        self.index += 1;
        Some(Val::Str(text))
    }

    fn number(&mut self) -> Option<Val> {
        let start = self.index;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.index += 1;
        }
        let text: String = self.chars[start..self.index].iter().collect();
        let mut value: f64 = text.parse().ok()?;
        // Unit suffixes (`80%`, `10pt`) are accepted; only `%` changes the value.
        let unit_start = self.index;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '%')
        {
            self.index += 1;
        }
        let unit: String = self.chars[unit_start..self.index].iter().collect();
        if unit == "%" {
            value /= 100.0;
        }
        Some(Val::Num(value))
    }

    fn hex(&mut self) -> Option<Val> {
        let start = self.index;
        self.index += 1;
        while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            self.index += 1;
        }
        let text: String = self.chars[start..self.index].iter().collect();
        rgb(&text)?;
        Some(Val::Color(text.to_ascii_lowercase()))
    }

    fn name_or_call(&mut self) -> Option<Val> {
        let start = self.index;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '!' | ':'))
        {
            self.index += 1;
        }
        if self.index == start {
            return None;
        }
        let name: String = self.chars[start..self.index].iter().collect();
        self.whitespace();
        if self.peek() == Some('(') {
            self.index += 1;
            return self.call(&name);
        }
        match name.to_ascii_uppercase().as_str() {
            "TRUE" => Some(Val::Num(1.0)),
            "FALSE" => Some(Val::Num(0.0)),
            _ if self.skip => self.dummy(),
            _ => (self.lookup)(&name),
        }
    }

    fn argument(&mut self, skipped: bool) -> Option<Val> {
        let saved = self.skip;
        self.skip = saved || skipped;
        let value = self.expression();
        self.skip = saved;
        value
    }

    fn arguments(&mut self) -> Option<Vec<Val>> {
        let mut args = Vec::new();
        if self.eat(")") {
            return Some(args);
        }
        loop {
            args.push(self.argument(false)?);
            if self.eat(")") {
                return Some(args);
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    fn call(&mut self, name: &str) -> Option<Val> {
        let upper = name.to_ascii_uppercase();
        if upper == "IF" {
            let condition = self.argument(false)?;
            if !self.eat(",") {
                return None;
            }
            let taken = self.skip || condition.truthy();
            let first = self.argument(!taken)?;
            let second = if self.eat(",") {
                self.argument(taken)?
            } else {
                Val::Num(0.0)
            };
            if !self.eat(")") {
                return None;
            }
            return Some(if taken { first } else { second });
        }
        let args = self.arguments()?;
        if self.skip {
            return self.dummy();
        }
        match upper.as_str() {
            "GUARD" | "THEMEGUARD" | "EVAL" => args.into_iter().next(),
            "AND" => Some(Val::Num(args.iter().all(Val::truthy) as u8 as f64)),
            "OR" => Some(Val::Num(args.iter().any(Val::truthy) as u8 as f64)),
            "NOT" => Some(Val::Num(!args.first()?.truthy() as u8 as f64)),
            "ABS" => Some(Val::Num(args.first()?.num()?.abs())),
            "MIN" => args.iter().filter_map(Val::num).reduce(f64::min).map(Val::Num),
            "MAX" => args.iter().filter_map(Val::num).reduce(f64::max).map(Val::Num),
            "LUM" => match args.first()? {
                Val::Color(hex) => Some(Val::Num(luminance(hex)?)),
                _ => None,
            },
            "LUMDIFF" => match (args.first()?, args.get(1)?) {
                (Val::Color(a), Val::Color(b)) => Some(Val::Num(luminance(a)? - luminance(b)?)),
                _ => None,
            },
            "THEMEVAL" | "THEME" => match args.first() {
                None => self.bare.map(|hex| Val::Color(hex.to_string())),
                Some(Val::Str(key)) if key.is_empty() => {
                    self.bare.map(|hex| Val::Color(hex.to_string()))
                }
                Some(Val::Str(key)) => self
                    .theme
                    .slot(key, false)
                    .map(Val::Color)
                    .or_else(|| args.get(1).cloned()),
                Some(_) => None,
            },
            "RGB" => {
                let channel = |index: usize| -> Option<u8> {
                    Some(args.get(index)?.num()?.round().clamp(0.0, 255.0) as u8)
                };
                Some(Val::Color(format!(
                    "#{:02x}{:02x}{:02x}",
                    channel(0)?,
                    channel(1)?,
                    channel(2)?
                )))
            }
            "TINT" | "MSOTINT" => match (args.first()?, args.get(1)?.num()) {
                (Val::Color(hex), Some(amount)) => Some(Val::Color(tint(hex, amount))),
                _ => None,
            },
            "SHADE" | "MSOSHADE" => match (args.first()?, args.get(1)?.num()) {
                (Val::Color(hex), Some(amount)) => Some(Val::Color(tint(hex, -amount))),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(formula: &str, bare: Option<&str>) -> Option<String> {
        let theme = Theme::office();
        evaluate_color(formula, &theme, bare, &mut |name| match name {
            "User.DarkerColor" => Some(Val::Color("#d80073".to_string())),
            "User.Flag" => Some(Val::Num(1.0)),
            _ => None,
        })
    }

    #[test]
    fn text_callout_formula_picks_darker_colour_on_light_background() {
        let formula = "IF(ABS(LUMDIFF(THEMEVAL(\"BackgroundColor\"),THEMEVAL()))<40,IF(LUM(THEMEVAL(\"BackgroundColor\"))>120,User.DarkerColor,THEMEVAL(\"Light\")),THEMEVAL())";
        // Text colour close to the (white) background -> use the darker user colour.
        assert_eq!(eval(formula, Some("#f0f0f0")).as_deref(), Some("#d80073"));
        // Text colour far from the background -> keep the themed text colour.
        assert_eq!(eval(formula, Some("#202020")).as_deref(), Some("#202020"));
    }

    #[test]
    fn untaken_branch_may_reference_missing_cells() {
        assert_eq!(
            eval("IF(User.Flag,#112233,User.Missing)", None).as_deref(),
            Some("#112233")
        );
        assert_eq!(
            eval("IF(NOT(User.Flag),User.Missing/0,RGB(1,2,3))", None).as_deref(),
            Some("#010203")
        );
    }

    #[test]
    fn luminance_scale_is_0_to_255() {
        assert_eq!(luminance("#ffffff"), Some(255.0));
        assert_eq!(luminance("#000000"), Some(0.0));
        assert_eq!(luminance("#d80073"), Some(108.0));
    }
}
