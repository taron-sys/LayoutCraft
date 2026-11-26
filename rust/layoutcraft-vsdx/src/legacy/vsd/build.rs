//! Records to drawing: which shape, page, master or style a record belongs to, and at the end the shape trees and the texts.
use super::glue::{self, Links};
use super::super::drawing::{Drawing, Master, Page, Picture, Row, Shape, Sheet, Style, TextPiece, MAX_DEPTH};
use super::records::{block_numbers, block_string, point_formula, shape_data_formula, windows_1252, zero_terminated, Reader, UNIT_STRING};
use std::collections::{BTreeMap, HashMap};

const NONE: u32 = u32::MAX;
const MAX_SHAPES: usize = 100_000;
const MAX_ROWS: usize = 20_000;
const MAX_GEOMETRIES: usize = 1_000;
/// Pages, and masters, that a file may hold; records are looked up among them one by one.
const MAX_CONTAINERS: usize = 4_000;

// Record types.
const FOREIGN_DATA: u32 = 0x0c;
const OLE_LIST: u32 = 0x0d;
const TEXT: u32 = 0x0e;
const PAGE: u32 = 0x15;
const OLE_DATA: u32 = 0x1f;
const NAME: u32 = 0x2d;
const PAGE_SHEET: u32 = 0x46;
const GROUP: u32 = 0x47;
const SHAPE: u32 = 0x48;
const STYLE_SHEET: u32 = 0x4a;
const GUIDE: u32 = 0x4d;
const FOREIGN: u32 = 0x4e;
const SHAPE_LIST: u32 = 0x65;
const FIELD_LIST: u32 = 0x66;
const CHARACTER_LIST: u32 = 0x69;
const PARAGRAPH_LIST: u32 = 0x6a;
const GEOMETRY_LIST: u32 = 0x6c;
const SHAPE_ID: u32 = 0x83;
const LINE: u32 = 0x85;
const FILL: u32 = 0x86;
const TEXT_BLOCK: u32 = 0x87;
const GEOMETRY: u32 = 0x89;
const MOVE_TO: u32 = 0x8a;
const LINE_TO: u32 = 0x8b;
const ARC_TO: u32 = 0x8c;
const INFINITE_LINE: u32 = 0x8d;
const ELLIPSE: u32 = 0x8f;
const ELLIPTICAL_ARC: u32 = 0x90;
const PAGE_PROPERTIES: u32 = 0x92;
const CHARACTER: u32 = 0x94;
const PARAGRAPH: u32 = 0x95;
const FOREIGN_TYPE: u32 = 0x98;
const CONNECTION_POINT: u32 = 0x99;
const TRANSFORM: u32 = 0x9b;
const TEXT_TRANSFORM: u32 = 0x9c;
const TRANSFORM_1D: u32 = 0x9d;
const EVENTS: u32 = 0xa4;
const FIELD: u32 = 0xa1;
const SPLINE_START: u32 = 0xa5;
const SPLINE_KNOT: u32 = 0xa6;
const LAYER_MEMBER: u32 = 0xa7;
const LAYER: u32 = 0xa8;
const PROPERTY: u32 = 0xb6;
const POLYLINE_TO: u32 = 0xc1;
const NURBS_TO: u32 = 0xc3;
const SHAPE_DATA: u32 = 0xd1;

/// Visio's colour table when a file brings none.
const STANDARD_COLORS: [&str; 24] = [
    "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#800000", "#008000", "#000080", "#808000", "#800080", "#008080",
    "#c0c0c0", "#e6e6e6", "#cdcdcd", "#b3b3b3", "#9a9a9a", "#808080", "#666666", "#4d4d4d", "#333333", "#1a1a1a",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    Document,
    Styles,
    Stencils,
    Master(u32),
    Pages,
    Page(u32),
}

struct Field {
    unit: u8,
    value: f64,
    name: u32,
    /// The field's format number (`visFieldFormats` of the automation interface), when the record carries it.
    format: Option<u16>,
}

/// A curve row whose points are in a shape data record that may come later.
struct PendingCurve {
    geometry: usize,
    row: usize,
    data: u32,
}

/// A shape (or page sheet, or style sheet) while its records arrive.
#[derive(Default)]
struct Raw {
    shape: Shape,
    parent: u32,
    /// `ShapeID` records by their position, and the stacking order that refers to those positions.
    children: BTreeMap<u32, u32>,
    child_order: Vec<u32>,
    geometry_order: Vec<Vec<u32>>,
    text: Option<Vec<u8>>,
    characters: Vec<(u32, u32)>,
    paragraphs: Vec<(u32, u32)>,
    character_order: Vec<u32>,
    paragraph_order: Vec<u32>,
    fields: BTreeMap<u32, Field>,
    field_order: Vec<u32>,
    names: BTreeMap<u32, String>,
    shape_data: BTreeMap<u32, Vec<u8>>,
    pending: Vec<PendingCurve>,
    links: Links,
}

#[derive(Default)]
struct Container {
    key: u32,
    background_page: Option<u32>,
    sheet: Raw,
    shapes: Vec<Raw>,
}

#[derive(Clone, Copy)]
enum Target {
    Nothing,
    PageSheet,
    Shape(usize),
    Style(usize),
}

pub(super) struct Builder {
    version: u8,
    pub drawing: Drawing,
    pages: Vec<Container>,
    masters: Vec<Container>,
    styles: Vec<(Raw, [u32; 3], u32)>,
    names: HashMap<u32, String>,
    page_names: HashMap<u32, u32>,
    master_names: HashMap<u32, u32>,
    fonts: BTreeMap<u32, String>,
    palette: Vec<[u8; 4]>,
    shapes: usize,
    /// Where the last record run ended, for the blobs that follow it.
    last: (Scope, usize),
}

