//! Patch writer: update shape cells, text, Shape Data and simple section cells
//! while preserving every other package entry.
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use serde_json::Value;

use crate::{Error, Package, Result};

#[derive(Debug, Clone, Default)]
pub struct ShapeCellPatch {
    pub visio_id: String,
    pub cells: BTreeMap<String, String>,
    pub text: Option<String>,
    /// New plain text for the shape; the `<pp>`/`<cp>` run markers around the changed part stay.
    pub text_edit: Option<String>,
    /// Character size (inches) for every `Character` row of the shape; the section is created
    /// when the shape has none of its own.
    pub char_size: Option<f64>,
    /// `(cell, value)` written to every `Character` row of the shape (`Font` name, `Color` `#rrggbb`).
    pub char_cells: Vec<(String, String)>,
    /// `(bits to set, bits to clear)` for the `Style` cell of every `Character` row: 1 bold, 2 italic, 4 underline.
    pub char_style: Option<(u32, u32)>,
    /// `(geometry section IX, cell, value)`: a geometry-level cell such as `NoLine`; the section is
    /// created on the page shape when only its master has it (an override, as Visio writes it).
    pub geometry_flags: Vec<(String, String, String)>,
    pub rich: Option<Value>,
    pub data: BTreeMap<String, String>,
    pub section_cells: Vec<SectionCellPatch>,
    pub geometry_rows: Option<(String, Vec<GeometryRow>)>,
    /// Cells whose cached value `V` is replaced while their formula `F` stays (glued connector
    /// ends: the formula keeps the glue live, the value is what a reader without a formula
    /// engine draws).
    pub cached_cells: BTreeMap<String, String>,
    /// `(cell, value, formula)`: a cell written with both its value and a new formula (a connector
    /// end glued to another shape).
    pub formula_cells: Vec<(String, String, String)>,
    /// A format change for a range of the shape's text (new runs and paragraphs are split off).
    pub text_range: Option<TextRangeFormat>,
    /// Rows the instance of a master overrides on its own (Visio does not recalculate the master's formulas for an
    /// instance when it loads a file: a resized instance must carry the recalculated cells itself).
    pub row_overrides: Vec<RowOverride>,
    /// Child shapes (XML `<Shape>` elements) appended to the shape's own `<Shapes>` (created when absent): the children of
    /// a group instance that the master supplies are written out by a resize, because Visio does not scale them on load.
    pub append_children: Option<String>,
    /// Shape Data rows written or removed as a whole (see `DataEdit`).
    pub data_edits: Vec<DataEdit>,
}

/// A Shape Data (`Property` section) row written as a whole, or removed (`remove`). The cells follow what Visio writes:
/// `Value` has the unit `STR` for text and lists (types 0, 1, 4), `BOOL` for a flag (3), `DATE` for a date (5), none for a
/// number (2) or a currency amount (7), and for a duration (6) the number of days with the unit of what was typed (`ED`, `EH`,
/// `EM`, `ES`, `EW`: Visio saved `1.5 ed` as `V='1.5' U='ED'` and `90 em` as `V='0.0625' U='EM'`).
#[derive(Debug, Clone, Default)]
pub struct DataEdit {
    pub name: String,
    pub remove: bool,
    pub label: String,
    pub value: String,
    pub prompt: String,
    pub format: String,
    pub kind: u32,
    pub invisible: bool,
    /// `SortKey`; empty keeps the key of the row that is replaced (and writes none for a new row).
    pub sort_key: String,
    /// With `remove`: the master supplies this row, so the instance gets a `Del='1'` row instead of losing its own.
    pub inherited: bool,
}

/// One row of a section of an instance with cells written as `(name, value, formula)`; the section and the row are
/// created when the shape has no such row of its own.
#[derive(Debug, Clone)]
pub struct RowOverride {
    pub section: String,
    pub section_index: String,
    pub row_index: String,
    pub row_type: Option<String>,
    pub cells: Vec<(String, String, String)>,
}

#[derive(Debug, Clone)]
pub struct GeometryRow {
    pub kind: String,
    pub index: String,
    pub cells: BTreeMap<String, String>,
    /// Cells written with a formula: `(name, formula)`; the value is the formula text itself
    /// (what Visio stores for `POLYLINE(...)` cells).
    pub formulas: BTreeMap<String, String>,
    /// Cells written with a value AND a formula: `(name, (value, formula))` (`Width*0.5` cells of a drawn shape).
    pub value_formulas: BTreeMap<String, (String, String)>,
}

impl GeometryRow {
    pub fn new(kind: impl Into<String>, index: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            index: index.into(),
            cells: BTreeMap::new(),
            formulas: BTreeMap::new(),
            value_formulas: BTreeMap::new(),
        }
    }

    pub fn with_formula(mut self, name: impl Into<String>, value: impl Into<String>, formula: impl Into<String>) -> Self {
        self.value_formulas.insert(name.into(), (value.into(), formula.into()));
        self
    }

    pub fn formula(mut self, name: impl Into<String>, formula: impl Into<String>) -> Self {
        self.formulas.insert(name.into(), formula.into());
        self
    }

    pub fn set(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cells.insert(name.into(), value.into());
        self
    }
}

impl ShapeCellPatch {
    pub fn new(visio_id: impl Into<String>) -> Self {
        Self {
            visio_id: visio_id.into(),
            cells: BTreeMap::new(),
            text: None,
            text_edit: None,
            char_size: None,
            char_cells: Vec::new(),
            char_style: None,
            geometry_flags: Vec::new(),
            rich: None,
            data: BTreeMap::new(),
            section_cells: Vec::new(),
            geometry_rows: None,
            cached_cells: BTreeMap::new(),
            formula_cells: Vec::new(),
            text_range: None,
            row_overrides: Vec::new(),
            append_children: None,
            data_edits: Vec::new(),
        }
    }

    /// Write or remove a Shape Data row (see `DataEdit`).
    pub fn data_edit(mut self, edit: DataEdit) -> Self {
        self.data_edits.push(edit);
        self
    }

    /// Append child shape elements to the shape (see `append_children`).
    pub fn append_children(mut self, xml: impl Into<String>) -> Self {
        self.append_children = Some(xml.into());
        self
    }

    /// Override a row of a section of the instance (see `RowOverride`).
    pub fn row_override(mut self, row: RowOverride) -> Self {
        self.row_overrides.push(row);
        self
    }

    /// Format a range of the shape's text (see `TextRangeFormat`).
    pub fn text_range(mut self, range: TextRangeFormat) -> Self {
        self.text_range = Some(range);
        self
    }

    /// Write a cell's value and give it the formula `formula` (the cell is created when absent).
    pub fn set_with_formula(mut self, name: impl Into<String>, value: impl Into<String>, formula: impl Into<String>) -> Self {
        self.formula_cells.push((name.into(), value.into(), formula.into()));
        self
    }

    /// Replace a cell's cached value and keep its formula.
    pub fn set_cached(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cached_cells.insert(name.into(), value.into());
        self
    }

    pub fn set(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cells.insert(name.into(), value.into());
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Edit the text like a person typing: only the characters that differ are replaced, so the
    /// formatting runs (`<cp>`) and paragraphs (`<pp>`) of the untouched text keep their format and
    /// inserted characters take the format of the run they are typed into.
    pub fn edit_text(mut self, text: impl Into<String>) -> Self {
        self.text_edit = Some(text.into());
        self
    }

    /// Set the size (inches; 12 pt = 12/72) of all text of the shape.
    pub fn char_size(mut self, inches: f64) -> Self {
        self.char_size = Some(inches);
        self
    }

    /// Set a cell of every `Character` row (`Font`, `Color`, ...), creating the section when absent.
    pub fn char_cell(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.char_cells.push((name.into(), value.into()));
        self
    }

    /// Set (`set`) and clear (`clear`) bits of the `Style` cell of every `Character` row.
    pub fn char_style(mut self, set: u32, clear: u32) -> Self {
        let (s, c) = self.char_style.unwrap_or((0, 0));
        self.char_style = Some((s | set, c | clear));
        self
    }

    /// Set a cell of geometry section `index` (`NoLine`, `NoFill`, ...), creating the override
    /// section when the page shape has none.
    pub fn geometry_flag(mut self, index: impl Into<String>, cell: impl Into<String>, value: impl Into<String>) -> Self {
        self.geometry_flags.push((index.into(), cell.into(), value.into()));
        self
    }

    pub fn rich(mut self, value: Value) -> Self {
        self.rich = Some(value);
        self
    }

    pub fn geometry_rows(
        mut self,
        section_index: impl Into<String>,
        rows: Vec<GeometryRow>,
    ) -> Self {
        self.geometry_rows = Some((section_index.into(), rows));
        self
    }

    /// Set a Shape Data row by its Visio row name (for example `Row_1`).
    pub fn data(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.insert(name.into(), value.into());
        self
    }

    /// Set a Section-level cell (NoFill/NoLine/NoShow and similar).
    pub fn section_cell(
        mut self,
        section: impl Into<String>,
        section_index: impl Into<String>,
        cell: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.section_cells.push(SectionCellPatch {
            section: section.into(),
            section_index: section_index.into(),
            row: None,
            cell: cell.into(),
            value: value.into(),
        });
        self
    }

    /// Set a cell inside a Section row. `row` matches `IX` for Geometry and
    /// Character rows, or `N` for named rows.
    pub fn row_cell(
        mut self,
        section: impl Into<String>,
        section_index: impl Into<String>,
        row: impl Into<String>,
        cell: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.section_cells.push(SectionCellPatch {
            section: section.into(),
            section_index: section_index.into(),
            row: Some(row.into()),
            cell: cell.into(),
            value: value.into(),
        });
        self
    }
}

#[derive(Debug, Clone)]
pub struct SectionCellPatch {
    pub section: String,
    pub section_index: String,
    pub row: Option<String>,
    pub cell: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct NewPicture {
    pub visio_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub bytes: Vec<u8>,
    pub extension: String,
    pub media_type: String,
    pub name: String,
    pub text: String,
}

impl NewPicture {
    pub fn new(
        visio_id: impl Into<String>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            visio_id: visio_id.into(),
            x,
            y,
            w,
            h,
            bytes,
            extension: "png".to_string(),
            media_type: "image/png".to_string(),
            name: String::new(),
            text: String::new(),
        }
    }

    pub fn extension(mut self, value: impl Into<String>) -> Self {
        self.extension = value.into();
        self
    }

    pub fn media_type(mut self, value: impl Into<String>) -> Self {
        self.media_type = value.into();
        self
    }

    pub fn name(mut self, value: impl Into<String>) -> Self {
        self.name = value.into();
        self
    }

    pub fn text(mut self, value: impl Into<String>) -> Self {
        self.text = value.into();
        self
    }
}

pub struct NewConnector {
    pub visio_id: String,
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub glue_from: Option<String>,
    pub glue_to: Option<String>,
    pub stroke: String,
    pub stroke_width: f64,
    pub label: String,
    pub arrow_end: bool,
}

impl NewConnector {
    pub fn new(visio_id: impl Into<String>, from: (f64, f64), to: (f64, f64)) -> Self {
        Self {
            visio_id: visio_id.into(),
            from,
            to,
            glue_from: None,
            glue_to: None,
            stroke: "#3f6ab6".to_string(),
            stroke_width: 1.0,
            label: String::new(),
            arrow_end: true,
        }
    }

    pub fn glue(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.glue_from = Some(from.into());
        self.glue_to = Some(to.into());
        self
    }

    pub fn stroke(mut self, value: impl Into<String>) -> Self {
        self.stroke = value.into();
        self
    }

    pub fn stroke_width(mut self, value: f64) -> Self {
        self.stroke_width = value;
        self
    }

    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = value.into();
        self
    }

    pub fn arrow_end(mut self, value: bool) -> Self {
        self.arrow_end = value;
        self
    }
}

pub struct NewShape {
    pub visio_id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub fill: String,
    pub stroke: String,
    pub stroke_width: f64,
    pub text: String,
    pub font_size: f64,
    pub text_color: String,
}

impl NewShape {
    pub fn new(visio_id: impl Into<String>, x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            visio_id: visio_id.into(),
            x,
            y,
            w,
            h,
            fill: "#dbe9f6".to_string(),
            stroke: "#3b6ea5".to_string(),
            stroke_width: 1.0,
            text: String::new(),
            font_size: 12.0,
            text_color: "#1b2733".to_string(),
        }
    }

    pub fn fill(mut self, value: impl Into<String>) -> Self {
        self.fill = value.into();
        self
    }

    pub fn stroke(mut self, value: impl Into<String>) -> Self {
        self.stroke = value.into();
        self
    }

    pub fn stroke_width(mut self, value: f64) -> Self {
        self.stroke_width = value;
        self
    }

    pub fn text(mut self, value: impl Into<String>) -> Self {
        self.text = value.into();
        self
    }

    pub fn font_size(mut self, value: f64) -> Self {
        self.font_size = value;
        self
    }

    pub fn text_color(mut self, value: impl Into<String>) -> Self {
        self.text_color = value.into();
        self
    }
}

/// A style edit for one imported part.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartStyle {
    pub fill: Option<String>,
    pub stroke: Option<String>,
    /// Line width in model pixels (96 per inch).
    pub stroke_width: Option<f64>,
}

/// Turn a style edit on an imported part into the cell patch that writes it
/// back. A part composited from a master child maps to the page-level child
/// shape Visio stores for that master shape; the patch sets that child's own
/// cells (`FillForegnd`, `LineColor`, `LineWeight`), which drops the inherited
/// formula and makes the value a local override, as Visio does when a
/// sub-shape is recoloured. Verified against Visio 16.0.14332.
///
/// A `fill` part accepts a fill edit, a `stroke` part accepts stroke edits.
/// `tint` parts are derived from the fill and cannot be edited directly.
pub fn part_style_patch(part: &layoutcraft_model::ShapePart, style: &PartStyle) -> Result<ShapeCellPatch> {
    let source = part
        .source
        .as_ref()
        .ok_or_else(|| Error("part has no source; it is not composited from a master child".into()))?;
    let page_id = source.page_shape_id.as_deref().ok_or_else(|| {
        Error(format!(
            "master shape {} has no page-level child to edit",
            source.master_shape_id
        ))
    })?;
    let mut patch = ShapeCellPatch::new(page_id);
    match source.role.as_str() {
        "fill" => {
            if style.stroke.is_some() || style.stroke_width.is_some() {
                return Err(Error("a fill part cannot take stroke edits".into()));
            }
            let fill = style
                .fill
                .as_deref()
                .ok_or_else(|| Error("no fill given for a fill part".into()))?;
            patch = patch.set("FillForegnd", fill);
        }
        "stroke" => {
            if style.fill.is_some() {
                return Err(Error("a stroke part cannot take a fill edit".into()));
            }
            if style.stroke.is_none() && style.stroke_width.is_none() {
                return Err(Error("no stroke edit given for a stroke part".into()));
            }
            if let Some(stroke) = style.stroke.as_deref() {
                patch = patch.set("LineColor", stroke);
            }
            if let Some(width) = style.stroke_width {
                if !width.is_finite() || width < 0.0 {
                    return Err(Error("stroke width must be a non-negative number".into()));
                }
                patch = patch.set("LineWeight", format!("{}", width / 96.0));
            }
        }
        other => {
            return Err(Error(format!(
                "a {other} part is derived from the fill and cannot be edited directly"
            )))
        }
    }
    Ok(patch)
}

/// Patch cells on page shapes and return a new VSDX package. All untouched
/// entries are copied byte-for-byte; only the page XML is rewritten.
pub fn patch_page_cells(
    package: &Package,
    page_index: usize,
    patches: &[ShapeCellPatch],
) -> Result<Vec<u8>> {
    patch_page(package, page_index, patches, &[], &[])
}

/// Patch existing shapes, remove shapes and add inline shapes in one package
/// rewrite.
pub fn patch_page(
    package: &Package,
    page_index: usize,
    patches: &[ShapeCellPatch],
    add: &[NewShape],
    remove: &[String],
) -> Result<Vec<u8>> {
    patch_page_with_connectors(package, page_index, patches, add, remove, &[])
}

pub fn patch_page_with_connectors(
    package: &Package,
    page_index: usize,
    patches: &[ShapeCellPatch],
    add: &[NewShape],
    remove: &[String],
    connectors: &[NewConnector],
) -> Result<Vec<u8>> {
    patch_page_with_media(package, page_index, patches, add, remove, connectors, &[])
}

