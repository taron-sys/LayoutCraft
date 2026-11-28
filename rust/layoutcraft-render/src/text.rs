//! Font-backed text measurement for line wrapping.
//!
//! Glyph advances come first from a table baked into the binary (`font_metrics.bin`, made from the open fonts in
//! `web/fonts` by `examples/export_open_font_metrics.rs`; Liberation, Carlito and Caladea are metric-compatible with
//! Arial, Times New Roman, Courier New, Calibri and Cambria, and the table is filed under both names; a maintainer
//! build may swap in a private table, see `build.rs`), so wrapping is the same in the
//! browser build, on Linux and on Windows; fonts missing from the table are looked up on the
//! system, and anything else is estimated per character class.
use std::cell::RefCell;
use std::collections::HashMap;

use fontdb::{Database, Family, Query, Style, Weight};

struct Measurer {
    db: Database,
}

impl Measurer {
    fn new() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        Self { db }
    }

    /// (size, offset) of the sub- or superscript of the first family that is installed, as shares of the em.
    fn script(&self, family: &str, bold: bool, italic: bool, superscript: bool) -> Option<(f64, f64)> {
        for family in parse_families(family) {
            let query = Query {
                families: &[family],
                weight: if bold { Weight::BOLD } else { Weight::NORMAL },
                stretch: fontdb::Stretch::default(),
                style: if italic { Style::Italic } else { Style::Normal },
            };
            let Some(id) = self.db.query(&query) else {
                continue;
            };
            let found = self.db.with_face_data(id, |data, index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                let units = f64::from(face.units_per_em());
                let metrics = if superscript { face.superscript_metrics()? } else { face.subscript_metrics()? };
                (units > 0.0 && metrics.y_size > 0).then(|| (f64::from(metrics.y_size) / units, f64::from(metrics.y_offset) / units))
            });
            if let Some(Some(found)) = found {
                return Some(found);
            }
        }
        None
    }

    fn width(&self, value: &str, family: &str, size: f64, bold: bool, italic: bool) -> Option<f64> {
        for family in parse_families(family) {
            // a face named with its weight is measured as that weight of its family, as it is drawn
            let (family, weight) = match family {
                Family::Name(name) => match crate::fonts::weighted_face(&self.db, name) {
                    Some((base, weight)) => (Family::Name(base), if bold { Weight::BOLD } else { Weight(weight) }),
                    None => (family, if bold { Weight::BOLD } else { Weight::NORMAL }),
                },
                _ => (family, if bold { Weight::BOLD } else { Weight::NORMAL }),
            };
            let query = Query {
                families: &[family],
                weight,
                stretch: fontdb::Stretch::default(),
                style: if italic { Style::Italic } else { Style::Normal },
            };
            let Some(id) = self.db.query(&query) else {
                continue;
            };
            if let Some(width) = self.db.with_face_data(id, |data, index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                let units = face.units_per_em() as f64;
                if units <= 0.0 {
                    return None;
                }
                let mut total = 0.0;
                let mut fallback = 0.0;
                for c in value.chars() {
                    match face.glyph_index(c) {
                        Some(glyph) => {
                            total += face.glyph_hor_advance(glyph).unwrap_or(0) as f64;
                        }
                        None => fallback += fallback_factor(c) * size,
                    }
                }
                Some(total / units * size + fallback)
            }) {
                return width;
            }
        }
        None
    }
}

#[cfg(not(private_font_metrics))]
static METRICS: &[u8] = include_bytes!("font_metrics.bin");
#[cfg(private_font_metrics)]
static METRICS: &[u8] = include_bytes!("../../../private/font-metrics-windows/font_metrics.bin");

/// Advances of one face from `font_metrics.bin`.
struct Face {
    units_per_em: f64,
    /// (first code point, advances; 0xFFFF = no glyph)
    ranges: Vec<(u32, Vec<u16>)>,
}

impl Face {
    fn advance(&self, c: char) -> Option<f64> {
        let cp = c as u32;
        self.ranges
            .iter()
            .find(|(start, advances)| cp >= *start && cp < *start + advances.len() as u32)
            .and_then(|(start, advances)| advances.get((cp - start) as usize))
            .filter(|advance| **advance != 0xFFFF)
            .map(|advance| f64::from(*advance))
    }
}

/// (lower-case family, bold, italic) -> face.
struct Table(HashMap<(String, bool, bool), Face>);

impl Table {
    fn load() -> Self {
        let mut faces = HashMap::new();
        let data = METRICS;
        let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
        let u32_at = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        if data.len() < 4 {
            return Self(faces);
        }
        let count = u32_at(0);
        let mut at = 4;
        for _ in 0..count {
            let name_len = data[at] as usize;
            let name = String::from_utf8_lossy(&data[at + 1..at + 1 + name_len]).into_owned();
            at += 1 + name_len;
            let flags = data[at];
            let units_per_em = f64::from(u16_at(at + 1));
            let range_count = u16_at(at + 3) as usize;
            at += 5;
            let mut ranges = Vec::with_capacity(range_count);
            for _ in 0..range_count {
                let start = u32_at(at);
                let len = u16_at(at + 4) as usize;
                at += 6;
                let advances = (0..len).map(|i| u16_at(at + i * 2)).collect();
                at += len * 2;
                ranges.push((start, advances));
            }
            faces.insert((name, flags & 1 != 0, flags & 2 != 0), Face { units_per_em, ranges });
        }
        Self(faces)
    }

