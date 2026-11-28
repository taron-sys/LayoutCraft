//! VSDX OPC package reader and page metadata layer.
//!
//! This is the first slice of the Rust VSDX importer: safe package loading,
//! relationships, and the document/page list. Shape/master inheritance and the
//! full editable model will build on these primitives.
use roxmltree::{Document as XmlDocument, Node};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

/// Shapes inside shapes: Visio drawings nest a few levels (a group in a group); the importer and the editor walk the tree
/// recursively, so a hostile file with thousands of levels would overflow the stack (a wasm trap that poisons the engine).
const MAX_SHAPE_DEPTH: usize = 200;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const REL_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub mod colorexpr;
pub mod containers;
pub mod comments;
pub mod docprops;
pub mod emf;
pub mod formula;
pub mod fresh;
pub mod geometry;
pub mod groupedit;
pub mod guides;
pub mod connpoints;
pub mod style_data;
pub mod theme_data;
/// The extra themes: a private, git-ignored module (see `build.rs`); absent in the public build.
#[cfg(private_themes)]
#[path = "../../../private/themes-extra/theme_data.rs"]
pub mod theme_data_private;
pub mod themegen;
pub mod themes;
pub mod replace;
pub mod alttext;
pub mod shapename;
pub mod hyperlinks;
pub mod router;
pub mod import;
pub mod legacy;
mod masterbody;
mod media;
pub mod shapes;
pub mod builtin;
pub mod draw;
pub mod pageops;
pub mod stencil;
pub mod styleedit;
pub mod styles;
pub mod writer;
pub use fresh::{write_blank_document, write_fresh_document, write_fresh_document_with_media, BlankOptions};
pub use import::{import_document, import_document_with, ImportState};
pub use media::media_bytes;
pub use shapes::{
    Cell, ForeignData, Master, MasterCatalog, MasterShape, ResolvedShape, Section, SectionRow,
    ShapeNode, TextToken,
};
pub use styles::{DocumentStyles, StyleSheet, Theme};
pub use writer::{
    part_style_patch, patch_page, patch_page_cells, patch_page_with_connectors,
    patch_page_with_media, GeometryRow, NewConnector, NewPicture, NewShape, PartStyle,
    ShapeCellPatch,
};

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<zip::result::ZipError> for Error {
    fn from(value: zip::result::ZipError) -> Self {
        Error(value.to_string())
    }
}

