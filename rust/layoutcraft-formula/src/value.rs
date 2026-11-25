//! Values a formula can produce.

use crate::error::ErrorValue;
use crate::unit::{Dim, Unit};

/// A number carrying a dimension. `value` is always in internal units: inches (square inches for
/// areas), radians, days.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub dim: Dim,
}

impl Quantity {
    pub const fn new(value: f64, dim: Dim) -> Self {
        Quantity { value, dim }
    }

    pub const fn plain(value: f64) -> Self {
        Quantity {
            value,
            dim: Dim::NONE,
        }
    }

    /// `value` expressed in `unit`, converted to internal units.
    pub fn in_unit(value: f64, unit: Unit) -> Self {
        Quantity {
            value: value * unit.factor(),
            dim: unit.dim(),
        }
    }

    /// The value expressed in `unit` (no dimension check).
    pub fn to_unit(self, unit: Unit) -> f64 {
        self.value / unit.factor()
    }
}

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Parses `#rrggbb` (either case).
    pub fn from_hex(text: &str) -> Option<Rgb> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel =
            |range: std::ops::Range<usize>| u8::from_str_radix(digits.get(range)?, 16).ok();
        Some(Rgb {
            r: channel(0..2)?,
            g: channel(2..4)?,
            b: channel(4..6)?,
        })
    }

    /// `#rrggbb`, lower case.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// The result of evaluating a formula.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number with a dimension.
    Number(Quantity),
    /// A date and time: days since 1899-12-30, the fraction being the time of day.
    Date(f64),
    Str(String),
    Bool(bool),
    Color(Rgb),
    /// An x/y pair (the `PNT` function).
    Point(Quantity, Quantity),
    Error(ErrorValue),
}

impl Value {
    /// A dimensionless number.
    pub fn number(value: f64) -> Value {
        Value::Number(Quantity::plain(value))
    }

    /// A length in inches.
    pub fn length(inches: f64) -> Value {
        Value::Number(Quantity::new(inches, Dim::LENGTH))
    }

    /// An angle in radians.
    pub fn angle(radians: f64) -> Value {
        Value::Number(Quantity::new(radians, Dim::ANGLE))
    }

    /// A duration in days.
    pub fn duration(days: f64) -> Value {
        Value::Number(Quantity::new(days, Dim::DURATION))
    }

    pub fn string(text: impl Into<String>) -> Value {
        Value::Str(text.into())
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    /// The error value, if this is one.
    pub fn error(&self) -> Option<ErrorValue> {
        match self {
            Value::Error(error) => Some(*error),
            _ => None,
        }
    }

    /// The internal-unit number of a number, boolean (1 or 0) or date; `None` for anything else.
    /// This is what a host reads from an evaluated cell (strings are *not* parsed here).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(q) => Some(q.value),
            Value::Date(days) => Some(*days),
            Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// The text a `&` concatenation or a string function sees. Numbers are shown without unit.
    pub fn to_text(&self) -> String {
        match self {
            Value::Number(q) => format_number(q.value),
            Value::Date(days) => format_number(*days),
            Value::Str(text) => text.clone(),
            Value::Bool(true) => "TRUE".to_string(),
            Value::Bool(false) => "FALSE".to_string(),
            Value::Color(rgb) => rgb.to_hex(),
            Value::Point(x, y) => {
                format!("({}, {})", format_number(x.value), format_number(y.value))
            }
            Value::Error(error) => error.text().to_string(),
        }
    }
}

/// Formats a number the way a cell shows it: 15 significant digits at most, no trailing zeros,
/// no exponent for ordinary magnitudes.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return ErrorValue::Num.text().to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    // Round to 15 significant digits through the scientific form, then let `Display` print the
    // shortest representation of the rounded number.
    let rounded: f64 = format!("{value:.14e}").parse().unwrap_or(value);
    format!("{rounded}")
}
