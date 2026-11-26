//! Write a [`Drawing`] as a `.vsdx` package ([MS-VSDX] part names, relationships and content types).
use super::drawing::{Cell, Connect, Drawing, Page, Shape, Sheet, TextPiece};
use super::Converted;
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{Cursor, Write as _};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const NAMESPACES: &str = "xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'";
const DECLARATION: &str = "<?xml version='1.0' encoding='utf-8' standalone='yes'?>";
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const VISIO_REL: &str = "http://schemas.microsoft.com/visio/2010/relationships";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
/// The package reader refuses more than 512 MiB of parts; stop a hostile file well before that.
const MAX_PACKAGE_BYTES: usize = 480 * 1024 * 1024;

/// Text for an attribute or element: the five XML characters are escaped, characters XML 1.0 does not allow are dropped.
fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' => out.push(c),
            '\r' => out.push_str("&#13;"),
            c if c < ' ' || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
}

fn attribute(out: &mut String, name: &str, value: &str) {
    let _ = write!(out, " {name}='");
    escape(value, out);
    out.push('\'');
}

fn cells(out: &mut String, cells: &[Cell]) {
    for cell in cells {
        out.push_str("<Cell");
        attribute(out, "N", &cell.name);
        if let Some(value) = &cell.value {
            attribute(out, "V", value);
        }
        if let Some(unit) = &cell.unit {
            attribute(out, "U", unit);
        }
        if let Some(formula) = &cell.formula {
            attribute(out, "F", formula);
        }
        out.push_str("/>");
    }
}

fn sheet(out: &mut String, sheet: &Sheet) {
    cells(out, &sheet.cells);
    for section in &sheet.sections {
        out.push_str("<Section");
        attribute(out, "N", &section.name);
        if let Some(index) = section.index {
            attribute(out, "IX", &index.to_string());
        }
        out.push('>');
        cells(out, &section.cells);
        for row in &section.rows {
            out.push_str("<Row");
            if let Some(name) = &row.name {
                attribute(out, "N", name);
            }
            if let Some(kind) = &row.kind {
                attribute(out, "T", kind);
            }
            if let Some(index) = row.index {
                attribute(out, "IX", &index.to_string());
            }
            if row.deleted {
                attribute(out, "Del", "1");
            }
            out.push('>');
            cells(out, &row.cells);
            out.push_str("</Row>");
        }
        out.push_str("</Section>");
    }
}

fn styles_of(out: &mut String, line: Option<u32>, fill: Option<u32>, text: Option<u32>) {
    for (name, value) in [("LineStyle", line), ("FillStyle", fill), ("TextStyle", text)] {
        if let Some(value) = value {
            attribute(out, name, &value.to_string());
        }
    }
}

/// The parts of the package while it is built.
#[derive(Default)]
struct Parts {
    entries: BTreeMap<String, Vec<u8>>,
    bytes: usize,
    /// Media already stored, by content, so a picture repeated on many shapes is kept once.
    media: BTreeMap<[u8; 32], String>,
    extensions: BTreeSet<&'static str>,
}

impl Parts {
    fn add(&mut self, name: &str, data: impl Into<Vec<u8>>) -> Result<()> {
        let data = data.into();
        self.bytes = self.bytes.saturating_add(data.len());
        if self.bytes > MAX_PACKAGE_BYTES {
            return Err(Error("the drawing is too large to convert".to_string()));
        }
        self.entries.insert(name.to_string(), data);
        Ok(())
    }

    fn media(&mut self, data: &[u8], extension: &'static str) -> Result<String> {
        use sha2::{Digest, Sha256};
        let key: [u8; 32] = Sha256::digest(data).into();
        if let Some(name) = self.media.get(&key) {
            return Ok(name.clone());
        }
        let name = format!("image{}.{extension}", self.media.len() + 1);
        self.add(&format!("visio/media/{name}"), data.to_vec())?;
        self.media.insert(key, name.clone());
        self.extensions.insert(extension);
        Ok(name)
    }
}

