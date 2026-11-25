//! Colour and theme functions.
//!
//! The HSL functions use the scale the reference gives: hue 0..=239, saturation and luminosity
//! 0..=240. A colour argument is a [`Value::Color`], text `#rrggbb`, or a number that the host
//! resolves as an index of the document palette.

use super::{boolean, plain, Call, Outcome};
use crate::env::ThemeKey;
use crate::error::ErrorValue;
use crate::value::{Rgb, Value};

/// Resolves argument `index` to a colour, `None` when it is not one.
fn color_of(call: &Call<'_>, index: usize) -> Option<Rgb> {
    match call.get(index)? {
        Value::Color(rgb) => Some(*rgb),
        Value::Str(text) => Rgb::from_hex(text.trim()),
        Value::Number(q) if q.dim.is_none() && q.value.is_finite() => {
            let palette_index = q.value.round() as i64;
            call.services().palette_color(palette_index)
        }
        _ => None,
    }
}

fn color(call: &Call<'_>, index: usize) -> Result<Rgb, ErrorValue> {
    color_of(call, index).ok_or(ErrorValue::Value)
}

/// Hue, saturation and luminosity on the 0..=240 scale (hue wraps at 240).
#[derive(Clone, Copy)]
struct Hsl {
    h: f64,
    s: f64,
    l: f64,
}

const SCALE: f64 = 240.0;