impl From<roxmltree::Error> for Error {
    fn from(value: roxmltree::Error) -> Self {
        Error(value.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error(value.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(value: std::str::Utf8Error) -> Self {
        Error(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Relationship {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub target_mode: Option<String>,
}

#[derive(Debug, Clone)]
struct PageEntry {
    id: Option<String>,
    name: String,
    rel_id: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    scale: Option<f64>,
    background: bool,
    back_page: Option<String>,
    layers: Vec<layoutcraft_model::Layer>,
    /// Page-level `VariationColorIndex` (None when absent or 65534 = inherit).
    variation: Option<usize>,
    /// PageSheet `ThemeIndex` (the gallery number of the page's own theme; 0 = no theme), when the cell is there.
    theme_index: Option<u32>,
    line_jump: Option<layoutcraft_model::LineJump>,
    /// PageSheet `ShdwOffsetX/Y` in inches: where a legacy (`ShdwPattern`) shadow falls.
    shadow_offset: Option<(f64, f64)>,
}

/// The page's theme number: `ColorSchemeIndex` (the number of the theme part that holds the colours: a real Visio file can say ThemeIndex 34 and ColorSchemeIndex 40 with the
/// colour scheme in the part numbered 40), else `ThemeIndex`. A drawing can hold several theme parts, one per page theme (step 272).
fn page_sheet_theme_index(page: Node<'_, '_>) -> Option<u32> {
    let sheet = page.children().find(|n| n.is_element() && n.tag_name().name() == "PageSheet")?;
    let cell = |name: &str| {
        sheet
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "Cell" && n.attribute("N") == Some(name))
            .and_then(|c| c.attribute("V"))
            .and_then(|v| v.trim().parse::<u32>().ok())
    };
    cell("ColorSchemeIndex").or_else(|| cell("ThemeIndex"))
}

/// The PageSheet's `ShdwOffsetX/Y`, the offset of every legacy shadow on the page.
fn page_sheet_shadow_offset(page: Node<'_, '_>) -> Option<(f64, f64)> {
    let sheet = page
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")?;
    let cell = |name: &str| {
        sheet
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "Cell" && n.attribute("N") == Some(name))
            .and_then(|cell| cell.attribute("V"))
            .and_then(|v| v.trim().parse::<f64>().ok())
    };
    Some((cell("ShdwOffsetX")?, cell("ShdwOffsetY")?))
}

/// The PageSheet's own `VariationColorIndex`, which overrides the document's
/// theme variation for that page. 65534 means "inherit".
fn page_sheet_variation(page: Node<'_, '_>) -> Option<usize> {
    page.children()
        .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")?
        .children()
        .find(|n| {
            n.is_element()
                && n.tag_name().name() == "Cell"
                && n.attribute("N") == Some("VariationColorIndex")
        })?
        .attribute("V")?
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|index| *index != 65534)
}

/// Parse the PageSheet `Layer` section (one row per layer, IX = layer index).
fn page_sheet_line_jump(page: Node<'_, '_>) -> Option<layoutcraft_model::LineJump> {
    let sheet = page
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")?;
    let cell = |name: &str| {
        sheet
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "Cell" && n.attribute("N") == Some(name))
            .and_then(|cell| cell.attribute("V"))
            .and_then(|v| v.parse::<f64>().ok())
    };
    // Visio leaves out cells that hold their defaults, so a page without any of these cells
    // (`None`) means the defaults; one with some of them gets the defaults for the rest.
    let names = ["LineJumpCode", "LineJumpStyle", "LineJumpFactorX", "LineJumpFactorY"];
    if names.iter().all(|name| cell(name).is_none()) {
        return None;
    }
    let default = layoutcraft_model::LineJump::default();
    Some(layoutcraft_model::LineJump {
        code: cell("LineJumpCode").map_or(default.code, |v| v as i32),
        style: cell("LineJumpStyle").map_or(default.style, |v| v as i32),
        factor_x: cell("LineJumpFactorX").unwrap_or(default.factor_x),
        factor_y: cell("LineJumpFactorY").unwrap_or(default.factor_y),
    })
}

/// A layer's `Color` cell as `#rrggbb`: 255 means no colour; 0..=23 index Visio's standard palette (the probe of a layer coloured by
/// index 2 / 3 / 6 draws red / green / magenta), an `#rrggbb` value is taken as it is.
fn layer_color_value(value: &str) -> Option<String> {
    const PALETTE: [&str; 24] = [
        "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#800000", "#008000", "#000080", "#808000",
        "#800080", "#008080", "#c0c0c0", "#e6e6e6", "#cdcdcd", "#b3b3b3", "#9a9a9a", "#808080", "#666666", "#4d4d4d", "#333333", "#1a1a1a",
    ];
    let value = value.trim();
    if value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(value.to_ascii_lowercase());
    }
    value.parse::<usize>().ok().and_then(|index| PALETTE.get(index)).map(|colour| colour.to_string())
}

fn page_sheet_layers(page: Node<'_, '_>) -> Vec<layoutcraft_model::Layer> {
    let Some(section) = page
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
        .and_then(|sheet| {
            sheet.children().find(|n| {
                n.is_element()
                    && n.tag_name().name() == "Section"
                    && n.attribute("N") == Some("Layer")
            })
        })
    else {
        return Vec::new();
    };
    section
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Row")
        .enumerate()
        .map(|(position, row)| {
            let cell = |name: &str| {
                row.children()
                    .find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "Cell"
                            && n.attribute("N") == Some(name)
                    })
                    .and_then(|cell| cell.attribute("V"))
            };
            let flag = |name: &str, fallback: bool| match cell(name) {
                Some(value) => value.trim() != "0",
                None => fallback,
            };
            let index = row
                .attribute("IX")
                .and_then(|value| value.parse().ok())
                .unwrap_or(position as u32);
            layoutcraft_model::Layer {
                index,
                name: cell("Name").unwrap_or_default().to_string(),
                visible: flag("Visible", true),
                print: flag("Print", true),
                locked: flag("Lock", false),
                color: cell("Color").and_then(layer_color_value),
                extra: Default::default(),
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect {
    pub from_sheet: String,
    pub from_cell: String,
    pub to_sheet: String,
    /// `PinX` for glue to a whole shape, `Connections.X<n>` for glue to connection point n.
    pub to_cell: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageRef {
    pub id: Option<String>,
    pub name: String,
    pub part: String,
    /// Page width in inches (Visio's cached internal units).
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Paper/internal drawing-unit ratio (`PageScale / DrawingScale`).
    pub scale: Option<f64>,
    pub back_part: Option<String>,
    pub back_height: Option<f64>,
    /// The layers of the background page: its shapes follow these, not the page's own.
    pub back_layers: Vec<layoutcraft_model::Layer>,
    pub back_scale: Option<f64>,
    /// PageSheet layers (empty when the page defines none).
    pub layers: Vec<layoutcraft_model::Layer>,
    /// Page-level theme colour variation index, when the page overrides it.
    pub variation: Option<usize>,
    /// PageSheet `ThemeIndex` (the page's own theme, matched with a theme part's `schemeEnum`), when the cell is there.
    pub theme_index: Option<u32>,
    /// PageSheet line-jump cells, when the page has `LineJumpCode`.
    pub line_jump: Option<layoutcraft_model::LineJump>,
    /// PageSheet `ShdwOffsetX/Y` in inches (legacy shadow offset), when the page sets them.
    pub shadow_offset: Option<(f64, f64)>,
}

/// True when `<Shape>` elements nest deeper than `limit` in the XML text (a scan of the tags; attribute values may hold `>`).
fn shape_depth_exceeds(xml: &[u8], limit: usize) -> bool {
    let mut depth = 0usize;
    let mut i = 0usize;
    while i + 6 < xml.len() {
        if xml[i] != b'<' {
            i += 1;
            continue;
        }
        let closing = xml[i + 1] == b'/';
        let name_at = i + if closing { 2 } else { 1 };
        let is_shape = xml.get(name_at..name_at + 5) == Some(b"Shape") && matches!(xml.get(name_at + 5), Some(b' ' | b'>' | b'/' | b'\t' | b'\r' | b'\n'));
        // walk to the end of the tag, honouring quotes
        let mut j = name_at;
        let mut quote = 0u8;
        while j < xml.len() {
            let c = xml[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        if is_shape {
            if closing {
                depth = depth.saturating_sub(1);
            } else if j > 0 && xml[j.saturating_sub(1)] != b'/' {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
        }
        i = j + 1;
    }
    false
}

#[derive(Debug)]
pub struct Package {
    entries: BTreeMap<String, Vec<u8>>,
    /// The ZIP bytes the package was read from: the writers copy the parts an edit leaves alone from here without inflating and
    /// deflating them again (a drawing with 12 MB of EMF pictures cost 220 ms per edit that way).
    source: Option<std::sync::Arc<Vec<u8>>>,
}

impl Package {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        Self::from_reader(file)
    }

    pub fn from_reader(mut reader: impl Read + std::io::Seek) -> Result<Self> {
        let mut source = Vec::new();
        reader.read_to_end(&mut source)?;
        let mut package = Self::from_zip_bytes(&source)?;
        package.source = Some(std::sync::Arc::new(source));
        Ok(package)
    }

    /// Read a package from ZIP bytes held in memory (one copy fewer than `from_reader(Cursor::new(bytes.clone()))`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut package = Self::from_zip_bytes(bytes)?;
        package.source = Some(std::sync::Arc::new(bytes.to_vec()));
        Ok(package)
    }

    fn from_zip_bytes(bytes: &[u8]) -> Result<Self> {
        let mut archive = ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut entries = BTreeMap::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().map_err(|e| Error(e.to_string()))?.to_string();
            normalize_part_name(&name)?;
            let size = entry.size();
            if size > MAX_ENTRY_BYTES {
                return Err(Error(format!("ZIP entry too large: {name} ({size} bytes)")));
            }
            let mut bytes = Vec::with_capacity(size.min(8 * 1024 * 1024) as usize);
            entry.read_to_end(&mut bytes)?;
            if name.ends_with(".xml") && (name.starts_with("visio/pages/") || name.starts_with("visio/masters/")) && shape_depth_exceeds(&bytes, MAX_SHAPE_DEPTH) {
                return Err(Error(format!("{name}: shapes are nested more than {MAX_SHAPE_DEPTH} levels deep")));
            }
            total = total.saturating_add(bytes.len() as u64);
            if total > MAX_TOTAL_BYTES {
                return Err(Error(
                    "VSDX package exceeds the uncompressed size limit".into(),
                ));
            }
            entries.insert(name, bytes);
        }
        Ok(Self { entries, source: None })
    }

    /// The ZIP bytes this package was read from (the parts of a written package that did not change are copied from them).
    pub(crate) fn source_bytes(&self) -> Option<&[u8]> {
        self.source.as_ref().map(|s| s.as_slice())
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn contains(&self, part: &str) -> bool {
        self.entries.contains_key(part)
    }

    pub fn entry(&self, part: &str) -> Option<&[u8]> {
        self.entries.get(part).map(Vec::as_slice)
    }

    pub fn text(&self, part: &str) -> Result<&str> {
        let bytes = self
            .entries
            .get(part)
            .ok_or_else(|| Error(format!("missing part: {part}")))?;
        Ok(std::str::from_utf8(bytes)?)
    }

    pub fn xml(&self, part: &str) -> Result<XmlDocument<'_>> {
        let text = self.text(part)?;
        if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
            return Err(Error(format!("XML declarations are not allowed in {part}")));
        }
        Ok(XmlDocument::parse(text)?)
    }

    pub fn relationships(&self, part: &str) -> Result<BTreeMap<String, Relationship>> {
        let rels_part = relationships_part(part)?;
        if !self.contains(&rels_part) {
            return Ok(BTreeMap::new());
        }
        let doc = self.xml(&rels_part)?;
        let mut out = BTreeMap::new();
        for node in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        {
            let id = node.attribute("Id").unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let kind = node.attribute("Type").unwrap_or("").to_string();
            let target = node.attribute("Target").unwrap_or("").to_string();
            let resolved = resolve_target(part, &target)?;
            out.insert(
                id.clone(),
                Relationship {
                    id,
                    kind,
                    target: resolved,
                    target_mode: node.attribute("TargetMode").map(str::to_string),
                },
            );
        }
        Ok(out)
    }

    /// Parse the `Pages/Page` list and resolve each page's XML part.
    pub fn connects(&self, part: &str) -> Result<Vec<Connect>> {
        let doc = self.xml(part)?;
        let Some(container) = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "Connects")
        else {
            return Ok(Vec::new());
        };
        Ok(container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Connect")
            .filter_map(|node| {
                Some(Connect {
                    from_sheet: node.attribute("FromSheet")?.to_string(),
                    from_cell: node.attribute("FromCell")?.to_string(),
                    to_sheet: node.attribute("ToSheet")?.to_string(),
                    to_cell: node.attribute("ToCell").unwrap_or_default().to_string(),
                })
            })
            .collect())
    }

    pub fn pages(&self) -> Result<Vec<PageRef>> {
        let (page_list_part, entries) = self.page_list()?;
        let rels = self.relationships(&page_list_part)?;
        let mut sorted_parts: Vec<&str> = self.names().filter(|n| is_page_part(n)).collect();
        // Natural order: page2.xml before page10.xml.
        sorted_parts.sort_by_key(|name| {
            name.trim_start_matches("visio/pages/page")
                .trim_end_matches(".xml")
                .parse::<u64>()
                .unwrap_or(u64::MAX)
        });

        let mut resolved_parts = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let part = entry
                .rel_id
                .as_deref()
                .and_then(|rid| rels.get(rid))
                .map(|r| r.target.clone())
                .filter(|part| self.contains(part))
                .or_else(|| sorted_parts.get(index).map(|s| s.to_string()));
            let Some(part) = part else {
                return Err(Error(format!("no XML part for page '{}'", entry.name)));
            };
            resolved_parts.push(part);
        }
        let mut backgrounds: BTreeMap<String, (String, Option<f64>, Option<f64>, Vec<layoutcraft_model::Layer>)> = BTreeMap::new();
        for (entry, part) in entries.iter().zip(&resolved_parts) {
            if entry.background {
                if let Some(id) = &entry.id {
                    backgrounds.insert(id.clone(), (part.clone(), entry.height, entry.scale, entry.layers.clone()));
                }
            }
        }
        let mut pages = Vec::new();
        // A file made only of background pages (some templates) would otherwise
        // list no pages at all; show them as ordinary pages instead.
        let has_foreground = entries.iter().any(|entry| !entry.background);
        for (entry, part) in entries.iter().zip(resolved_parts) {
            if entry.background && has_foreground {
                continue;
            }
            let back = entry.back_page.as_ref().and_then(|id| backgrounds.get(id));
            pages.push(PageRef {
                id: entry.id.clone(),
                name: entry.name.clone(),
                part,
                width: entry.width,
                height: entry.height,
                scale: entry.scale,
                back_part: back.map(|value| value.0.clone()),
                back_height: back.and_then(|value| value.1),
                back_layers: back.map(|value| value.3.clone()).unwrap_or_default(),
                back_scale: back.and_then(|value| value.2),
                layers: entry.layers.clone(),
                variation: entry.variation,
                theme_index: entry.theme_index,
                line_jump: entry.line_jump.clone(),
                shadow_offset: entry.shadow_offset,
            });
        }
        if pages.is_empty() {
            // Some producers omit an explicit page list; fall back to page parts.
            for (index, part) in sorted_parts.iter().enumerate() {
                pages.push(PageRef {
                    id: None,
                    name: format!("Page-{}", index + 1),
                    part: (*part).to_string(),
                    width: None,
                    height: None,
                    scale: None,
                    back_part: None,
                    back_height: None,
                    back_layers: Vec::new(),
                    back_scale: None,
                    layers: Vec::new(),
                    variation: None,
                    theme_index: None,
                    line_jump: None,
                    shadow_offset: None,
                });
            }
        }
        if pages.is_empty() {
            return Err(Error("no pages found in VSDX package".into()));
        }
        Ok(pages)
    }

    fn page_list(&self) -> Result<(String, Vec<PageEntry>)> {
        let document_part = "visio/document.xml";
        let doc = self.xml(document_part)?;
        if let Some(pages) = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "Pages")
        {
            let entries = page_entries(pages);
            if !entries.is_empty() {
                return Ok((document_part.to_string(), entries));
            }
        }
        // Document rels usually point at visio/pages/pages.xml.
        let rels = self.relationships(document_part)?;
        let target = rels
            .values()
            .find(|r| r.kind.ends_with("/pages") || r.target.ends_with("pages.xml"))
            .map(|r| r.target.clone());
        if let Some(part) = target {
            let doc = self.xml(&part)?;
            if let Some(pages) = doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "Pages")
            {
                let entries = page_entries(pages);
                if !entries.is_empty() {
                    return Ok((part, entries));
                }
            }
        }
        Err(Error("VSDX has no page list".into()))
    }
}