fn shape(out: &mut String, node: &Shape, parts: &mut Parts, rels: &mut Vec<(String, String)>) -> Result<()> {
    out.push_str("<Shape");
    attribute(out, "ID", &node.id.to_string());
    if let Some(name) = &node.name {
        attribute(out, "NameU", name);
        attribute(out, "Name", name);
    }
    attribute(out, "Type", if node.kind.is_empty() { "Shape" } else { node.kind });
    if let Some(master) = node.master {
        attribute(out, "Master", &master.to_string());
    }
    if let Some(master_shape) = node.master_shape {
        attribute(out, "MasterShape", &master_shape.to_string());
    }
    styles_of(out, node.line_style, node.fill_style, node.text_style);
    out.push('>');
    sheet(out, &node.sheet);
    if let Some(picture) = &node.picture {
        let media = parts.media(&picture.data, picture.extension)?;
        let id = match rels.iter().find(|(_, target)| *target == media) {
            Some((id, _)) => id.clone(),
            None => {
                let id = format!("rId{}", rels.len() + 1);
                rels.push((id.clone(), media));
                id
            }
        };
        out.push_str("<ForeignData");
        attribute(out, "ForeignType", &picture.foreign_type);
        if let Some(compression) = &picture.compression {
            attribute(out, "CompressionType", compression);
        }
        out.push_str("><Rel r:id='");
        out.push_str(&id);
        out.push_str("'/></ForeignData>");
    }
    if let Some(text) = &node.text {
        out.push_str("<Text>");
        for piece in text {
            match piece {
                TextPiece::Text(value) => escape(value, out),
                TextPiece::Character(ix) => {
                    let _ = write!(out, "<cp IX='{ix}'/>");
                }
                TextPiece::Paragraph(ix) => {
                    let _ = write!(out, "<pp IX='{ix}'/>");
                }
                TextPiece::Tabs(ix) => {
                    let _ = write!(out, "<tp IX='{ix}'/>");
                }
                TextPiece::Field(ix, value) => {
                    let _ = write!(out, "<fld IX='{ix}'>");
                    escape(value, out);
                    out.push_str("</fld>");
                }
            }
        }
        out.push_str("</Text>");
    }
    if !node.children.is_empty() {
        out.push_str("<Shapes>");
        for child in &node.children {
            shape(out, child, parts, rels)?;
        }
        out.push_str("</Shapes>");
    }
    out.push_str("</Shape>");
    Ok(())
}

/// A `PageContents` or `MasterContents` part and, when it shows pictures, its relationships.
fn contents(parts: &mut Parts, part: &str, root: &str, shapes: &[Shape], connects: &[Connect]) -> Result<()> {
    let mut rels = Vec::new();
    let mut out = format!("{DECLARATION}<{root} {NAMESPACES}><Shapes>");
    for node in shapes {
        shape(&mut out, node, parts, &mut rels)?;
    }
    out.push_str("</Shapes>");
    if !connects.is_empty() {
        out.push_str("<Connects>");
        for connect in connects {
            out.push_str("<Connect");
            attribute(&mut out, "FromSheet", &connect.from_sheet);
            attribute(&mut out, "FromCell", &connect.from_cell);
            attribute(&mut out, "ToSheet", &connect.to_sheet);
            attribute(&mut out, "ToCell", &connect.to_cell);
            out.push_str("/>");
        }
        out.push_str("</Connects>");
    }
    let _ = write!(out, "</{root}>");
    parts.add(part, out)?;
    if !rels.is_empty() {
        let (directory, file) = part.rsplit_once('/').unwrap_or(("visio", part));
        let list: String = rels.iter().map(|(id, media)| format!("<Relationship Id='{id}' Type='{IMAGE_REL}' Target='../media/{media}'/>")).collect();
        parts.add(&format!("{directory}/_rels/{file}.rels"), format!("{DECLARATION}<Relationships xmlns='{RELATIONSHIPS}'>{list}</Relationships>"))?;
    }
    Ok(())
}

fn relationships(list: &[(String, String, String)]) -> String {
    let body: String = list.iter().map(|(id, kind, target)| format!("<Relationship Id='{id}' Type='{kind}' Target='{target}'/>")).collect();
    format!("{DECLARATION}<Relationships xmlns='{RELATIONSHIPS}'>{body}</Relationships>")
}

