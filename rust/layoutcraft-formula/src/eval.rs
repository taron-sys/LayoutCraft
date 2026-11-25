//! Expression evaluation: operators, coercion and unit arithmetic.
//!
//! Sources: Microsoft Learn, "About Operators" (operators, conversion of operands, string
//! comparison), "About Units of Measure", "About Error Values" (including `#DIM!` for exponents
//! outside -128..=127 and for non-integer powers of a dimensioned value) and "About Date, Time,
//! and Duration Values" (date and duration arithmetic).

use crate::ast::*;
use crate::env::Env;
use crate::error::{ErrorValue, ParseError};
use crate::functions;
use crate::unit::Dim;
use crate::value::{Quantity, Value};
use crate::MAX_STRING_BYTES;

/// The outcome of one evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluation {
    pub value: Value,
    /// Functions that were called but are not implemented, upper-cased, first use first.
    pub unsupported: Vec<String>,
}

/// Evaluates a parsed formula. Never panics; every failure is an error *value*.
pub fn evaluate(expr: &Expr, env: &dyn Env) -> Evaluation {
    let mut evaluator = Evaluator {
        env,
        unsupported: Vec::new(),
    };
    let value = evaluator.eval(expr);
    Evaluation {
        value,
        unsupported: evaluator.unsupported,
    }
}

/// Parses and evaluates `source`.
pub fn evaluate_str(source: &str, env: &dyn Env) -> Result<Evaluation, ParseError> {
    Ok(evaluate(&crate::parse(source)?, env))
}

pub(crate) struct Evaluator<'e> {
    pub(crate) env: &'e dyn Env,
    pub(crate) unsupported: Vec<String>,
}

impl Evaluator<'_> {
    pub(crate) fn eval(&mut self, expr: &Expr) -> Value {
        match &expr.kind {
            ExprKind::Number { value, unit } => match unit {
                None => finite(Quantity::plain(*value)),
                Some(unit) => finite(Quantity::in_unit(*value, *unit)),
            },
            ExprKind::Str(text) => Value::Str(text.clone()),
            ExprKind::Bool(flag) => Value::Bool(*flag),
            ExprKind::Color(rgb) => Value::Color(*rgb),
            ExprKind::Ref(reference) => match self.env.cell(reference) {
                Ok(value) => value,
                Err(error) => Value::Error(error),
            },
            ExprKind::Unary { op, operand } => {
                let operand = self.eval(operand);
                unary(*op, &operand)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs = self.eval(lhs);
                let rhs = self.eval(rhs);
                binary(*op, &lhs, &rhs)
            }
            ExprKind::Call { name, args, .. } => functions::call(self, name, args),
            ExprKind::Empty => Value::number(0.0),
        }
    }

    pub(crate) fn note_unsupported(&mut self, name: &str) {
        let name = name.to_ascii_uppercase();
        if !self.unsupported.contains(&name) {
            self.unsupported.push(name);
        }
    }
}

