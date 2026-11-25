//! String functions.
//!
//! Positions and lengths count Unicode scalar values (unspecified in the sources, which do not say
//! how characters outside the basic plane are counted). Results are capped at
//! [`crate::MAX_STRING_BYTES`] so a formula cannot build an enormous string.

use super::{boolean, number, plain, Call, Outcome};
use crate::error::ErrorValue;
use crate::unit::Dim;
use crate::value::Value;
use crate::MAX_STRING_BYTES;

fn chars_of(call: &Call<'_>, index: usize) -> Result<Vec<char>, ErrorValue> {
    Ok(call.text(index)?.chars().collect())
}

fn string(chars: &[char]) -> Outcome {
    Ok(Value::Str(chars.iter().collect()))
}

fn same_char(a: char, b: char, ignore_case: bool) -> bool {
    a == b || (ignore_case && a.to_lowercase().eq(b.to_lowercase()))
}

/// Index of the first occurrence of `needle` in `haystack` at or after `from`.
fn find_chars(haystack: &[char], needle: &[char], from: usize, ignore_case: bool) -> Option<usize> {
    if needle.is_empty() {
        return (from <= haystack.len()).then_some(from);
    }
    let last = haystack.len().checked_sub(needle.len())?;
    (from..=last).find(|&start| {
        haystack
            .get(start..start + needle.len())
            .is_some_and(|window| {
                window
                    .iter()
                    .zip(needle)
                    .all(|(&a, &b)| same_char(a, b, ignore_case))
            })
    })
}

/// A count argument that must not be negative.
fn count(call: &Call<'_>, index: usize) -> Result<usize, ErrorValue> {
    match call.whole_opt(index)? {
        None => Ok(1),
        Some(n) if n < 0 => Err(ErrorValue::Value),
        Some(n) => Ok(usize::try_from(n).unwrap_or(usize::MAX)),
    }
}

pub(crate) fn left(call: &mut Call<'_>) -> Outcome {
    let chars = chars_of(call, 0)?;
    let n = count(call, 1)?.min(chars.len());
    string(chars.get(..n).unwrap_or(&[]))
}

pub(crate) fn right(call: &mut Call<'_>) -> Outcome {
    let chars = chars_of(call, 0)?;
    let n = count(call, 1)?.min(chars.len());
    string(chars.get(chars.len() - n..).unwrap_or(&[]))
}

/// `MID(text, start, count)`; a negative count returns everything from `start` on.
pub(crate) fn mid(call: &mut Call<'_>) -> Outcome {
    let chars = chars_of(call, 0)?;
    let start = call.whole(1)?;
    let wanted = call.whole(2)?;
    if start < 1 {
        return Err(ErrorValue::Value);
    }
    let from = usize::try_from(start - 1).unwrap_or(usize::MAX);
    if from >= chars.len() {
        return Ok(Value::Str(String::new()));
    }
    let to = match usize::try_from(wanted) {
        Ok(n) => from.saturating_add(n).min(chars.len()),
        Err(_) => chars.len(),
    };
    string(chars.get(from..to).unwrap_or(&[]))
}

pub(crate) fn len(call: &mut Call<'_>) -> Outcome {
    plain(call.text(0)?.chars().count() as f64)
}

pub(crate) fn lower(call: &mut Call<'_>) -> Outcome {
    Ok(Value::Str(call.text(0)?.to_lowercase()))
}

pub(crate) fn upper(call: &mut Call<'_>) -> Outcome {
    Ok(Value::Str(call.text(0)?.to_uppercase()))
}

/// Removes leading and trailing spaces and collapses runs of spaces to one.
pub(crate) fn trim(call: &mut Call<'_>) -> Outcome {
    let text = call.text(0)?;
    let words: Vec<&str> = text.split(' ').filter(|word| !word.is_empty()).collect();
    Ok(Value::Str(words.join(" ")))
}

/// `FIND(find, within, [start], [ignore_case])`: 1-based position, `#VALUE!` when absent.
pub(crate) fn find(call: &mut Call<'_>) -> Outcome {
    let needle = chars_of(call, 0)?;
    let haystack = chars_of(call, 1)?;
    let start = call.whole_opt(2)?.unwrap_or(1);
    let ignore_case = call.flag_opt(3)?.unwrap_or(false);
    if start < 1
        || usize::try_from(start - 1).map_or(true, |from| from > haystack.len().saturating_sub(1))
    {
        return Err(ErrorValue::Value);
    }
    let from = usize::try_from(start - 1).map_err(|_| ErrorValue::Value)?;
    match find_chars(&haystack, &needle, from, ignore_case) {
        Some(position) => plain((position + 1) as f64),
        None => Err(ErrorValue::Value),
    }
}