fn container(list: &mut Vec<Container>, key: u32) -> &mut Container {
    let at = match list.iter().position(|c| c.key == key) {
        Some(at) => at,
        // Beyond the limit everything further lands in the last one.
        None if list.len() >= MAX_CONTAINERS => list.len() - 1,
        None => {
            list.push(Container { key, ..Container::default() });
            list.len() - 1
        }
    };
    &mut list[at]
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Cells that follow one another as unit and value pairs.
fn cells(sheet: &mut Sheet, r: &mut Reader, names: &[&str]) {
    for name in names {
        let Some((_, value)) = r.cell() else { return };
        sheet.number(name, value);
    }
}

fn row_cells(row: &mut Row, r: &mut Reader, names: &[&str]) {
    for name in names {
        let Some((_, value)) = r.cell() else { return };
        row.number(name, value);
    }
}

/// The ids of a list record: after the data length of its own header come `header length`, `list length in bytes`, the header, the ids.
fn list_ids(tail: &[u8]) -> Vec<u32> {
    let mut r = Reader::at(tail, 0);
    let (Some(header), Some(length)) = (r.u32(), r.u32()) else { return Vec::new() };
    r.skip(header as usize);
    (0..(length as usize / 4).min(MAX_ROWS)).filter_map(|_| r.u32()).collect()
}

/// The items sorted by an explicit order of their ids; items the order does not name keep their place at the end.
fn in_order<T>(items: &mut [T], order: &[u32], id: impl Fn(&T) -> u32) {
    if order.is_empty() {
        return;
    }
    let position: HashMap<u32, usize> = order.iter().enumerate().map(|(at, id)| (*id, at)).collect();
    items.sort_by_key(|item| position.get(&id(item)).copied().unwrap_or(usize::MAX));
}

impl Builder {
    pub fn new(version: u8) -> Self {
        Builder {
            version,
            drawing: Drawing::default(),
            pages: Vec::new(),
            masters: Vec::new(),
            styles: Vec::new(),
            names: HashMap::new(),
            page_names: HashMap::new(),
            master_names: HashMap::new(),
            fonts: BTreeMap::new(),
            palette: Vec::new(),
            shapes: 0,
            last: (Scope::Document, usize::MAX),
        }
    }

    pub fn colors(&mut self, blob: &[u8]) {
        let count = blob.get(2).copied().unwrap_or(0) as usize;
        let mut r = Reader::at(blob, 4);
        self.palette = (0..count).filter_map(|_| r.bytes::<4>()).collect();
    }

    pub fn name(&mut self, index: u32, blob: &[u8]) {
        if let Some(text) = blob.get(4..) {
            self.names.insert(index, zero_terminated(text, self.version));
        }
    }

    /// Which name belongs to which page or master: `count`, then `name, universal name, element, 1 byte`.
    pub fn name_index(&mut self, scope: Scope, blob: &[u8]) {
        let into = match scope {
            Scope::Pages => &mut self.page_names,
            Scope::Stencils => &mut self.master_names,
            _ => return,
        };
        let mut r = Reader::at(blob, 0);
        let count = r.u32().unwrap_or(0);
        for _ in 0..count.min(100_000) {
            let (Some(name), Some(_), Some(element), Some(_)) = (r.u32(), r.u32(), r.u32(), r.u8()) else { return };
            into.insert(element, name);
        }
    }

    pub fn font(&mut self, index: u32, blob: &[u8]) {
        if let Some(text) = blob.get(4..) {
            let name = zero_terminated(text, 11);
            if !name.is_empty() {
                self.fonts.insert(index, name);
            }
        }
    }

    /// Fonts before version 11: the name is the 8-bit string at the end of the entry.
    pub fn old_font(&mut self, index: u32, blob: &[u8]) {
        let end = blob.len() - blob.iter().rev().take_while(|&&b| b == 0).count();
        let start = end - blob[..end].iter().rev().take_while(|&&b| (0x20..0x7f).contains(&b)).count();
        if end > start {
            self.fonts.insert(index, windows_1252(&blob[start..end]));
        }
    }

    /// A colour of a record: version 11 stores the value next to the table index, older versions only the index.
    fn color(&self, index: u8, rgba: [u8; 4]) -> (String, f64) {
        let transparency = rgba[3] as f64 / 255.0;
        if self.version >= 11 {
            return (hex(rgba[0], rgba[1], rgba[2]), transparency);
        }
        let color = match self.palette.get(index as usize) {
            Some(c) => hex(c[0], c[1], c[2]),
            None => STANDARD_COLORS.get(index as usize).copied().unwrap_or("#000000").to_string(),
        };
        (color, transparency)
    }

    fn read_color(&self, r: &mut Reader) -> Option<(u8, String, f64)> {
        let index = r.u8()?;
        let rgba = r.bytes::<4>()?;
        let (color, transparency) = self.color(index, rgba);
        Some((index, color, transparency))
    }

    pub fn chunk_stream(&mut self, scope: Scope, index: u32, data: &[u8]) {
        let mut target = Target::Nothing;
        const HEADER: usize = 19;
        let mut at = 0usize;
        loop {
            // Records are separated by trailers and zero padding; a record type never starts with a zero byte.
            while data.get(at) == Some(&0) {
                at += 1;
            }
            if data.len().saturating_sub(at) < HEADER {
                break;
            }
            let mut r = Reader::at(data, at);
            let header = (|| Some((r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u16()?, r.u8()?)))();
            let Some((record, id, list, length, level, extra)) = header else { break };
            let start = at + HEADER;
            let end = start.saturating_add(length as usize);
            let Some(body) = data.get(start..end) else {
                // A record that claims more than the stream holds ends the stream.
                self.drawing.note("a damaged record ended a part of the drawing early");
                break;
            };
            self.record(scope, &mut target, record, if id == NONE { index } else { id }, body, &data[start..]);
            at = end + trailer_length(self.version, record, list, level, extra);
        }
        if let Target::Shape(shape) = target {
            self.last = (scope, shape);
        }
    }

    /// A stream holding one piece of data that belongs to the shape of the preceding record run.
    pub fn blob(&mut self, scope: Scope, kind: u32, index: u32, blob: &[u8]) {
        let (last_scope, shape) = self.last;
        if last_scope == scope && matches!(kind, FOREIGN_DATA | TEXT | SHAPE_DATA | NAME | OLE_DATA) {
            let mut target = Target::Shape(shape);
            self.record(scope, &mut target, kind, index, blob, blob);
        }
    }

    fn record(&mut self, scope: Scope, target: &mut Target, record: u32, id: u32, body: &[u8], tail: &[u8]) {
        let list = match scope {
            Scope::Page(key) => Some((&mut self.pages, key)),
            Scope::Master(key) => Some((&mut self.masters, key)),
            _ => None,
        };
        match record {
            PAGE => {
                if let Scope::Page(key) = scope {
                    let back = Reader::at(body, 8).u32().filter(|&b| b != NONE && b != key);
                    container(&mut self.pages, key).background_page = back;
                }
                return;
            }
            PAGE_SHEET => {
                if let Some((list, key)) = list {
                    container(list, key);
                    *target = Target::PageSheet;
                }
                return;
            }
            GROUP | SHAPE | GUIDE | FOREIGN => {
                if let Some((list, key)) = list {
                    if self.shapes >= MAX_SHAPES {
                        self.drawing.note("shapes beyond the size limit were left out");
                        *target = Target::Nothing;
                        return;
                    }
                    self.shapes += 1;
                    let mut raw = shape_header(body);
                    raw.shape.id = id;
                    raw.shape.kind = match record {
                        GROUP => "Group",
                        GUIDE => "Guide",
                        FOREIGN => "Foreign",
                        _ => "Shape",
                    };
                    let shapes = &mut container(list, key).shapes;
                    shapes.push(raw);
                    *target = Target::Shape(shapes.len() - 1);
                }
                return;
            }
            STYLE_SHEET => {
                if scope == Scope::Styles {
                    let header = shape_header(body);
                    // A style sheet header lists its parents as line, fill, text.
                    let parents = [header.shape.fill_style, header.shape.line_style, header.shape.text_style].map(|p| p.unwrap_or(NONE));
                    self.styles.push((Raw::default(), parents, id));
                    *target = Target::Style(self.styles.len() - 1);
                }
                return;
            }
            _ => {}
        }
        // Take the record's shape out of the builder so that reading the record can use the builder's tables.
        let mut raw = match (*target, scope) {
            (Target::Style(at), Scope::Styles) => match self.styles.get_mut(at) {
                Some(style) => std::mem::take(&mut style.0),
                None => return,
            },
            (Target::PageSheet, Scope::Page(key)) => std::mem::take(&mut container(&mut self.pages, key).sheet),
            (Target::PageSheet, Scope::Master(key)) => std::mem::take(&mut container(&mut self.masters, key).sheet),
            (Target::Shape(at), Scope::Page(key)) => match container(&mut self.pages, key).shapes.get_mut(at) {
                Some(shape) => std::mem::take(shape),
                None => return,
            },
            (Target::Shape(at), Scope::Master(key)) => match container(&mut self.masters, key).shapes.get_mut(at) {
                Some(shape) => std::mem::take(shape),
                None => return,
            },
            _ => return,
        };
        self.apply(&mut raw, record, id, body, tail);
        match (*target, scope) {
            (Target::Style(at), _) => self.styles[at].0 = raw,
            (Target::PageSheet, Scope::Page(key)) => container(&mut self.pages, key).sheet = raw,
            (Target::PageSheet, Scope::Master(key)) => container(&mut self.masters, key).sheet = raw,
            (Target::Shape(at), Scope::Page(key)) => container(&mut self.pages, key).shapes[at] = raw,
            (Target::Shape(at), Scope::Master(key)) => container(&mut self.masters, key).shapes[at] = raw,
            _ => {}
        }
    }

    fn apply(&mut self, raw: &mut Raw, record: u32, id: u32, body: &[u8], tail: &[u8]) {
        let version = self.version;
        let mut r = Reader::at(body, 0);
        let sheet = &mut raw.shape.sheet;
        match record {
            SHAPE_ID => {
                if let Some(child) = r.u32() {
                    raw.children.insert(id, child);
                }
            }
            SHAPE_LIST => raw.child_order = list_ids(tail),
            FIELD_LIST => raw.field_order = list_ids(tail),
            CHARACTER_LIST => raw.character_order = list_ids(tail),
            PARAGRAPH_LIST => raw.paragraph_order = list_ids(tail),
            GEOMETRY_LIST if raw.geometry_order.len() < MAX_GEOMETRIES => {
                let index = raw.geometry_order.len() as u32;
                raw.geometry_order.push(list_ids(tail));
                sheet.section("Geometry", Some(index));
            }
            TRANSFORM => {
                cells(sheet, &mut r, &["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle"]);
                for name in ["FlipX", "FlipY"] {
                    if let Some(flag) = r.u8() {
                        sheet.number(name, (flag != 0) as u8 as f64);
                    }
                }
            }
            TRANSFORM_1D => {
                cells(sheet, &mut r, &["BeginX", "BeginY", "EndX", "EndY"]);
                raw.links.transform_1d(body);
            }
            EVENTS => raw.links.events(body),
            TEXT_TRANSFORM => cells(sheet, &mut r, &["TxtPinX", "TxtPinY", "TxtWidth", "TxtHeight", "TxtLocPinX", "TxtLocPinY", "TxtAngle"]),
            PAGE_PROPERTIES => cells(sheet, &mut r, &["PageWidth", "PageHeight", "ShdwOffsetX", "ShdwOffsetY", "PageScale", "DrawingScale"]),
            FOREIGN_TYPE => cells(sheet, &mut r, &["ImgOffsetX", "ImgOffsetY", "ImgWidth", "ImgHeight"]),
            LINE => self.line(sheet, &mut r),
            FILL => self.fill(sheet, &mut r),
            TEXT_BLOCK => {
                cells(sheet, &mut r, &["LeftMargin", "RightMargin", "TopMargin", "BottomMargin"]);
                if let Some(align) = r.u8() {
                    sheet.number("VerticalAlign", align as f64);
                }
                // The background is stored as colour index plus one, 0 meaning none; version 11 stores the colour itself next to it.
                if let (Some(index), Some(rgba)) = (r.u8(), r.bytes::<4>()) {
                    if index != 0 && index != 0xff {
                        let (color, transparency) = self.color(index - 1, rgba);
                        sheet.set("TextBkgnd", color);
                        sheet.number("TextBkgndTrans", transparency);
                    }
                }
                cells(sheet, &mut r, &["DefaultTabStop"]);
            }
            GEOMETRY => {
                if let (Some(flags), Some(section)) = (r.u8(), sheet.sections.iter_mut().rev().find(|s| s.name == "Geometry")) {
                    for (bit, name) in ["NoFill", "NoLine", "NoShow"].iter().enumerate() {
                        let value = if flags & (1 << bit) != 0 { "1" } else { "0" };
                        section.cells.push(super::super::drawing::Cell { name: name.to_string(), value: Some(value.to_string()), ..Default::default() });
                    }
                }
            }
            MOVE_TO | LINE_TO | ARC_TO | INFINITE_LINE | ELLIPSE | ELLIPTICAL_ARC | SPLINE_START | SPLINE_KNOT | POLYLINE_TO | NURBS_TO => geometry_row(raw, record, id, body),
            CONNECTION_POINT => {
                let mut row = Row { index: Some(id), ..Row::default() };
                row_cells(&mut row, &mut r, &["X", "Y", "DirX", "DirY"]);
                let section = sheet.section("Connection", None);
                if section.rows.len() < MAX_ROWS {
                    section.rows.push(row);
                }
            }
            CHARACTER => self.character(raw, id, body),
            PARAGRAPH => paragraph(raw, id, body),
            TEXT => raw.text = Some(body.get(8..).unwrap_or_default().to_vec()),
            NAME => {
                raw.names.insert(id, zero_terminated(body, version));
            }
            FIELD => {
                r.skip(7);
                if let Some(unit) = r.u8() {
                    let mut name = r;
                    let name = name.u32().unwrap_or(NONE);
                    let value = r.f64().unwrap_or(0.0);
                    raw.fields.insert(id, Field { unit, value, name, format: field_format(body) });
                }
            }
            SHAPE_DATA => {
                raw.shape_data.insert(id, body.to_vec());
            }
            LAYER => {
                let index = sheet.sections.iter().find(|s| s.name == "Layer").map_or(0, |s| s.rows.len());
                if index < MAX_ROWS {
                    let mut row = Row { index: Some(index as u32), ..Row::default() };
                    row.set("Name", block_string(body, 0, version).unwrap_or_else(|| format!("Layer {}", index + 1)));
                    r.skip(8);
                    if let Some(color) = r.u8() {
                        row.number("Color", color as f64);
                    }
                    r.skip(5);
                    let flags = [r.u8(), r.u8(), r.u8(), r.u8()];
                    for (name, flag) in [("Visible", flags[0]), ("Print", flags[1]), ("Lock", flags[3])] {
                        if let Some(flag) = flag {
                            row.number(name, (flag != 0) as u8 as f64);
                        }
                    }
                    sheet.section("Layer", None).rows.push(row);
                }
            }
            LAYER_MEMBER => {
                if let Some(members) = block_string(body, 0, version).filter(|m| !m.is_empty()) {
                    sheet.set("LayerMember", members);
                }
            }
            PROPERTY => property(raw, id, body, version),
            FOREIGN_DATA => match picture(body) {
                Some(picture) => raw.shape.picture = Some(picture),
                None => self.drawing.note("a picture or embedded object in an unknown format was left out"),
            },
            OLE_LIST | OLE_DATA => self.drawing.note("embedded OLE objects are not carried over"),
            _ => {}
        }
    }

    fn line(&self, sheet: &mut Sheet, r: &mut Reader) {
        cells(sheet, r, &["LineWeight"]);
        let Some((_, color, transparency)) = self.read_color(r) else { return };
        sheet.set("LineColor", color);
        sheet.number("LineColorTrans", transparency);
        let Some(pattern) = r.u8() else { return };
        sheet.number("LinePattern", pattern as f64);
        cells(sheet, r, &["Rounding"]);
        for name in ["EndArrowSize", "BeginArrow", "EndArrow", "LineCap", "BeginArrowSize"] {
            if let Some(value) = r.u8() {
                sheet.number(name, value as f64);
            }
        }
    }

    fn fill(&self, sheet: &mut Sheet, r: &mut Reader) {
        for (color_cell, transparency_cell) in [("FillForegnd", "FillForegndTrans"), ("FillBkgnd", "FillBkgndTrans")] {
            let Some((_, color, transparency)) = self.read_color(r) else { return };
            sheet.set(color_cell, color);
            sheet.number(transparency_cell, transparency);
        }
        let Some(pattern) = r.u8() else { return };
        sheet.number("FillPattern", pattern as f64);
        for (color_cell, transparency_cell) in [("ShdwForegnd", "ShdwForegndTrans"), ("ShdwBkgnd", "ShdwBkgndTrans")] {
            let Some((_, color, transparency)) = self.read_color(r) else { return };
            sheet.set(color_cell, color);
            sheet.number(transparency_cell, transparency);
        }
        let Some(shadow) = r.u8() else { return };
        sheet.number("ShdwPattern", shadow as f64);
        if self.version >= 11 {
            if let Some(kind) = r.u8() {
                sheet.number("ShapeShdwType", kind as f64);
                cells(sheet, r, &["ShapeShdwOffsetX", "ShapeShdwOffsetY"]);
            }
        }
    }

    fn character(&mut self, raw: &mut Raw, id: u32, body: &[u8]) {
        let mut r = Reader::at(body, 0);
        let Some(count) = r.u32() else { return };
        let section = raw.shape.sheet.section("Character", None);
        let index = section.rows.len();
        if index >= MAX_ROWS {
            return;
        }
        let mut row = Row { index: Some(index as u32), ..Row::default() };
        if let Some(font) = r.u16() {
            if let Some(name) = self.fonts.get(&(font as u32)) {
                row.set("Font", name.clone());
            }
        }
        if let Some((_, color, transparency)) = self.read_color(&mut r) {
            row.set("Color", color);
            row.number("ColorTrans", transparency);
        }
        for name in ["Style", "Case", "Pos"] {
            if let Some(value) = r.u8() {
                row.number(name, value as f64);
            }
        }
        if let Some(scale) = r.u16() {
            row.number("FontScale", scale as f64 / 10_000.0);
        }
        r.skip(1);
        row_cells(&mut row, &mut r, &["Size"]);
        section.rows.push(row);
        raw.characters.push((id, count));
    }

    pub fn finish(mut self) -> Drawing {
        let mut drawing = std::mem::take(&mut self.drawing);
        for (index, color) in self.palette.iter().enumerate() {
            drawing.colors.insert(index as u32, hex(color[0], color[1], color[2]));
        }
        drawing.fonts = std::mem::take(&mut self.fonts).into_iter().collect();
        let parent = |id: u32| (id != NONE).then_some(id);
        for (mut raw, [line, fill, text], id) in std::mem::take(&mut self.styles) {
            finish_shape(&mut raw, self.version, &mut drawing);
            drawing.styles.push(Style {
                id,
                name: format!("Style {id}"),
                line_style: parent(line),
                fill_style: parent(fill),
                text_style: parent(text),
                sheet: raw.shape.sheet,
            });
        }
        for container in std::mem::take(&mut self.masters) {
            // Shapes refer to a master by the index of its stream.
            let id = container.key;
            let name = self.master_names.get(&container.key).and_then(|n| self.names.get(n)).cloned();
            let (sheet, shapes, _) = tree(container, self.version, &mut drawing);
            drawing.masters.push(Master { id, name: name.unwrap_or_default(), sheet, shapes, connects: Vec::new() });
        }
        let pages = std::mem::take(&mut self.pages);
        let known: Vec<u32> = pages.iter().map(|p| p.key).collect();
        let backgrounds: Vec<u32> = pages.iter().filter_map(|p| p.background_page).collect();
        for container in pages {
            let key = container.key;
            let back_page = container.background_page.filter(|b| known.contains(b));
            let name = self.page_names.get(&key).and_then(|n| self.names.get(n)).cloned();
            let (sheet, mut shapes, links) = tree(container, self.version, &mut drawing);
            // The file also keeps stream entries without a drawing page behind them.
            if sheet.cell("PageWidth").is_none() && shapes.is_empty() {
                continue;
            }
            let connects = glue::connect(&mut shapes, &links);
            drawing.pages.push(Page {
                id: key,
                name: name.unwrap_or_default(),
                background: backgrounds.contains(&key),
                back_page,
                sheet,
                shapes,
                connects,
            });
        }
        let masters: HashMap<u32, HashMap<u32, &Shape>> = drawing
            .masters
            .iter()
            .map(|master| {
                let mut shapes = HashMap::new();
                index_shapes(&master.shapes, &mut shapes);
                (master.id, shapes)
            })
            .collect();
        let mut notes = Vec::new();
        for page in &mut drawing.pages {
            complete_instances(&mut page.shapes, None, &masters, &mut notes);
        }
        for note in notes {
            drawing.note(note);
        }
        drawing
    }
}

/// How many bytes follow a record's data before the next record (zero padding aside).
fn trailer_length(version: u8, record: u32, list: u32, level: u16, extra: u8) -> usize {
    if version < 11 {
        return if list != 0 { 8 } else { 0 };
    }
    if matches!(record, 0x1f | 0x2d | 0xc9 | 0xd1) {
        return 0;
    }
    let mut length = 0;
    if list != 0 || matches!(record, 0x2c | 0x65 | 0x66 | 0x69 | 0x6a | 0x6b | 0x70 | 0x71) {
        length += 8;
    }
    if list != 0 || (level == 2 && extra == 0x55) || (level == 2 && extra == 0x54 && record == 0xaa) || (level == 3 && extra != 0x50 && extra != 0x54) {
        length += 4;
    }
    if length != 12 && length != 4 && matches!(record, 0x64 | 0x65 | 0x66 | 0x69 | 0x6a | 0x6b | 0x6f | 0x71 | 0x92 | 0xa9 | 0xb4 | 0xb6 | 0xb9 | 0xc7) {
        length += 4;
    }
    length
}

/// The fixed fields of a shape record: the group it sits in, its master and master shape, and its styles.
fn shape_header(body: &[u8]) -> Raw {
    let word = |at: usize| Reader::at(body, at).u32();
    let reference = |value: Option<u32>| value.filter(|&v| v != NONE);
    Raw {
        parent: word(10).unwrap_or(0),
        shape: Shape {
            master: reference(word(18)),
            master_shape: reference(word(26)),
            fill_style: reference(word(34)),
            line_style: reference(word(42)),
            text_style: reference(word(50)),
            ..Shape::default()
        },
        ..Raw::default()
    }
}

fn geometry_row(raw: &mut Raw, record: u32, id: u32, body: &[u8]) {
    let Some(geometry) = raw.geometry_order.len().checked_sub(1) else { return };
    let section = raw.shape.sheet.section("Geometry", Some(geometry as u32));
    if section.rows.len() >= MAX_ROWS {
        return;
    }
    let mut r = Reader::at(body, 0);
    let (kind, names): (&str, &[&str]) = match record {
        MOVE_TO => ("MoveTo", &["X", "Y"]),
        LINE_TO => ("LineTo", &["X", "Y"]),
        ARC_TO => ("ArcTo", &["X", "Y", "A"]),
        INFINITE_LINE => ("InfiniteLine", &["X", "Y", "A", "B"]),
        ELLIPSE => ("Ellipse", &["X", "Y", "A", "B", "C", "D"]),
        ELLIPTICAL_ARC => ("EllipticalArcTo", &["X", "Y", "A", "B", "C", "D"]),
        SPLINE_START => ("SplineStart", &["X", "Y"]),
        SPLINE_KNOT => ("SplineKnot", &["X", "Y"]),
        POLYLINE_TO => ("PolylineTo", &["X", "Y"]),
        _ => ("NURBSTo", &["X", "Y"]),
    };
    let mut row = Row { index: Some(id), kind: Some(kind.to_string()), ..Row::default() };
    row_cells(&mut row, &mut r, names);
    let plain: &[&str] = match record {
        SPLINE_START => &["A", "B", "C"],
        SPLINE_KNOT => &["A"],
        NURBS_TO => &["A", "B", "C", "D"],
        _ => &[],
    };
    for name in plain {
        if let Some(value) = r.f64() {
            row.number(name, value);
        }
    }
    if record == SPLINE_START {
        if let Some(degree) = r.u8() {
            row.number("D", degree as f64);
        }
    }
    if matches!(record, POLYLINE_TO | NURBS_TO) {
        // The point list: a marker, then either a reference to a shape data record or the formula itself.
        let formula_cell = if record == NURBS_TO { "E" } else { "A" };
        let fixed = if record == NURBS_TO { 50 } else { 18 };
        let marker = body.get(fixed + 1).copied();
        if matches!(marker, Some(0x8a | 0x8b)) {
            if let Some(data) = Reader::at(body, fixed + 5).u32() {
                raw.pending.push(PendingCurve { geometry, row: section.rows.len(), data });
            }
        } else {
            let (function, number) = if record == NURBS_TO { ("NURBS", 6) } else { ("POLYLINE", 2) };
            if let Some(values) = block_numbers(body, fixed, number) {
                row.set(formula_cell, point_formula(function, &values));
            }
        }
    }
    section.rows.push(row);
}

fn paragraph(raw: &mut Raw, id: u32, body: &[u8]) {
    let mut r = Reader::at(body, 0);
    let Some(count) = r.u32() else { return };
    let section = raw.shape.sheet.section("Paragraph", None);
    let index = section.rows.len();
    if index >= MAX_ROWS {
        return;
    }
    let mut row = Row { index: Some(index as u32), ..Row::default() };
    row_cells(&mut row, &mut r, &["IndFirst", "IndLeft", "IndRight", "SpLine", "SpBefore", "SpAfter"]);
    for name in ["HorzAlign", "Bullet"] {
        if let Some(value) = r.u8() {
            row.number(name, value as f64);
        }
    }
    section.rows.push(row);
    raw.paragraphs.push((id, count));
}

/// Shape data: the value, its type and the label, prompt and format strings.
fn property(raw: &mut Raw, id: u32, body: &[u8], version: u8) {
    let mut r = Reader::at(body, 0);
    let Some(unit) = r.u8() else { return };
    let mut name = r;
    let name = name.u32().unwrap_or(NONE);
    let value = r.f64().unwrap_or(0.0);
    r.skip(16);
    let Some(kind) = r.u8() else { return };
    let mut row = Row { name: Some(format!("Row_{id}")), ..Row::default() };
    if unit == UNIT_STRING {
        if let Some(text) = raw.names.get(&name) {
            row.set("Value", text.clone());
        }
    } else if let Some(text) = block_string(body, 0, version) {
        row.set("Value", text);
    } else if !matches!(kind, 0 | 1 | 4) {
        row.number("Value", value);
    }
    row.number("Type", kind as f64);
    for (cell, number) in [("Prompt", 1), ("Label", 2), ("Format", 3)] {
        if let Some(text) = block_string(body, number, version) {
            row.set(cell, text);
        }
    }
    let section = raw.shape.sheet.section("Property", None);
    if section.rows.len() < MAX_ROWS {
        section.rows.push(row);
    }
}

/// The picture of a foreign shape, recognised by its first bytes. A device-independent bitmap gets the file header it lacks.
pub(in crate::legacy) fn picture(data: &[u8]) -> Option<Picture> {
    let bitmap = |compression: &str, extension: &'static str, data: Vec<u8>| {
        Some(Picture { foreign_type: "Bitmap".to_string(), compression: Some(compression.to_string()), extension, data })
    };
    let metafile = |kind: &str, extension: &'static str| Some(Picture { foreign_type: kind.to_string(), compression: None, extension, data: data.to_vec() });
    if data.starts_with(b"\x89PNG") {
        return bitmap("PNG", "png", data.to_vec());
    }
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        return bitmap("JPEG", "jpg", data.to_vec());
    }
    if data.starts_with(b"GIF8") {
        return bitmap("GIF", "gif", data.to_vec());
    }
    if data.starts_with(b"BM") {
        return bitmap("DIB", "bmp", data.to_vec());
    }
    if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") {
        return bitmap("TIFF", "tif", data.to_vec());
    }
    if data.starts_with(&[1, 0, 0, 0]) && data.get(40..44) == Some(b" EMF") {
        return metafile("EnhMetaFile", "emf");
    }
    if data.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) || data.starts_with(&[1, 0, 9, 0]) || data.starts_with(&[2, 0, 9, 0]) {
        return metafile("MetaFile", "wmf");
    }
    let mut r = Reader::at(data, 0);
    let header = r.u32()?;
    if !matches!(header, 40 | 108 | 124) {
        return None;
    }
    r.skip(10);
    let bits = r.u16()?;
    let compression = r.u32()?;
    r.skip(12);
    let used = r.u32()?;
    let colors = if used > 0 { used.min(256) } else if bits <= 8 { 1 << bits } else { 0 };
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let offset = 14u32 + header + colors * 4 + masks;
    let size = u32::try_from(data.len()).ok()?.checked_add(14)?;
    let mut file = b"BM".to_vec();
    file.extend_from_slice(&size.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&offset.to_le_bytes());
    file.extend_from_slice(data);
    bitmap("DIB", "bmp", file)
}

