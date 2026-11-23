//! PRIVATE tool: export glyph advances of the installed Windows fonts (proprietary) into a table. The output belongs in `private/font-metrics-windows/`, never in `src/`
//! (the public table is made by `export_open_font_metrics` from the open fonts in `web/fonts`).
//!
//! usage: export_font_metrics <out.bin>
//!
//! Format (little endian): u32 font count, then per font
//!   u8 name length, name (lower case), u8 flags (1 bold, 2 italic), u16 units per em,
//!   u16 range count, then per range u32 first code point, u16 length, length * u16 advances
//!   (0xFFFF = the font has no glyph).
use fontdb::{Database, Family, Query, Style, Weight};
use std::env;

const FAMILIES: &[&str] = &[
    "Arial", "Arial Black", "Arial Narrow", "Arial Unicode MS", "Bahnschrift", "Book Antiqua", "Bookman Old Style",
    "Calibri", "Calibri Light", "Cambria", "Candara", "Century Gothic", "Century", "Comic Sans MS", "Consolas",
    "Constantia", "Corbel", "Courier New", "Franklin Gothic Book", "Franklin Gothic Medium", "Garamond", "Georgia",
    "Gill Sans MT", "Impact", "Lucida Console", "Lucida Sans Unicode", "Microsoft Sans Serif", "Palatino Linotype",
    "Segoe UI", "Segoe UI Light", "Segoe UI Semibold", "Segoe UI Symbol", "Tahoma", "Times New Roman",
    "Trebuchet MS", "Tw Cen MT", "Verdana",
];

/// Code point blocks that matter for wrapping Latin, Greek, Cyrillic and common punctuation.
const BLOCKS: &[(u32, u32)] = &[
    (0x20, 0x24F),
    (0x370, 0x3FF),
    (0x400, 0x4FF),
    (0x2000, 0x206F),
    (0x20A0, 0x20BF),
    (0x2100, 0x214F),
    (0x2190, 0x21FF),
];

fn main() {
    let out = env::args().nth(1).expect("usage: export_font_metrics <out.bin>");
    let mut db = Database::new();
    db.load_system_fonts();
    let mut fonts: Vec<Vec<u8>> = Vec::new();
    let mut count = 0u32;
    for family in FAMILIES {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let query = Query {
                families: &[Family::Name(family)],
                weight: if bold { Weight::BOLD } else { Weight::NORMAL },
                stretch: fontdb::Stretch::default(),
                style: if italic { Style::Italic } else { Style::Normal },
            };
            let Some(id) = db.query(&query) else { continue };
            // fontdb falls back to a neighbouring face; keep a face only when it really is the
            // requested style, otherwise the runtime falls back to the regular one itself.
            let Some(info) = db.face(id) else { continue };
            if !info.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(family)) {
                continue;
            }
            let face_bold = info.weight.0 >= 600;
            let face_italic = info.style != Style::Normal;
            if (face_bold, face_italic) != (bold, italic) {
                continue;
            }
            let entry = db.with_face_data(id, |data, index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                let upem = face.units_per_em();
                let mut ranges: Vec<(u32, Vec<u16>)> = Vec::new();
                for &(lo, hi) in BLOCKS {
                    let mut advances = Vec::new();
                    let mut any = false;
                    for cp in lo..=hi {
                        let advance = char::from_u32(cp)
                            .and_then(|c| face.glyph_index(c))
                            .and_then(|g| face.glyph_hor_advance(g));
                        any |= advance.is_some();
                        advances.push(advance.unwrap_or(0xFFFF));
                    }
                    if any {
                        ranges.push((lo, advances));
                    }
                }
                let mut bytes = Vec::new();
                let name = family.to_ascii_lowercase();
                bytes.push(name.len() as u8);
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(u8::from(bold) | (u8::from(italic) << 1));
                bytes.extend_from_slice(&upem.to_le_bytes());
                bytes.extend_from_slice(&(ranges.len() as u16).to_le_bytes());
                for (start, advances) in ranges {
                    bytes.extend_from_slice(&start.to_le_bytes());
                    bytes.extend_from_slice(&(advances.len() as u16).to_le_bytes());
                    for a in advances {
                        bytes.extend_from_slice(&a.to_le_bytes());
                    }
                }
                Some(bytes)
            });
            if let Some(Some(bytes)) = entry {
                fonts.push(bytes);
                count += 1;
                println!("{family} bold={bold} italic={italic}");
            }
        }
    }
    let mut data = count.to_le_bytes().to_vec();
    for font in fonts {
        data.extend(font);
    }
    std::fs::write(&out, &data).expect("write");
    println!("{count} faces, {} bytes", data.len());
}