fn document_xml(drawing: &Drawing) -> String {
    let mut out = format!("{DECLARATION}<VisioDocument {NAMESPACES}><DocumentSettings");
    for (name, value) in ["DefaultTextStyle", "DefaultLineStyle", "DefaultFillStyle"].iter().zip(drawing.default_styles) {
        attribute(&mut out, name, &value.unwrap_or(0).to_string());
    }
    out.push_str("/>");
    if !drawing.colors.is_empty() {
        out.push_str("<Colors>");
        for (index, color) in &drawing.colors {
            let _ = write!(out, "<ColorEntry IX='{index}' RGB='");
            escape(color, &mut out);
            out.push_str("'/>");
        }
        out.push_str("</Colors>");
    }
    out.push_str("<FaceNames>");
    for (_, name) in &drawing.fonts {
        out.push_str("<FaceName");
        attribute(&mut out, "NameU", name);
        out.push_str("/>");
    }
    out.push_str("</FaceNames><StyleSheets>");
    for style in &drawing.styles {
        out.push_str("<StyleSheet");
        attribute(&mut out, "ID", &style.id.to_string());
        attribute(&mut out, "NameU", &style.name);
        attribute(&mut out, "Name", &style.name);
        styles_of(&mut out, style.line_style, style.fill_style, style.text_style);
        out.push('>');
        sheet(&mut out, &style.sheet);
        out.push_str("</StyleSheet>");
    }
    out.push_str("</StyleSheets></VisioDocument>");
    out
}

/// Old files can point at a master or master shape that is no longer there; `.vsdx` readers refuse such a reference, so it is dropped
/// and the shape keeps what it holds itself.
fn drop_dangling(shapes: &mut [Shape], inherited: Option<u32>, masters: &BTreeMap<u32, BTreeSet<u32>>, dropped: &mut usize) {
    for node in shapes {
        if node.master.is_some_and(|id| !masters.contains_key(&id)) {
            node.master = None;
            *dropped += 1;
        }
        let master = node.master.or(inherited);
        if let Some(reference) = node.master_shape {
            if !master.and_then(|id| masters.get(&id)).is_some_and(|ids| ids.contains(&reference)) {
                node.master_shape = None;
                *dropped += 1;
            }
        }
        drop_dangling(&mut node.children, master, masters, dropped);
    }
}

fn shape_ids(shapes: &[Shape], into: &mut BTreeSet<u32>) {
    for node in shapes {
        into.insert(node.id);
        shape_ids(&node.children, into);
    }
}

/// A name no other page or master has (they must differ).
fn unique(name: &str, fallback: &str, id: u32, used: &mut BTreeSet<String>) -> String {
    let base: String = name.chars().filter(|c| !c.is_control()).collect();
    let base = if base.trim().is_empty() { format!("{fallback}-{}", id + 1) } else { base };
    let mut candidate = base.clone();
    let mut n = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{base} ({n})");
        n += 1;
    }
    candidate
}

fn empty_page() -> Page {
    let mut page = Page { name: "Page-1".to_string(), ..Page::default() };
    page.sheet.number("PageWidth", 8.5);
    page.sheet.number("PageHeight", 11.0);
    page
}