#[allow(clippy::too_many_arguments)]
pub fn patch_page_with_media(
    package: &Package,
    page_index: usize,
    patches: &[ShapeCellPatch],
    add: &[NewShape],
    remove: &[String],
    connectors: &[NewConnector],
    pictures: &[NewPicture],
) -> Result<Vec<u8>> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let page_height = page.height.unwrap_or(11.0);
    let mut xml = package.text(&page.part)?.to_string();
    if !add.is_empty() || !connectors.is_empty() || !pictures.is_empty() {
        // a page Visio saved without shapes has `<Shapes/>` (or only a self-closing root): open it so the new shapes can go in
        if !xml.contains("</Shapes>") {
            if let Some(at) = xml.find("<Shapes/>") {
                xml.replace_range(at..at + "<Shapes/>".len(), "<Shapes></Shapes>");
            }
        }
    }
    for id in remove {
        xml = remove_shape(&xml, id)?;
    }
    for shape in add {
        if find_shape_range(&xml, &shape.visio_id).is_some() {
            return Err(Error(format!("shape {} already exists", shape.visio_id)));
        }
        let insert = xml
            .rfind("</Shapes>")
            .ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
        xml.insert_str(insert, &new_shape_xml(shape, page_height));
    }
    for connector in connectors {
        if find_shape_range(&xml, &connector.visio_id).is_some() {
            return Err(Error(format!(
                "shape {} already exists",
                connector.visio_id
            )));
        }
        let insert = xml
            .rfind("</Shapes>")
            .ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
        xml.insert_str(insert, &new_connector_xml(connector, page_height));
        xml = insert_connects(&xml, connector);
    }

    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for picture in pictures {
        if find_shape_range(&xml, &picture.visio_id).is_some() {
            return Err(Error(format!("shape {} already exists", picture.visio_id)));
        }
        let (shape_xml, media_part, rel_id) =
            add_picture(package, &page.part, page_height, picture, &mut extras)?;
        let insert = xml
            .rfind("</Shapes>")
            .ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
        xml.insert_str(insert, &shape_xml);
        let _ = (media_part, rel_id);
    }
    for patch in patches {
        xml = patch_shape_in_text(&xml, patch)?;
    }
    extras.insert(page.part.clone(), xml.into_bytes());
    write_package(package, extras)
}

fn add_picture(
    package: &Package,
    page_part: &str,
    page_height: f64,
    picture: &NewPicture,
    extras: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(String, String, String)> {
    let extension = picture
        .extension
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let mut sequence = 1usize;
    let media_part = loop {
        let candidate = format!("visio/media/layoutcraft{sequence}.{extension}");
        if !package.contains(&candidate) && !extras.contains_key(&candidate) {
            break candidate;
        }
        sequence += 1;
    };
    let rels_part = page_rels_part(page_part)?;
    let mut rels = extras
        .get(&rels_part)
        .map(|bytes| String::from_utf8_lossy(bytes).to_string())
        .or_else(|| package.text(&rels_part).ok().map(str::to_string))
        .unwrap_or_else(|| {
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_string()
        });
    let rel_id = loop {
        let candidate = format!("rIdRust{sequence}");
        if !rels.contains(&format!("Id='{candidate}'"))
            && !rels.contains(&format!("Id=\"{candidate}\""))
        {
            break candidate;
        }
        sequence += 1;
    };
    let rel = format!(
        "<Relationship Id='{}' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/image' Target='../media/{}'/>",
        escape_attr(&rel_id),
        escape_attr(media_part.rsplit('/').next().unwrap_or("picture.png"))
    );
    if let Some(end) = rels.rfind("</Relationships>") {
        rels.insert_str(end, &rel);
    } else {
        return Err(Error(format!(
            "invalid page relationships part {rels_part}"
        )));
    }
    extras.insert(rels_part, rels.into_bytes());

    let types_part = "[Content_Types].xml";
    let mut types = extras
        .get(types_part)
        .map(|bytes| String::from_utf8_lossy(bytes).to_string())
        .or_else(|| package.text(types_part).ok().map(str::to_string))
        .ok_or_else(|| Error("package has no [Content_Types].xml".to_string()))?;
    let has_default = types.contains(&format!("Extension='{extension}'"))
        || types.contains(&format!("Extension=\"{extension}\""));
    if !has_default {
        if let Some(end) = types.rfind("</Types>") {
            types.insert_str(
                end,
                &format!(
                    "<Default Extension='{}' ContentType='{}'/>",
                    escape_attr(&extension),
                    escape_attr(&picture.media_type)
                ),
            );
        }
    }
    extras.insert(types_part.to_string(), types.into_bytes());

    let w_in = picture.w.max(1.0) / 96.0;
    let h_in = picture.h.max(1.0) / 96.0;
    let pin_x = (picture.x + picture.w / 2.0) / 96.0;
    let pin_y = page_height - (picture.y + picture.h / 2.0) / 96.0;
    let compression = extension.to_ascii_uppercase();
    let mut shape = format!(
        "<Shape ID='{}' Type='Foreign'>",
        escape_attr(&picture.visio_id)
    );
    for (name, value) in [
        ("PinX", pin_x),
        ("PinY", pin_y),
        ("Width", w_in),
        ("Height", h_in),
        ("LocPinX", w_in / 2.0),
        ("LocPinY", h_in / 2.0),
        ("Angle", 0.0),
        ("FlipX", 0.0),
        ("FlipY", 0.0),
        ("ImgOffsetX", 0.0),
        ("ImgOffsetY", 0.0),
        ("ImgWidth", w_in),
        ("ImgHeight", h_in),
        ("FillPattern", 0.0),
        ("LinePattern", 0.0),
    ] {
        shape.push_str(&format!("<Cell N='{name}' V='{}'/>", num_text(value)));
    }
    shape.push_str(
        "<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/><Cell N='NoSnap' V='0'/><Cell N='NoQuickDrag' V='0'/>",
    );
    for (index, (x, y)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]
        .iter()
        .enumerate()
    {
        let kind = if index == 0 { "RelMoveTo" } else { "RelLineTo" };
        shape.push_str(&format!(
            "<Row T='{kind}' IX='{}'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row>",
            index + 1,
            num_text(*x),
            num_text(*y)
        ));
    }
    shape.push_str(&format!(
        "</Section><ForeignData ForeignType='Bitmap' CompressionType='{compression}'><Rel r:id='{}'/></ForeignData>",
        escape_attr(&rel_id)
    ));
    if !picture.name.is_empty() || !picture.text.is_empty() {
        let label = if picture.text.is_empty() {
            picture.name.clone()
        } else {
            picture.text.clone()
        };
        shape.push_str(&format!("<Text>{}</Text>", escape_text(&label)));
    }
    shape.push_str("</Shape>");
    extras.insert(media_part.clone(), picture.bytes.clone());
    Ok((shape, media_part, rel_id))
}

fn page_rels_part(page_part: &str) -> Result<String> {
    let path = std::path::Path::new(page_part);
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error(format!("invalid page part {page_part}")))?;
    let parent = path
        .parent()
        .and_then(|path| path.to_str())
        .ok_or_else(|| Error(format!("invalid page part {page_part}")))?;
    Ok(format!("{parent}/_rels/{file}.rels").replace('\\', "/"))
}