pub(crate) fn is_page_part(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("visio/pages/page") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".xml") else {
        return false;
    };
    !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit())
}

fn page_entries(pages: Node<'_, '_>) -> Vec<PageEntry> {
    pages
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Page")
        .map(|node| PageEntry {
            id: node.attribute("ID").map(str::to_string),
            name: node
                .attribute("NameU")
                .or_else(|| node.attribute("Name"))
                .map(str::to_string)
                .unwrap_or_default(),
            // Visio writes `<Page ...><PageSheet/><Rel r:id="rId1"/></Page>`; some
            // producers put `r:id` on the Page element itself.
            rel_id: node
                .attribute((REL_NS, "id"))
                .or_else(|| {
                    node.attributes()
                        .find(|a| a.name() == "id" && a.namespace().is_some())
                        .map(|a| a.value())
                })
                .or_else(|| {
                    node.children()
                        .find(|n| n.is_element() && n.tag_name().name() == "Rel")
                        .and_then(|rel| {
                            rel.attribute((REL_NS, "id")).or_else(|| {
                                rel.attributes()
                                    .find(|a| a.name() == "id" && a.namespace().is_some())
                                    .map(|a| a.value())
                            })
                        })
                })
                .map(str::to_string),
            width: node
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
                .and_then(|sheet| {
                    sheet.children().find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "Cell"
                            && n.attribute("N") == Some("PageWidth")
                    })
                })
                .and_then(|cell| cell.attribute("V"))
                .and_then(|v| v.parse().ok()),
            height: node
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
                .and_then(|sheet| {
                    sheet.children().find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "Cell"
                            && n.attribute("N") == Some("PageHeight")
                    })
                })
                .and_then(|cell| cell.attribute("V"))
                .and_then(|v| v.parse().ok()),
            scale: node
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
                .and_then(|sheet| {
                    let cell = |name: &str| {
                        sheet
                            .children()
                            .find(|n| {
                                n.is_element()
                                    && n.tag_name().name() == "Cell"
                                    && n.attribute("N") == Some(name)
                            })
                            .and_then(|cell| cell.attribute("V"))
                            .and_then(|v| v.parse::<f64>().ok())
                    };
                    let page_scale = cell("PageScale").unwrap_or(1.0);
                    let drawing_scale = cell("DrawingScale").unwrap_or(1.0);
                    (page_scale > 0.0 && drawing_scale > 0.0).then_some(page_scale / drawing_scale)
                }),
            background: node.attribute("Background") == Some("1"),
            back_page: node.attribute("BackPage").map(str::to_string),
            layers: page_sheet_layers(node),
            variation: page_sheet_variation(node),
            theme_index: page_sheet_theme_index(node),
            line_jump: page_sheet_line_jump(node),
            shadow_offset: page_sheet_shadow_offset(node),
        })
        .collect()
}