/// The format number in a field record's formula block `length, 2, 2, integer N, picture function`.
fn field_format(body: &[u8]) -> Option<u16> {
    body.windows(7).find_map(|w| (w[0] == 2 && w[1] == 2 && w[2] == 0x62 && w[5] == 0x80 && w[6] == 0xc2).then(|| u16::from_le_bytes([w[3], w[4]])))
}

/// The text a field showed, worked out from its value and unit.
fn field_text(field: &Field, raw_names: &BTreeMap<u32, String>) -> String {
    if field.unit == UNIT_STRING {
        return raw_names.get(&field.name).cloned().unwrap_or_default();
    }
    // Format numbers: 0 to 9 are general and 0 to 3 decimal places, the odd ones with the unit; 11 radians, 12 degrees.
    let format = field.format.unwrap_or(1);
    let (code, value) = match format {
        11 => (83, field.value),
        12 => (81, field.value),
        _ => (field.unit, field.value),
    };
    // Unit codes as published for the automation interface; values are stored in inches, radians and days.
    let (factor, suffix) = match code {
        33 => (100.0, "%"),
        40 => return date_text(field.value),
        // Elapsed time: weeks, days, hours, minutes, seconds.
        43 => (1.0 / 7.0, " ew."),
        44 => (1.0, " ed."),
        45 => (24.0, " eh."),
        46 => (1_440.0, " em."),
        47 => (86_400.0, " es."),
        50 => (72.0, " pt"),
        51 => (6.0, " p"),
        // A didot is 0.376 mm; a cicero is 12 didots.
        53 => (25.4 / 0.376_065, " d"),
        54 => (25.4 / 0.376_065 / 12.0, " c"),
        65 => (1.0, " in."),
        66 => (1.0 / 12.0, " ft."),
        68 => (1.0 / 63_360.0, " mi."),
        69 => (2.54, " cm"),
        70 => (25.4, " mm"),
        71 => (0.0254, " m"),
        72 => (0.000_025_4, " km"),
        75 => (1.0 / 36.0, " yd."),
        76 => (1.0 / 72_913.385_8, " nm."),
        80 | 81 => (180.0 / std::f64::consts::PI, " deg."),
        83 => (1.0, " rad."),
        _ => (1.0, ""),
    };
    let value = value * factor;
    let text = match format {
        2..=9 => format!("{value:.*}", (format as usize - 2) / 2),
        _ => {
            let text = format!("{value:.4}");
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    let text = if text == "-0" { "0".to_string() } else { text };
    let without_unit = format < 10 && format % 2 == 0;
    format!("{text}{}", if without_unit { "" } else { suffix })
}

/// A date value (days since 1899-12-30) as year-month-day, with the time when there is one.
fn date_text(value: f64) -> String {
    if !(-700_000.0..3_000_000.0).contains(&value) {
        return String::new();
    }
    let days = value.floor() as i64;
    // Civil date from a day count (Gregorian calendar), counted from 0000-03-01.
    let z = days - 25_569 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let minutes = ((value - value.floor()) * 1440.0).round() as i64;
    if minutes == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02} {:02}:{:02}", (minutes / 60).min(23), minutes % 60)
    }
}