/// Like `write_package_with`, and the parts named in `remove` are left out of the new package.
pub fn write_package_edit(package: &Package, extras: BTreeMap<String, Vec<u8>>, remove: &[String]) -> Result<Vec<u8>> {
    let mut archive = package.source_bytes().and_then(|bytes| zip::ZipArchive::new(Cursor::new(bytes)).ok());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::with_capacity(package.source_bytes().map_or(0, |b| b.len() + 65536))));
    for name in package.names() {
        if remove.iter().any(|r| r == name) {
            continue;
        }
        if let Some(bytes) = extras.get(name) {
            writer.start_file(name, entry_options(name))?;
            writer.write_all(bytes)?;
            continue;
        }
        // a part nothing changed: the compressed bytes of the source go over as they are
        if let Some(file) = archive.as_mut().and_then(|a| a.by_name(name).ok()) {
            writer.raw_copy_file(file)?;
            continue;
        }
        writer.start_file(name, entry_options(name))?;
        writer.write_all(package.entry(name).unwrap_or(&[]))?;
    }
    for (name, bytes) in extras {
        if package.contains(&name) || remove.contains(&name) {
            continue;
        }
        writer.start_file(&name, entry_options(&name))?;
        writer.write_all(&bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

/// How a part is stored: pictures that are compressed already (PNG, JPEG, GIF) are stored as they are - deflating a 40 MB
/// stencil's media on every edit costs seconds and saves nothing - everything else is deflated.
fn entry_options(name: &str) -> zip::write::SimpleFileOptions {
    let lower = name.to_ascii_lowercase();
    let packed = [".png", ".jpg", ".jpeg", ".gif"].iter().any(|ext| lower.ends_with(ext));
    // the zip "made by" host defaults to the build OS; pin it so the same model gives the same bytes everywhere
    zip::write::SimpleFileOptions::default().compression_method(if packed { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated }).system(zip::System::Dos)
}

/// The package with the given parts replaced or added (`write_package` for other modules of the crate).
pub(crate) fn write_package_with(package: &Package, extras: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    write_package(package, extras)
}

pub(crate) fn write_package(package: &Package, extras: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    write_package_edit(package, extras, &[])
}

fn remove_shape(xml: &str, id: &str) -> Result<String> {
    let (start, end) =
        find_shape_range(xml, id).ok_or_else(|| Error(format!("shape {id} not found")))?;
    // Every shape inside the removed element goes with it, and so do the glue rows naming them.
    let mut gone = vec![id.to_string()];
    let mut at = 0usize;
    while let Some(relative) = xml[start..end][at..].find("<Shape ") {
        let tag_start = start + at + relative;
        let tag_end = xml[tag_start..].find('>').map_or(end, |n| tag_start + n + 1);
        if let Some(inner) = attr(&xml[tag_start..tag_end], "ID") {
            gone.push(inner);
        }
        at = tag_end - start;
    }
    let mut out = format!("{}{}", &xml[..start], &xml[end..]);
    let mut search = 0usize;
    while let Some(relative) = out[search..].find("<Connect") {
        let tag_start = search + relative;
        let Some(close) = out[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + close + 1;
        let tag = &out[tag_start..tag_end];
        if attr(tag, "FromSheet").is_some_and(|sheet| gone.contains(&sheet))
            || attr(tag, "ToSheet").is_some_and(|sheet| gone.contains(&sheet))
        {
            out.replace_range(tag_start..tag_end, "");
            search = tag_start;
        } else {
            search = tag_end;
        }
    }
    Ok(out)
}

fn new_shape_xml(shape: &NewShape, page_height: f64) -> String {
    let w_in = shape.w.max(1.0) / 96.0;
    let h_in = shape.h.max(1.0) / 96.0;
    let pin_x = (shape.x + shape.w / 2.0) / 96.0;
    let pin_y = page_height - (shape.y + shape.h / 2.0) / 96.0;
    let fill_pattern = if shape.fill == "none" { 0 } else { 1 };
    let line_pattern = if shape.stroke == "none" || shape.stroke_width <= 0.0 {
        0
    } else {
        1
    };
    let font_size = shape.font_size.max(1.0) / 96.0;
    let mut out = String::new();
    out.push_str(&format!(
        "<Shape ID='{}' Type='Shape'>",
        escape_attr(&shape.visio_id)
    ));
    for (name, value) in [
        ("PinX", num_text(pin_x)),
        ("PinY", num_text(pin_y)),
        ("Width", num_text(w_in)),
        ("Height", num_text(h_in)),
        ("LocPinX", num_text(w_in / 2.0)),
        ("LocPinY", num_text(h_in / 2.0)),
        ("Angle", "0".to_string()),
        ("FlipX", "0".to_string()),
        ("FlipY", "0".to_string()),
        ("FillPattern", fill_pattern.to_string()),
        ("LinePattern", line_pattern.to_string()),
    ] {
        out.push_str(&format!("<Cell N='{name}' V='{value}'/>"));
    }
    if fill_pattern != 0 {
        out.push_str(&format!(
            "<Cell N='FillForegnd' V='{}'/>",
            escape_attr(&shape.fill)
        ));
    }
    if line_pattern != 0 {
        out.push_str(&format!(
            "<Cell N='LineColor' V='{}'/><Cell N='LineWeight' V='{}' U='IN'/>",
            escape_attr(&shape.stroke),
            num_text(shape.stroke_width.max(0.0) / 96.0)
        ));
    }
    out.push_str("<Section N='Character'><Row IX='0'>");
    out.push_str(&format!(
        "<Cell N='Color' V='{}'/><Cell N='Size' V='{}' U='PT'/>",
        escape_attr(&shape.text_color),
        num_text(font_size)
    ));
    out.push_str("</Row></Section><Section N='Geometry' IX='0'>");
    out.push_str("<Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/><Cell N='NoSnap' V='0'/><Cell N='NoQuickDrag' V='0'/>");
    for (index, (x, y)) in [
        (0.0, 0.0),
        (w_in, 0.0),
        (w_in, h_in),
        (0.0, h_in),
        (0.0, 0.0),
    ]
    .iter()
    .enumerate()
    {
        let kind = if index == 0 { "MoveTo" } else { "LineTo" };
        out.push_str(&format!(
            "<Row T='{kind}' IX='{}'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row>",
            index + 1,
            num_text(*x),
            num_text(*y)
        ));
    }
    out.push_str("</Section>");
    if !shape.text.is_empty() {
        out.push_str(&format!("<Text>{}</Text>", escape_text(&shape.text)));
    }
    out.push_str("</Shape>");
    out
}

pub(crate) fn new_connector_xml(connector: &NewConnector, page_height: f64) -> String {
    let begin_x = connector.from.0 / 96.0;
    let begin_y = page_height - connector.from.1 / 96.0;
    let end_x = connector.to.0 / 96.0;
    let end_y = page_height - connector.to.1 / 96.0;
    let width = end_x - begin_x;
    let height = end_y - begin_y;
    let mut out = format!(
        "<Shape ID='{}' Type='Shape'>",
        escape_attr(&connector.visio_id)
    );
    for (name, value) in [
        ("PinX", num_text((begin_x + end_x) / 2.0)),
        ("PinY", num_text((begin_y + end_y) / 2.0)),
        ("Width", num_text(width)),
        ("Height", num_text(height)),
        ("LocPinX", num_text(width / 2.0)),
        ("LocPinY", num_text(height / 2.0)),
        ("BeginX", num_text(begin_x)),
        ("BeginY", num_text(begin_y)),
        ("EndX", num_text(end_x)),
        ("EndY", num_text(end_y)),
        ("Angle", "0".to_string()),
        ("FlipX", "0".to_string()),
        ("FlipY", "0".to_string()),
        ("ObjType", "2".to_string()),
        ("ShapeRouteStyle", "2".to_string()),
        ("ConFixedCode", "6".to_string()),
    ] {
        out.push_str(&format!("<Cell N='{name}' V='{value}'/>"));
    }
    out.push_str(&format!(
        "<Cell N='LineColor' V='{}'/><Cell N='LineWeight' V='{}' U='IN'/>",
        escape_attr(&connector.stroke),
        num_text(connector.stroke_width.max(0.0) / 96.0)
    ));
    if connector.arrow_end {
        out.push_str("<Cell N='EndArrow' V='4'/>");
    }
    out.push_str("<Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/>");
    out.push_str("<Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row>");
    out.push_str(&format!(
        "<Row T='LineTo' IX='2'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row></Section>",
        num_text(width),
        num_text(height)
    ));
    if !connector.label.is_empty() {
        out.push_str(&format!("<Text>{}</Text>", escape_text(&connector.label)));
    }
    out.push_str("</Shape>");
    out
}

pub(crate) fn insert_connects(xml: &str, connector: &NewConnector) -> String {
    let mut rows = String::new();
    if let Some(from) = &connector.glue_from {
        rows.push_str(&format!(
            "<Connect FromSheet='{}' FromCell='BeginX' FromPart='9' ToSheet='{}' ToCell='PinX' ToPart='3'/>",
            escape_attr(&connector.visio_id),
            escape_attr(from)
        ));
    }
    if let Some(to) = &connector.glue_to {
        rows.push_str(&format!(
            "<Connect FromSheet='{}' FromCell='EndX' FromPart='12' ToSheet='{}' ToCell='PinX' ToPart='3'/>",
            escape_attr(&connector.visio_id),
            escape_attr(to)
        ));
    }
    if rows.is_empty() {
        return xml.to_string();
    }
    if let Some(end) = xml.rfind("</Connects>") {
        let mut out = xml.to_string();
        out.insert_str(end, &rows);
        out
    } else {
        let insert = xml
            .rfind("</PageContents>")
            .or_else(|| xml.rfind("</Shapes>"))
            .unwrap_or(xml.len());
        let mut out = xml.to_string();
        out.insert_str(insert, &format!("<Connects>{rows}</Connects>"));
        out
    }
}

pub(crate) fn num_text(value: f64) -> String {
    let rounded = (value * 1e9).round() / 1e9;
    if (rounded - rounded.round()).abs() < 1e-12 {
        format!("{}", rounded.round() as i64)
    } else {
        let mut text = format!("{rounded:.9}");
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        text
    }
}

fn patch_shape_in_text(xml: &str, patch: &ShapeCellPatch) -> Result<String> {
    let (start, end) = find_shape_range(xml, &patch.visio_id)
        .ok_or_else(|| Error(format!("shape {} not found", patch.visio_id)))?;
    let mut shape = xml[start..end].to_string();
    // A self-closing `<Shape .../>` (an empty master-instance override) has no
    // body to hold cells; expand it first or the cells land outside the shape.
    if shape.trim_end().ends_with("/>") {
        let trimmed = shape.trim_end().len();
        shape.truncate(trimmed - 2);
        let body_end = shape.trim_end().len();
        shape.truncate(body_end);
        shape.push_str("></Shape>");
    }
    // Patching a cell can shorten (a dropped formula) or lengthen (an inserted cell) the text,
    // so the end of the shape's own cells is found again for every patch.
    let direct_end = |shape: &str| {
        // `<Text>` ends the shape's own cells too: a cell that is missing is inserted BEFORE it. (A shape with text and no section yet used to get
        // its first size cells AFTER the text; the next resize then wrote a second set before the sections and the stale first set won: the shape would not
        // resize a second time. User bug 2026-11-16.)
        ["<Section", "<Shapes", "<Text>", "<Text/>", "</Shape>"]
            .iter()
            .filter_map(|needle| shape.find(needle))
            .min()
            .unwrap_or(shape.len())
    };
    for (name, value) in &patch.cells {
        shape = patch_cell_in_range(&shape, 0, direct_end(&shape), name, value, "V");
    }
    for (name, value) in &patch.cached_cells {
        shape = patch_cell_range(&shape, 0, direct_end(&shape), name, value, "V", true);
    }
    let resized = patch.cells.contains_key("Width") || patch.cells.contains_key("Height") || patch.cached_cells.contains_key("Width") || patch.cached_cells.contains_key("Height");
    if resized {
        shape = refresh_formula_values(&shape);
    }
    for (name, value, formula) in &patch.formula_cells {
        shape = patch_cell_range(&shape, 0, direct_end(&shape), name, value, "V", true);
        shape = patch_cell_range(&shape, 0, direct_end(&shape), name, formula, "F", true);
    }
    for patch in &patch.section_cells {
        shape = patch_section_cell(&shape, patch)?;
    }
    if let Some(rich) = &patch.rich {
        shape = patch_rich_text(&shape, rich);
    } else if let Some(text) = &patch.text {
        shape = patch_shape_text(&shape, text);
    }
    if let Some(text) = &patch.text_edit {
        shape = patch_shape_text_edit(&shape, text)?;
    }
    if let Some(range) = &patch.text_range {
        shape = format_text_range(&shape, range)?;
    }
    for row in &patch.row_overrides {
        shape = apply_row_override(&shape, row);
    }
    if let Some(children) = &patch.append_children {
        shape = append_child_shapes(&shape, children);
    }
    if patch.char_size.is_some() || !patch.char_cells.is_empty() || patch.char_style.is_some() {
        let mut cells = patch.char_cells.clone();
        if let Some(size) = patch.char_size {
            cells.push(("Size".to_string(), num_text(size)));
        }
        shape = set_character_cells(&shape, &cells, patch.char_style);
    }
    for (index, cell, value) in &patch.geometry_flags {
        shape = set_geometry_flag(&shape, index, cell, value)?;
    }
    if let Some((index, rows)) = &patch.geometry_rows {
        shape = replace_geometry_rows(&shape, index, rows)?;
    }
    for (name, value) in &patch.data {
        shape = patch_shape_data(&shape, name, value);
    }
    for edit in &patch.data_edits {
        shape = apply_data_edit(&shape, edit)?;
    }
    let mut out = String::with_capacity(xml.len() + shape.len());
    out.push_str(&xml[..start]);
    out.push_str(&shape);
    out.push_str(&xml[end..]);
    Ok(out)
}

pub(crate) fn find_shape_range(xml: &str, id: &str) -> Option<(usize, usize)> {
    let id_at = find_id_attribute(xml, id)?;
    let open = xml[..id_at].rfind("<Shape")?;
    let open_end = xml[open..].find('>')? + open;
    if xml[open..open_end].trim_end().ends_with('/') {
        return Some((open, open_end + 1));
    }
    let mut depth = 1usize;
    let mut index = open_end + 1;
    while index < xml.len() {
        let next = xml[index..].find('<')? + index;
        if xml[next..].starts_with("</Shape>") {
            depth -= 1;
            if depth == 0 {
                return Some((open, next + "</Shape>".len()));
            }
            index = next + "</Shape>".len();
        } else if is_shape_open(xml, next) {
            let tag_end = xml[next..].find('>')? + next;
            if !xml[next..tag_end].trim_end().ends_with('/') {
                depth += 1;
            }
            index = tag_end + 1;
        } else {
            index = next + 1;
        }
    }
    None
}

/// The `ID` attribute of the shape with this ID, not `OriginalID` or a cell
/// reference. Attribute boundaries keep `ID='1'` from matching `ID='12'`.
fn find_id_attribute(xml: &str, id: &str) -> Option<usize> {
    for quote in ['\'', '"'] {
        let needle = format!("ID={quote}{id}{quote}");
        let mut search = 0;
        while let Some(relative) = xml[search..].find(&needle) {
            let at = search + relative;
            let before = xml[..at].chars().next_back();
            let after = xml[at + needle.len()..].chars().next();
            let before_ok = before.is_none_or(|c| c.is_whitespace() || c == '<');
            let after_ok = after.is_none_or(|c| c.is_whitespace() || c == '>' || c == '/');
            if before_ok && after_ok {
                return Some(at);
            }
            search = at + needle.len();
        }
    }
    None
}

/// `<Shape ...>` or `<Shape .../>`, but not `<Shapes>`.
fn is_shape_open(xml: &str, at: usize) -> bool {
    let Some(rest) = xml.get(at..) else {
        return false;
    };
    let Some(after) = rest.strip_prefix("<Shape") else {
        return false;
    };
    after
        .chars()
        .next()
        .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
}

fn patch_cell_in_range(
    value: &str,
    start: usize,
    end: usize,
    name: &str,
    new_value: &str,
    attr_name: &str,
) -> String {
    patch_cell_range(value, start, end, name, new_value, attr_name, false)
}

fn patch_cell_range(
    value: &str,
    start: usize,
    end: usize,
    name: &str,
    new_value: &str,
    attr_name: &str,
    keep_formula: bool,
) -> String {
    let mut search = start;
    while let Some(relative) = value[search..end].find("<Cell") {
        let cell_start = search + relative;
        let Some(close) = value[cell_start..end].find('>') else {
            break;
        };
        let cell_end = cell_start + close + 1;
        if attr(&value[cell_start..cell_end], "N").as_deref() == Some(name) {
            let mut replacement = set_attr(&value[cell_start..cell_end], attr_name, new_value);
            if attr_name == "V" && !keep_formula {
                replacement = remove_attr(&replacement, "F");
            }
            return format!(
                "{}{}{}",
                &value[..cell_start],
                replacement,
                &value[cell_end..]
            );
        }
        search = cell_end;
    }
    let insert = if attr_name == "V" {
        ["<Row", "</Section", "<Shapes"]
            .iter()
            .filter_map(|needle| value[start..end].find(needle).map(|at| start + at))
            .min()
            .unwrap_or(end)
    } else {
        end
    };
    let cell = format!(
        "<Cell N='{}' {}='{}'/>",
        escape_attr(name),
        attr_name,
        escape_attr(new_value)
    );
    format!("{}{}{}", &value[..insert], cell, &value[insert..])
}

fn patch_section_cell(shape: &str, patch: &SectionCellPatch) -> Result<String> {
    let (start, end) = find_section_range(shape, &patch.section, &patch.section_index)
        .ok_or_else(|| Error(format!("section {} not found", patch.section)))?;
    if let Some(row_key) = &patch.row {
        let (row_start, row_end) = find_row_range(shape, start, end, row_key).ok_or_else(|| {
            Error(format!(
                "row {row_key} not found in section {}",
                patch.section
            ))
        })?;
        let result = patch_cell_in_range(shape, row_start, row_end, &patch.cell, &patch.value, "V");
        Ok(result)
    } else {
        let first_row = shape[start..end]
            .find("<Row")
            .map(|at| start + at)
            .unwrap_or(end);
        let result = patch_cell_in_range(shape, start, first_row, &patch.cell, &patch.value, "V");
        Ok(result)
    }
}

fn find_section_range(text: &str, section: &str, index: &str) -> Option<(usize, usize)> {
    let mut search = 0usize;
    while let Some(relative) = text[search..].find("<Section") {
        let start = search + relative;
        let close = text[start..].find('>')? + start;
        let tag = &text[start..close + 1];
        let section_name = attr(tag, "N").unwrap_or_default();
        let section_index = attr(tag, "IX").unwrap_or_default();
        if section_name == section && (index.is_empty() || section_index == index) {
            let end = text[close..].find("</Section>")? + close + "</Section>".len();
            return Some((start, end));
        }
        search = close + 1;
    }
    None
}

fn find_row_range(text: &str, start: usize, end: usize, key: &str) -> Option<(usize, usize)> {
    let mut search = start;
    while let Some(relative) = text[search..end].find("<Row") {
        let row_start = search + relative;
        let close = text[row_start..end].find('>')? + row_start;
        let tag = &text[row_start..close + 1];
        if attr(tag, "IX").as_deref() == Some(key) || attr(tag, "N").as_deref() == Some(key) {
            // a self-closing row (`<Row N='X' Del='1'/>`) ends at its own `>`
            if tag.ends_with("/>") {
                return Some((row_start, close + 1));
            }
            let row_end = text[close..end].find("</Row>")? + close + "</Row>".len();
            return Some((row_start, row_end));
        }
        search = close + 1;
    }
    None
}

pub(crate) fn patch_rich_text(shape: &str, rich: &Value) -> String {
    let mut text = String::new();
    let mut characters: BTreeMap<String, &Value> = BTreeMap::new();
    let mut paragraphs: BTreeMap<String, &Value> = BTreeMap::new();
    let mut fields = String::new();
    let mut field_index = 0usize;
    if let Some(items) = rich.get("paragraphs").and_then(Value::as_array) {
        for (paragraph_index, paragraph) in items.iter().enumerate() {
            let pp = paragraph
                .get("pp")
                .and_then(Value::as_str)
                .unwrap_or("0")
                .to_string();
            paragraphs.entry(pp.clone()).or_insert(paragraph);
            text.push_str(&format!("<pp IX='{}'/>", escape_attr(&pp)));
            if let Some(runs) = paragraph.get("runs").and_then(Value::as_array) {
                for run in runs {
                    let cp = run
                        .get("cp")
                        .and_then(Value::as_str)
                        .unwrap_or("0")
                        .to_string();
                    characters.entry(cp.clone()).or_insert(run);
                    text.push_str(&format!("<cp IX='{}'/>", escape_attr(&cp)));
                    let content = if let Some(field) = run.get("field") {
                        let ix = field_index;
                        field_index += 1;
                        let formula = field.get("formula").and_then(Value::as_str).unwrap_or("");
                        let value = field.get("value").and_then(Value::as_str).unwrap_or("");
                        let format = field.get("format").and_then(Value::as_str).unwrap_or("");
                        fields.push_str(&format!(
                            "<Row IX='{ix}'><Cell N='Value' V='{}' F='{}'/><Cell N='Format' V='{}' U='STR' F='{}'/></Row>",
                            escape_attr(value),
                            escape_attr(formula),
                            escape_attr(format),
                            escape_attr(format)
                        ));
                        format!(
                            "<fld IX='{ix}'>{}</fld>",
                            escape_text(run.get("text").and_then(Value::as_str).unwrap_or(value))
                        )
                    } else {
                        escape_text(run.get("text").and_then(Value::as_str).unwrap_or(""))
                    };
                    text.push_str(&content);
                }
            }
            if paragraph_index + 1 < items.len() {
                text.push('\n');
            }
        }
    }

    let mut character_section = String::from("<Section N='Character'>");
    for (ix, run) in &characters {
        let style = run.get("style");
        character_section.push_str(&format!("<Row IX='{}'>", escape_attr(ix)));
        if let Some(font) = style
            .and_then(|style| style.get("fontFamily"))
            .and_then(Value::as_str)
        {
            character_section.push_str(&format!("<Cell N='Font' V='{}'/>", escape_attr(font)));
        }
        if let Some(color) = style
            .and_then(|style| style.get("textColor"))
            .and_then(Value::as_str)
        {
            character_section.push_str(&format!("<Cell N='Color' V='{}'/>", escape_attr(color)));
        }
        if let Some(size) = style
            .and_then(|style| style.get("fontSize"))
            .and_then(Value::as_f64)
        {
            character_section.push_str(&format!(
                "<Cell N='Size' V='{}' U='PT'/>",
                num_text(size / 96.0)
            ));
        }
        let mut style_bits = 0u32;
        if style
            .and_then(|style| style.get("bold"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            style_bits |= 1;
        }
        if style
            .and_then(|style| style.get("italic"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            style_bits |= 2;
        }
        if style
            .and_then(|style| style.get("underline"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            style_bits |= 4;
        }
        character_section.push_str(&format!("<Cell N='Style' V='{style_bits}'/>"));
        character_section.push_str("</Row>");
    }
    character_section.push_str("</Section>");
    if characters.is_empty() {
        character_section.clear();
    }

    let mut paragraph_section = String::from("<Section N='Paragraph'>");
    for (ix, paragraph) in &paragraphs {
        paragraph_section.push_str(&format!("<Row IX='{}'>", escape_attr(ix)));
        let align = match paragraph
            .get("align")
            .and_then(Value::as_str)
            .unwrap_or("center")
        {
            "left" => 0,
            "right" => 2,
            _ => 1,
        };
        paragraph_section.push_str(&format!("<Cell N='HorzAlign' V='{align}'/>"));
        paragraph_section.push_str(&format!(
            "<Cell N='Bullet' V='{}'/>",
            if paragraph
                .get("bullet")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                1
            } else {
                0
            }
        ));
        paragraph_section.push_str(&format!(
            "<Cell N='BulletStr' V='{}' U='STR'/>",
            escape_attr(
                paragraph
                    .get("bulletStr")
                    .and_then(Value::as_str)
                    .unwrap_or("•")
            )
        ));
        paragraph_section.push_str(&format!(
            "<Cell N='IndLeft' V='{}'/>",
            num_text(
                paragraph
                    .get("indent")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    / 96.0
            )
        ));
        paragraph_section.push_str("</Row>");
    }
    paragraph_section.push_str("</Section>");

    let mut shape = replace_or_insert_section(shape, "Character", &character_section);
    shape = replace_or_insert_section(&shape, "Paragraph", &paragraph_section);
    if fields.is_empty() {
        shape = remove_section(&shape, "Field");
    } else {
        shape = replace_or_insert_section(
            &shape,
            "Field",
            &format!("<Section N='Field'>{fields}</Section>"),
        );
    }
    patch_shape_raw_text(&shape, &text)
}

fn replace_or_insert_section(shape: &str, name: &str, xml: &str) -> String {
    if let Some((start, end)) = find_section_range(shape, name, "") {
        return format!("{}{}{}", &shape[..start], xml, &shape[end..]);
    }
    if xml.is_empty() {
        return shape.to_string();
    }
    let insert = ["<Section", "<Text", "<Shapes", "</Shape>"]
        .iter()
        .filter_map(|needle| shape.find(needle))
        .min()
        .unwrap_or(shape.len());
    format!("{}{}{}", &shape[..insert], xml, &shape[insert..])
}

fn remove_section(shape: &str, name: &str) -> String {
    if let Some((start, end)) = find_section_range(shape, name, "") {
        format!("{}{}", &shape[..start], &shape[end..])
    } else {
        shape.to_string()
    }
}

fn replace_geometry_rows(shape: &str, index: &str, rows: &[GeometryRow]) -> Result<String> {
    // An instance of a master (a dropped stencil arrow, say) has no Geometry section of its own: it takes the master's. Rows
    // written for it need the section, which then overrides the master's row by row (Visio writes it that way when such a
    // connector is re-routed), so an empty one is created first.
    if find_section_range(shape, "Geometry", index).is_none() {
        let insert = ["<Text", "<Shapes"]
            .iter()
            .filter_map(|needle| shape.find(needle))
            .min()
            .unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
        let created = format!("{}<Section N='Geometry' IX='{}'></Section>{}", &shape[..insert], escape_attr(index), &shape[insert..]);
        if find_section_range(&created, "Geometry", index).is_none() {
            return Err(Error(format!("geometry section {index} not found")));
        }
        return replace_geometry_rows(&created, index, rows);
    }
    let (start, end) = find_section_range(shape, "Geometry", index)
        .ok_or_else(|| Error(format!("geometry section {index} not found")))?;
    let section = &shape[start..end];
    let mut cleaned = String::new();
    let mut search = 0usize;
    while let Some(relative) = section[search..].find("<Row") {
        let row_start = search + relative;
        cleaned.push_str(&section[search..row_start]);
        let close = section[row_start..]
            .find('>')
            .ok_or_else(|| Error("invalid geometry row".to_string()))?;
        // a self-closing row (`<Row T='LineTo' IX='3' Del='1'/>`) ends at its own `>`; any other row at its `</Row>`
        let row_end = if section[row_start..row_start + close].ends_with('/') {
            row_start + close + 1
        } else {
            section[row_start + close..]
                .find("</Row>")
                .map(|at| row_start + close + at + "</Row>".len())
                .unwrap_or(section.len())
        };
        search = row_end;
    }
    cleaned.push_str(&section[search..]);
    let insert = cleaned
        .rfind("</Section>")
        .ok_or_else(|| Error("invalid geometry section".to_string()))?;
    let mut rows_xml = String::new();
    for row in rows {
        let mut row_xml = format!(
            "<Row T='{}' IX='{}'>",
            escape_attr(&row.kind),
            escape_attr(&row.index)
        );
        for (name, value) in &row.cells {
            row_xml.push_str(&format!(
                "<Cell N='{}' V='{}'/>",
                escape_attr(name),
                escape_attr(value)
            ));
        }
        for (name, (value, formula)) in &row.value_formulas {
            row_xml.push_str(&format!("<Cell N='{}' V='{}' F='{}'/>", escape_attr(name), escape_attr(value), escape_attr(formula)));
        }
        for (name, formula) in &row.formulas {
            // Visio tags POLYLINE and NURBS cells with their own unit (without U='NURBS' it draws the curve as a straight line until the formula is recalculated).
            let unit = if formula.starts_with("POLYLINE(") { " U='POLYLINE'" } else if formula.starts_with("NURBS(") { " U='NURBS'" } else { "" };
            row_xml.push_str(&format!(
                "<Cell N='{}' V='{}'{} F='{}'/>",
                escape_attr(name),
                escape_attr(formula),
                unit,
                escape_attr(formula)
            ));
        }
        row_xml.push_str("</Row>");
        rows_xml.push_str(&row_xml);
    }
    cleaned.insert_str(insert, &rows_xml);
    Ok(format!("{}{}{}", &shape[..start], cleaned, &shape[end..]))
}

fn patch_shape_raw_text(shape: &str, inner: &str) -> String {
    let direct_end = shape.find("<Shapes").unwrap_or(shape.len());
    if let Some(start) = shape[..direct_end].find("<Text>") {
        let content_start = start + "<Text>".len();
        if let Some(relative) = shape[content_start..direct_end].find("</Text>") {
            let content_end = content_start + relative;
            return format!(
                "{}<Text>{}</Text>{}",
                &shape[..start],
                inner,
                &shape[content_end + "</Text>".len()..]
            );
        }
    }
    let insert = shape
        .find("<Shapes")
        .unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
    format!(
        "{}<Text>{}</Text>{}",
        &shape[..insert],
        inner,
        &shape[insert..]
    )
}

fn patch_shape_text(shape: &str, text: &str) -> String {
    let direct_end = shape.find("<Shapes").unwrap_or(shape.len());
    if let Some(start) = shape[..direct_end].find("<Text>") {
        let content_start = start + "<Text>".len();
        if let Some(relative) = shape[content_start..direct_end].find("</Text>") {
            let content_end = content_start + relative;
            return format!(
                "{}<Text>{}</Text>{}",
                &shape[..start],
                escape_text(text),
                &shape[content_end + "</Text>".len()..]
            );
        }
    }
    let insert = shape
        .find("<Shapes")
        .unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
    format!(
        "{}<Text>{}</Text>{}",
        &shape[..insert],
        escape_text(text),
        &shape[insert..]
    )
}

/// Append `shape_xml` to the page's root `Shapes` and `connect_rows` to its `Connects` (created
/// when the page has none); returns the new package bytes.
pub(crate) fn append_shape_with_connects(
    package: &Package,
    page_index: usize,
    shape_xml: &str,
    connect_rows: &str,
) -> Result<Vec<u8>> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut xml = package.text(&page.part)?.to_string();
    let insert = xml
        .rfind("</Shapes>")
        .ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
    xml.insert_str(insert, shape_xml);
    if !connect_rows.is_empty() {
        if let Some(end) = xml.rfind("</Connects>") {
            xml.insert_str(end, connect_rows);
        } else {
            let at = xml
                .rfind("</PageContents>")
                .or_else(|| xml.rfind("</Shapes>").map(|n| n + "</Shapes>".len()))
                .unwrap_or(xml.len());
            xml.insert_str(at, &format!("<Connects>{connect_rows}</Connects>"));
        }
    }
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(page.part.clone(), xml.into_bytes());
    write_package(package, extras)
}

/// Drop the `Connect` rows of shape `from_sheet` whose `FromCell` is one of `from_cells` and add
/// `new_rows` (XML text of `Connect` elements); returns the new package bytes. This is how a
/// connector end is glued to another shape or set free.
pub(crate) fn replace_connect_rows(
    package: &Package,
    page_index: usize,
    from_sheet: &str,
    from_cells: &[&str],
    new_rows: &str,
) -> Result<Vec<u8>> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut xml = package.text(&page.part)?.to_string();
    let mut search = 0usize;
    while let Some(relative) = xml[search..].find("<Connect ") {
        let tag_start = search + relative;
        let Some(close) = xml[tag_start..].find('>') else { break };
        let tag_end = tag_start + close + 1;
        let tag = &xml[tag_start..tag_end];
        if attr(tag, "FromSheet").as_deref() == Some(from_sheet)
            && attr(tag, "FromCell").is_some_and(|cell| from_cells.contains(&cell.as_str()))
        {
            xml.replace_range(tag_start..tag_end, "");
            search = tag_start;
        } else {
            search = tag_end;
        }
    }
    if !new_rows.is_empty() {
        if let Some(end) = xml.rfind("</Connects>") {
            xml.insert_str(end, new_rows);
        } else {
            let at = xml
                .rfind("</PageContents>")
                .or_else(|| xml.rfind("</Shapes>").map(|n| n + "</Shapes>".len()))
                .unwrap_or(xml.len());
            xml.insert_str(at, &format!("<Connects>{new_rows}</Connects>"));
        }
    }
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(page.part.clone(), xml.into_bytes());
    write_package(package, extras)
}

/// The XML of shape `id` (with everything inside it) as written on page `page_index`.
pub fn shape_xml(package: &Package, page_index: usize, id: &str) -> Result<String> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let xml = package.text(&page.part)?;
    let (start, end) = find_shape_range(xml, id).ok_or_else(|| Error(format!("shape {id} not found")))?;
    Ok(xml[start..end].to_string())
}

/// The value of a cell the shape element itself carries (not one of its members').
fn direct_cell(shape: &str, name: &str) -> Option<f64> {
    let direct_end = ["<Section", "<Shapes"].iter().filter_map(|n| shape.find(n)).min().unwrap_or(shape.len());
    let mut at = 0;
    while let Some(relative) = shape[at..direct_end].find("<Cell") {
        let start = at + relative;
        let end = shape[start..direct_end].find('>').map_or(direct_end, |n| start + n + 1);
        let tag = &shape[start..end];
        if attr(tag, "N").as_deref() == Some(name) {
            return attr(tag, "V").and_then(|v| v.parse().ok());
        }
        at = end;
    }
    None
}

/// Paste copies of the shapes (as returned by `shape_xml`) onto page `page_index`, offset by
/// (`dx`, `dy`) inches: every shape and every shape inside a group gets a fresh ID, references
/// such as `Sheet.7!Connections.X1` that point into the pasted set follow the renumbering, and a
/// name that ends in the old ID (Visio's "Rectangle.5") ends in the new one. The copies go on
/// top. Returns the new package bytes and the new IDs of the pasted top-level shapes.
pub fn paste_shapes(
    package: &Package,
    page_index: usize,
    shapes: &[String],
    dx: f64,
    dy: f64,
) -> Result<(Vec<u8>, Vec<String>)> {
    paste_shapes_with_connects(package, page_index, shapes, &[], dx, dy)
}

/// Apply `patch` to the shape element `patch.visio_id` inside a shape's XML text (the text
/// `shape_xml` returns) and return the new text.
pub fn patch_shape_xml(xml: &str, patch: &ShapeCellPatch) -> Result<String> {
    patch_shape_in_text(xml, patch)
}

/// Like `paste_shapes`, and also adds `connect_rows` (XML `Connect` elements that name the OLD
/// shape ids) to the page's `Connects`, with both sheets renumbered to the pasted copies; a row
/// that names a shape that was not pasted is dropped. Pasted connectors (shapes with
/// `BeginX/BeginY/EndX/EndY`) move as a whole: their end cells shift by (`dx`, `dy`) like the pin.
pub fn paste_shapes_with_connects(
    package: &Package,
    page_index: usize,
    shapes: &[String],
    connect_rows: &[String],
    dx: f64,
    dy: f64,
) -> Result<(Vec<u8>, Vec<String>)> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut next = next_shape_id(package, page_index)?;
    let catalog = crate::MasterCatalog::load(package)?;
    // Pass 1: the renumbering of every shape element of every pasted shape.
    let mut map: Vec<(String, u64)> = Vec::new();
    for shape in shapes {
        let mut at = 0usize;
        while let Some(relative) = shape[at..].find("<Shape ") {
            let start = at + relative;
            let end = shape[start..].find('>').map_or(shape.len(), |n| start + n + 1);
            if let Some(id) = attr(&shape[start..end], "ID") {
                map.push((id, next));
                next += 1 + attr(&shape[start..end], "Master").map_or(0, |m| implicit_sub_shapes(&catalog, &m));
            }
            at = end;
        }
    }
    // Pass 2: rewrite each pasted shape.
    let mut pasted = String::new();
    let mut top_ids = Vec::new();
    let mut cursor = 0usize;
    for shape in shapes {
        let mut out = String::with_capacity(shape.len());
        let mut at = 0usize;
        let mut first = true;
        while let Some(relative) = shape[at..].find("<Shape ") {
            let start = at + relative;
            let end = shape[start..].find('>').map_or(shape.len(), |n| start + n + 1);
            out.push_str(&shape[at..start]);
            let mut tag = shape[start..end].to_string();
            let old = attr(&tag, "ID").unwrap_or_default();
            let new = map[cursor].1.to_string();
            cursor += 1;
            tag = set_attr(&tag, "ID", &new);
            for name in ["NameU", "Name"] {
                if let Some(value) = attr(&tag, name) {
                    if let Some(stem) = value.strip_suffix(&format!(".{old}")) {
                        tag = set_attr(&tag, name, &format!("{stem}.{new}"));
                    } else if first && attr(&tag, "Master").is_some() {
                        // The first instance of a master is just "Box"; Visio calls the next "Box.7".
                        tag = set_attr(&tag, name, &format!("{value}.{new}"));
                    }
                }
            }
            if first {
                top_ids.push(new);
                first = false;
            }
            out.push_str(&tag);
            at = end;
        }
        out.push_str(&shape[at..]);
        // Formulas that point at pasted shapes: through a marker so that a new ID that equals
        // another old ID is not renumbered twice.
        for (old, new) in &map {
            out = out.replace(&format!("Sheet.{old}!"), &format!("Sheet.{new}\u{1}!"));
        }
        pasted.push_str(&out.replace('\u{1}', ""));
    }
    let mut xml = package.text(&page.part)?.to_string();
    let insert = xml
        .rfind("</Shapes>")
        .ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
    xml.insert_str(insert, &pasted);
    // Offset the pins of the pasted top-level shapes.
    for id in &top_ids {
        let (start, end) = find_shape_range(&xml, id).ok_or_else(|| Error(format!("pasted shape {id} not found")))?;
        let shape = &xml[start..end];
        let (Some(px), Some(py)) = (direct_cell(shape, "PinX"), direct_cell(shape, "PinY")) else {
            return Err(Error(format!("pasted shape {id} has no position of its own")));
        };
        let mut patch = ShapeCellPatch::new(id.clone()).set("PinX", num_text(px + dx)).set("PinY", num_text(py + dy));
        // A connector moves as a whole: its ends (cached values; a glued end keeps its formula).
        let ends: Vec<Option<f64>> = ["BeginX", "BeginY", "EndX", "EndY"].iter().map(|name| direct_cell(shape, name)).collect();
        if ends.iter().all(|v| v.is_some()) {
            for (name, value) in ["BeginX", "BeginY", "EndX", "EndY"].iter().zip(&ends) {
                let shift = if name.ends_with('X') { dx } else { dy };
                patch = patch.set_cached(*name, num_text(value.unwrap_or(0.0) + shift));
            }
        }
        xml = patch_shape_in_text(&xml, &patch)?;
    }
    // Glue rows of the pasted connectors, renumbered.
    let mut rows = String::new();
    for row in connect_rows {
        let mut tag = row.clone();
        let mut ok = true;
        for name in ["FromSheet", "ToSheet"] {
            match attr(&tag, name).and_then(|old| map.iter().find(|(o, _)| *o == old)) {
                Some((_, new)) => tag = set_attr(&tag, name, &new.to_string()),
                None => ok = false,
            }
        }
        if ok {
            rows.push_str(&tag);
        }
    }
    if !rows.is_empty() {
        if let Some(end) = xml.rfind("</Connects>") {
            xml.insert_str(end, &rows);
        } else {
            let at = xml
                .rfind("</PageContents>")
                .or_else(|| xml.rfind("</Shapes>").map(|n| n + "</Shapes>".len()))
                .unwrap_or(xml.len());
            xml.insert_str(at, &format!("<Connects>{rows}</Connects>"));
        }
    }
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(page.part.clone(), xml.into_bytes());
    Ok((write_package(package, extras)?, top_ids))
}

/// How many sub-shapes Visio creates itself for an instance of master `master_id`: the master's
/// shapes below its top shape. They do not appear in the page XML (unless overridden) but Visio
/// gives them the IDs that follow the instance's, so a later shape with such an ID would be taken
/// for a sub-shape (it showed up as a member of the instance).
fn implicit_sub_shapes(catalog: &crate::MasterCatalog, master_id: &str) -> u64 {
    catalog.masters.get(master_id).map_or(0, |m| m.shapes.len().saturating_sub(1) as u64)
}

/// The ID a new shape on page `page_index` can take: above every shape ID on the page and above
/// the IDs Visio reserves for the implicit sub-shapes of master instances.
pub fn next_shape_id(package: &Package, page_index: usize) -> Result<u64> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let xml = package.text(&page.part)?;
    let catalog = crate::MasterCatalog::load(package)?;
    let mut largest = 0u64;
    let mut at = 0usize;
    while let Some(relative) = xml[at..].find("<Shape ") {
        let start = at + relative;
        let end = xml[start..].find('>').map_or(xml.len(), |n| start + n + 1);
        let tag = &xml[start..end];
        if let Some(id) = attr(tag, "ID").and_then(|id| id.parse::<u64>().ok()) {
            let reserved = attr(tag, "Master").map_or(0, |m| implicit_sub_shapes(&catalog, &m));
            largest = largest.max(id + reserved);
        }
        at = end;
    }
    Ok(largest + 1)
}