fn parent_part(part: &str) -> String {
    part.rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default()
}

fn relationships_part(part: &str) -> Result<String> {
    let (dir, file) = part
        .rsplit_once('/')
        .ok_or_else(|| Error(format!("not a package part: {part}")))?;
    if dir.is_empty() || file.is_empty() {
        return Err(Error(format!("not a package part: {part}")));
    }
    Ok(format!("{dir}/_rels/{file}.rels"))
}

fn normalize_part_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') {
        return Err(Error(format!("unsafe ZIP entry name: {name}")));
    }
    let trailing_dir = name.ends_with('/');
    let components: Vec<&str> = name.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        if component.is_empty() {
            if trailing_dir && index + 1 == components.len() {
                continue;
            }
            return Err(Error(format!("unsafe ZIP entry name: {name}")));
        }
        if *component == "." || *component == ".." {
            return Err(Error(format!("unsafe ZIP entry name: {name}")));
        }
    }
    Ok(())
}

fn resolve_target(part: &str, target: &str) -> Result<String> {
    let base = parent_part(part);
    let target = target.trim_start_matches('/');
    if target.contains('\\') {
        return Err(Error(format!("unsafe relationship target: {target}")));
    }
    let combined = if base.is_empty() {
        target.to_string()
    } else {
        format!("{base}/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for component in combined.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error(format!("relationship escapes package: {target}")));
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Err(Error("empty relationship target".into()));
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_names() {
        assert!(normalize_part_name("visio/pages/page1.xml").is_ok());
        assert!(normalize_part_name("../evil.xml").is_err());
        assert!(normalize_part_name("visio/../evil.xml").is_err());
        assert!(normalize_part_name("visio\\evil.xml").is_err());
    }

    #[test]
    fn resolves_relationship_targets() {
        assert_eq!(
            resolve_target("visio/document.xml", "pages/pages.xml").unwrap(),
            "visio/pages/pages.xml"
        );
        assert_eq!(
            resolve_target("visio/pages/pages.xml", "page1.xml").unwrap(),
            "visio/pages/page1.xml"
        );
        assert!(resolve_target("visio/pages/pages.xml", "../../../evil.xml").is_err());
    }
}