/// The text with its formatting marks: runs say how many characters (UTF-16 units, or bytes before version 11) each formatting row covers;
/// a field stands in the text as one placeholder character.
const FIELD_NOTE: &str = "text fields show the value saved in the file, in a simple number or date format, and no longer update";

/// What a binary shape leaves to its master and `.vsdx` has to be told.
///
/// The record list of a geometry section names exactly the rows the shape has, so a row only the master's section has is gone.
/// A shape with field values but no text shows its master's text with its own values in the fields.
fn complete_instances(shapes: &mut [Shape], inherited: Option<u32>, masters: &HashMap<u32, HashMap<u32, &Shape>>, drawing_notes: &mut Vec<&'static str>) {
    for shape in shapes {
        let master = shape.master.or(inherited);
        // The top shape of an instance may leave the master shape unnamed: it is the master's only top shape.
        let source = master.and_then(|id| masters.get(&id)).and_then(|ids| match shape.master_shape {
            Some(id) => ids.get(&id),
            None => None,
        });
        if let Some(source) = source {
            for section in &mut shape.sheet.sections {
                let starts_fresh = matches!(section.rows.first().and_then(|row| row.kind.as_deref()), Some("MoveTo" | "Ellipse" | "SplineStart" | "InfiniteLine"));
                if section.name != "Geometry" || !starts_fresh {
                    continue;
                }
                let Some(inherited) = source.sheet.sections.iter().find(|s| s.name == "Geometry" && s.index == section.index) else { continue };
                let own: Vec<Option<u32>> = section.rows.iter().map(|row| row.index).collect();
                let gone = inherited.rows.iter().filter(|row| !own.contains(&row.index));
                section.rows.extend(gone.map(|row| Row { index: row.index, deleted: true, ..Row::default() }).collect::<Vec<_>>());
            }
            if shape.text.is_none() && !shape.field_values.is_empty() {
                if let Some(text) = &source.text {
                    let mut text = text.clone();
                    for piece in &mut text {
                        if let TextPiece::Field(index, value) = piece {
                            if let Some(own) = shape.field_values.get(*index as usize) {
                                value.clone_from(own);
                            }
                        }
                    }
                    shape.text = Some(text);
                    drawing_notes.push(FIELD_NOTE);
                }
            }
        }
        complete_instances(&mut shape.children, master, masters, drawing_notes);
    }
}

