//! Visio 2003 XML drawings (`.vdx`, `.vsx`, `.vtx`): one XML document in which cells are elements grouped by section, where `.vsdx`
//! has `Cell`, `Section` and `Row` elements.
//!
//! Evidence: the published Visio 2003 XML schema reference, and drawings saved both ways compared cell by cell
//! (`docs/47-legacy-formats.md`).
use super::drawing::{Cell, Connect, Drawing, Master, Page, Picture, Row, Section, Shape, Sheet, Style, TextPiece, MAX_DEPTH};
use crate::{Error, Result};
use roxmltree::{Document, Node, ParsingOptions};

const NAMESPACE: &str = "schemas.microsoft.com/visio/2003/core";
const MAX_BYTES: usize = 192 * 1024 * 1024;
const MAX_NODES: u32 = 6_000_000;

/// Elements of a shape or sheet whose children are single cells.
const CELL_GROUPS: &[&str] = &[
    "XForm", "XForm1D", "Line", "Fill", "TextBlock", "TextXForm", "Misc", "Event", "Protection", "Help", "Layout", "Group", "Image", "LayerMem", "StyleProp",
    "Foreign", "Align", "PageProps", "PageLayout", "PrintProps", "RulerGrid", "DocProps",
];
/// Elements that are one row of a section each, and the section's name in `.vsdx`.
const ROW_SECTIONS: &[(&str, &str)] = &[
    ("Char", "Character"),
    ("Para", "Paragraph"),
    ("Prop", "Property"),
    ("User", "User"),
    ("Hyperlink", "Hyperlink"),
    ("Control", "Control"),
    ("Connection", "Connection"),
    ("Act", "Actions"),
    ("Field", "Field"),
    ("Layer", "Layer"),
    ("Scratch", "Scratch"),
    ("Annotation", "Annotation"),
    ("SmartTagDef", "ActionTag"),
    ("Reviewer", "Reviewer"),
];
const COLOR_CELLS: &[&str] = &["Color", "LineColor", "FillForegnd", "FillBkgnd", "ShdwForegnd", "ShdwBkgnd", "TextBkgnd", "GlowColor"];
const FONT_CELLS: &[&str] = &["Font", "AsianFont", "ComplexScriptFont"];

/// UTF-16 with a byte order mark, or UTF-8.
fn decode(bytes: &[u8]) -> Option<String> {
    let utf16 = |data: &[u8], little: bool| -> String {
        let units: Vec<u16> = data.chunks_exact(2).map(|b| if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => Some(utf16(rest, true)),
        [0xfe, 0xff, rest @ ..] => Some(utf16(rest, false)),
        [0xef, 0xbb, 0xbf, rest @ ..] => std::str::from_utf8(rest).ok().map(str::to_string),
        _ => std::str::from_utf8(bytes).ok().map(str::to_string),
    }
}

pub(super) fn looks_like_vdx(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096) & !1];
    let Some(text) = decode(head).or_else(|| Some(String::from_utf8_lossy(head).into_owned())) else { return false };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with('<') && ((text.contains("<VisioDocument") && text.contains(NAMESPACE)) || unqualified_root(text))
}

/// Some programs write these drawings without any namespace: then the first element must be a `VisioDocument` with content and
/// no default namespace of its own (`.vsdx` parts name theirs).
fn unqualified_root(mut text: &str) -> bool {
    // the XML declaration, other processing instructions and comments come before the root
    while let Some(end) = if text.starts_with("<?") { Some("?>") } else { text.starts_with("<!--").then_some("-->") } {
        let Some((_, rest)) = text.split_once(end) else { return false };
        text = rest.trim_start();
    }
    let Some((attributes, _)) = text.strip_prefix("<VisioDocument").and_then(|rest| rest.split_once('>')) else { return false };
    (attributes.is_empty() || attributes.starts_with(char::is_whitespace)) && !attributes.ends_with('/') && !attributes.contains("xmlns=")
}

