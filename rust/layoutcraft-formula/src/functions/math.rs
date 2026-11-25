//! Arithmetic, rounding, trigonometry and bit functions.
//!
//! Sources: the per-function pages of Microsoft Learn, "Functions (Visio ShapeSheet Reference)".
//! Angles are radians internally; trigonometric functions accept an angle with or without unit
//! (a bare number is radians) and the inverse functions return an angle.

use std::f64::consts::{PI, TAU};

use super::{number, plain, Call, Outcome};
use crate::ast::BinaryOp;
use crate::error::ErrorValue;
use crate::eval;
use crate::unit::Dim;
use crate::value::Value;

/// The documented default fuzz of `SIGN`; also the slack that keeps `CEILING(0.3, 0.1)` at `0.3`.
const FUZZ: f64 = 1e-9;

/// An angle argument in radians. A length or duration is `#DIM!`.
fn angle(call: &Call<'_>, index: usize) -> Result<f64, ErrorValue> {
    let q = call.num(index)?;
    if q.dim.is_none() || q.dim == Dim::ANGLE {
        Ok(q.value)
    } else {
        Err(ErrorValue::Dim)
    }
}

pub(crate) fn abs(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    number(q.value.abs(), q.dim)
}

pub(crate) fn sin(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.sin())
}

pub(crate) fn cos(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.cos())
}

pub(crate) fn tan(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.tan())
}

pub(crate) fn sinh(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.sinh())
}

pub(crate) fn cosh(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.cosh())
}

pub(crate) fn tanh(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.tanh())
}

pub(crate) fn asin(call: &mut Call<'_>) -> Outcome {
    let x = call.plain(0)?;
    if !(-1.0..=1.0).contains(&x) {
        return Err(ErrorValue::Num);
    }
    number(x.asin(), Dim::ANGLE)
}

pub(crate) fn acos(call: &mut Call<'_>) -> Outcome {
    let x = call.plain(0)?;
    if !(-1.0..=1.0).contains(&x) {
        return Err(ErrorValue::Num);
    }
    number(x.acos(), Dim::ANGLE)
}

pub(crate) fn atan(call: &mut Call<'_>) -> Outcome {
    number(call.plain(0)?.atan(), Dim::ANGLE)
}

/// `ATAN2(y, x)`; `ATAN2(0, 0)` is 0 as documented.
pub(crate) fn atan2(call: &mut Call<'_>) -> Outcome {
    let (xy, _) = call.unified(0)?;
    match xy.as_slice() {
        [y, x] => number(y.atan2(*x), Dim::ANGLE),
        _ => Err(ErrorValue::Value),
    }
}

/// Radians to degrees (a plain number).
pub(crate) fn deg(call: &mut Call<'_>) -> Outcome {
    plain(angle(call, 0)?.to_degrees())
}

/// Degrees to radians. Unspecified in the sources: an argument that already has an angle unit is
/// returned unchanged, since it is in radians internally.
pub(crate) fn rad(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    match q.dim {
        d if d == Dim::ANGLE => number(q.value, Dim::ANGLE),
        d if d.is_none() => plain(q.value.to_radians()),
        _ => Err(ErrorValue::Dim),
    }
}

/// Normalises an angle into `0..2*PI`.
pub(crate) fn ang360(call: &mut Call<'_>) -> Outcome {
    let radians = angle(call, 0)?;
    let wrapped = radians.rem_euclid(TAU);
    number(if wrapped >= TAU { 0.0 } else { wrapped }, Dim::ANGLE)
}

pub(crate) fn pi(_call: &mut Call<'_>) -> Outcome {
    plain(PI)
}

pub(crate) fn int(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    number(q.value.floor(), q.dim)
}

/// Unspecified in the sources: "rounds up to the next integer" is taken as towards +infinity.
pub(crate) fn intup(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    number(q.value.ceil(), q.dim)
}

/// `10^digits`, `None` when it does not fit.
fn power_of_ten(digits: i64) -> Option<f64> {
    let digits = i32::try_from(digits).ok()?;
    let scale = 10f64.powi(digits);
    (scale.is_finite() && scale != 0.0).then_some(scale)
}

/// Applies `step` to `value` scaled so that `digits` decimals become integer digits.
fn at_digits(value: f64, digits: i64, step: fn(f64) -> f64) -> f64 {
    let Some(scale) = power_of_ten(digits.abs()) else {
        // Beyond f64 range the digits cannot matter: huge positive means keep, huge negative means 0.
        return if digits > 0 { value } else { 0.0 };
    };
    let result = if digits >= 0 {
        step(value * scale) / scale
    } else {
        step(value / scale) * scale
    };
    if result.is_finite() {
        result
    } else {
        value
    }
}

/// Rounds half away from zero.
pub(crate) fn round(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    let digits = call.whole_opt(1)?.unwrap_or(0);
    number(at_digits(q.value, digits, f64::round), q.dim)
}