/// Drop an instance of master `master_id` on page `page_index` with its pin at (`x`, `y`) inches
/// (y up), the way Visio writes one: a shape that names the master and carries only its position;
/// the size, geometry, text and style come from the master. Returns the new package bytes and
/// the ID of the new shape.
pub fn add_master_instance(
    package: &Package,
    page_index: usize,
    master_id: &str,
    x: f64,
    y: f64,
) -> Result<(Vec<u8>, String)> {
    let catalog = crate::MasterCatalog::load(package)?;
    let master = catalog
        .masters
        .get(master_id)
        .ok_or_else(|| Error(format!("the drawing has no master {master_id}")))?;
    let id = next_shape_id(package, page_index)?;
    let pages = package.pages()?;
    let page = &pages[page_index];
    let mut xml = package.text(&page.part)?.to_string();
    let name = if master.name.is_empty() { format!("Master.{master_id}") } else { master.name.clone() };
    let name = format!("{name}.{id}");
    // A 1-D master (its top shape has BeginX/EndX, like a stencil's cable or line) is drawn between its
    // ends: an instance that only names a position is dropped by Visio when it loads (probe-stencil-drop2),
    // so the ends are written too, the way Visio writes its own drop: a horizontal line through the pin.
    let top = master.top.as_ref().and_then(|top| master.shapes.get(top));
    let top_cell = |name: &str| top.and_then(|t| t.cells.get(name)).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
    let one_d = top.is_some_and(|t| t.cells.contains_key("BeginX") && t.cells.contains_key("EndX"));
    // A picture master (its top shape is `Foreign`) must be instanced as a `Foreign` shape: Visio draws an instance
    // written as an ordinary shape as nothing at all (found with a stencil of PNG pictures: Visio's own drop writes
    // `Type='Foreign'`, ours `Type='Shape'` was exported blank).
    let top_type = top.and_then(|t| t.shape_type.as_deref());
    // A group master is instanced as a Group (Visio's own drop writes `Type='Group'`, its children inside).
    let kind = if one_d { top_type.unwrap_or("Shape") } else if top_type == Some("Foreign") { "Foreign" } else if top_type == Some("Group") { "Group" } else { "Shape" };
    // A 1-D GROUP master is a symbol (rack PDUs, UPS: the CyberPower / APC stencils), not a line: its box is centred on the drop point
    // (its own origin is the bottom right corner, so a 72 in tall PDU dropped by the pin would stand entirely above the page).
    let symbol = one_d && top_type == Some("Group") && top_cell("Height").is_some_and(|h| h.abs() > 0.05); // a flat 1-D group (servers) keeps the old line-like drop
    // A master authored on a scaled page (PageScale / DrawingScale of its page sheet, 1:10 for the CyberPower and Dell rack stencils) is
    // shrunk by Visio when it is dropped on an unscaled page: 2.2 x 72 in becomes 0.22 x 7.2 in (probe-pdu-drop.ps1: page 1:1 -> factor
    // master ratio, page 2:1 -> master ratio / 2, pages 1:5, 1:10, 1:20, 1:24, 1:48 -> 1, for masters of 1:10 and 1:24).
    let target_ratio = crate::pageops::page_scale_ratio(package, page_index).unwrap_or(1.0);
    let factor = if master.page_ratio > 0.0 && target_ratio >= 1.0 - 1e-9 { master.page_ratio / target_ratio } else { 1.0 };
    let factor = if (factor - 1.0).abs() < 1e-9 { 1.0 } else { factor };
    let (mut pin_x, mut pin_y) = (x, y);
    let ends = if one_d {
        let width = top_cell("Width").unwrap_or(1.0);
        let loc_x = top_cell("LocPinX").unwrap_or(width / 2.0);
        let (begin_x, line_y) = if symbol {
            let height = top_cell("Height").unwrap_or(0.0);
            pin_x = x - width / 2.0 + loc_x;
            let _ = height;
            // round 11b (Q5): Visio's own drop (probe-tall-1d-turn.ps1, PDU83107 at 6, 12) keeps the PIN at the drop height: Begin y = 12.0
            // (LocPinY = 0), while x stays centred on the drop point; the box used to be centred on y as well (3.6 in too low)
            pin_y = y;
            // the ends are those of the shrunk shape (a drop on an unscaled page is resized about its pin height below)
            let final_pin_x = x - width * factor / 2.0 + loc_x * factor;
            (final_pin_x - loc_x * factor, y - top_cell("LocPinY").unwrap_or(0.0) * factor)
        } else {
            (x - loc_x * factor, y)
        };
        format!(
            "<Cell N='BeginX' V='{}'/><Cell N='BeginY' V='{}'/><Cell N='EndX' V='{}'/><Cell N='EndY' V='{}'/>",
            num_text(begin_x),
            num_text(line_y),
            num_text(begin_x + width * factor),
            num_text(line_y)
        )
    } else {
        String::new()
    };
    let shape = format!(
        "<Shape ID='{id}' NameU='{n}' Name='{n}' Type='{kind}' Master='{m}'><Cell N='PinX' V='{px}'/><Cell N='PinY' V='{py}'/>{ends}</Shape>",
        n = escape_attr(&name),
        m = escape_attr(master_id),
        px = num_text(pin_x),
        py = num_text(pin_y),
    );
    let insert = xml
        .rfind("</Shapes>")
        .ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
    xml.insert_str(insert, &shape);
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(page.part.clone(), xml.into_bytes());
    if let Some((rels_part, rels)) = page_master_relationship(package, page_index, master_id)? {
        extras.insert(rels_part, rels.into_bytes());
    }
    let bytes = write_package(package, extras)?;
    if factor == 1.0 {
        return Ok((bytes, id.to_string()));
    }
    // The shrunk drop: the instance is resized like any other (its sub-shapes are written scaled, Visio does not recalculate the inherited
    // cells of an instance whose Width changed), about its centre for a symbol and about its pin for any other shape.
    let dropped = Package::from_reader(std::io::Cursor::new(bytes))?;
    let (w, h) = (top_cell("Width").unwrap_or(0.0), top_cell("Height").unwrap_or(0.0));
    let anchor = if symbol {
        // x: centred on the drop point; y: the pin height stays where the drop put it
        (0.5, if h.abs() > 1e-9 { top_cell("LocPinY").unwrap_or(0.0) / h } else { 0.0 })
    } else {
        let frac = |loc: Option<f64>, size: f64| if size.abs() > 1e-9 { loc.map_or(0.5, |l| l / size) } else { 0.5 };
        (frac(top_cell("LocPinX"), w), frac(top_cell("LocPinY"), h))
    };
    let editor = crate::groupedit::GroupEditor::open(&dropped, page_index)?;
    let patches = editor.resize_member(&id.to_string(), (w * factor, h * factor), anchor, false)?;
    Ok((patch_page_cells(&dropped, page_index, &patches)?, id.to_string()))
}