    /// Width of `value` in the named family, when the table has it. A missing bold or italic
    /// face uses the closest one it has, as the system lookup does.
    fn width(&self, value: &str, family: &str, size: f64, bold: bool, italic: bool) -> Option<f64> {
        let name = match family.to_ascii_lowercase().as_str() {
            "sans-serif" | "helvetica" | "helvetica neue" => "arial".to_string(),
            "serif" | "times" => "times new roman".to_string(),
            "monospace" | "courier" => "courier new".to_string(),
            other => other.to_string(),
        };
        let face = [(bold, italic), (bold, false), (false, italic), (false, false)]
            .iter()
            .find_map(|(b, i)| self.0.get(&(name.clone(), *b, *i)))?;
        let mut total = 0.0;
        let mut fallback = 0.0;
        for c in value.chars() {
            match face.advance(c) {
                Some(advance) => total += advance,
                None => fallback += fallback_factor(c) * size,
            }
        }
        Some(total / face.units_per_em * size + fallback)
    }
}

thread_local! {
    static MEASURER: RefCell<Option<Measurer>> = const { RefCell::new(None) };
    static TABLE: Table = Table::load();
}

pub fn measure(value: &str, family: &str, size: f64, bold: bool, italic: bool) -> f64 {
    MEASURER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Measurer::new());
        }
        // Per family in the list: the baked table first, then the system fonts.
        for name in parse_family_names(family) {
            if let Some(width) = TABLE.with(|table| table.width(value, &name, size, bold, italic)) {
                return width;
            }
            if let Some(width) = slot.as_ref().and_then(|m| m.width(value, &name, size, bold, italic)) {
                return width;
            }
        }
        fallback_width(value, size, bold)
    })
}

/// The sub- (`pos` 2) or superscript (`pos` 1) size and shift of a font, read from its OS/2 table as Visio does; None for normal text.
/// Where no installed face is found the values of Calibri are used (size 0.65, shift 0.14 down / 0.48 up).
pub fn script_shift(pos: u8, family: &str, bold: bool, italic: bool) -> Option<crate::scene::ScriptShift> {
    let superscript = match pos {
        1 => true,
        2 => false,
        _ => return None,
    };
    let measured = MEASURER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Measurer::new());
        }
        slot.as_ref().and_then(|m| m.script(family, bold, italic, superscript))
    });
    let (scale, offset) = measured.unwrap_or((0.65, if superscript { 0.48 } else { 0.14 }));
    Some(crate::scene::ScriptShift { scale, shift: if superscript { -offset } else { offset } })
}

fn parse_family_names(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| part.trim().trim_matches(['"', '\'']).trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn parse_families(value: &str) -> Vec<Family<'_>> {
    let mut out = Vec::new();
    for part in value.split(',') {
        let name = part.trim().trim_matches(['"', '\'']).trim();
        if name.is_empty() {
            continue;
        }
        out.push(match name.to_ascii_lowercase().as_str() {
            "serif" => Family::Serif,
            "sans-serif" => Family::SansSerif,
            "monospace" => Family::Monospace,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            _ => Family::Name(name),
        });
    }
    if out.is_empty() {
        out.push(Family::SansSerif);
    }
    out
}

fn fallback_width(value: &str, size: f64, bold: bool) -> f64 {
    let factor: f64 = value.chars().map(fallback_factor).sum();
    factor * size * if bold { 1.04 } else { 1.0 }
}

fn fallback_factor(c: char) -> f64 {
    let cp = c as u32;
    if matches!(cp, 0x1100..=0x11FF | 0x2E80..=0xA4CF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6) {
        // CJK ideographs, kana, Hangul and full-width forms are one em wide.
        1.0
    } else if c.is_whitespace() {
        0.30
    } else if c.is_ascii_digit() {
        0.56
    } else if c.is_ascii_uppercase() {
        0.64
    } else if "iljtfI|.,:;'!".contains(c) {
        0.28
    } else if "mwMW@".contains(c) {
        0.86
    } else {
        0.53
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_table_measures_without_system_fonts() {
        // Arial advances (2048 units/em): H 1479, e 1139, l 455, l 455, o 1139.
        let width = TABLE.with(|t| t.width("Hello", "Arial", 10.0, false, false)).unwrap();
        assert!((width - 4667.0 / 2048.0 * 10.0).abs() < 1e-9, "{width}");
        // Generic families map to Arial / Times New Roman; bold uses the bold face.
        let bold = TABLE.with(|t| t.width("Hello", "sans-serif", 10.0, true, false)).unwrap();
        assert!(bold > width);
        assert!(TABLE.with(|t| t.width("x", "No Such Font", 10.0, false, false)).is_none());
    }

    #[test]
    fn cjk_characters_are_one_em_wide_when_no_font_has_them() {
        assert!((fallback_factor('漢') - 1.0).abs() < 1e-12);
        assert!((fallback_factor('か') - 1.0).abs() < 1e-12);
        assert!(fallback_factor('a') < 1.0);
    }
}