/// `SUBSTITUTE(text, old, new, [occurrence], [ignore_case])`.
pub(crate) fn substitute(call: &mut Call<'_>) -> Outcome {
    let text = chars_of(call, 0)?;
    let old = chars_of(call, 1)?;
    let new = chars_of(call, 2)?;
    let occurrence = call.whole_opt(3)?;
    let ignore_case = call.flag_opt(4)?.unwrap_or(false);
    if matches!(occurrence, Some(n) if n < 1) {
        return Err(ErrorValue::Value);
    }
    if old.is_empty() {
        return string(&text);
    }
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    let mut at = 0;
    let mut seen = 0i64;
    while let Some(position) = find_chars(&text, &old, at, ignore_case) {
        seen += 1;
        out.extend(text.get(at..position).unwrap_or(&[]));
        let replace_this = occurrence.is_none_or(|n| n == seen);
        out.extend(if replace_this {
            new.as_slice()
        } else {
            old.as_slice()
        });
        at = position + old.len();
        if out.len() > MAX_STRING_BYTES {
            return Err(ErrorValue::Value);
        }
    }
    out.extend(text.get(at..).unwrap_or(&[]));
    string(&out)
}

/// `REPLACE(text, start, count, new)`.
pub(crate) fn replace(call: &mut Call<'_>) -> Outcome {
    let text = chars_of(call, 0)?;
    let start = call.whole(1)?;
    let removed = call.whole(2)?;
    let new = chars_of(call, 3)?;
    if start < 1 || removed < 0 {
        return Err(ErrorValue::Value);
    }
    let from = usize::try_from(start - 1)
        .unwrap_or(usize::MAX)
        .min(text.len());
    let to = from
        .saturating_add(usize::try_from(removed).unwrap_or(usize::MAX))
        .min(text.len());
    let mut out: Vec<char> = text.get(..from).unwrap_or(&[]).to_vec();
    out.extend(&new);
    out.extend(text.get(to..).unwrap_or(&[]));
    if out.len() > MAX_STRING_BYTES {
        return Err(ErrorValue::Value);
    }
    string(&out)
}

pub(crate) fn rept(call: &mut Call<'_>) -> Outcome {
    let text = call.text(0)?;
    let times = call.whole(1)?;
    if times < 0 {
        return Err(ErrorValue::Value);
    }
    let times = usize::try_from(times).unwrap_or(usize::MAX);
    if !text.is_empty() && times > MAX_STRING_BYTES / text.len() {
        return Err(ErrorValue::Value);
    }
    Ok(Value::Str(text.repeat(times)))
}

/// `CHAR(n)` for 1..=255. Unspecified in the sources: the code page is not named, so the number is
/// taken as the Unicode code point (Latin-1).
pub(crate) fn char_(call: &mut Call<'_>) -> Outcome {
    let n = call.whole(0)?;
    match u8::try_from(n) {
        Ok(byte) if byte >= 1 => Ok(Value::Str(char::from(byte).to_string())),
        _ => Err(ErrorValue::Value),
    }
}

/// `UNICHAR(n)` for 1..=65535 (surrogates are not characters and give `#VALUE!`).
pub(crate) fn unichar(call: &mut Call<'_>) -> Outcome {
    let n = call.whole(0)?;
    match u32::try_from(n)
        .ok()
        .filter(|n| (1..=0xFFFF).contains(n))
        .and_then(char::from_u32)
    {
        Some(c) => Ok(Value::Str(c.to_string())),
        None => Err(ErrorValue::Value),
    }
}

/// Splits `list` at `delimiter` (default `;`); consecutive delimiters give empty items.
fn items(call: &Call<'_>, list: usize, delimiter: usize) -> Result<Vec<String>, ErrorValue> {
    let list = call.text(list)?;
    let delimiter = call
        .text_opt(delimiter)
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| ";".to_string());
    Ok(list.split(delimiter.as_str()).map(str::to_string).collect())
}