/// `Number` for a finite quantity, `#NUM!` otherwise.
pub(crate) fn finite(q: Quantity) -> Value {
    if q.value.is_finite() {
        Value::Number(q)
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// Parses text as a plain number; `None` when it is not one.
pub(crate) fn parse_numeric_text(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    // `f64::from_str` also accepts "inf" and "NaN"; a cell never holds those as numbers.
    let first = trimmed.chars().next()?;
    if !(first.is_ascii_digit() || matches!(first, '+' | '-' | '.')) {
        return None;
    }
    trimmed.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Converts any value to a number the way an operator does: text that is not a number, and
/// colours, count as zero ("the typed equivalent of nothing"); error values propagate.
pub(crate) fn to_quantity(value: &Value) -> Result<Quantity, ErrorValue> {
    match value {
        Value::Number(q) => Ok(*q),
        Value::Bool(flag) => Ok(Quantity::plain(if *flag { 1.0 } else { 0.0 })),
        Value::Date(days) => Ok(Quantity::plain(*days)),
        Value::Str(text) => Ok(Quantity::plain(parse_numeric_text(text).unwrap_or(0.0))),
        Value::Color(_) => Ok(Quantity::plain(0.0)),
        Value::Point(..) => Err(ErrorValue::Value),
        Value::Error(error) => Err(*error),
    }
}

/// Truth of a value: non-zero numbers, `TRUE`, text that reads `TRUE` or a non-zero number.
pub(crate) fn truthy(value: &Value) -> Result<bool, ErrorValue> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Str(text) if text.eq_ignore_ascii_case("TRUE") => Ok(true),
        Value::Str(text) if text.eq_ignore_ascii_case("FALSE") => Ok(false),
        // Unspecified in the sources: a colour counts as TRUE (the earlier evaluator did too).
        Value::Color(_) => Ok(true),
        other => Ok(to_quantity(other)?.value != 0.0),
    }
}

fn unary(op: UnaryOp, operand: &Value) -> Value {
    let q = match to_quantity(operand) {
        Ok(q) => q,
        Err(error) => return Value::Error(error),
    };
    let value = match op {
        UnaryOp::Plus => q.value,
        UnaryOp::Minus => -q.value,
        UnaryOp::Percent => q.value / 100.0,
    };
    finite(Quantity::new(value, q.dim))
}

pub(crate) fn binary(op: BinaryOp, lhs: &Value, rhs: &Value) -> Value {
    if let Value::Error(error) = lhs {
        return Value::Error(*error);
    }
    if let Value::Error(error) = rhs {
        return Value::Error(*error);
    }
    let result = match op {
        BinaryOp::Add => add(lhs, rhs, false),
        BinaryOp::Sub => add(lhs, rhs, true),
        BinaryOp::Mul => mul(lhs, rhs),
        BinaryOp::Div => div(lhs, rhs),
        BinaryOp::Pow => pow(lhs, rhs),
        BinaryOp::Concat => concat(lhs, rhs),
        BinaryOp::Compare(cmp, precision) => compare(lhs, rhs, cmp, precision),
    };
    result.unwrap_or_else(Value::Error)
}

/// The dimension two operands of `+`, `-` or a comparison share. A dimensionless operand takes
/// the other's dimension: a bare number in a dimensional cell is in internal units (inches,
/// radians, days). Two different dimensions are `#DIM!`.
pub(crate) fn common_dim(a: Dim, b: Dim) -> Result<Dim, ErrorValue> {
    if a == b || b.is_none() {
        Ok(a)
    } else if a.is_none() {
        Ok(b)
    } else {
        Err(ErrorValue::Dim)
    }
}

fn add(lhs: &Value, rhs: &Value, subtract: bool) -> Result<Value, ErrorValue> {
    // Date arithmetic: date -/+ duration is a date, date - date is a duration, date + date is a
    // date (as documented), duration -/+ date is a date.
    if matches!(lhs, Value::Date(_)) || matches!(rhs, Value::Date(_)) {
        let (a, b) = (to_quantity(lhs)?.value, to_quantity(rhs)?.value);
        let value = if subtract { a - b } else { a + b };
        let both_dates = matches!((lhs, rhs), (Value::Date(_), Value::Date(_)));
        let result = if subtract && both_dates {
            Value::duration(value)
        } else {
            Value::Date(value)
        };
        return if value.is_finite() {
            Ok(result)
        } else {
            Err(ErrorValue::Num)
        };
    }
    let (a, b) = (to_quantity(lhs)?, to_quantity(rhs)?);
    let dim = common_dim(a.dim, b.dim)?;
    let value = if subtract {
        a.value - b.value
    } else {
        a.value + b.value
    };
    checked(value, dim)
}

fn checked(value: f64, dim: Dim) -> Result<Value, ErrorValue> {
    if value.is_finite() {
        Ok(Value::Number(Quantity::new(value, dim)))
    } else {
        Err(ErrorValue::Num)
    }
}

fn mul(lhs: &Value, rhs: &Value) -> Result<Value, ErrorValue> {
    let (a, b) = (to_quantity(lhs)?, to_quantity(rhs)?);
    let dim = a.dim.checked_add(b.dim).ok_or(ErrorValue::Dim)?;
    checked(a.value * b.value, dim)
}

fn div(lhs: &Value, rhs: &Value) -> Result<Value, ErrorValue> {
    let (a, b) = (to_quantity(lhs)?, to_quantity(rhs)?);
    if b.value == 0.0 {
        return Err(ErrorValue::DivZero);
    }
    let dim = a.dim.checked_sub(b.dim).ok_or(ErrorValue::Dim)?;
    checked(a.value / b.value, dim)
}

fn pow(lhs: &Value, rhs: &Value) -> Result<Value, ErrorValue> {
    let (base, exponent) = (to_quantity(lhs)?, to_quantity(rhs)?);
    if !exponent.dim.is_none() {
        return Err(ErrorValue::Dim);
    }
    let dim = if base.dim.is_none() {
        Dim::NONE
    } else {
        // A dimensioned base needs a whole-number exponent so the result has a whole dimension.
        let whole = exponent.value.fract() == 0.0 && exponent.value.abs() <= f64::from(i32::MAX);
        if !whole {
            return Err(ErrorValue::Dim);
        }
        base.dim
            .checked_scale(exponent.value as i32)
            .ok_or(ErrorValue::Dim)?
    };
    checked(base.value.powf(exponent.value), dim)
}

fn concat(lhs: &Value, rhs: &Value) -> Result<Value, ErrorValue> {
    let (a, b) = (lhs.to_text(), rhs.to_text());
    if a.len() + b.len() > MAX_STRING_BYTES {
        return Err(ErrorValue::Value);
    }
    Ok(Value::Str(a + &b))
}

/// Absolute tolerance of the symbolic comparison operators.
///
/// Unspecified in the sources: the documentation only says the word forms compare all 15 stored
/// digits while the symbols do not. 1e-9 is the documented default fuzz of `SIGN`.
const TOLERANCE: f64 = 1e-9;

/// `value` rounded to 15 significant digits, the precision the documentation says values are stored with.
pub(crate) fn round15(value: f64) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    format!("{value:.14e}").parse().unwrap_or(value)
}