fn index_shapes<'a>(shapes: &'a [Shape], into: &mut HashMap<u32, &'a Shape>) {
    for shape in shapes {
        into.insert(shape.id, shape);
        index_shapes(&shape.children, into);
    }
}

fn text_pieces(raw: &mut Raw, version: u8, drawing: &mut Drawing) -> Option<Vec<TextPiece>> {
    let mut field_ids: Vec<u32> = raw.fields.keys().copied().collect();
    in_order(&mut field_ids, &raw.field_order, |id| *id);
    let Some(bytes) = raw.text.take() else {
        let values = field_ids.iter().filter_map(|id| raw.fields.get(id));
        raw.shape.field_values = values.map(|field| field_text(field, &raw.names)).collect();
        return None;
    };
    let units: Vec<u16> = if version >= 11 {
        bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect()
    } else {
        windows_1252(&bytes).encode_utf16().collect()
    };
    let end = units.len() - units.iter().rev().take_while(|&&u| u == 0).count();
    let units = &units[..end];
    let placeholder: u16 = if version >= 11 { 0xfffc } else { 0x1e };
    // Where each formatting row starts. A count of 0 stands for "all that is left".
    let starts = |runs: &mut Vec<(u32, u32)>, order: &[u32]| -> BTreeMap<usize, u32> {
        let mut indexed: Vec<(u32, u32, u32)> = runs.iter().enumerate().map(|(row, &(id, count))| (id, count, row as u32)).collect();
        in_order(&mut indexed, order, |run| run.0);
        let mut at = 0usize;
        let mut out = BTreeMap::new();
        for (_, count, row) in indexed {
            if at >= units.len() {
                break;
            }
            out.insert(at, row);
            if count == 0 {
                break;
            }
            at = at.saturating_add(count as usize);
        }
        out
    };
    let characters = starts(&mut raw.characters, &raw.character_order);
    let paragraphs = starts(&mut raw.paragraphs, &raw.paragraph_order);
    let mut fields = field_ids.iter();
    let mut field_index = 0u32;
    let mut pieces = Vec::new();
    let mut run: Vec<u16> = Vec::new();
    let flush = |run: &mut Vec<u16>, pieces: &mut Vec<TextPiece>| {
        if !run.is_empty() {
            pieces.push(TextPiece::Text(String::from_utf16_lossy(run)));
            run.clear();
        }
    };
    for (at, &unit) in units.iter().enumerate() {
        let (paragraph, character) = (paragraphs.get(&at), characters.get(&at));
        if paragraph.is_some() || character.is_some() {
            flush(&mut run, &mut pieces);
        }
        if let Some(&row) = paragraph {
            pieces.push(TextPiece::Paragraph(row));
        }
        if let Some(&row) = character {
            pieces.push(TextPiece::Character(row));
        }
        if unit == placeholder {
            flush(&mut run, &mut pieces);
            let value = match fields.next().and_then(|id| raw.fields.get(id)) {
                Some(field) => field_text(field, &raw.names),
                // A shape that inherits its fields from the master has no values of its own.
                None => String::new(),
            };
            drawing.note(FIELD_NOTE);
            pieces.push(TextPiece::Field(field_index, value));
            field_index += 1;
        } else {
            run.push(unit);
        }
    }
    flush(&mut run, &mut pieces);
    Some(pieces)
}

