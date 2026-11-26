//! The binary drawing format (versions 6 and 11): the tree of streams inside the `VisioDocument` stream.
//!
//! Microsoft publishes no specification of it. The layout used here was checked byte by byte against sample files, against the same
//! drawings saved as XML, and by comparing rendered pages; `docs/47-legacy-formats.md` records what is known and what is not.
//!
//! The file is a header, then streams that are found through pointers. A stream holds pointers to further streams, a run of records
//! ("chunks"), or one blob (a name, a font, the colour table, a picture). Everything is read defensively: a damaged or hostile file
//! gives an error or loses the damaged part, and never panics or loops.
mod build;
mod glue;
mod records;

use super::cfb;
use super::drawing::Drawing;
use crate::{Error, Result};
pub(super) use build::picture;
use build::{Builder, Scope};
use std::collections::HashSet;

const HEADER_VERSION: usize = 0x1a;
const HEADER_TRAILER_POINTER: usize = 0x24;
/// Limits against hostile files: what all streams may expand to together, how many streams and how deep the tree may go.
const MAX_EXPANDED: usize = 256 * 1024 * 1024;
const MAX_STREAMS: usize = 200_000;
const MAX_STREAM_DEPTH: usize = 12;

// Stream (and record) types.
const TRAILER: u32 = 0x14;
const PAGE: u32 = 0x15;
const COLORS: u32 = 0x16;
const FONT_LIST_OLD: u32 = 0x18;
const FONT_FACE_OLD: u32 = 0x19;
const STYLES: u32 = 0x1a;
const STENCILS: u32 = 0x1d;
const MASTER: u32 = 0x1e;
const PAGES: u32 = 0x27;
const NAME: u32 = 0x33;
const NAME_LIST: u32 = 0x32;
const NAME_INDEX: u32 = 0xc9;
const FONT_FACE: u32 = 0xd7;
const FONT_LIST: u32 = 0xd8;

#[derive(Debug, Clone, Copy)]
struct Pointer {
    kind: u32,
    offset: usize,
    length: usize,
    format: u16,
}

impl Pointer {
    fn compressed(&self) -> bool {
        self.format & 2 != 0
    }

    /// A run of records.
    fn holds_chunks(&self) -> bool {
        matches!(self.format >> 4, 0x8 | 0xc | 0xd)
    }

    /// Pointers to more streams.
    fn holds_pointers(&self) -> bool {
        self.kind == TRAILER || (self.format >> 4 == 0x5 && self.kind != COLORS)
    }

    const SIZE: usize = 18;

    /// `type, address, offset, length, format`.
    fn read(data: &[u8], at: usize) -> Option<Pointer> {
        let mut r = records::Reader::at(data, at);
        let kind = r.u32()?;
        r.skip(4);
        let offset = r.u32()? as usize;
        let length = r.u32()? as usize;
        Some(Pointer { kind, offset, length, format: r.u16()? })
    }
}

/// The LZ77 variant of compressed streams: a flag byte says for the next eight items whether each is a literal byte or a
/// two-byte back-reference into a 4096-byte window (12 bits of position, 4 bits of length minus 3).
fn expand(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut window = [0u8; 4096];
    let mut position = 0usize;
    let mut out = Vec::with_capacity(data.len().saturating_mul(3).min(limit));
    let mut input = data.iter().copied();
    'blocks: while let Some(flags) = input.next() {
        for bit in 0..8 {
            if flags & (1 << bit) != 0 {
                let Some(byte) = input.next() else { break 'blocks };
                window[position & 4095] = byte;
                position += 1;
                out.push(byte);
            } else {
                let (Some(low), Some(high)) = (input.next(), input.next()) else { break 'blocks };
                let length = (high & 0x0f) as usize + 3;
                let stored = (((high & 0xf0) as usize) << 4) | low as usize;
                // Positions are stored relative to an encoder that starts 18 bytes before the end of the window.
                let from = if stored > 4078 { stored - 4078 } else { stored + 18 };
                for i in 0..length {
                    let byte = window[(from + i) & 4095];
                    window[(position + i) & 4095] = byte;
                    out.push(byte);
                }
                position += length;
            }
        }
        if out.len() > limit {
            return None;
        }
    }
    (out.len() <= limit).then_some(out)
}

struct Walker<'a> {
    document: &'a [u8],
    expanded: usize,
    streams: usize,
    seen: HashSet<usize>,
    builder: Builder,
}