/// Visio writes some control characters as character references that XML 1.0 forbids; they are removed so the file parses.
fn without_forbidden_references(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("&#") {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.bytes().take(12).position(|b| b == b';');
        let forbidden = end.is_some_and(|end| {
            let body = &rest[2..end];
            let code = match body.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => body.parse().ok(),
            };
            code.is_some_and(|c| (c < 32 && !matches!(c, 9 | 10 | 13)) || (0xd800..0xe000).contains(&c) || c == 0xfffe || c == 0xffff)
        });
        match end {
            Some(end) if forbidden => rest = &rest[end + 1..],
            _ => {
                out.push_str("&#");
                rest = &rest[2..];
            }
        }
    }
    out.push_str(rest);
    if out.chars().any(|c| c < ' ' && !matches!(c, '\t' | '\n' | '\r')) {
        out.retain(|c| c >= ' ' || matches!(c, '\t' | '\n' | '\r'));
    }
    out
}

fn elements<'a, 'i>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(|n| n.is_element())
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    elements(node).find(|n| n.tag_name().name() == name)
}

fn number<T: std::str::FromStr>(node: Node, name: &str) -> Option<T> {
    node.attribute(name)?.trim().parse().ok()
}

struct Reader {
    drawing: Drawing,
}

impl Reader {
    fn cell(&self, node: Node) -> Cell {
        let name = node.tag_name().name().to_string();
        let mut value: Option<String> = node.text().map(str::to_string).or_else(|| (!node.has_attribute("F")).then(String::new));
        let formula = node.attribute("F").filter(|f| *f != "No Formula").map(str::to_string);
        // `.vsdx` writes colours as `#rrggbb` and fonts by name; this format may use their table numbers.
        if let Some(index) = value.as_deref().and_then(|v| v.trim().parse::<u32>().ok()) {
            if COLOR_CELLS.contains(&name.as_str()) && !(name == "TextBkgnd") {
                if let Some(color) = self.drawing.colors.get(&index) {
                    value = Some(color.clone());
                }
            } else if FONT_CELLS.contains(&name.as_str()) {
                if let Some((_, font)) = self.drawing.fonts.iter().find(|(id, _)| *id == index) {
                    value = Some(font.clone());
                }
            }
        }
        Cell { name, value, formula, unit: node.attribute("Unit").map(str::to_string) }
    }

    fn row(&self, node: Node) -> Row {
        Row {
            index: number(node, "IX").or_else(|| number(node, "ID")),
            name: node.attribute("NameU").or_else(|| node.attribute("Name")).map(str::to_string),
            kind: None,
            deleted: node.attribute("Del") == Some("1"),
            cells: elements(node).map(|c| self.cell(c)).collect(),
        }
    }

    /// The element `node` of a shape or sheet, if it is a cell group or a section.
    fn part(&self, node: Node, sheet: &mut Sheet) {
        let tag = node.tag_name().name();
        if CELL_GROUPS.contains(&tag) {
            sheet.cells.extend(elements(node).map(|c| self.cell(c)));
        } else if tag == "Geom" {
            let mut section = Section { name: "Geometry".to_string(), index: number(node, "IX"), ..Section::default() };
            for item in elements(node) {
                if item.has_attribute("IX") {
                    let mut row = self.row(item);
                    row.kind = Some(item.tag_name().name().to_string());
                    row.name = None;
                    section.rows.push(row);
                } else {
                    section.cells.push(self.cell(item));
                }
            }
            if node.attribute("Del") == Some("1") {
                section.cells.push(Cell { name: "NoShow".to_string(), value: Some("1".to_string()), ..Cell::default() });
            }
            sheet.sections.push(section);
        } else if tag == "Tabs" {
            self.tabs(node, sheet);
        } else if let Some((_, name)) = ROW_SECTIONS.iter().find(|(element, _)| *element == tag) {
            let mut row = self.row(node);
            // Rows with a name are told apart by it; the others by their index.
            if !matches!(tag, "Prop" | "User" | "Hyperlink" | "Control" | "Act" | "SmartTagDef") {
                row.name = None;
            } else if row.name.is_none() {
                row.name = Some(format!("Row_{}", row.index.unwrap_or(0)));
            }
            if row.name.is_some() {
                row.index = None;
            }
            sheet.section(name, None).rows.push(row);
        }
    }

    /// `.vsdx` keeps the tab stops of a row as numbered cells.
    fn tabs(&self, node: Node, sheet: &mut Sheet) {
        let mut row = Row { index: number(node, "IX"), ..Row::default() };
        for tab in elements(node) {
            let n: u32 = number(tab, "IX").unwrap_or(0);
            for item in elements(tab) {
                let mut cell = self.cell(item);
                cell.name = format!("{}{n}", cell.name);
                row.cells.push(cell);
            }
        }
        sheet.section("Tabs", None).rows.push(row);
    }