/// `INDEX(index, list, [delimiter], [error_value])`: the zero-based item, or `error_value` (default
/// an empty string) when out of range.
pub(crate) fn index(call: &mut Call<'_>) -> Outcome {
    let position = call.whole(0)?;
    let items = items(call, 1, 2)?;
    let item = usize::try_from(position)
        .ok()
        .and_then(|p| items.into_iter().nth(p));
    match item {
        Some(item) => Ok(Value::Str(item)),
        None => Ok(call
            .get(3)
            .cloned()
            .unwrap_or_else(|| Value::Str(String::new()))),
    }
}

/// `LOOKUP(key, list, [delimiter])`: zero-based position (case-insensitive), -1 when absent or
/// when the key itself contains the delimiter.
pub(crate) fn lookup(call: &mut Call<'_>) -> Outcome {
    let key = call.text(0)?;
    let delimiter = call
        .text_opt(2)
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| ";".to_string());
    let items = items(call, 1, 2)?;
    if key.contains(delimiter.as_str()) {
        return plain(-1.0);
    }
    let key: Vec<char> = key.chars().collect();
    let position = items.iter().position(|item| {
        let item: Vec<char> = item.chars().collect();
        item.len() == key.len() && item.iter().zip(&key).all(|(&a, &b)| same_char(a, b, true))
    });
    plain(position.map_or(-1.0, |p| p as f64))
}

fn same(a: &str, b: &str, ignore_case: bool) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(&x, &y)| same_char(x, y, ignore_case))
}

pub(crate) fn str_same(call: &mut Call<'_>) -> Outcome {
    let ignore_case = call.flag_opt(2)?.unwrap_or(false);
    boolean(same(&call.text(0)?, &call.text(1)?, ignore_case))
}

/// `STRSAMEEX(a, b, locale, flags)`: only flag bit 1 (ignore case) is modelled; the locale is
/// ignored and any other flag bit gives [`ErrorValue::Unsupported`].
pub(crate) fn str_same_ex(call: &mut Call<'_>) -> Outcome {
    let flags = call.whole(3)?;
    if flags & !1 != 0 {
        return Err(ErrorValue::Unsupported);
    }
    boolean(same(&call.text(0)?, &call.text(1)?, flags & 1 == 1))
}

/// `SHAPETEXT(TheText, [flags])`: the text the host reports for the referenced `TheText` cell,
/// with the flags of the function page that need no field or hyphenation data: 4 (tab to one
/// space), 16 (carriage return and line feed to a space each), 32 (typographer quotes to plain
/// quotes) and 64 (adjacent white space to one space). Flags 1, 2 and 8 are
/// [`ErrorValue::Unsupported`]; a flag outside 0..=127 is `#VALUE!`.
pub(crate) fn shape_text(call: &mut Call<'_>) -> Outcome {
    let text = call.text(0)?;
    let flags = call.whole_opt(1)?.unwrap_or(0);
    if !(0..=127).contains(&flags) {
        return Err(ErrorValue::Value);
    }
    if flags & (1 | 2 | 8) != 0 {
        return Err(ErrorValue::Unsupported);
    }
    let mut out: String = text
        .chars()
        .map(|c| match c {
            '\t' if flags & 4 != 0 => ' ',
            '\r' | '\n' if flags & 16 != 0 => ' ',
            '\u{2018}' | '\u{2019}' if flags & 32 != 0 => '\'',
            '\u{201c}' | '\u{201d}' if flags & 32 != 0 => '"',
            other => other,
        })
        .collect();
    if flags & 64 != 0 {
        let mut collapsed = String::with_capacity(out.len());
        let mut in_space = false;
        for c in out.chars() {
            if c.is_whitespace() {
                if !in_space {
                    collapsed.push(' ');
                }
                in_space = true;
            } else {
                collapsed.push(c);
                in_space = false;
            }
        }
        out = collapsed;
    }
    Ok(Value::Str(out))
}

fn text_extent(call: &Call<'_>, height: bool) -> Outcome {
    let text = call.text(0)?;
    let limit = call.num_opt(1)?.map(|q| q.value);
    match call.services().text_size(&text, limit) {
        Some((w, h)) => number(if height { h } else { w }, Dim::LENGTH),
        None => Err(ErrorValue::Unsupported),
    }
}

pub(crate) fn text_width(call: &mut Call<'_>) -> Outcome {
    text_extent(call, false)
}

pub(crate) fn text_height(call: &mut Call<'_>) -> Outcome {
    text_extent(call, true)
}
