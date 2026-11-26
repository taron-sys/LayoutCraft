//! What both legacy readers produce: the parts of a drawing in the shape `.vsdx` stores them (cells, sections and rows with the cell
//! names of [MS-VSDX]), so writing the package is a direct transcription.
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub(super) struct Cell {
    pub name: String,
    /// `None`: the cell has a formula only.
    pub value: Option<String>,
    pub formula: Option<String>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Row {
    pub index: Option<u32>,
    pub name: Option<String>,
    /// The row type of a geometry row (`MoveTo`, `LineTo`, ...).
    pub kind: Option<String>,
    pub deleted: bool,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Section {
    pub name: String,
    pub index: Option<u32>,
    pub cells: Vec<Cell>,
    pub rows: Vec<Row>,
}

/// Cells and sections: what a shape, a style sheet and a page sheet have in common.
#[derive(Debug, Clone, Default)]
pub(super) struct Sheet {
    pub cells: Vec<Cell>,
    pub sections: Vec<Section>,
}

impl Sheet {
    /// Set a cell with a value, replacing one of the same name.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        set_cell(&mut self.cells, name, value.into());
    }

    pub fn number(&mut self, name: &str, value: f64) {
        self.set(name, number(value));
    }

    pub fn cell(&self, name: &str) -> Option<&Cell> {
        self.cells.iter().find(|cell| cell.name == name)
    }

    /// The section `name` (and `index`), added when it is not there yet.
    pub fn section(&mut self, name: &str, index: Option<u32>) -> &mut Section {
        let at = match self.sections.iter().position(|s| s.name == name && s.index == index) {
            Some(at) => at,
            None => {
                self.sections.push(Section { name: name.to_string(), index, ..Section::default() });
                self.sections.len() - 1
            }
        };
        &mut self.sections[at]
    }
}

impl Row {
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        set_cell(&mut self.cells, name, value.into());
    }

    pub fn number(&mut self, name: &str, value: f64) {
        self.set(name, number(value));
    }
}

fn set_cell(cells: &mut Vec<Cell>, name: &str, value: String) {
    match cells.iter_mut().find(|cell| cell.name == name) {
        Some(cell) => cell.value = Some(value),
        None => cells.push(Cell { name: name.to_string(), value: Some(value), ..Cell::default() }),
    }
}

/// A number as `.vsdx` writes it; what is not finite becomes 0.
pub(super) fn number(value: f64) -> String {
    if !value.is_finite() || value == 0.0 {
        return "0".to_string();
    }
    crate::writer::num_text(value)
}

#[derive(Debug, Clone)]
pub(super) enum TextPiece {
    Text(String),
    /// Start of the character formatting row.
    Character(u32),
    /// Start of the paragraph formatting row.
    Paragraph(u32),
    /// Start of the tab row.
    Tabs(u32),
    /// A text field and the text it showed when the file was saved.
    Field(u32, String),
}

#[derive(Debug, Clone)]
pub(super) struct Picture {
    /// `Bitmap`, `MetaFile`, `EnhMetaFile`, `Object` ...
    pub foreign_type: String,
    pub compression: Option<String>,
    /// File name extension of the media part.
    pub extension: &'static str,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Shape {
    pub id: u32,
    pub kind: &'static str,
    pub name: Option<String>,
    pub master: Option<u32>,
    pub master_shape: Option<u32>,
    pub line_style: Option<u32>,
    pub fill_style: Option<u32>,
    pub text_style: Option<u32>,
    pub sheet: Sheet,
    pub text: Option<Vec<TextPiece>>,
    /// The field values of a binary shape that has no text of its own: they fill the fields of its master's text.
    pub field_values: Vec<String>,
    pub picture: Option<Picture>,
    pub children: Vec<Shape>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Style {
    pub id: u32,
    pub name: String,
    pub line_style: Option<u32>,
    pub fill_style: Option<u32>,
    pub text_style: Option<u32>,
    pub sheet: Sheet,
}

#[derive(Debug, Clone)]
pub(super) struct Connect {
    pub from_sheet: String,
    pub from_cell: String,
    pub to_sheet: String,
    pub to_cell: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Master {
    pub id: u32,
    pub name: String,
    pub sheet: Sheet,
    pub shapes: Vec<Shape>,
    pub connects: Vec<Connect>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Page {
    pub id: u32,
    pub name: String,
    pub background: bool,
    pub back_page: Option<u32>,
    pub sheet: Sheet,
    pub shapes: Vec<Shape>,
    pub connects: Vec<Connect>,
}

#[derive(Debug, Default)]
pub(super) struct Drawing {
    pub styles: Vec<Style>,
    pub masters: Vec<Master>,
    pub pages: Vec<Page>,
    /// The document's colour table, by index.
    pub colors: BTreeMap<u32, String>,
    pub fonts: Vec<(u32, String)>,
    pub default_styles: [Option<u32>; 3],
    /// What was left behind, with how often.
    pub notes: BTreeMap<String, usize>,
}

impl Drawing {
    pub fn note(&mut self, what: &str) {
        *self.notes.entry(what.to_string()).or_default() += 1;
    }
}

/// How deep groups may nest (the package reader refuses deeper drawings).
pub(super) const MAX_DEPTH: usize = 180;