pub(crate) fn trunc(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    let digits = call.whole_opt(1)?.unwrap_or(0);
    number(at_digits(q.value, digits, f64::trunc), q.dim)
}

/// Rounds to a multiple of `multiple` (away from zero for `CEILING`, towards zero for `FLOOR`).
fn to_multiple(call: &Call<'_>, away_from_zero: bool) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    let x = numbers.first().copied().ok_or(ErrorValue::Value)?;
    let Some(multiple) = numbers.get(1).copied() else {
        return number(
            if away_from_zero {
                x.abs().ceil()
            } else {
                x.abs().floor()
            } * x.signum_or_zero(),
            dim,
        );
    };
    if x == 0.0 || multiple == 0.0 {
        return number(0.0, dim);
    }
    if (x < 0.0) != (multiple < 0.0) {
        return Err(ErrorValue::Num);
    }
    let steps = x.abs() / multiple.abs();
    let whole = if away_from_zero {
        (steps - FUZZ).ceil()
    } else {
        (steps + FUZZ).floor()
    };
    number(whole.max(0.0) * multiple.abs() * x.signum(), dim)
}

trait SignumOrZero {
    fn signum_or_zero(self) -> f64;
}

impl SignumOrZero for f64 {
    fn signum_or_zero(self) -> f64 {
        if self == 0.0 {
            0.0
        } else {
            self.signum()
        }
    }
}

pub(crate) fn ceiling(call: &mut Call<'_>) -> Outcome {
    to_multiple(call, true)
}

pub(crate) fn floor(call: &mut Call<'_>) -> Outcome {
    to_multiple(call, false)
}

/// `SIGN(number, fuzz)`: 0 when the number is within `fuzz` (default 1e-9) of zero.
pub(crate) fn sign(call: &mut Call<'_>) -> Outcome {
    let x = call.num(0)?.value;
    let fuzz = call.num_opt(1)?.map_or(FUZZ, |q| q.value.abs());
    plain(if x.abs() <= fuzz { 0.0 } else { x.signum() })
}

pub(crate) fn sqrt(call: &mut Call<'_>) -> Outcome {
    let q = call.num(0)?;
    if q.value < 0.0 {
        return Err(ErrorValue::Num);
    }
    let dim = Dim {
        length: halve(q.dim.length)?,
        angle: halve(q.dim.angle)?,
        time: halve(q.dim.time)?,
    };
    number(q.value.sqrt(), dim)
}

fn halve(exponent: i8) -> Result<i8, ErrorValue> {
    if exponent % 2 == 0 {
        Ok(exponent / 2)
    } else {
        Err(ErrorValue::Dim)
    }
}

/// `POW` with the special cases the reference lists, including the ones that read oddly.
pub(crate) fn pow(call: &mut Call<'_>) -> Outcome {
    let base = call.num(0)?;
    let exponent = call.plain(1)?;
    let (n, e) = (base.value, exponent);
    if n != 0.0 && e == 0.0 {
        return plain(1.0);
    }
    if (n == 0.0 && e <= 0.0) || (n < 0.0 && e.fract() != 0.0) {
        return plain(0.0);
    }
    // Documented as returning an indeterminate value when both are negative.
    if n < 0.0 && e < 0.0 {
        return Err(ErrorValue::Num);
    }
    let whole = e.fract() == 0.0 && e.abs() <= f64::from(i32::MAX);
    let dim = if base.dim.is_none() {
        Dim::NONE
    } else if whole {
        base.dim.checked_scale(e as i32).ok_or(ErrorValue::Dim)?
    } else {
        return Err(ErrorValue::Dim);
    };
    number(n.powf(e), dim)
}

pub(crate) fn ln(call: &mut Call<'_>) -> Outcome {
    let x = call.plain(0)?;
    if x <= 0.0 {
        return Err(ErrorValue::Num);
    }
    plain(x.ln())
}

pub(crate) fn log10(call: &mut Call<'_>) -> Outcome {
    let x = call.plain(0)?;
    if x <= 0.0 {
        return Err(ErrorValue::Num);
    }
    plain(x.log10())
}

pub(crate) fn exp(call: &mut Call<'_>) -> Outcome {
    plain(call.plain(0)?.exp())
}

/// The remainder with the sign of the divisor, as `MODULUS` documents.
pub(crate) fn modulus(call: &mut Call<'_>) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    let (a, b) = match numbers.as_slice() {
        [a, b] => (*a, *b),
        _ => return Err(ErrorValue::Value),
    };
    if b == 0.0 {
        return Err(ErrorValue::DivZero);
    }
    let mut remainder = a % b;
    if remainder != 0.0 && (remainder < 0.0) != (b < 0.0) {
        remainder += b;
    }
    number(remainder, dim)
}

/// The legacy `MOD`: `|a| mod |b|` carrying the divisor's sign (`MOD(-7, 3)` is 1). The reference
/// says to prefer `MODULUS`, which differs for a negative dividend.
pub(crate) fn legacy_mod(call: &mut Call<'_>) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    let (a, b) = match numbers.as_slice() {
        [a, b] => (*a, *b),
        _ => return Err(ErrorValue::Value),
    };
    if b == 0.0 {
        return Err(ErrorValue::DivZero);
    }
    let magnitude = a.abs() % b.abs();
    number(if b < 0.0 { -magnitude } else { magnitude }, dim)
}

