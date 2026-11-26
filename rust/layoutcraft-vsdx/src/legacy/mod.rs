//! Drawings in the formats that came before `.vsdx`: the binary `.vsd` / `.vss` / `.vst` and the XML `.vdx` / `.vsx` / `.vtx`.
//!
//! Neither is edited in place. A file is read into [`drawing::Drawing`] (styles, masters, pages, shapes and their cells) and written out
//! as a new `.vsdx` package, which the rest of the engine opens like any other drawing. [`Converted::notes`] says what was left behind.
//!
//! Evidence and limits: `docs/47-legacy-formats.md`.
use crate::{Error, Result};

mod cfb;
mod drawing;
mod emit;
mod vsd;
mod xml;

/// A bitmap stored without its file header, with the header added.
fn vsd_bitmap(data: &[u8]) -> Option<Vec<u8>> {
    vsd::picture(data).map(|picture| picture.data)
}

/// A legacy drawing rewritten as a `.vsdx` package.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The new package.
    pub vsdx: Vec<u8>,
    /// What the user should know: what the old file holds that the new package does not, or only approximately.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyKind {
    /// An OLE compound file (`.vsd`, `.vss`, `.vst`).
    Binary,
    /// Visio 2003 XML (`.vdx`, `.vsx`, `.vtx`).
    Xml,
}

impl LegacyKind {
    pub fn describe(self) -> &'static str {
        match self {
            LegacyKind::Binary => "binary Visio format (.vsd / .vss / .vst)",
            LegacyKind::Xml => "Visio 2003 XML format (.vdx / .vsx / .vtx)",
        }
    }
}

/// What kind of legacy drawing the bytes look like, by content (file names lie). `None` for anything else, including `.vsdx`.
pub fn detect(bytes: &[u8]) -> Option<LegacyKind> {
    if bytes.starts_with(&cfb::MAGIC) {
        return Some(LegacyKind::Binary);
    }
    xml::looks_like_vdx(bytes).then_some(LegacyKind::Xml)
}

/// Convert a legacy drawing (see [`detect`]) to a `.vsdx` package.
pub fn convert(bytes: &[u8]) -> Result<Converted> {
    let drawing = match detect(bytes) {
        Some(LegacyKind::Binary) => vsd::read(bytes)?,
        Some(LegacyKind::Xml) => xml::read(bytes)?,
        None => return Err(Error("not a binary or XML Visio drawing".to_string())),
    };
    emit::package(drawing)
}