fn to_hsl(rgb: Rgb) -> Hsl {
    let (r, g, b) = (
        f64::from(rgb.r) / 255.0,
        f64::from(rgb.g) / 255.0,
        f64::from(rgb.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        return Hsl {
            h: 0.0,
            s: 0.0,
            l: (l * SCALE).round(),
        };
    }
    let s = if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let sector = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    Hsl {
        h: (sector / 6.0 * SCALE).round().rem_euclid(SCALE),
        s: (s * SCALE).round(),
        l: (l * SCALE).round(),
    }
}

fn from_hsl(hsl: Hsl) -> Rgb {
    let h = hsl.h.rem_euclid(SCALE) / SCALE;
    let s = (hsl.s / SCALE).clamp(0.0, 1.0);
    let l = (hsl.l / SCALE).clamp(0.0, 1.0);
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f64| -> u8 {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgb {
        r: channel(h + 1.0 / 3.0),
        g: channel(h),
        b: channel(h - 1.0 / 3.0),
    }
}

fn channel(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

pub(crate) fn rgb(call: &mut Call<'_>) -> Outcome {
    let (r, g, b) = (call.plain(0)?, call.plain(1)?, call.plain(2)?);
    Ok(Value::Color(Rgb {
        r: channel(r),
        g: channel(g),
        b: channel(b),
    }))
}

pub(crate) fn hsl(call: &mut Call<'_>) -> Outcome {
    let (h, s, l) = (call.plain(0)?, call.plain(1)?, call.plain(2)?);
    Ok(Value::Color(from_hsl(Hsl {
        h,
        s: s.clamp(0.0, SCALE),
        l: l.clamp(0.0, SCALE),
    })))
}

/// A component of a colour, or 0 for an argument that is not a colour (documented).
fn component(call: &Call<'_>, read: fn(Rgb) -> f64) -> Outcome {
    plain(color_of(call, 0).map_or(0.0, read))
}

pub(crate) fn hue(call: &mut Call<'_>) -> Outcome {
    component(call, |c| to_hsl(c).h)
}

pub(crate) fn sat(call: &mut Call<'_>) -> Outcome {
    component(call, |c| to_hsl(c).s)
}

pub(crate) fn lum(call: &mut Call<'_>) -> Outcome {
    component(call, |c| to_hsl(c).l)
}

pub(crate) fn red(call: &mut Call<'_>) -> Outcome {
    component(call, |c| f64::from(c.r))
}

pub(crate) fn green(call: &mut Call<'_>) -> Outcome {
    component(call, |c| f64::from(c.g))
}

pub(crate) fn blue(call: &mut Call<'_>) -> Outcome {
    component(call, |c| f64::from(c.b))
}

/// First colour's component minus the second's (signed).
fn difference(call: &Call<'_>, read: fn(Hsl) -> f64) -> Outcome {
    plain(read(to_hsl(color(call, 0)?)) - read(to_hsl(color(call, 1)?)))
}

pub(crate) fn hue_diff(call: &mut Call<'_>) -> Outcome {
    difference(call, |c| c.h)
}

pub(crate) fn sat_diff(call: &mut Call<'_>) -> Outcome {
    difference(call, |c| c.s)
}

pub(crate) fn lum_diff(call: &mut Call<'_>) -> Outcome {
    difference(call, |c| c.l)
}

/// Adds `sign * amount` to the luminosity (`TINT`, `SHADE`) or subtracts it from the saturation
/// (`TONE`), clamped to 0..=240.
fn adjust(call: &Call<'_>, saturation: bool, sign: f64) -> Outcome {
    let mut hsl = to_hsl(color(call, 0)?);
    let amount = call.plain(1)?.trunc() * sign;
    if saturation {
        hsl.s = (hsl.s + amount).clamp(0.0, SCALE);
    } else {
        hsl.l = (hsl.l + amount).clamp(0.0, SCALE);
    }
    Ok(Value::Color(from_hsl(hsl)))
}

pub(crate) fn tint(call: &mut Call<'_>) -> Outcome {
    adjust(call, false, 1.0)
}

pub(crate) fn shade(call: &mut Call<'_>) -> Outcome {
    adjust(call, false, -1.0)
}

pub(crate) fn tone(call: &mut Call<'_>) -> Outcome {
    adjust(call, true, -1.0)
}

/// Moves each channel `percent` of the way to white (positive) or to black (negative).
fn toward(call: &Call<'_>, direction: f64) -> Outcome {
    let rgb = color(call, 0)?;
    let amount = (call.plain(1)? * direction / 100.0).clamp(-1.0, 1.0);
    let target = if amount >= 0.0 { 255.0 } else { 0.0 };
    let mix = |c: u8| channel(f64::from(c) + (target - f64::from(c)) * amount.abs());
    Ok(Value::Color(Rgb {
        r: mix(rgb.r),
        g: mix(rgb.g),
        b: mix(rgb.b),
    }))
}

pub(crate) fn mso_tint(call: &mut Call<'_>) -> Outcome {
    toward(call, 1.0)
}

pub(crate) fn mso_shade(call: &mut Call<'_>) -> Outcome {
    toward(call, -1.0)
}

/// `BLEND(color1, color2, fraction)`: `fraction` of the way from the first colour to the second.
pub(crate) fn blend(call: &mut Call<'_>) -> Outcome {
    let (a, b) = (color(call, 0)?, color(call, 1)?);
    let t = call.plain(2)?;
    if !(0.0..=1.0).contains(&t) {
        return Err(ErrorValue::Num);
    }
    let mix = |x: u8, y: u8| channel(f64::from(x) * (1.0 - t) + f64::from(y) * t);
    Ok(Value::Color(Rgb {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
    }))
}

/// The theme key an argument names: a whole number 1..=8 (also as text) is a colour-scheme index.
fn theme_key(call: &Call<'_>, index: usize) -> Result<ThemeKey, ErrorValue> {
    let value = call.value(index)?;
    let as_index = |v: f64| -> Option<ThemeKey> {
        (v.fract() == 0.0 && (1.0..=8.0).contains(&v)).then_some(ThemeKey::ColorIndex(v as u8))
    };
    match value {
        Value::Number(q) => as_index(q.value).ok_or(ErrorValue::Value),
        Value::Str(text) if text.is_empty() => Ok(ThemeKey::HostCell),
        Value::Str(text) => Ok(text
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(as_index)
            .unwrap_or_else(|| ThemeKey::Name(text.clone()))),
        _ => Err(ErrorValue::Value),
    }
}

/// `THEMEVAL([name], [default])` and `THEME(...)`: the value from the host's theme, else the
/// default, else `#N/A!`.
pub(crate) fn theme_val(call: &mut Call<'_>) -> Outcome {
    let key = if call.get(0).is_some() {
        theme_key(call, 0)?
    } else {
        ThemeKey::HostCell
    };
    match call.services().theme_value(&key) {
        Some(value) => Ok(value),
        None => call.get(1).cloned().ok_or(ErrorValue::Na),
    }
}

/// `THEMEGUARD(expression)` evaluates to the expression; without one it behaves like `THEMEVAL()`.
pub(crate) fn theme_guard(call: &mut Call<'_>) -> Outcome {
    match call.get(0) {
        Some(value) => Ok(value.clone()),
        None => call
            .services()
            .theme_value(&ThemeKey::HostCell)
            .ok_or(ErrorValue::Na),
    }
}

pub(crate) fn is_themed(call: &mut Call<'_>) -> Outcome {
    match call.services().is_themed() {
        Some(flag) => boolean(flag),
        None => Err(ErrorValue::Unsupported),
    }
}