pub(crate) fn max(call: &mut Call<'_>) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    number(numbers.into_iter().fold(f64::NEG_INFINITY, f64::max), dim)
}

pub(crate) fn min(call: &mut Call<'_>) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    number(numbers.into_iter().fold(f64::INFINITY, f64::min), dim)
}

pub(crate) fn sum(call: &mut Call<'_>) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    number(numbers.into_iter().sum(), dim)
}

pub(crate) fn average(call: &mut Call<'_>) -> Outcome {
    let (numbers, dim) = call.unified(0)?;
    let count = numbers.len() as f64;
    number(numbers.into_iter().sum::<f64>() / count, dim)
}

/// `SQRT((a*A)^2 + (b*B)^2)`.
pub(crate) fn magnitude(call: &mut Call<'_>) -> Outcome {
    let (ca, cb) = (call.plain(0)?, call.plain(2)?);
    let (a, b) = (call.num(1)?, call.num(3)?);
    let dim = eval::common_dim(a.dim, b.dim)?;
    number((ca * a.value).hypot(cb * b.value), dim)
}

/// The 16-bit view of a bit-function argument.
fn bits(call: &Call<'_>, index: usize) -> Result<u16, ErrorValue> {
    let wide = call.whole(index)?;
    Ok((wide & 0xFFFF) as u16)
}

pub(crate) fn bit_and(call: &mut Call<'_>) -> Outcome {
    plain(f64::from(bits(call, 0)? & bits(call, 1)?))
}

pub(crate) fn bit_or(call: &mut Call<'_>) -> Outcome {
    plain(f64::from(bits(call, 0)? | bits(call, 1)?))
}

pub(crate) fn bit_xor(call: &mut Call<'_>) -> Outcome {
    plain(f64::from(bits(call, 0)? ^ bits(call, 1)?))
}

pub(crate) fn bit_not(call: &mut Call<'_>) -> Outcome {
    plain(f64::from(!bits(call, 0)?))
}

/// The text-upright angle: 180 degrees when `angle` lies between the two limits (either order,
/// limits included), otherwise 0. Defaults are 90 and 270 degrees.
pub(crate) fn gravity(call: &mut Call<'_>) -> Outcome {
    let normalise = |radians: f64| radians.rem_euclid(TAU);
    let value = normalise(angle(call, 0)?);
    let first = normalise(if call.get(1).is_some() {
        angle(call, 1)?
    } else {
        PI / 2.0
    });
    let second = normalise(if call.get(2).is_some() {
        angle(call, 2)?
    } else {
        3.0 * PI / 2.0
    });
    let (low, high) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    number(
        if value >= low && value <= high {
            PI
        } else {
            0.0
        },
        Dim::ANGLE,
    )
}

pub(crate) fn pnt(call: &mut Call<'_>) -> Outcome {
    Ok(Value::Point(call.num(0)?, call.num(1)?))
}

pub(crate) fn pnt_x(call: &mut Call<'_>) -> Outcome {
    match call.value(0)? {
        Value::Point(x, _) => Ok(Value::Number(*x)),
        _ => Err(ErrorValue::Value),
    }
}

pub(crate) fn pnt_y(call: &mut Call<'_>) -> Outcome {
    match call.value(0)? {
        Value::Point(_, y) => Ok(Value::Number(*y)),
        _ => Err(ErrorValue::Value),
    }
}

fn operator(call: &Call<'_>, op: BinaryOp) -> Outcome {
    match eval::binary(op, call.value(0)?, call.value(1)?) {
        Value::Error(error) => Err(error),
        value => Ok(value),
    }
}

pub(crate) fn add(call: &mut Call<'_>) -> Outcome {
    operator(call, BinaryOp::Add)
}

pub(crate) fn sub(call: &mut Call<'_>) -> Outcome {
    operator(call, BinaryOp::Sub)
}

pub(crate) fn mul(call: &mut Call<'_>) -> Outcome {
    operator(call, BinaryOp::Mul)
}

pub(crate) fn div(call: &mut Call<'_>) -> Outcome {
    operator(call, BinaryOp::Div)
}

pub(crate) fn uminus(call: &mut Call<'_>) -> Outcome {
    match eval::negate(call.value(0)?) {
        Value::Error(error) => Err(error),
        value => Ok(value),
    }
}

pub(crate) fn uplus(call: &mut Call<'_>) -> Outcome {
    match eval::plus(call.value(0)?) {
        Value::Error(error) => Err(error),
        value => Ok(value),
    }
}

pub(crate) fn pct(call: &mut Call<'_>) -> Outcome {
    match eval::percent(call.value(0)?) {
        Value::Error(error) => Err(error),
        value => Ok(value),
    }
}
