//! Export glyph advances of the open-licensed fonts in `web/fonts` (Liberation, Carlito, Caladea; SIL OFL 1.1) into `src/font_metrics.bin`,
//! so text is measured the same way on every platform (the browser/WASM build has no system fonts).
//!
//! Liberation Sans / Serif / Mono, Carlito and Caladea are metric-compatible with Arial, Times New Roman, Courier New, Calibri and
//! Cambria: the table is filed under both names. No proprietary font is read.
//!
//! usage: export_open_font_metrics <web/fonts dir> <out.bin>      (same format as export_font_metrics)
use std::env;
use std::path::Path;

/// (file stem, names the table is filed under)
const FONTS: &[(&str, &[&str])] = &[
    ("LiberationSans", &["arial", "liberation sans"]),
    ("LiberationSerif", &["times new roman", "liberation serif"]),
    ("LiberationMono", &["courier new", "liberation mono"]),
    ("Carlito", &["calibri", "carlito"]),
    ("Caladea", &["cambria", "caladea"]),
];
const STYLES: &[(&str, bool, bool)] = &[("Regular", false, false), ("Bold", true, false), ("Italic", false, true), ("BoldItalic", true, true)];
const BLOCKS: &[(u32, u32)] = &[(0x20, 0x24F), (0x370, 0x3FF), (0x400, 0x4FF), (0x2000, 0x206F), (0x20A0, 0x20BF), (0x2100, 0x214F), (0x2190, 0x21FF)];

fn main() {
    let dir = env::args().nth(1).expect("usage: export_open_font_metrics <web/fonts dir> <out.bin>");
    let out = env::args().nth(2).expect("usage: export_open_font_metrics <web/fonts dir> <out.bin>");
    let mut fonts: Vec<Vec<u8>> = Vec::new();
    for (stem, names) in FONTS {
        for (style, bold, italic) in STYLES {
            let path = Path::new(&dir).join(format!("{stem}-{style}.ttf"));
            let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let face = ttf_parser::Face::parse(&data, 0).expect("font");
            let upem = face.units_per_em();
            let mut ranges: Vec<(u32, Vec<u16>)> = Vec::new();
            for &(lo, hi) in BLOCKS {
                let mut advances = Vec::new();
                let mut any = false;
                for cp in lo..=hi {
                    let advance = char::from_u32(cp).and_then(|c| face.glyph_index(c)).and_then(|g| face.glyph_hor_advance(g));
                    any |= advance.is_some();
                    advances.push(advance.unwrap_or(0xFFFF));
                }
                if any {
                    ranges.push((lo, advances));
                }
            }
            for name in *names {
                let mut bytes = Vec::new();
                bytes.push(name.len() as u8);
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(u8::from(*bold) | (u8::from(*italic) << 1));
                bytes.extend_from_slice(&upem.to_le_bytes());
                bytes.extend_from_slice(&(ranges.len() as u16).to_le_bytes());
                for (start, advances) in &ranges {
                    bytes.extend_from_slice(&start.to_le_bytes());
                    bytes.extend_from_slice(&(advances.len() as u16).to_le_bytes());
                    for a in advances {
                        bytes.extend_from_slice(&a.to_le_bytes());
                    }
                }
                fonts.push(bytes);
                println!("{name} bold={bold} italic={italic}");
            }
        }
    }
    let mut data = (fonts.len() as u32).to_le_bytes().to_vec();
    for font in fonts {
        data.extend(font);
    }
    std::fs::write(&out, &data).expect("write");
    println!("{} faces, {} bytes", fonts_len(&data), data.len());
}

fn fonts_len(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}