impl Walker<'_> {
    fn visit(&mut self, pointer: Pointer, index: u32, scope: Scope, depth: usize) -> Result<()> {
        self.streams += 1;
        if self.streams > MAX_STREAMS || depth > MAX_STREAM_DEPTH {
            return Err(Error("the drawing has too many or too deeply nested streams".to_string()));
        }
        let Some(raw) = pointer.offset.checked_add(pointer.length).and_then(|end| self.document.get(pointer.offset..end)) else {
            self.builder.drawing.note("a part of the file points outside the file and was skipped");
            return Ok(());
        };
        let expanded;
        let data: &[u8] = if pointer.compressed() {
            let Some(bytes) = expand(raw, MAX_EXPANDED - self.expanded.min(MAX_EXPANDED)) else {
                return Err(Error("the drawing expands to more than the size limit".to_string()));
            };
            expanded = bytes;
            &expanded
        } else {
            raw
        };
        self.expanded = self.expanded.saturating_add(data.len());
        if self.expanded > MAX_EXPANDED {
            return Err(Error("the drawing expands to more than the size limit".to_string()));
        }
        // Compressed streams other than record runs start with four bytes that are not part of the content.
        let shift = if pointer.compressed() { 4 } else { 0 };
        let scope = match pointer.kind {
            STYLES => Scope::Styles,
            STENCILS => Scope::Stencils,
            MASTER => Scope::Master(index),
            PAGES => Scope::Pages,
            PAGE => Scope::Page(index),
            _ => scope,
        };
        if pointer.holds_chunks() {
            self.builder.chunk_stream(scope, index, data);
        } else if !pointer.holds_pointers() {
            let blob = data.get(shift..).unwrap_or_default();
            match pointer.kind {
                COLORS => self.builder.colors(blob),
                NAME => self.builder.name(index, blob),
                NAME_INDEX => self.builder.name_index(scope, blob),
                FONT_FACE => self.builder.font(index, blob),
                FONT_FACE_OLD => self.builder.old_font(index, blob),
                kind => self.builder.blob(scope, kind, index, blob),
            }
        }
        if pointer.holds_pointers() {
            // The same list reached twice would be a cycle.
            if !self.seen.insert(pointer.offset) {
                return Ok(());
            }
            let mut children = self.pointers(data, shift);
            if pointer.kind == TRAILER {
                // The colour table, the fonts and the names are needed while the pages are read.
                children.sort_by_key(|(_, child)| !matches!(child.kind, COLORS | FONT_LIST_OLD | FONT_LIST | NAME_LIST));
            }
            for (index, child) in children {
                self.visit(child, index, scope, depth + 1)?;
            }
        }
        Ok(())
    }

    /// The pointers of a pointer stream with their index, in the order the file wants them read: at the offset the stream starts with,
    /// `order count, pointer count, 4 bytes, the pointers, the order`.
    fn pointers(&mut self, data: &[u8], shift: usize) -> Vec<(u32, Pointer)> {
        let at = records::Reader::at(data, shift).u32().and_then(|offset| (offset as usize).checked_add(shift)?.checked_sub(4));
        let Some(at) = at else { return Vec::new() };
        let mut r = records::Reader::at(data, at);
        let (Some(order_count), Some(count)) = (r.u32(), r.u32()) else { return Vec::new() };
        let start = at + 12;
        let count = (count as usize).min(data.len().saturating_sub(start) / Pointer::SIZE);
        let list: Vec<(u32, Pointer)> = (0..count)
            .filter_map(|i| Some((i as u32, Pointer::read(data, start + i * Pointer::SIZE)?)))
            .filter(|(_, p)| p.kind != 0 && p.length > 0)
            .collect();
        // An explicit order (of pages, masters, shapes) may follow the pointers.
        let after = start + count * Pointer::SIZE;
        let order: Vec<u32> = (0..(order_count as usize).min(count)).filter_map(|i| records::Reader::at(data, after + i * 4).u32()).collect();
        if order.is_empty() {
            return list;
        }
        let mut taken = vec![false; count];
        let mut sorted = Vec::with_capacity(list.len());
        for index in order.into_iter().chain(0..count as u32) {
            let Some(slot) = taken.get_mut(index as usize) else { continue };
            if !*slot {
                *slot = true;
                if let Ok(found) = list.binary_search_by_key(&index, |(i, _)| *i) {
                    sorted.push(list[found]);
                }
            }
        }
        sorted
    }
}

pub(super) fn read(bytes: &[u8]) -> Result<Drawing> {
    let document = cfb::stream(bytes, "VisioDocument")?;
    if !document.starts_with(b"Visio (TM) Drawing\r\n") {
        return Err(Error("the VisioDocument stream has no drawing header".to_string()));
    }
    let version = *document.get(HEADER_VERSION).ok_or_else(|| Error("the drawing header is cut short".to_string()))?;
    if !matches!(version, 6 | 11) {
        return Err(Error(format!(
            "this binary drawing has file version {version} ({}); only versions 6 (Visio 2000-2002) and 11 (Visio 2003-2010) can be read",
            if version < 6 { "Visio 5 or older" } else { "unknown" }
        )));
    }
    let trailer = Pointer::read(&document, HEADER_TRAILER_POINTER).ok_or_else(|| Error("the drawing header is cut short".to_string()))?;
    let mut walker = Walker { document: &document, expanded: 0, streams: 0, seen: HashSet::new(), builder: Builder::new(version) };
    walker.visit(Pointer { kind: TRAILER, ..trailer }, 0, Scope::Document, 0)?;
    let mut builder = walker.builder;
    if cfb::stream(bytes, "_VBA_PROJECT").is_ok() || cfb::stream(bytes, "dir").is_ok() {
        builder.drawing.note("macros are not carried over");
    }
    // Always true of this format, as far as it is read here.
    builder.drawing.note("formulas are not read: shapes keep their saved form, resizing scales them plainly, and behaviour that formulas gave them is gone");
    builder.drawing.note("connectors are glued only where the connector itself stores the link: a link that comes from its master is not restored");
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use super::expand;

    #[test]
    fn expands_literals_and_back_references() {
        // Four literal bytes, then a back-reference to the start of the window.
        let stored = 4096 - 18; // window position 0
        let data = [0x0f, b'a', b'b', b'c', b'd', (stored & 0xff) as u8, ((stored >> 8) << 4) as u8 | 1];
        assert_eq!(expand(&data, 100).unwrap(), b"abcdabcd");
        assert!(expand(&data, 3).is_none(), "the size limit holds");
    }
}