/// After a shape's `Width` or `Height` changed: the cached values (`V`) of its own cells whose formula is just a
/// multiple of the width or height (`Width*0.5`, `Height*1`, `Width`) follow the new size, and so do the cells
/// that close a path with `Geometry1.X1` / `Geometry1.Y1`. Geometry, connection points and text block cells of
/// a drawn shape are written that way, and a reader (the importer, a Visio that does not recalculate on load) uses
/// the cached values. Formulas of any other form are left alone.
fn refresh_formula_values(shape: &str) -> String {
    let (Some(w), Some(h)) = (direct_cell(shape, "Width"), direct_cell(shape, "Height")) else {
        return shape.to_string();
    };
    let own_end = shape.find("<Shapes").unwrap_or(shape.len());
    let factor = |formula: &str, name: &str| -> Option<f64> {
        let f = formula.replace(' ', "");
        if f == name {
            return Some(1.0);
        }
        f.strip_prefix(&format!("{name}*")).and_then(|k| k.parse::<f64>().ok())
    };
    let mut out = String::with_capacity(shape.len());
    let mut at = 0;
    while let Some(relative) = shape[at..own_end].find("<Cell ") {
        let start = at + relative;
        let Some(close) = shape[start..own_end].find('>') else { break };
        let end = start + close + 1;
        let tag = &shape[start..end];
        out.push_str(&shape[at..start]);
        let name = attr(tag, "N").unwrap_or_default();
        let new_value = attr(tag, "F").and_then(|formula| {
            if name == "Width" || name == "Height" {
                return None;
            }
            factor(&formula, "Width").map(|k| k * w).or_else(|| factor(&formula, "Height").map(|k| k * h))
        });
        match new_value {
            Some(v) => out.push_str(&set_attr(tag, "V", &num_text(v))),
            None => out.push_str(tag),
        }
        at = end;
    }
    out.push_str(&shape[at..]);
    // The cells that close a path back to its first point.
    let first = |text: &str, cell: &str| -> Option<String> {
        let row = text.find("T='MoveTo' IX='1'").or_else(|| text.find("T=\"MoveTo\" IX=\"1\""))?;
        let tail = &text[row..];
        let row_end = tail.find("</Row>")?;
        let tag_at = tail[..row_end].find(&format!("N='{cell}'")).or_else(|| tail[..row_end].find(&format!("N=\"{cell}\"")))?;
        let tag_start = tail[..tag_at].rfind("<Cell ")?;
        let tag_end = tag_start + tail[tag_start..].find('>')? + 1;
        attr(&tail[tag_start..tag_end], "V")
    };
    let (fx, fy) = (first(&out, "X"), first(&out, "Y"));
    if fx.is_some() || fy.is_some() {
        let mut done = String::with_capacity(out.len());
        let mut at = 0;
        while let Some(relative) = out[at..].find("<Cell ") {
            let start = at + relative;
            let Some(close) = out[start..].find('>') else { break };
            let end = start + close + 1;
            let tag = &out[start..end];
            done.push_str(&out[at..start]);
            let formula = attr(tag, "F").unwrap_or_default().replace(' ', "");
            match (formula.as_str(), &fx, &fy) {
                ("Geometry1.X1", Some(v), _) | ("Geometry1.Y1", _, Some(v)) => done.push_str(&set_attr(tag, "V", v)),
                _ => done.push_str(tag),
            }
            at = end;
        }
        done.push_str(&out[at..]);
        return done;
    }
    out
}

/// The package with one part replaced (or added).
pub(crate) fn with_part(package: &Package, part: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(part.to_string(), bytes);
    write_package(package, extras)
}

/// The page's relationships part with a relationship to master `master_id`'s part added, when it has none
/// yet: Visio lists every master a page uses in the page's own relationships, and drops an instance of a
/// master that is not listed there when it loads the drawing (probe-stencil-drop2 / bisect of the stencil
/// drop). `None` when the page already relates to that master (or the master has no part).
pub(crate) fn page_master_relationship(package: &Package, page_index: usize, master_id: &str) -> Result<Option<(String, String)>> {
    const MASTER_REL: &str = "http://schemas.microsoft.com/visio/2010/relationships/master";
    let catalog = crate::MasterCatalog::load(package)?;
    let Some(master) = catalog.masters.get(master_id) else { return Ok(None) };
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let existing = package.relationships(&page.part)?;
    if existing.values().any(|rel| rel.target == master.part) {
        return Ok(None);
    }
    let (dir, file) = page.part.rsplit_once('/').unwrap_or(("", page.part.as_str()));
    let rels_part = format!("{dir}/_rels/{file}.rels");
    let mut rels = if package.contains(&rels_part) {
        package.text(&rels_part)?.to_string()
    } else {
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'></Relationships>".to_string()
    };
    let next = existing.keys().filter_map(|id| id.strip_prefix("rId").and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0) + 1;
    let target = format!("../masters/{}", master.part.rsplit('/').next().unwrap_or(""));
    let rel = format!("<Relationship Id='rId{next}' Type='{MASTER_REL}' Target='{}'/>", escape_attr(&target));
    if !insert_relationship(&mut rels, &rel) {
        return Err(Error(format!("bad page relationships part {rels_part}")));
    }
    Ok(Some((rels_part, rels)))
}

/// Add `rel` as the last child of a relationships part's root element: before `</Relationships>`, or, when
/// the part is a single empty element (`<Relationships xmlns='...'/>`, as some writers save a part with no
/// relationships), by opening that element. `false` when the text has no `Relationships` root element.
pub(crate) fn insert_relationship(rels: &mut String, rel: &str) -> bool {
    if let Some(at) = rels.rfind("</Relationships>") {
        rels.insert_str(at, rel);
        return true;
    }
    let mut from = 0;
    while let Some(found) = rels[from..].find("<Relationships") {
        let start = from + found;
        let after = start + "<Relationships".len();
        // the root element, not an element whose name only starts with "Relationships"
        if rels[after..].starts_with(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>') {
            let Some(close) = rels[after..].find('>').map(|i| after + i) else { return false };
            if !rels[..close].ends_with('/') {
                return false;
            }
            rels.replace_range(close - 1..=close, &format!(">{rel}</Relationships>"));
            return true;
        }
        from = after;
    }
    false
}

/// Set a cell directly under geometry section `index` of the shape; the section is created (before
/// the text and the members, after the shape's cells and other sections) when the shape has none.
fn set_geometry_flag(shape: &str, index: &str, cell: &str, value: &str) -> Result<String> {
    if find_section_range(shape, "Geometry", index).is_some() {
        return patch_section_cell(
            shape,
            &SectionCellPatch {
                section: "Geometry".to_string(),
                section_index: index.to_string(),
                row: None,
                cell: cell.to_string(),
                value: value.to_string(),
            },
        );
    }
    let insert = ["<Text", "<Shapes"]
        .iter()
        .filter_map(|needle| shape.find(needle))
        .min()
        .unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
    let section = format!(
        "<Section N='Geometry' IX='{}'><Cell N='{}' V='{}'/></Section>",
        escape_attr(index),
        escape_attr(cell),
        escape_attr(value)
    );
    Ok(format!("{}{}{}", &shape[..insert], section, &shape[insert..]))
}

/// The value of cell `name` in a row's (or shape's) XML text.
fn cell_value_in(text: &str, name: &str) -> Option<String> {
    let mut at = 0;
    while let Some(relative) = text[at..].find("<Cell") {
        let start = at + relative;
        let end = text[start..].find('>').map_or(text.len(), |n| start + n + 1);
        let tag = &text[start..end];
        if attr(tag, "N").as_deref() == Some(name) {
            return attr(tag, "V");
        }
        at = end;
    }
    None
}

/// The new `Style` value: the current one with the bits of `style.0` set and those of `style.1` cleared.
fn restyled(current: Option<String>, style: (u32, u32)) -> String {
    let now = current.and_then(|v| v.trim().parse::<f64>().ok()).map_or(0, |v| v.round() as u32);
    ((now & !style.1) | style.0).to_string()
}

/// Give every `Character` row of the shape the cells `cells` (`(name, value)`: Size in inches, Font
/// name, Color `#rrggbb`) and apply `style` = (bits to set, bits to clear) to its `Style` cell (1 bold,
/// 2 italic, 4 underline). A shape without a `Character` section of its own gets one with a single
/// row, the way Visio writes a shape whose text was formatted.
fn set_character_cells(shape: &str, cells: &[(String, String)], style: Option<(u32, u32)>) -> String {
    let cell_xml = |name: &str, value: &str| format!("<Cell N='{}' V='{}'/>", escape_attr(name), escape_attr(value));
    let Some((start, end)) = find_section_range(shape, "Character", "") else {
        let insert = ["<Text", "<Shapes"]
            .iter()
            .filter_map(|needle| shape.find(needle))
            .min()
            .unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
        let mut row = String::new();
        for (name, value) in cells {
            row.push_str(&cell_xml(name, value));
        }
        if let Some(style) = style {
            row.push_str(&cell_xml("Style", &restyled(None, style)));
        }
        let section = format!("<Section N='Character'><Row IX='0'>{row}</Row></Section>");
        return format!("{}{}{}", &shape[..insert], section, &shape[insert..]);
    };
    // The row keys first: patching changes the lengths.
    let mut keys = Vec::new();
    let mut at = start;
    while let Some(relative) = shape[at..end].find("<Row") {
        let row_start = at + relative;
        let close = shape[row_start..end].find('>').map_or(end, |n| row_start + n + 1);
        if let Some(key) = attr(&shape[row_start..close], "IX") {
            keys.push(key);
        }
        at = close;
    }
    if keys.is_empty() {
        let insert = shape[start..end].rfind("</Section>").map_or(end, |n| start + n);
        let mut row = String::new();
        for (name, value) in cells {
            row.push_str(&cell_xml(name, value));
        }
        if let Some(style) = style {
            row.push_str(&cell_xml("Style", &restyled(None, style)));
        }
        return format!("{}<Row IX='0'>{row}</Row>{}", &shape[..insert], &shape[insert..]);
    }
    let mut out = shape.to_string();
    for key in keys {
        let mut wanted: Vec<(String, String)> = cells.to_vec();
        if let Some(style) = style {
            let Some((section_start, section_end)) = find_section_range(&out, "Character", "") else { break };
            let Some((row_start, row_end)) = find_row_range(&out, section_start, section_end, &key) else { continue };
            wanted.push(("Style".to_string(), restyled(cell_value_in(&out[row_start..row_end], "Style"), style)));
        }
        for (name, value) in wanted {
            let Some((section_start, section_end)) = find_section_range(&out, "Character", "") else { break };
            let Some((row_start, row_end)) = find_row_range(&out, section_start, section_end, &key) else { break };
            let row = &out[row_start..row_end];
            let has = row.contains(&format!("N='{name}'")) || row.contains(&format!("N=\"{name}\""));
            out = if has {
                patch_cell_in_range(&out, row_start, row_end, &name, &value, "V")
            } else {
                // A missing cell goes at the end of the row (the range patcher would put it before the row).
                let at = row_start + row.rfind("</Row>").unwrap_or(row.len());
                format!("{}{}{}", &out[..at], cell_xml(&name, &value), &out[at..])
            };
        }
    }
    out
}

/// Make sure the document lists the fonts `names` (`FaceNames` in `visio/document.xml`), the way
/// Visio adds a face when a shape uses it for the first time; returns the new package bytes (the
/// unchanged package when every font is listed already).
pub fn ensure_face_names(package: &Package, names: &[String]) -> Result<Vec<u8>> {
    const PART: &str = "visio/document.xml";
    let mut xml = package.text(PART)?.to_string();
    let mut add = String::new();
    for name in names {
        let escaped = escape_attr(name);
        let listed = |text: &str| text.contains(&format!("NameU='{escaped}'")) || text.contains(&format!("NameU=\"{escaped}\""));
        if !listed(&xml) && !listed(&add) {
            add.push_str(&format!("<FaceName NameU='{escaped}'/>"));
        }
    }
    if add.is_empty() {
        return write_package(package, BTreeMap::new());
    }
    if let Some(at) = xml.find("</FaceNames>") {
        xml.insert_str(at, &add);
    } else if let Some(at) = xml.find("<FaceNames/>") {
        xml.replace_range(at..at + "<FaceNames/>".len(), &format!("<FaceNames>{add}</FaceNames>"));
    } else {
        let at = xml
            .find("<StyleSheets")
            .or_else(|| xml.find("</Colors>").map(|n| n + "</Colors>".len()))
            .ok_or_else(|| Error("document.xml has no place for the font list".to_string()))?;
        xml.insert_str(at, &format!("<FaceNames>{add}</FaceNames>"));
    }
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(PART.to_string(), xml.into_bytes());
    write_package(package, extras)
}

/// One piece of a shape's `<Text>` element.
#[derive(Debug, Clone, PartialEq)]
enum TextTok {
    /// A zero-length marker (`<pp/>` at the start, `<cp/>`, `<tp/>`).
    Tag(String),
    /// Character data (entities decoded).
    Chars(String),
    /// A `<pp/>` that starts a later paragraph without a newline character in the data (the way
    /// Lucidchart writes them): one character, "\n", in the plain text.
    Break(String),
}

fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name.strip_prefix('#').and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                }).and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn tokenize_text(inner: &str) -> Result<Vec<TextTok>> {
    let mut toks = Vec::new();
    let mut i = 0;
    let (mut seen_chars, mut last_newline) = (false, false);
    while i < inner.len() {
        if inner[i..].starts_with('<') {
            let end = inner[i..]
                .find('>')
                .ok_or_else(|| Error("unterminated tag in shape text".to_string()))?
                + i
                + 1;
            let tag = &inner[i..end];
            if tag.starts_with("<fld") {
                return Err(Error("this text contains fields, which cannot be edited here".to_string()));
            }
            if tag.starts_with("<pp") && seen_chars && !last_newline {
                toks.push(TextTok::Break(tag.to_string()));
                last_newline = true;
            } else {
                toks.push(TextTok::Tag(tag.to_string()));
            }
            i = end;
        } else {
            let end = inner[i..].find('<').map_or(inner.len(), |n| i + n);
            let chars = unescape_text(&inner[i..end]);
            if !chars.is_empty() {
                seen_chars = true;
                last_newline = chars.ends_with('\n');
            }
            toks.push(TextTok::Chars(chars));
            i = end;
        }
    }
    Ok(toks)
}