    fn sheet(&self, node: Node) -> Sheet {
        let mut sheet = Sheet::default();
        for item in elements(node) {
            self.part(item, &mut sheet);
        }
        sheet
    }

    fn shapes(&mut self, container: Node, depth: usize) -> Vec<Shape> {
        let Some(list) = child(container, "Shapes") else { return Vec::new() };
        elements(list).filter(|n| n.tag_name().name() == "Shape").filter_map(|n| self.shape(n, depth)).collect()
    }

    fn shape(&mut self, node: Node, depth: usize) -> Option<Shape> {
        if depth > MAX_DEPTH {
            self.drawing.note("groups nested beyond the limit were left out");
            return None;
        }
        let mut shape = Shape {
            id: number(node, "ID")?,
            kind: match node.attribute("Type") {
                Some("Group") => "Group",
                Some("Foreign") => "Foreign",
                Some("Guide") => "Guide",
                _ => "Shape",
            },
            name: node.attribute("NameU").or_else(|| node.attribute("Name")).map(str::to_string),
            master: number(node, "Master"),
            master_shape: number(node, "MasterShape"),
            line_style: number(node, "LineStyle"),
            fill_style: number(node, "FillStyle"),
            text_style: number(node, "TextStyle"),
            sheet: self.sheet(node),
            ..Shape::default()
        };
        if let Some(text) = child(node, "Text") {
            let mut pieces = Vec::new();
            for item in text.children() {
                if item.is_text() {
                    pieces.push(TextPiece::Text(item.text().unwrap_or_default().to_string()));
                    continue;
                }
                let index = number(item, "IX").unwrap_or(0);
                match item.tag_name().name() {
                    "cp" => pieces.push(TextPiece::Character(index)),
                    "pp" => pieces.push(TextPiece::Paragraph(index)),
                    "tp" => pieces.push(TextPiece::Tabs(index)),
                    "fld" => pieces.push(TextPiece::Field(index, item.text().unwrap_or_default().to_string())),
                    _ => {}
                }
            }
            shape.text = Some(pieces);
        }
        if let Some(foreign) = child(node, "ForeignData") {
            shape.picture = self.picture(foreign);
        }
        shape.children = self.shapes(node, depth + 1);
        Some(shape)
    }

    fn picture(&mut self, node: Node) -> Option<Picture> {
        let data = base64(node.text().unwrap_or_default());
        let foreign_type = node.attribute("ForeignType").unwrap_or("Bitmap").to_string();
        let compression = node.attribute("CompressionType").map(str::to_string);
        let extension = match (foreign_type.as_str(), compression.as_deref()) {
            ("EnhMetaFile", _) => "emf",
            ("MetaFile", _) => "wmf",
            ("Object", _) => {
                self.drawing.note("embedded OLE objects are not carried over");
                return None;
            }
            (_, Some("PNG")) => "png",
            (_, Some("JPEG")) => "jpg",
            (_, Some("GIF")) => "gif",
            (_, Some("TIFF")) => "tif",
            _ => "bmp",
        };
        match data {
            Some(mut data) if !data.is_empty() => {
                // A bitmap may be stored without its 14-byte file header.
                if extension == "bmp" && !data.starts_with(b"BM") {
                    data = super::vsd_bitmap(&data)?;
                }
                Some(Picture { foreign_type, compression, extension, data })
            }
            _ => {
                self.drawing.note("a picture that could not be decoded was left out");
                None
            }
        }
    }

    fn connects(&self, node: Node) -> Vec<Connect> {
        let Some(list) = child(node, "Connects") else { return Vec::new() };
        elements(list)
            .filter_map(|n| {
                Some(Connect {
                    from_sheet: n.attribute("FromSheet")?.to_string(),
                    from_cell: n.attribute("FromCell")?.to_string(),
                    to_sheet: n.attribute("ToSheet")?.to_string(),
                    to_cell: n.attribute("ToCell").unwrap_or_default().to_string(),
                })
            })
            .collect()
    }