pub(super) fn package(mut drawing: Drawing) -> Result<Converted> {
    if drawing.pages.is_empty() {
        if drawing.masters.is_empty() {
            return Err(Error("no pages or shapes could be read from the drawing".to_string()));
        }
        drawing.note("the file is a stencil without drawing pages; an empty page was added");
        drawing.pages.push(empty_page());
    }
    let masters: BTreeMap<u32, BTreeSet<u32>> = drawing
        .masters
        .iter()
        .map(|master| {
            let mut ids = BTreeSet::new();
            shape_ids(&master.shapes, &mut ids);
            (master.id, ids)
        })
        .collect();
    let mut dropped = 0;
    for page in &mut drawing.pages {
        drop_dangling(&mut page.shapes, None, &masters, &mut dropped);
    }
    if dropped > 0 {
        *drawing.notes.entry("references to masters that the file does not hold were dropped; those shapes may look incomplete".to_string()).or_default() += dropped;
    }
    // Backgrounds after the pages that use them, as `.vsdx` files list them.
    drawing.pages.sort_by_key(|page| page.background);
    let mut parts = Parts::default();
    let mut overrides = vec![
        ("/visio/document.xml".to_string(), "application/vnd.ms-visio.drawing.main+xml"),
        ("/visio/masters/masters.xml".to_string(), "application/vnd.ms-visio.masters+xml"),
        ("/visio/pages/pages.xml".to_string(), "application/vnd.ms-visio.pages+xml"),
        ("/visio/windows.xml".to_string(), "application/vnd.ms-visio.windows+xml"),
    ];

    let mut masters_xml = format!("{DECLARATION}<Masters {NAMESPACES}>");
    let mut masters_rels = Vec::new();
    let mut used = BTreeSet::new();
    for (index, master) in drawing.masters.iter().enumerate() {
        let n = index + 1;
        let name = unique(&master.name, "Master", master.id, &mut used);
        masters_xml.push_str("<Master");
        attribute(&mut masters_xml, "ID", &master.id.to_string());
        attribute(&mut masters_xml, "NameU", &name);
        attribute(&mut masters_xml, "Name", &name);
        masters_xml.push_str("><PageSheet>");
        sheet(&mut masters_xml, &master.sheet);
        let _ = write!(masters_xml, "</PageSheet><Rel r:id='rId{n}'/></Master>");
        masters_rels.push((format!("rId{n}"), format!("{VISIO_REL}/master"), format!("master{n}.xml")));
        overrides.push((format!("/visio/masters/master{n}.xml"), "application/vnd.ms-visio.master+xml"));
        contents(&mut parts, &format!("visio/masters/master{n}.xml"), "MasterContents", &master.shapes, &master.connects)?;
    }
    masters_xml.push_str("</Masters>");
    parts.add("visio/masters/masters.xml", masters_xml)?;
    parts.add("visio/masters/_rels/masters.xml.rels", relationships(&masters_rels))?;

    let mut pages_xml = format!("{DECLARATION}<Pages {NAMESPACES}>");
    let mut pages_rels = Vec::new();
    let mut used = BTreeSet::new();
    for (index, page) in drawing.pages.iter().enumerate() {
        let n = index + 1;
        let name = unique(&page.name, "Page", page.id, &mut used);
        pages_xml.push_str("<Page");
        attribute(&mut pages_xml, "ID", &page.id.to_string());
        attribute(&mut pages_xml, "NameU", &name);
        attribute(&mut pages_xml, "Name", &name);
        if page.background {
            attribute(&mut pages_xml, "Background", "1");
        }
        if let Some(back) = page.back_page {
            attribute(&mut pages_xml, "BackPage", &back.to_string());
        }
        pages_xml.push_str("><PageSheet LineStyle='0' FillStyle='0' TextStyle='0'>");
        sheet(&mut pages_xml, &page.sheet);
        let _ = write!(pages_xml, "</PageSheet><Rel r:id='rId{n}'/></Page>");
        pages_rels.push((format!("rId{n}"), format!("{VISIO_REL}/page"), format!("page{n}.xml")));
        overrides.push((format!("/visio/pages/page{n}.xml"), "application/vnd.ms-visio.page+xml"));
        contents(&mut parts, &format!("visio/pages/page{n}.xml"), "PageContents", &page.shapes, &page.connects)?;
    }
    pages_xml.push_str("</Pages>");
    parts.add("visio/pages/pages.xml", pages_xml)?;
    parts.add("visio/pages/_rels/pages.xml.rels", relationships(&pages_rels))?;

    parts.add("visio/document.xml", document_xml(&drawing))?;
    parts.add(
        "visio/windows.xml",
        format!("{DECLARATION}<Windows ClientWidth='1920' ClientHeight='900' {NAMESPACES}/>"),
    )?;
    parts.add(
        "visio/_rels/document.xml.rels",
        relationships(&[
            ("rId1".into(), format!("{VISIO_REL}/masters"), "masters/masters.xml".into()),
            ("rId2".into(), format!("{VISIO_REL}/pages"), "pages/pages.xml".into()),
            ("rId3".into(), format!("{VISIO_REL}/windows"), "windows.xml".into()),
        ]),
    )?;
    parts.add(
        "_rels/.rels",
        relationships(&[("rId1".into(), format!("{VISIO_REL}/document"), "visio/document.xml".into())]),
    )?;
    let mut types = format!("{DECLARATION}<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/xml'/>");
    for extension in &parts.extensions {
        let media_type = match *extension {
            "png" => "image/png",
            "jpg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tif" => "image/tiff",
            "emf" => "image/x-emf",
            "wmf" => "image/x-wmf",
            _ => "application/octet-stream",
        };
        let _ = write!(types, "<Default Extension='{extension}' ContentType='{media_type}'/>");
    }
    for (part, content_type) in &overrides {
        let _ = write!(types, "<Override PartName='{part}' ContentType='{content_type}'/>");
    }
    types.push_str("</Types>");
    parts.add("[Content_Types].xml", types)?;

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).compression_level(Some(4));
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in &parts.entries {
        zip.start_file(name.as_str(), options).map_err(|e| Error(e.to_string()))?;
        zip.write_all(data)?;
    }
    let vsdx = zip.finish().map_err(|e| Error(e.to_string()))?.into_inner();
    let notes = drawing.notes.iter().map(|(what, count)| if *count > 1 { format!("{what} ({count}×)") } else { what.clone() }).collect();
    Ok(Converted { vsdx, notes })
}
