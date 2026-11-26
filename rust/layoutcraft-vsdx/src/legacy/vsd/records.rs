//! The fields of single records. A record starts with fixed fields; a "cell" among them is a unit byte followed by an IEEE double in the
//! file's internal units (inches, radians, days), the same value `.vsdx` stores. After the fixed fields come blocks with the
//! formulas, of which only literal strings and the point lists of curves are read. Offsets: `docs/47-legacy-formats.md`.
//!
//! Every function reads as far as the record goes; a short record just sets fewer cells.

/// Unit byte of a cell whose value is a string.
pub(super) const UNIT_STRING: u8 = 0xe8;
const TOKEN_STRING: u8 = 0x60;
const TOKEN_INTEGER: u8 = 0x62;
const TOKEN_FUNCTION: u8 = 0x81;
const BLOCK_END: u8 = 0xfe;

#[derive(Clone, Copy)]
pub(super) struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn at(data: &'a [u8], at: usize) -> Self {
        Reader { data, at }
    }

    pub fn skip(&mut self, n: usize) {
        self.at = self.at.saturating_add(n);
    }

    pub fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.at.checked_add(N)?;
        let out = self.data.get(self.at..end)?.try_into().ok()?;
        self.at = end;
        Some(out)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.bytes::<1>().map(|b| b[0])
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.bytes().map(u16::from_le_bytes)
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.bytes().map(u32::from_le_bytes)
    }

    pub fn f64(&mut self) -> Option<f64> {
        self.bytes().map(f64::from_le_bytes).filter(|v| v.is_finite())
    }

    /// A unit byte and its value.
    pub fn cell(&mut self) -> Option<(u8, f64)> {
        let unit = self.u8()?;
        let value = self.bytes().map(f64::from_le_bytes)?;
        Some((unit, if value.is_finite() { value } else { 0.0 }))
    }
}

/// 8-bit text of the versions before 11, read as Windows-1252.
pub(super) fn windows_1252(bytes: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
        '\u{9d}', 'ž', 'Ÿ',
    ];
    bytes.iter().map(|&b| if (0x80..0xa0).contains(&b) { HIGH[(b - 0x80) as usize] } else { b as char }).collect()
}

/// A string up to its terminating zero: UTF-16 from version 11 on, 8-bit before.
pub(super) fn zero_terminated(bytes: &[u8], version: u8) -> String {
    if version >= 11 {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).take_while(|&u| u != 0).collect();
        String::from_utf16_lossy(&units)
    } else {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        windows_1252(&bytes[..end])
    }
}

/// The literal string a formula block gives cell number `cell`, if there is one. A block is
/// `length u32, 2, cell number, 0x60, characters, the characters, terminator, 0xfe`.
pub(super) fn block_string(data: &[u8], cell: u8, version: u8) -> Option<String> {
    let width = if version >= 11 { 2 } else { 1 };
    (0..data.len().saturating_sub(8)).find_map(|at| {
        if data[at + 4] != 2 || data[at + 5] != cell || data[at + 6] != TOKEN_STRING {
            return None;
        }
        let length = Reader::at(data, at).u32()? as usize;
        let characters = data[at + 7] as usize;
        let expected = 8 + (characters + 1) * width + 1;
        if length != expected || *data.get(at + length - 1)? != BLOCK_END {
            return None;
        }
        let text = data.get(at + 8..at + 8 + characters * width)?;
        Some(zero_terminated(text, version))
    })
}

/// The numbers of an inline point-list formula (`NURBS(...)` or `POLYLINE(...)`) stored for cell number `cell`; `None` when it refers to
/// other cells.
pub(super) fn block_numbers(data: &[u8], from: usize, cell: u8) -> Option<Vec<f64>> {
    let start = (from..data.len().saturating_sub(6)).find(|&at| {
        let length = Reader::at(data, at).u32().unwrap_or(0) as usize;
        data[at + 4] == 2 && data[at + 5] == cell && length > 7 && at.checked_add(length - 1).and_then(|end| data.get(end)) == Some(&BLOCK_END)
    })?;
    let mut r = Reader::at(data, start + 6);
    let mut out = Vec::new();
    loop {
        match r.u8()? {
            TOKEN_FUNCTION => return Some(out),
            TOKEN_INTEGER => out.push(r.u16()? as i16 as f64),
            0x20..=0x5f => out.push(r.f64()?),
            _ => return None,
        }
    }
}

/// The point list of a NURBS or polyline row as the formula `.vsdx` stores in the row, from a shape data record: byte 0 says which kind.
pub(super) fn shape_data_formula(data: &[u8]) -> Option<String> {
    let kind = *data.first()?;
    let mut r = Reader::at(data, 16);
    let mut args = Vec::new();
    let per_point = match kind {
        0x82 => {
            let last_knot = r.f64()?;
            let degree = r.u16()?;
            args.extend([last_knot, degree as f64, r.u8()? as f64, r.u8()? as f64]);
            4
        }
        0x80 => {
            args.extend([r.u8()? as f64, r.u8()? as f64]);
            2
        }
        _ => return None,
    };
    let count = r.u32()? as usize;
    if count > data.len() / 16 {
        return None;
    }
    for _ in 0..count * per_point {
        args.push(r.f64()?);
    }
    Some(point_formula(if kind == 0x82 { "NURBS" } else { "POLYLINE" }, &args))
}

pub(super) fn point_formula(function: &str, args: &[f64]) -> String {
    let list: Vec<String> = args.iter().map(|&v| super::super::drawing::number(v)).collect();
    format!("{function}({})", list.join(","))
}