fn finish_shape(raw: &mut Raw, version: u8, drawing: &mut Drawing) {
    // Curves first: their rows are addressed by position.
    for pending in std::mem::take(&mut raw.pending) {
        let formula = raw.shape_data.get(&pending.data).and_then(|data| shape_data_formula(data));
        let section = raw.shape.sheet.sections.iter_mut().find(|s| s.name == "Geometry" && s.index == Some(pending.geometry as u32));
        let row = section.and_then(|s| s.rows.get_mut(pending.row));
        match (formula, row) {
            (Some(formula), Some(row)) => {
                let cell = if row.kind.as_deref() == Some("NURBSTo") { "E" } else { "A" };
                row.set(cell, formula);
            }
            _ => drawing.note("a curve whose points could not be read is drawn as a straight line"),
        }
    }
    for section in &mut raw.shape.sheet.sections {
        if section.name != "Geometry" {
            continue;
        }
        for row in &mut section.rows {
            let curve = match row.kind.as_deref() {
                Some("NURBSTo") => !row.cells.iter().any(|c| c.name == "E"),
                Some("PolylineTo") => !row.cells.iter().any(|c| c.name == "A"),
                _ => false,
            };
            if curve {
                row.kind = Some("LineTo".to_string());
                row.cells.retain(|c| c.name == "X" || c.name == "Y");
                drawing.note("a curve whose points could not be read is drawn as a straight line");
            }
        }
        if let Some(order) = section.index.and_then(|i| raw.geometry_order.get(i as usize)) {
            in_order(&mut section.rows, order, |row| row.index.unwrap_or(0));
        }
    }
    raw.shape.text = text_pieces(raw, version, drawing);
}