fn compare(
    lhs: &Value,
    rhs: &Value,
    cmp: CmpOp,
    precision: Precision,
) -> Result<Value, ErrorValue> {
    if let (Value::Color(a), Value::Color(b)) = (lhs, rhs) {
        return match cmp {
            CmpOp::Eq => Ok(Value::Bool(a == b)),
            CmpOp::Ne => Ok(Value::Bool(a != b)),
            _ => Err(ErrorValue::Value),
        };
    }
    // Text is converted to numbers first (documented), so two different words compare equal.
    let (a, b) = (to_quantity(lhs)?, to_quantity(rhs)?);
    common_dim(a.dim, b.dim)?;
    let (x, y) = match precision {
        Precision::Tolerant => (a.value, b.value),
        Precision::Exact => (round15(a.value), round15(b.value)),
    };
    let equal = match precision {
        Precision::Tolerant => (x - y).abs() <= TOLERANCE,
        Precision::Exact => x == y,
    };
    let result = match cmp {
        CmpOp::Eq => equal,
        CmpOp::Ne => !equal,
        CmpOp::Lt => x < y && !equal,
        CmpOp::Le => x < y || equal,
        CmpOp::Gt => x > y && !equal,
        CmpOp::Ge => x > y || equal,
    };
    Ok(Value::Bool(result))
}

pub(crate) fn negate(value: &Value) -> Value {
    unary(UnaryOp::Minus, value)
}

pub(crate) fn plus(value: &Value) -> Value {
    unary(UnaryOp::Plus, value)
}

pub(crate) fn percent(value: &Value) -> Value {
    unary(UnaryOp::Percent, value)
}