    fn document(&mut self, root: Node) {
        if let Some(colors) = child(root, "Colors") {
            for entry in elements(colors) {
                if let (Some(index), Some(color)) = (number(entry, "IX"), entry.attribute("RGB")) {
                    self.drawing.colors.insert(index, color.to_ascii_lowercase());
                }
            }
        }
        if let Some(faces) = child(root, "FaceNames") {
            for face in elements(faces) {
                if let (Some(index), Some(name)) = (number(face, "ID"), face.attribute("NameU").or_else(|| face.attribute("Name"))) {
                    self.drawing.fonts.push((index, name.to_string()));
                }
            }
        }
        if let Some(settings) = child(root, "DocumentSettings") {
            self.drawing.default_styles = ["DefaultTextStyle", "DefaultLineStyle", "DefaultFillStyle"].map(|name| number(settings, name));
        }
        if let Some(styles) = child(root, "StyleSheets") {
            for node in elements(styles) {
                let Some(id) = number(node, "ID") else { continue };
                self.drawing.styles.push(Style {
                    id,
                    name: node.attribute("NameU").or_else(|| node.attribute("Name")).unwrap_or_default().to_string(),
                    line_style: number(node, "LineStyle"),
                    fill_style: number(node, "FillStyle"),
                    text_style: number(node, "TextStyle"),
                    sheet: self.sheet(node),
                });
            }
        }
        if let Some(masters) = child(root, "Masters") {
            for node in elements(masters).filter(|n| n.tag_name().name() == "Master") {
                let Some(id) = number(node, "ID") else { continue };
                let master = Master {
                    id,
                    name: node.attribute("NameU").or_else(|| node.attribute("Name")).unwrap_or_default().to_string(),
                    sheet: child(node, "PageSheet").map(|n| self.sheet(n)).unwrap_or_default(),
                    shapes: self.shapes(node, 0),
                    connects: self.connects(node),
                };
                self.drawing.masters.push(master);
            }
        }
        if let Some(pages) = child(root, "Pages") {
            for node in elements(pages).filter(|n| n.tag_name().name() == "Page") {
                let Some(id) = number(node, "ID") else { continue };
                let page = Page {
                    id,
                    name: node.attribute("NameU").or_else(|| node.attribute("Name")).unwrap_or_default().to_string(),
                    background: node.attribute("Background") == Some("1"),
                    back_page: number(node, "BackPage"),
                    sheet: child(node, "PageSheet").map(|n| self.sheet(n)).unwrap_or_default(),
                    shapes: self.shapes(node, 0),
                    connects: self.connects(node),
                };
                self.drawing.pages.push(page);
            }
        }
        if child(root, "VBProjectData").is_some() {
            self.drawing.note("macros are not carried over");
        }
        if child(root, "DataConnections").is_some() || child(root, "DataRecordSets").is_some() {
            self.drawing.note("links to external data are not carried over");
        }
    }
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut have) = (0u32, 0);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        };
        bits = (bits << 6) | value as u32;
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
        }
    }
    Some(out)
}

/// Shape elements nest recursively in the parser too; refuse what would exhaust the stack before building the tree.
fn too_deep(text: &str) -> bool {
    let mut depth = 0usize;
    for tag in text.split('<').skip(1) {
        if tag.starts_with(['!', '?']) {
            continue;
        }
        if tag.starts_with('/') {
            depth = depth.saturating_sub(1);
        } else if !tag.split_once('>').is_some_and(|(inside, _)| inside.ends_with('/')) {
            depth += 1;
            if depth > MAX_DEPTH * 2 + 40 {
                return true;
            }
        }
    }
    false
}

pub(super) fn read(bytes: &[u8]) -> Result<Drawing> {
    if bytes.len() > MAX_BYTES {
        return Err(Error("the XML drawing is too large".to_string()));
    }
    let text = decode(bytes).ok_or_else(|| Error("the XML drawing is not valid UTF-8 or UTF-16".to_string()))?;
    let text = without_forbidden_references(&text);
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(Error("XML declarations are not allowed in a drawing".to_string()));
    }
    if too_deep(&text) {
        return Err(Error("the XML drawing is nested too deeply".to_string()));
    }
    let options = ParsingOptions { nodes_limit: MAX_NODES, ..ParsingOptions::default() };
    let document = Document::parse_with_options(&text, options)?;
    let root = document.root_element();
    if root.tag_name().name() != "VisioDocument" {
        return Err(Error("not a Visio XML drawing".to_string()));
    }
    let mut reader = Reader { drawing: Drawing::default() };
    reader.document(root);
    Ok(reader.drawing)
}