/// A container's page sheet and its shapes nested into their groups, bottom shape first.
fn tree(mut container: Container, version: u8, drawing: &mut Drawing) -> (Sheet, Vec<Shape>, HashMap<u32, Links>) {
    finish_shape(&mut container.sheet, version, drawing);
    let mut by_id: BTreeMap<u32, Raw> = BTreeMap::new();
    let mut ids = Vec::new();
    let mut links = HashMap::new();
    for mut raw in std::mem::take(&mut container.shapes) {
        finish_shape(&mut raw, version, drawing);
        let id = raw.shape.id;
        links.insert(id, std::mem::take(&mut raw.links));
        if by_id.insert(id, raw).is_none() {
            ids.push(id);
        }
    }
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut roots = Vec::new();
    for &id in &ids {
        let parent = by_id[&id].parent;
        if parent != id && by_id.contains_key(&parent) {
            children.entry(parent).or_default().push(id);
        } else {
            roots.push(id);
        }
    }
    let stacking = |list: &mut Vec<u32>, owner: &Raw| {
        let order: Vec<u32> = owner.child_order.iter().filter_map(|at| owner.children.get(at)).copied().collect();
        in_order(list, &order, |id| *id);
    };
    stacking(&mut roots, &container.sheet);
    fn build(id: u32, depth: usize, by_id: &mut BTreeMap<u32, Raw>, children: &HashMap<u32, Vec<u32>>, parent_master: Option<u32>, stacking: &dyn Fn(&mut Vec<u32>, &Raw)) -> Option<Shape> {
        let raw = by_id.remove(&id)?;
        let mut list = children.get(&id).cloned().unwrap_or_default();
        stacking(&mut list, &raw);
        let mut shape = raw.shape;
        let master = shape.master.or(parent_master);
        // `.vsdx` names the master on the instance's top shape only; members refer to their master shape within it.
        if shape.master.is_some() && shape.master == parent_master {
            shape.master = None;
        }
        if depth < MAX_DEPTH {
            shape.children = list.into_iter().filter_map(|child| build(child, depth + 1, by_id, children, master, stacking)).collect();
        }
        if !shape.children.is_empty() && shape.kind == "Shape" {
            shape.kind = "Group";
        }
        Some(shape)
    }
    let shapes = roots.into_iter().filter_map(|id| build(id, 0, &mut by_id, &children, None, &stacking)).collect();
    let mut sheet = container.sheet.shape.sheet;
    // A page sheet carries a transform record too; it means nothing there.
    sheet.cells.retain(|cell| !matches!(cell.name.as_str(), "PinX" | "PinY" | "Width" | "Height" | "LocPinX" | "LocPinY" | "Angle" | "FlipX" | "FlipY"));
    (sheet, shapes, links)
}