fn plain_of(toks: &[TextTok]) -> String {
    toks.iter()
        .map(|t| match t {
            TextTok::Chars(s) => s.as_str(),
            TextTok::Break(_) => "\n",
            TextTok::Tag(_) => "",
        })
        .collect()
}

/// The tokens after replacing the plain text by `new`: the common start and end stay untouched and
/// the difference goes into the run where the change begins.
fn edit_tokens(toks: Vec<TextTok>, new: &str) -> Vec<TextTok> {
    let old: Vec<char> = plain_of(&toks).chars().collect();
    let newc: Vec<char> = new.chars().collect();
    let mut p = 0;
    while p < old.len() && p < newc.len() && old[p] == newc[p] {
        p += 1;
    }
    let mut s = 0;
    while s < old.len() - p && s < newc.len() - p && old[old.len() - 1 - s] == newc[newc.len() - 1 - s] {
        s += 1;
    }
    let q = old.len() - s;
    let ins: String = newc[p..newc.len() - s].iter().collect();
    if p == q && ins.is_empty() {
        return toks;
    }
    let (mut out, mut pos, mut placed) = (Vec::new(), 0usize, false);
    for tok in toks {
        match tok {
            TextTok::Tag(x) => out.push(TextTok::Tag(x)),
            TextTok::Break(x) => {
                let (a, b) = (pos, pos + 1);
                pos = b;
                if !(a >= p && b <= q) {
                    out.push(TextTok::Break(x));
                }
            }
            TextTok::Chars(text) => {
                let cs: Vec<char> = text.chars().collect();
                let (a, b) = (pos, pos + cs.len());
                pos = b;
                let here = !placed && ((a < p && p <= b) || a == p);
                let mut res = String::new();
                for (k, c) in cs.iter().enumerate() {
                    let g = a + k;
                    if here && g == p {
                        res.push_str(&ins);
                        placed = true;
                    }
                    if g < p || g >= q {
                        res.push(*c);
                    }
                }
                if here && !placed {
                    res.push_str(&ins);
                    placed = true;
                }
                out.push(TextTok::Chars(res));
            }
        }
    }
    if !placed && !ins.is_empty() {
        out.push(TextTok::Chars(ins));
    }
    out
}

fn text_of_tokens(toks: &[TextTok]) -> String {
    toks.iter()
        .map(|t| match t {
            TextTok::Tag(x) | TextTok::Break(x) => x.clone(),
            TextTok::Chars(s) => escape_text(s),
        })
        .collect()
}

/// The inner XML of the shape's own `<Text>` element (not a member's), if it has one.
fn own_text_range(shape: &str) -> Option<(usize, usize)> {
    let direct_end = shape.find("<Shapes").unwrap_or(shape.len());
    let start = shape[..direct_end].find("<Text>")? + "<Text>".len();
    let end = start + shape[start..direct_end].find("</Text>")?;
    Some((start, end))
}

fn patch_shape_text_edit(shape: &str, new: &str) -> Result<String> {
    let Some((start, end)) = own_text_range(shape) else {
        return Ok(patch_shape_text(shape, new));
    };
    let toks = tokenize_text(&shape[start..end])?;
    let edited = edit_tokens(toks, new);
    Ok(format!("{}{}{}", &shape[..start], text_of_tokens(&edited), &shape[end..]))
}

/// The plain text of shape `id`'s own `<Text>` (paragraph breaks as "\n"), or `None` when it has
/// none. Fails when the text contains fields.
pub fn own_text(package: &Package, page_index: usize, id: &str) -> Result<Option<String>> {
    let pages = package.pages()?;
    let page = pages
        .get(page_index)
        .ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let xml = package.text(&page.part)?;
    let (start, end) = find_shape_range(xml, id).ok_or_else(|| Error(format!("shape {id} not found")))?;
    let shape = &xml[start..end];
    let Some((a, b)) = own_text_range(shape) else {
        return Ok(None);
    };
    Ok(Some(plain_of(&tokenize_text(&shape[a..b])?)))
}

fn patch_shape_data(shape: &str, name: &str, value: &str) -> String {
    let mut result = shape.to_string();
    if let Some((start, end)) = find_section_range(&result, "Property", "") {
        if let Some((row_start, row_end)) = find_row_range(&result, start, end, name) {
            return patch_cell_in_range(&result, row_start, row_end, "Value", value, "V");
        }
        let row = property_row(name, value);
        let insert = end - "</Section>".len();
        result.insert_str(insert, &row);
        return result;
    }
    let section = format!(
        "<Section N='Property'>{}</Section>",
        property_row(name, value)
    );
    let insert = result.rfind("</Shape>").unwrap_or(result.len());
    result.insert_str(insert, &section);
    result
}

/// Valid Shape Data row names: a letter or underscore, then letters, digits and underscores (what Visio accepts in `Prop.Name`).
pub fn valid_data_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && name.len() <= 64
}

/// A duration typed as `<number> <unit>` (`w`, `d`, `h`, `min` or `m`, `s`; an `e` in front as in Visio's own unit names is allowed):
/// the number of days and Visio's unit code. A bare number counts as days; nothing is zero days.
fn parse_duration(text: &str) -> Result<(f64, &'static str)> {
    let text = text.trim();
    if text.is_empty() {
        return Ok((0.0, "ED"));
    }
    let split = text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len());
    let number: f64 = text[..split].trim().parse().map_err(|_| Error(format!("'{text}' is not a duration (a number and a unit: 2 d, 3 h, 90 min, 30 s, 1 w)")))?;
    if !number.is_finite() {
        return Err(Error("a duration needs a number".to_string()));
    }
    let unit = text[split..].trim().to_ascii_lowercase();
    let unit = unit.strip_prefix('e').filter(|rest| !rest.is_empty() && *rest != "e").unwrap_or(&unit);
    let (per_day, code) = match unit {
        "" | "d" | "day" | "days" => (1.0, "ED"),
        "w" | "week" | "weeks" => (7.0, "EW"),
        "h" | "hr" | "hour" | "hours" => (1.0 / 24.0, "EH"),
        "m" | "min" | "mins" | "minute" | "minutes" => (1.0 / 1440.0, "EM"),
        "s" | "sec" | "secs" | "second" | "seconds" => (1.0 / 86400.0, "ES"),
        _ => return Err(Error(format!("'{}' is not a duration unit (w, d, h, min, s)", &text[split..]))),
    };
    Ok((number * per_day, code))
}

fn data_row_xml(edit: &DataEdit) -> Result<String> {
    if !valid_data_name(&edit.name) {
        return Err(Error(format!("'{}' is not a valid data row name (letters, digits and underscore, not starting with a digit)", edit.name)));
    }
    if !matches!(edit.kind, 0..=7) {
        return Err(Error(format!("data type {} is not supported", edit.kind)));
    }
    let value = edit.value.trim();
    let (value_cell, format) = match edit.kind {
        6 => {
            let (days, unit) = parse_duration(value)?;
            (format!("<Cell N='Value' V='{}' U='{unit}'/>", num_text(days)), String::new())
        }
        2 | 7 => {
            let number: f64 = if value.is_empty() { 0.0 } else { value.parse().map_err(|_| Error(format!("'{value}' is not a number")))? };
            if !number.is_finite() {
                return Err(Error("a number is needed".to_string()));
            }
            (format!("<Cell N='Value' V='{}'/>", num_text(number)), edit.format.clone())
        }
        3 => {
            let word = value.to_ascii_lowercase();
            if !matches!(word.as_str(), "" | "0" | "1" | "true" | "false" | "yes" | "no" | "on" | "off") {
                return Err(Error(format!("'{value}' is not a yes / no value (yes, no, true, false, 1, 0)")));
            }
            let on = matches!(word.as_str(), "1" | "true" | "yes" | "on");
            (format!("<Cell N='Value' V='{}' U='BOOL'/>", u8::from(on)), String::new())
        }
        // a fixed list (Visio's Shape Data window offers only its entries); a variable list (4) takes any text
        1 if !value.is_empty() && !edit.format.split(';').any(|item| item.trim() == value) => {
            return Err(Error(format!("'{value}' is not one of the list entries ({})", edit.format)));
        }
        5 => {
            if value.is_empty() {
                ("<Cell N='Value' V='0' U='DATE'/>".to_string(), edit.format.clone())
            } else {
                let day = value.get(..10).unwrap_or(value);
                let parts: Vec<&str> = day.split('-').collect();
                let ok = parts.len() == 3 && parts[0].len() == 4 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
                if !ok {
                    return Err(Error(format!("'{value}' is not a date (YYYY-MM-DD)")));
                }
                (format!("<Cell N='Value' V='{day}T00:00:00' U='DATE' F='DATETIME(&quot;{day}&quot;)'/>"), edit.format.clone())
            }
        }
        _ => (format!("<Cell N='Value' V='{}' U='STR'/>", escape_attr(&edit.value)), edit.format.clone()),
    };
    Ok(format!(
        "<Row N='{}'>{}<Cell N='Prompt' V='{}'/><Cell N='Label' V='{}'/><Cell N='Format' V='{}'/><Cell N='SortKey' V='{}'/><Cell N='Type' V='{}'/><Cell N='Invisible' V='{}'/><Cell N='Verify' V='0'/><Cell N='DataLinked' V='0'/><Cell N='LangID' V='en-US'/><Cell N='Calendar' V='0'/></Row>",
        escape_attr(&edit.name),
        value_cell,
        escape_attr(&edit.prompt),
        escape_attr(&edit.label),
        escape_attr(&format),
        escape_attr(&edit.sort_key),
        edit.kind,
        u8::from(edit.invisible)
    ))
}

/// Write the row of `edit` (replacing a row of the same name, else appended) or remove it. Removing needs the row in the shape's
/// own `Property` section: a row only the master has would come back.
fn apply_data_edit(shape: &str, edit: &DataEdit) -> Result<String> {
    let mut result = shape.to_string();
    let section = find_section_range(&result, "Property", "");
    if edit.remove && edit.inherited {
        // A row the master supplies cannot be taken away by dropping it: Visio (probed) writes `<Row N='X' Del='1'/>` in the
        // instance's own Property section, in place of an override row the instance had.
        let marker = format!("<Row N='{}' Del='1'/>", escape_attr(&edit.name));
        match section {
            Some((start, end)) => match find_row_range(&result, start, end, &edit.name) {
                Some((row_start, row_end)) => result.replace_range(row_start..row_end, &marker),
                None => result.insert_str(end - "</Section>".len(), &marker),
            },
            None => {
                let insert = result.rfind("</Shape>").unwrap_or(result.len());
                result.insert_str(insert, &format!("<Section N='Property'>{marker}</Section>"));
            }
        }
        return Ok(result);
    }
    if edit.remove {
        let (start, end) = section.ok_or_else(|| Error(format!("the shape has no data row {} of its own", edit.name)))?;
        let (row_start, row_end) = find_row_range(&result, start, end, &edit.name).ok_or_else(|| Error(format!("the shape has no data row {} of its own (rows of the master cannot be removed)", edit.name)))?;
        result.replace_range(row_start..row_end, "");
        // an empty section is dropped
        if let Some((s, e)) = find_section_range(&result, "Property", "") {
            if !result[s..e].contains("<Row") {
                result.replace_range(s..e, "");
            }
        }
        return Ok(result);
    }
    let mut edit = edit.clone();
    // replacing a row keeps the sort key it had when none is given
    if edit.sort_key.is_empty() {
        if let Some((start, end)) = section {
            if let Some((row_start, row_end)) = find_row_range(&result, start, end, &edit.name) {
                let old = &result[row_start..row_end];
                let key = "<Cell N='SortKey' V='";
                if let Some(at) = old.find(key) {
                    if let Some(close) = old[at + key.len()..].find(char::from(39)) {
                        edit.sort_key = old[at + key.len()..at + key.len() + close].to_string();
                    }
                }
            }
        }
    }
    let row = data_row_xml(&edit)?;
    match section {
        Some((start, end)) => match find_row_range(&result, start, end, &edit.name) {
            Some((row_start, row_end)) => result.replace_range(row_start..row_end, &row),
            None => result.insert_str(end - "</Section>".len(), &row),
        },
        None => {
            let insert = result.rfind("</Shape>").unwrap_or(result.len());
            result.insert_str(insert, &format!("<Section N='Property'>{row}</Section>"));
        }
    }
    Ok(result)
}

fn property_row(name: &str, value: &str) -> String {
    format!(
        "<Row N='{}'><Cell N='Value' V='{}' U='STR'/><Cell N='Prompt' V=''/><Cell N='Label' V=''/><Cell N='Format' V=''/><Cell N='SortKey' V=''/><Cell N='Type' V='0'/><Cell N='Invisible' V='0'/><Cell N='Verify' V='0'/><Cell N='DataLinked' V='0'/><Cell N='LangID' V='en-US'/><Cell N='Calendar' V='0'/></Row>",
        escape_attr(name),
        escape_attr(value)
    )
}

pub(crate) fn attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let pattern = format!(" {name}={quote}");
        if let Some(start) = tag.find(&pattern) {
            let value_start = start + pattern.len();
            let value_end = tag[value_start..].find(quote)? + value_start;
            return Some(tag[value_start..value_end].to_string());
        }
    }
    None
}

pub(crate) fn set_attr(tag: &str, name: &str, value: &str) -> String {
    let escaped = escape_attr(value);
    for quote in ['\'', '"'] {
        let pattern = format!(" {name}={quote}");
        if let Some(start) = tag.find(&pattern) {
            let value_start = start + pattern.len();
            if let Some(relative) = tag[value_start..].find(quote) {
                let value_end = value_start + relative;
                return format!(
                    "{}{}{}{}",
                    &tag[..value_start],
                    escaped,
                    &tag[value_end..value_end + 1],
                    &tag[value_end + 1..]
                );
            }
        }
    }
    let insert = tag
        .rfind("/>")
        .or_else(|| tag.rfind('>'))
        .unwrap_or(tag.len());
    format!(
        "{} {}='{}'{}",
        &tag[..insert],
        name,
        escaped,
        &tag[insert..]
    )
}

fn remove_attr(tag: &str, name: &str) -> String {
    for quote in ['\'', '"'] {
        let pattern = format!(" {name}={quote}");
        if let Some(start) = tag.find(&pattern) {
            let value_start = start + pattern.len();
            if let Some(relative) = tag[value_start..].find(quote) {
                let end = value_start + relative + 1;
                return format!("{}{}", &tag[..start], &tag[end..]);
            }
        }
    }
    tag.to_string()
}

pub(crate) fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}


// ---- formatting a range of a shape's text -----------------------------------------------------------------------------

/// A format change for the characters `[start, end)` of a shape's plain text (counted in characters, a paragraph break
/// is one). `char_cells` / `char_style` change the Character row of the runs the range covers (new rows and `<cp>`
/// markers split the runs at the ends of the range, the way Visio does when a part of a text is formatted);
/// `para_cells` change the Paragraph rows of every paragraph the range touches (a caret, `start == end`, touches one).
#[derive(Debug, Clone, Default)]
pub struct TextRangeFormat {
    pub start: usize,
    pub end: usize,
    pub char_cells: Vec<(String, String)>,
    pub char_style: Option<(u32, u32)>,
    pub para_cells: Vec<(String, String)>,
    /// The shape's effective Size / Font / Color (what its first run shows, e.g. inherited from its master). Run rows
    /// the range format creates get them as explicit cells, like Visio's own rows: a new row has no master row to
    /// inherit from, and a row without them is read as size 0.
    pub base_cells: Vec<(String, String)>,
}

fn marker_ix(tag: &str) -> String {
    attr(tag, "IX").unwrap_or_else(|| "0".to_string())
}

/// The largest `IX` of the rows of section `name` and of the `<tag IX>` markers of the text, plus one.
fn next_row_ix(shape: &str, section: &str, toks: &[TextTok], tag: &str) -> u64 {
    let mut max = 0u64;
    if let Some((s, e)) = find_section_range(shape, section, "") {
        let mut at = s;
        while let Some(relative) = shape[at..e].find("<Row") {
            let row_start = at + relative;
            let close = shape[row_start..e].find('>').map_or(e, |n| row_start + n + 1);
            if let Some(ix) = attr(&shape[row_start..close], "IX").and_then(|v| v.parse::<u64>().ok()) {
                max = max.max(ix);
            }
            at = close;
        }
    }
    for tok in toks {
        if let TextTok::Tag(t) | TextTok::Break(t) = tok {
            if t.starts_with(tag) {
                if let Some(ix) = attr(t, "IX").and_then(|v| v.parse::<u64>().ok()) {
                    max = max.max(ix);
                }
            }
        }
    }
    max + 1
}

/// Set cells of a standalone `<Row ...>...</Row>` text (a missing cell is added at the end of the row).
fn row_with_cells(row: &str, cells: &[(String, String)]) -> String {
    let mut out = row.to_string();
    for (name, value) in cells {
        let has = out.contains(&format!("N='{name}'")) || out.contains(&format!("N=\"{name}\""));
        out = if has {
            let len = out.len();
            patch_cell_in_range(&out, 0, len, name, value, "V")
        } else {
            let at = out.rfind("</Row>").unwrap_or(out.len());
            format!("{}<Cell N='{}' V='{}'/>{}", &out[..at], escape_attr(name), escape_attr(value), &out[at..])
        };
    }
    out
}

/// New rows `(orig, new)` of section `section` (a clone of the row `orig` of the shape, if it has one, with the
/// IX `new` and the cells applied); the section is created before the shape's `<Text>` when it is absent.
fn add_rows(shape: &str, section: &str, pairs: &[(String, String)], cells: &[(String, String)], style: Option<(u32, u32)>) -> String {
    if pairs.is_empty() {
        return shape.to_string();
    }
    let mut out = shape.to_string();
    if find_section_range(&out, section, "").is_none() {
        let insert = ["<Text", "<Shapes"].iter().filter_map(|needle| out.find(needle)).min().unwrap_or_else(|| out.rfind("</Shape>").unwrap_or(out.len()));
        out.insert_str(insert, &format!("<Section N='{section}'></Section>"));
    }
    let mut rows = String::new();
    for (orig, new) in pairs {
        let (s, e) = find_section_range(&out, section, "").unwrap_or((0, 0));
        let base = find_row_range(&out, s, e, orig).map(|(a, b)| out[a..b].to_string());
        let mut row = match base {
            Some(base) => {
                let close = base.find('>').unwrap_or(0);
                let tag = &base[..close + 1];
                let new_tag = set_attr(tag, "IX", new);
                format!("{new_tag}{}", &base[close + 1..])
            }
            None => format!("<Row IX='{}'></Row>", escape_attr(new)),
        };
        // a self-closing source row (<Row IX='0'/>) becomes an ordinary one
        if row.trim_end().ends_with("/>") && !row.contains("</Row>") {
            row = format!("{}></Row>", row.trim_end().trim_end_matches("/>").trim_end());
        }
        let mut wanted = cells.to_vec();
        if let Some(style) = style {
            wanted.push(("Style".to_string(), restyled(cell_value_in(&row, "Style"), style)));
        }
        rows.push_str(&row_with_cells(&row, &wanted));
    }
    let (s, e) = find_section_range(&out, section, "").unwrap_or((0, 0));
    let close = s + out[s..e].rfind("</Section>").unwrap_or(e - s);
    out.insert_str(close, &rows);
    out
}


/// Make every Character row say what it means for the cells `names`: a row without the cell gets the value row 0 has
/// (what a reader that lets a run inherit row 0 would use), or the neutral default. Visio takes a missing cell from the
/// style sheet, not from row 0, so after a range was formatted the other runs must not depend on row 0 for these cells.
fn pin_character_cells(shape: &str, names: &[String], row0: Option<&str>) -> String {
    let Some((s, e)) = find_section_range(shape, "Character", "") else { return shape.to_string() };
    let default_of = |name: &str| match name {
        "Style" | "Case" | "Strikethru" | "DblUnderline" | "Overline" | "Letterspace" | "DoubleStrikethrough" => Some("0".to_string()),
        _ => None,
    };
    let mut rows = String::new();
    for (ix, row) in rows_of(shape, "Character") {
        let mut add: Vec<(String, String)> = Vec::new();
        for name in names {
            let has = row.contains(&format!("N='{name}'")) || row.contains(&format!("N=\"{name}\""));
            if !has {
                if let Some(value) = row0.and_then(|r0| cell_value_in(r0, name)).or_else(|| default_of(name)) {
                    add.push((name.clone(), value));
                }
            }
        }
        let clone = row_clone(Some(&row), &ix);
        rows.push_str(&if add.is_empty() { clone } else { row_with_cells(&clone, &add) });
    }
    let mut out = shape.to_string();
    out.replace_range(s..e, &format!("<Section N='Character'>{rows}</Section>"));
    out
}

/// The caller counts a CR LF pair once (as a text box does); the runs count both characters.
fn ui_to_run_pos(plain: &[char], ui: usize) -> usize {
    let (mut seen, mut i) = (0usize, 0usize);
    while i < plain.len() && seen < ui {
        i += if plain[i] == '\r' && plain.get(i + 1) == Some(&'\n') { 2 } else { 1 };
        seen += 1;
    }
    if seen < ui { plain.len() } else { i }
}

/// The name of the face at position `index` (0-based, the number a `Font` cell holds) in a document's `FaceNames`.
fn face_name_by_index(document_xml: &str, index: usize) -> Option<String> {
    let mut at = 0;
    let mut count = 0;
    while let Some(relative) = document_xml[at..].find("<FaceName ") {
        let start = at + relative;
        let end = document_xml[start..].find('>').map_or(document_xml.len(), |n| start + n + 1);
        if count == index {
            let tag = &document_xml[start..end];
            let key = " NameU='";
            return tag.find(key).and_then(|k| tag[k + key.len()..].find(char::from(39)).map(|e| tag[k + key.len()..k + key.len() + e].to_string()));
        }
        count += 1;
        at = end;
    }
    None
}

const STATE_CHAR_CELLS: [&str; 9] = ["Style", "Case", "Strikethru", "Letterspace", "Size", "Font", "Color", "DblUnderline", "Overline"];
const STATE_PARA_CELLS: [&str; 8] = ["HorzAlign", "Bullet", "BulletStr", "IndLeft", "IndFirst", "SpLine", "SpBefore", "SpAfter"];

/// The formatting of a range of the text of shape `id` (offsets as a text box counts them; a caret, `start == end`,
/// reads the character before it and the paragraph it is in), as JSON
/// `{"char": {cell: value | "mixed"}, "para": {...}}`. A cell is left out when no row of the characters in the
/// range sets it (the value then comes from the shape's style). Read-only.
pub fn text_state(package: &Package, page_index: usize, id: &str, start: usize, end: usize) -> Result<String> {
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let xml = package.text(&page.part)?;
    let (a, b) = find_shape_range(xml, id).ok_or_else(|| Error(format!("shape {id} not found")))?;
    let shape = &xml[a..b];
    let Some((ts, te)) = own_text_range(shape) else {
        return Err(Error("the shape has no text".to_string()));
    };
    let toks = tokenize_text(&shape[ts..te])?;
    let plain: Vec<char> = plain_of(&toks).chars().collect();
    // (cp IX, pp IX, is a paragraph break) of every position
    let mut at: Vec<(String, String, bool)> = Vec::new();
    let (mut cp, mut pp) = ("0".to_string(), "0".to_string());
    for tok in &toks {
        match tok {
            TextTok::Tag(t) if t.starts_with("<cp") => cp = marker_ix(t),
            TextTok::Tag(t) if t.starts_with("<pp") => pp = marker_ix(t),
            TextTok::Break(t) => {
                at.push((cp.clone(), pp.clone(), true));
                pp = marker_ix(t);
            }
            TextTok::Chars(text) => {
                for _ in text.chars() {
                    at.push((cp.clone(), pp.clone(), false));
                }
            }
            _ => {}
        }
    }
    let last = at.len().saturating_sub(1);
    let (s0, e0) = (ui_to_run_pos(&plain, start).min(plain.len()), ui_to_run_pos(&plain, end).min(plain.len()));
    let (char_range, para_range) = if e0 > s0 {
        (s0..e0, s0..e0)
    } else {
        let before = s0.saturating_sub(1).min(last);
        (before..before + 1, s0.min(last)..s0.min(last) + 1)
    };
    let collect = |section: &str, names: &[&str], positions: std::ops::Range<usize>, pick: &dyn Fn(&(String, String, bool)) -> Option<String>| {
        let rows = rows_of(shape, section);
        let mut out = serde_json::Map::new();
        for name in names {
            let mut seen: Vec<Option<String>> = Vec::new();
            let list: Vec<&(String, String, bool)> = if at.is_empty() { Vec::new() } else { positions.clone().filter_map(|i| at.get(i)).collect() };
            let fallback = ("0".to_string(), "0".to_string(), false);
            let list = if list.is_empty() { vec![&fallback] } else { list };
            for position in list {
                if position.2 && section == "Character" {
                    continue;
                }
                let ix = pick(position).unwrap_or_else(|| "0".to_string());
                let value = rows.iter().find(|(i, _)| *i == ix).and_then(|(_, row)| cell_value_in(row, name));
                if !seen.contains(&value) {
                    seen.push(value);
                }
            }
            match seen.len() {
                0 => {}
                1 => {
                    if let Some(value) = &seen[0] {
                        out.insert(name.to_string(), serde_json::Value::String(value.clone()));
                    }
                }
                _ => {
                    out.insert(name.to_string(), serde_json::Value::String("mixed".to_string()));
                }
            }
        }
        serde_json::Value::Object(out)
    };
    let chars = collect("Character", &STATE_CHAR_CELLS, char_range, &|p| Some(p.0.clone()));
    let paras = collect("Paragraph", &STATE_PARA_CELLS, para_range, &|p| Some(p.1.clone()));
    let mut chars = chars;
    // The `Font` cell of a run is an index into the document's `FaceNames` (or a font name): `FontName` is the name.
    if let Some(font) = chars.get("Font").and_then(|v| v.as_str()).map(str::to_string) {
        let name = if font == "mixed" || font.parse::<u32>().is_err() {
            Some(font)
        } else {
            package.text("visio/document.xml").ok().and_then(|document| face_name_by_index(document, font.parse::<usize>().unwrap_or(usize::MAX)))
        };
        if let (Some(name), Some(map)) = (name, chars.as_object_mut()) {
            map.insert("FontName".to_string(), serde_json::Value::String(name));
        }
    }
    Ok(serde_json::json!({ "char": chars, "para": paras }).to_string())
}

fn format_text_range(shape: &str, r: &TextRangeFormat) -> Result<String> {
    let Some((ts, te)) = own_text_range(shape) else {
        return Err(Error("the shape has no text to format".to_string()));
    };
    let mut toks = tokenize_text(&shape[ts..te])?;
    let plain: Vec<char> = plain_of(&toks).chars().collect();
    let total = plain.len();
    let (start, end) = (ui_to_run_pos(&plain, r.start).min(total), ui_to_run_pos(&plain, r.end).min(total));
    let end = end.max(start);
    let mut char_pairs: Vec<(String, String)> = Vec::new();
    let mut para_pairs: Vec<(String, String)> = Vec::new();
    let mut next_cp = next_row_ix(shape, "Character", &toks, "<cp");
    let mut next_pp = next_row_ix(shape, "Paragraph", &toks, "<pp");

    // --- characters: split the runs at the ends of the range
    if (!r.char_cells.is_empty() || r.char_style.is_some()) && end > start {
        let map = |orig: &str, pairs: &mut Vec<(String, String)>, next: &mut u64| -> String {
            if let Some((_, new)) = pairs.iter().find(|(o, _)| o == orig) {
                return new.clone();
            }
            let new = next.to_string();
            *next += 1;
            pairs.push((orig.to_string(), new.clone()));
            new
        };
        let mut out: Vec<TextTok> = Vec::new();
        let (mut pos, mut cur, mut emitted) = (0usize, "0".to_string(), "0".to_string());
        let cp_tag = |ix: &str| TextTok::Tag(format!("<cp IX='{}'/>", escape_attr(ix)));
        for tok in toks {
            match tok {
                TextTok::Tag(t) if t.starts_with("<cp") => {
                    cur = marker_ix(&t);
                    if pos >= start && pos < end {
                        let new = map(&cur, &mut char_pairs, &mut next_cp);
                        out.push(cp_tag(&new));
                        emitted = new;
                    } else {
                        emitted = cur.clone();
                        out.push(TextTok::Tag(t));
                    }
                }
                TextTok::Tag(t) => out.push(TextTok::Tag(t)),
                TextTok::Break(t) => {
                    pos += 1;
                    out.push(TextTok::Break(t));
                }
                TextTok::Chars(text) => {
                    let chars: Vec<char> = text.chars().collect();
                    let mut i = 0usize;
                    while i < chars.len() {
                        let here = pos + i;
                        let inside = here >= start && here < end;
                        // the piece up to the next boundary
                        let limit = if here < start { start - here } else if here < end { end - here } else { chars.len() - i };
                        let take = limit.min(chars.len() - i);
                        let want = if inside { map(&cur, &mut char_pairs, &mut next_cp) } else { cur.clone() };
                        if emitted != want {
                            if matches!(out.last(), Some(TextTok::Tag(t)) if t.starts_with("<cp")) {
                                out.pop();
                            }
                            out.push(cp_tag(&want));
                            emitted = want;
                        }
                        out.push(TextTok::Chars(chars[i..i + take].iter().collect()));
                        i += take;
                    }
                    pos += chars.len();
                }
            }
        }
        toks = out;
    }

    // --- paragraphs: every paragraph the range touches gets its own `<pp>`
    if !r.para_cells.is_empty() {
        // paragraph starts: (start position, marker token index or None, pp IX in effect)
        struct Para {
            start: usize,
            marker: Option<usize>,
            orig: String,
        }
        let mut paras = vec![Para { start: 0, marker: None, orig: "0".to_string() }];
        let (mut pos, mut cur_pp) = (0usize, "0".to_string());
        let mut chars_in_para = false;
        for (i, tok) in toks.iter().enumerate() {
            match tok {
                TextTok::Tag(t) if t.starts_with("<pp") => {
                    cur_pp = marker_ix(t);
                    let last = paras.last_mut().unwrap();
                    if last.start == pos && !chars_in_para {
                        last.marker = Some(i);
                        last.orig = cur_pp.clone();
                    }
                }
                TextTok::Break(t) => {
                    cur_pp = marker_ix(t);
                    pos += 1;
                    paras.push(Para { start: pos, marker: Some(i), orig: cur_pp.clone() });
                    chars_in_para = false;
                }
                TextTok::Chars(text) => {
                    for c in text.chars() {
                        pos += 1;
                        if c == '\n' || c == '\r' {
                            paras.push(Para { start: pos, marker: None, orig: cur_pp.clone() });
                            chars_in_para = false;
                        } else {
                            chars_in_para = true;
                        }
                    }
                }
                TextTok::Tag(_) => {}
            }
        }
        let count = paras.len();
        let affected: Vec<bool> = (0..count)
            .map(|k| {
                let ps = paras[k].start;
                // the end of the paragraph's own text: the position of its break (or the end of the text)
                let pe = if k + 1 < count { paras[k + 1].start - 1 } else { total };
                if end > start { ps < end && pe >= start } else { ps <= start && pe >= start }
            })
            .collect();
        let new_of = |orig: &str, pairs: &mut Vec<(String, String)>, next: &mut u64| -> String {
            if let Some((_, new)) = pairs.iter().find(|(o, _)| o == orig) {
                return new.clone();
            }
            let new = next.to_string();
            *next += 1;
            pairs.push((orig.to_string(), new.clone()));
            new
        };
        // what to put at which paragraph start: (position, IX) for paragraphs that have no marker of their own
        let mut inserts: Vec<(usize, String)> = Vec::new();
        for k in 0..count {
            if affected[k] {
                let new = new_of(&paras[k].orig, &mut para_pairs, &mut next_pp);
                match paras[k].marker {
                    Some(i) => {
                        toks[i] = match &toks[i] {
                            TextTok::Break(t) => TextTok::Break(set_attr(t, "IX", &new)),
                            TextTok::Tag(t) => TextTok::Tag(set_attr(t, "IX", &new)),
                            other => other.clone(),
                        }
                    }
                    None => inserts.push((paras[k].start, new)),
                }
            } else if k > 0 && affected[k - 1] && paras[k].marker.is_none() {
                // the paragraph relied on the one before it: give it back its own pp
                inserts.push((paras[k].start, paras[k].orig.clone()));
            }
        }
        if !inserts.is_empty() {
            let mut out: Vec<TextTok> = Vec::new();
            let mut pos = 0usize;
            let tag = |ix: &str| TextTok::Tag(format!("<pp IX='{}'/>", escape_attr(ix)));
            let mut pending = inserts;
            pending.sort_by_key(|(p, _)| *p);
            for tok in toks {
                match tok {
                    TextTok::Chars(text) => {
                        let chars: Vec<char> = text.chars().collect();
                        let mut i = 0usize;
                        while i < chars.len() {
                            if let Some(at) = pending.iter().position(|(p, _)| *p == pos + i) {
                                let (_, ix) = pending.remove(at);
                                out.push(tag(&ix));
                            }
                            let next_cut = pending.iter().map(|(p, _)| *p).filter(|p| *p > pos + i && *p < pos + chars.len()).min();
                            let upto = next_cut.map_or(chars.len(), |p| p - pos);
                            out.push(TextTok::Chars(chars[i..upto].iter().collect()));
                            i = upto;
                        }
                        pos += chars.len();
                    }
                    TextTok::Break(t) => {
                        pos += 1;
                        out.push(TextTok::Break(t));
                    }
                    other => out.push(other),
                }
            }
            for (_, ix) in pending {
                out.push(tag(&ix));
            }
            toks = out;
        }
    }

    let rows_before: Vec<String> = rows_of(shape, "Character").into_iter().map(|(ix, _)| ix).collect();
    let mut result = format!("{}{}{}", &shape[..ts], text_of_tokens(&toks), &shape[te..]);
    result = add_rows(&result, "Character", &char_pairs, &r.char_cells, r.char_style);
    if !char_pairs.is_empty() {
        let mut names: Vec<String> = r.char_cells.iter().map(|(k, _)| k.clone()).collect();
        if r.char_style.is_some() {
            names.push("Style".to_string());
        }
        let row0 = rows_of(shape, "Character").into_iter().find(|(ix, _)| ix == "0").map(|(_, row)| row);
        result = pin_character_cells(&result, &names, row0.as_deref());
    }
    result = add_rows(&result, "Paragraph", &para_pairs, &r.para_cells, None);
    let renumbered = renumber_runs(&result)?;
    Ok(fill_new_run_rows(&renumbered, &rows_before, &r.base_cells))
}

/// Character rows that did not exist before the format (new run rows, clones of an inheriting row 0) get the shape's
/// effective `base` cells wherever they have none of their own.
fn fill_new_run_rows(shape: &str, before: &[String], base: &[(String, String)]) -> String {
    if base.is_empty() {
        return shape.to_string();
    }
    let Some((s, e)) = find_section_range(shape, "Character", "") else { return shape.to_string() };
    let mut rows = String::new();
    for (ix, row) in rows_of(shape, "Character") {
        let missing: Vec<(String, String)> = if before.contains(&ix) {
            Vec::new()
        } else {
            base.iter().filter(|(name, _)| cell_value_in(&row, name).is_none()).cloned().collect()
        };
        rows.push_str(&if missing.is_empty() { row } else { row_with_cells(&row, &missing) });
    }
    let mut out = shape.to_string();
    out.replace_range(s..e, &format!("<Section N='Character'>{rows}</Section>"));
    out
}


/// Append `children` (shape elements) inside the shape's own `<Shapes>`; the element is created when the shape has none.
fn append_child_shapes(shape: &str, children: &str) -> String {
    let mut out = shape.to_string();
    if let Some(at) = out.find("<Shapes>") {
        if let Some(end) = out[at..].find("</Shapes>") {
            // the first `</Shapes>` after the opening one closes it only when no nested `<Shapes>` lies between: walk depth
            let mut depth = 0usize;
            let mut i = at;
            while i < out.len() {
                if out[i..].starts_with("<Shapes>") {
                    depth += 1;
                    i += "<Shapes>".len();
                } else if out[i..].starts_with("</Shapes>") {
                    depth -= 1;
                    if depth == 0 {
                        out.insert_str(i, children);
                        return out;
                    }
                    i += "</Shapes>".len();
                } else {
                    i += 1;
                }
            }
            let _ = end;
        }
    }
    let insert = out.rfind("</Shape>").unwrap_or(out.len());
    out.insert_str(insert, &format!("<Shapes>{children}</Shapes>"));
    out
}

/// Write `row` into the shape's own XML: the section and the row are added when absent (before the text, the members or the
/// foreign data), every cell gets its value and its formula.
fn apply_row_override(shape: &str, row: &RowOverride) -> String {
    let mut out = shape.to_string();
    if find_section_range(&out, &row.section, &row.section_index).is_none() {
        let insert = ["<Text", "<Shapes", "<ForeignData"].iter().filter_map(|needle| out.find(needle)).min().unwrap_or_else(|| out.rfind("</Shape>").unwrap_or(out.len()));
        let index = if row.section_index.is_empty() { String::new() } else { format!(" IX='{}'", escape_attr(&row.section_index)) };
        out.insert_str(insert, &format!("<Section N='{}'{index}></Section>", escape_attr(&row.section)));
    }
    let (s, e) = find_section_range(&out, &row.section, &row.section_index).unwrap_or((0, 0));
    if find_row_range(&out, s, e, &row.row_index).is_none() {
        let close = s + out[s..e].rfind("</Section>").unwrap_or(e - s);
        let kind = row.row_type.as_ref().map(|t| format!(" T='{}'", escape_attr(t))).unwrap_or_default();
        out.insert_str(close, &format!("<Row{kind} IX='{}'></Row>", escape_attr(&row.row_index)));
    }
    for (name, value, formula) in &row.cells {
        let (s, e) = find_section_range(&out, &row.section, &row.section_index).unwrap_or((0, 0));
        let Some((rs, re)) = find_row_range(&out, s, e, &row.row_index) else { continue };
        let text = &out[rs..re];
        let has = text.contains(&format!("N='{name}'")) || text.contains(&format!("N=\"{name}\""));
        out = if has {
            let with_value = patch_cell_in_range(&out, rs, re, name, value, "V");
            if formula.is_empty() {
                with_value
            } else {
                let (s, e) = find_section_range(&with_value, &row.section, &row.section_index).unwrap_or((0, 0));
                let (rs, re) = find_row_range(&with_value, s, e, &row.row_index).unwrap_or((rs, re));
                patch_cell_in_range(&with_value, rs, re, name, formula, "F")
            }
        } else {
            let at = rs + text.rfind("</Row>").unwrap_or(text.len());
            let f = if formula.is_empty() { String::new() } else { format!(" F='{}'", escape_attr(formula)) };
            format!("{}<Cell N='{}' V='{}'{f}/>{}", &out[..at], escape_attr(name), escape_attr(value), &out[at..])
        };
    }
    out
}

/// The rows of section `name` of a shape's own XML as `(IX, row xml)`.
fn rows_of(shape: &str, name: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let Some((s, e)) = find_section_range(shape, name, "") else { return rows };
    let mut at = s;
    while let Some(relative) = shape[at..e].find("<Row") {
        let row_start = at + relative;
        let close = shape[row_start..e].find('>').map_or(e, |n| row_start + n + 1);
        let tag = &shape[row_start..close];
        let end = if tag.trim_end().ends_with("/>") { close } else { shape[close..e].find("</Row>").map_or(e, |n| close + n + "</Row>".len()) };
        if let Some(ix) = attr(tag, "IX") {
            rows.push((ix, shape[row_start..end].to_string()));
        }
        at = end;
    }
    rows
}

/// A clone of row xml `base` with the IX `new` (a self-closing row becomes an ordinary one).
fn row_clone(base: Option<&str>, new: &str) -> String {
    match base {
        Some(base) => {
            let close = base.find('>').unwrap_or(0);
            let tag = &base[..close + 1];
            if tag.trim_end().ends_with("/>") {
                format!("{}></Row>", set_attr(tag, "IX", new).trim_end().trim_end_matches("/>").trim_end())
            } else {
                format!("{}{}", set_attr(tag, "IX", new), &base[close + 1..])
            }
        }
        None => format!("<Row IX='{}'></Row>", escape_attr(new)),
    }
}

/// Number the `<cp>` and `<pp>` markers of a shape's text 0, 1, 2 ... in text order, each with its own row (a clone of the
/// row it pointed to), and start the text with explicit `<cp IX='0'/><pp IX='0'/>` the way Visio writes it. Found by
/// probing Visio: a run that points back to an EARLIER row (`<cp IX='0'/>` again after `<cp IX='1'/>`) is not applied
/// (the formatting is lost), and a text without a leading `<pp>` ignores its first Paragraph row.
fn renumber_runs(shape: &str) -> Result<String> {
    let Some((ts, te)) = own_text_range(shape) else { return Ok(shape.to_string()) };
    let toks = tokenize_text(&shape[ts..te])?;
    let first_content = toks.iter().position(|t| matches!(t, TextTok::Chars(_) | TextTok::Break(_))).unwrap_or(toks.len());
    let has_before = |prefix: &str| toks[..first_content].iter().any(|t| matches!(t, TextTok::Tag(x) if x.starts_with(prefix)));
    let mut work: Vec<TextTok> = Vec::new();
    if !has_before("<cp") {
        work.push(TextTok::Tag("<cp IX='0'/>".to_string()));
    }
    if !has_before("<pp") {
        work.push(TextTok::Tag("<pp IX='0'/>".to_string()));
    }
    work.extend(toks);
    let (mut cmap, mut pmap): (Vec<(String, String)>, Vec<(String, String)>) = (Vec::new(), Vec::new());
    let mut out: Vec<TextTok> = Vec::new();
    for tok in work {
        match tok {
            TextTok::Tag(t) if t.starts_with("<cp") => {
                let n = cmap.len().to_string();
                cmap.push((marker_ix(&t), n.clone()));
                out.push(TextTok::Tag(set_attr(&t, "IX", &n)));
            }
            TextTok::Tag(t) if t.starts_with("<pp") => {
                let n = pmap.len().to_string();
                pmap.push((marker_ix(&t), n.clone()));
                out.push(TextTok::Tag(set_attr(&t, "IX", &n)));
            }
            TextTok::Break(t) => {
                let n = pmap.len().to_string();
                pmap.push((marker_ix(&t), n.clone()));
                out.push(TextTok::Break(set_attr(&t, "IX", &n)));
            }
            other => out.push(other),
        }
    }
    let old_char = rows_of(shape, "Character");
    let old_para = rows_of(shape, "Paragraph");
    let mut result = format!("{}{}{}", &shape[..ts], text_of_tokens(&out), &shape[te..]);
    for (name, map, old) in [("Character", &cmap, &old_char), ("Paragraph", &pmap, &old_para)] {
        let Some((s, e)) = find_section_range(&result, name, "") else { continue };
        let mut rows = String::new();
        for (k, n) in map.iter() {
            let base = old.iter().find(|(ix, _)| ix == k).map(|(_, row)| row.as_str());
            rows.push_str(&row_clone(base, n));
        }
        result.replace_range(s..e, &format!("<Section N='{name}'>{rows}</Section>"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NESTED: &str = "<PageContents><Shapes>\
        <Shape ID='1' Name='outer'><Cell N='PinX' V='1'/><Shapes>\
        <Shape ID='2'/><Shape ID='3'><Shapes><Shape ID='4'/></Shapes></Shape>\
        </Shapes></Shape>\
        <Shape ID='12' Name='target'><Cell N='PinX' V='2'/></Shape>\
        <Shape ID='120' Name='other'/>\
        </Shapes></PageContents>";

    #[test]
    fn shape_ranges_skip_shapes_elements_and_prefix_ids() {
        let (start, end) = find_shape_range(NESTED, "12").expect("shape 12");
        let shape = &NESTED[start..end];
        assert!(shape.starts_with("<Shape ID='12'"));
        assert!(shape.contains("PinX"));
        assert!(!shape.contains("ID='120'"));

        let (start, end) = find_shape_range(NESTED, "1").expect("outer shape");
        let shape = &NESTED[start..end];
        assert!(shape.contains("ID='2'"));
        assert!(shape.contains("ID='3'"));
        assert!(shape.contains("ID='4'"));
        assert!(!shape.contains("ID='12'"));
        assert!(!shape.contains("ID='120'"));

        assert!(find_shape_range(NESTED, "4").is_some());
        assert!(find_shape_range(NESTED, "2").is_some());
    }

    #[test]
    fn a_relationship_goes_into_an_empty_relationships_element() {
        const REL: &str = "<Relationship Id='rId1' Type='t' Target='../masters/master1.xml'/>";
        for empty in [
            "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships' />",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
            "<Relationships/>",
        ] {
            let mut rels = empty.to_string();
            assert!(insert_relationship(&mut rels, REL), "{empty}");
            assert!(rels.contains(&format!("{REL}</Relationships>")), "{rels}");
            assert_eq!(rels.matches("<Relationships").count(), 1, "{rels}");
            assert_eq!(rels.matches("/>").count(), 1, "only the new relationship is self-closing: {rels}");
        }
    }

    #[test]
    fn a_relationship_is_appended_after_the_existing_ones() {
        let mut rels = "<Relationships xmlns='x'><Relationship Id='rId1' Type='t' Target='a'/></Relationships>".to_string();
        assert!(insert_relationship(&mut rels, "<Relationship Id='rId2' Type='t' Target='b'/>"));
        let first = rels.find("Id='rId1'").unwrap();
        let second = rels.find("Id='rId2'").unwrap();
        assert!(first < second && second < rels.find("</Relationships>").unwrap(), "{rels}");
    }

    #[test]
    fn text_without_a_relationships_root_is_refused() {
        for bad in ["", "<Types/>", "<RelationshipsX/>", "<Relationships xmlns='x'>"] {
            let mut rels = bad.to_string();
            assert!(!insert_relationship(&mut rels, "<Relationship Id='rId1'/>"), "{bad}");
            assert_eq!(rels, bad);
        }
    }

    #[test]
    fn shape_id_does_not_match_original_id() {
        let xml = "<Shapes><Shape ID='7' OriginalID='9'/><Shape ID='9'/></Shapes>";
        let (start, end) = find_shape_range(xml, "9").expect("shape 9");
        assert!(xml[start..end].starts_with("<Shape ID='9'"));
    }

    fn edited(inner: &str, new: &str) -> String {
        text_of_tokens(&edit_tokens(tokenize_text(inner).unwrap(), new))
    }

    #[test]
    fn typing_at_the_end_of_a_run_keeps_the_markers() {
        let inner = "<pp IX=\"0\"/><cp IX=\"0\"/>Patient arrives <pp IX=\"1\"/>for appointment";
        assert_eq!(plain_of(&tokenize_text(inner).unwrap()), "Patient arrives \nfor appointment");
        assert_eq!(
            edited(inner, "Patient arrives \nfor appointment today"),
            "<pp IX=\"0\"/><cp IX=\"0\"/>Patient arrives <pp IX=\"1\"/>for appointment today"
        );
    }

    #[test]
    fn a_change_inside_one_run_leaves_the_other_runs_alone() {
        let inner = "<cp IX='0'/>Hello <cp IX='1'/>bold<cp IX='2'/> world";
        assert_eq!(edited(inner, "Hello bold & world"), "<cp IX='0'/>Hello <cp IX='1'/>bold<cp IX='2'/> &amp; world");
    }

    #[test]
    fn inserted_text_joins_the_run_it_is_typed_into() {
        let inner = "<cp IX='0'/>Hello <cp IX='1'/>bold<cp IX='2'/> world";
        // typed inside the bold run
        assert_eq!(edited(inner, "Hello boXld world"), "<cp IX='0'/>Hello <cp IX='1'/>boXld<cp IX='2'/> world");
        // typed at the start of the document joins the first run
        assert_eq!(edited(inner, "Oh Hello bold world"), "<cp IX='0'/>Oh Hello <cp IX='1'/>bold<cp IX='2'/> world");
        // typed at the very end joins the last run
        assert_eq!(edited(inner, "Hello bold world!"), "<cp IX='0'/>Hello <cp IX='1'/>bold<cp IX='2'/> world!");
    }

    #[test]
    fn deleting_across_runs_keeps_the_markers_and_drops_the_characters() {
        let inner = "<cp IX='0'/>Hello <cp IX='1'/>bold<cp IX='2'/> world";
        assert_eq!(edited(inner, "Held"), "<cp IX='0'/>Hel<cp IX='1'/><cp IX='2'/>d");
    }

    #[test]
    fn deleting_a_paragraph_break_merges_the_paragraphs() {
        let inner = "<pp IX=\"0\"/><cp IX=\"0\"/>one<pp IX=\"1\"/>two";
        assert_eq!(edited(inner, "onetwo"), "<pp IX=\"0\"/><cp IX=\"0\"/>onetwo");
        assert_eq!(edited(inner, "one\nthree"), "<pp IX=\"0\"/><cp IX=\"0\"/>one<pp IX=\"1\"/>three");
    }

    #[test]
    fn markup_characters_are_escaped_and_entities_decoded() {
        let inner = "<cp IX='0'/>a &amp; b";
        assert_eq!(plain_of(&tokenize_text(inner).unwrap()), "a & b");
        assert_eq!(edited(inner, "a < b"), "<cp IX='0'/>a &lt; b");
    }

    #[test]
    fn text_with_fields_is_refused_and_an_unchanged_text_is_untouched() {
        assert!(tokenize_text("<cp IX='0'/>Page <fld IX='0'>1</fld>").is_err());
        let inner = "<pp IX='0'/><cp IX='0'/>same";
        assert_eq!(edited(inner, "same"), inner);
    }

    #[test]
    fn a_shape_without_text_gets_a_text_element() {
        let shape = "<Shape ID='7'><Cell N='PinX' V='1'/></Shape>";
        let out = patch_shape_text_edit(shape, "new").unwrap();
        assert!(out.contains("<Text>new</Text>"), "{out}");
    }
}

/// The Shape Data rows a shape of the page itself carries, in the file's row order, with their `SortKey` (empty when the row has
/// none): `[(name, sort key)]`. Rows a master supplies are not listed (they have no row on the shape).
pub fn data_sort_keys(package: &Package, page_index: usize, shape_id: &str) -> Result<Vec<(String, String)>> {
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let xml = package.text(&part)?.to_string();
    let (start, end) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let fragment = &xml[start..end];
    let doc = roxmltree::Document::parse(fragment).map_err(|e| Error(format!("shape xml: {e}")))?;
    let shape = doc.root_element();
    let mut out = Vec::new();
    for section in shape.children().filter(|n| n.is_element() && n.tag_name().name() == "Section" && n.attribute("N") == Some("Property")) {
        for row in section.children().filter(|n| n.is_element() && n.tag_name().name() == "Row") {
            let Some(name) = row.attribute("N") else { continue };
            let key = row.children().find(|c| c.is_element() && c.attribute("N") == Some("SortKey")).and_then(|c| c.attribute("V")).unwrap_or("");
            out.push((name.to_string(), key.to_string()));
        }
    }
    Ok(out)
}
