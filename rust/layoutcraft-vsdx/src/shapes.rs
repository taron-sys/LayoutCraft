//! Page shape traversal and master inheritance metadata.
//!
//! Evidence: the shape tree, `Master` / `MasterShape` references and cell inheritance are the documented file structure ([MS-VSDX] shapes, masters, cell
//! inheritance); edge cases are pinned by `tests/master_scope.rs` and `tests/golden_packages.rs` (private corpus).
//!
//! This layer resolves the page/master shape graph but intentionally stops
//! before geometry, style inheritance and the editable document model. It gives
//! the next importer slice a tested shape tree with merged master cells.
use roxmltree::Node;
use std::collections::BTreeMap;

use crate::{Error, Package, Result, REL_NS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextToken {
    Text(String),
    Char(String),
    Para(String),
    Field { ix: String, text: String },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForeignData {
    pub rel_id: Option<String>,
    pub foreign_type: Option<String>,
    pub compression: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub name: String,
    pub index: Option<String>,
    pub cells: BTreeMap<String, Cell>,
    pub rows: Vec<SectionRow>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionRow {
    pub index: Option<String>,
    pub row_type: Option<String>,
    pub name: Option<String>,
    pub cells: BTreeMap<String, Cell>,
    /// `Del='1'`: the row is deleted. On an instance it removes the matching inherited
    /// row; on a master shape it removes a row the shape would inherit from its style.
    pub deleted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cell {
    pub value: Option<String>,
    pub formula: Option<String>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ShapeNode {
    pub id: String,
    pub name: Option<String>,
    pub shape_type: Option<String>,
    pub master: Option<String>,
    pub master_shape: Option<String>,
    pub fill_style: Option<String>,
    pub line_style: Option<String>,
    pub text_style: Option<String>,
    pub cells: BTreeMap<String, Cell>,
    pub sections: Vec<Section>,
    pub foreign: Option<ForeignData>,
    pub has_text: bool,
    pub text: Option<String>,
    pub text_tokens: Vec<TextToken>,
    pub parent: Option<String>,
    pub depth: usize,
    pub children: Vec<ShapeNode>,
}

impl ShapeNode {
    pub fn walk(&self, visit: &mut impl FnMut(&ShapeNode)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }

    pub fn shape_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        self.walk(&mut |node| ids.push(node.id.clone()));
        ids
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MasterShape {
    pub id: String,
    pub order: usize,
    pub name: Option<String>,
    pub shape_type: Option<String>,
    pub fill_style: Option<String>,
    pub line_style: Option<String>,
    pub text_style: Option<String>,
    pub cells: BTreeMap<String, Cell>,
    pub sections: Vec<Section>,
    pub foreign: Option<ForeignData>,
    pub has_text: bool,
    pub text: Option<String>,
    pub text_tokens: Vec<TextToken>,
    pub parent: Option<String>,
    pub depth: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Master {
    pub id: String,
    pub name: String,
    pub part: String,
    pub top: Option<String>,
    pub shapes: BTreeMap<String, MasterShape>,
    /// The master's own page scale (`PageScale / DrawingScale` of its page sheet in masters.xml; 0 when it has none): CyberPower and Dell
    /// masters are authored on a 1:10 page, so this is 0.1. Visio shrinks such a master when it is dropped on a 1:1 page.
    pub page_ratio: f64,
}

#[derive(Debug, Clone, Default)]
pub struct MasterCatalog {
    pub masters: BTreeMap<String, Master>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedShape {
    pub id: String,
    pub name: Option<String>,
    pub shape_type: Option<String>,
    pub master_id: Option<String>,
    pub master_name: Option<String>,
    /// True when the page shape itself names a master. Nested shapes often
    /// inherit the parent's master context; their body is drawn by the parent
    /// instance, so they must not composite it a second time.
    pub master_explicit: bool,
    pub master_shape_id: Option<String>,
    pub master_part: Option<String>,
    pub fill_style: Option<String>,
    pub line_style: Option<String>,
    pub text_style: Option<String>,
    pub cells: BTreeMap<String, Cell>,
    pub sections: Vec<Section>,
    /// Sections as written on the page shape, before master inheritance.
    pub own_sections: Vec<Section>,
    /// Sections inherited from the selected master shape.
    pub master_sections: Vec<Section>,
    /// Nested body shapes of the selected master shape (document order).
    pub master_children: Vec<MasterShape>,
    pub master_top_id: Option<String>,
    /// `User.*` values of the master's top shape, for a nested shape's formulas such as
    /// `GUARD(Sheet.5!User.IsInstance)` (own rows win).
    pub master_top_user: BTreeMap<String, f64>,
    pub master_width: Option<f64>,
    pub master_height: Option<f64>,
    pub foreign: Option<ForeignData>,
    pub has_text: bool,
    pub text: Option<String>,
    pub text_tokens: Vec<TextToken>,
    pub parent: Option<String>,
    pub depth: usize,
    pub children: Vec<ResolvedShape>,
}

impl ResolvedShape {
    pub fn walk(&self, visit: &mut impl FnMut(&ResolvedShape)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }

    pub fn shape_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        self.walk(&mut |node| ids.push(node.id.clone()));
        ids
    }

    pub fn is_master_derived(&self) -> bool {
        self.master_id.is_some()
    }
}

impl MasterCatalog {
    pub fn load(package: &Package) -> Result<Self> {
        let mut masters = BTreeMap::new();
        let masters_part = "visio/masters/masters.xml";
        if !package.contains(masters_part) {
            return Ok(Self { masters });
        }
        let rels = package.relationships(masters_part)?;
        let doc = package.xml(masters_part)?;
        for node in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Master")
        {
            let Some(id) = node.attribute("ID") else {
                continue;
            };
            let rel_id = node
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "Rel")
                .and_then(|rel| rel.attribute((REL_NS, "id")))
                .or_else(|| {
                    node.children()
                        .find(|n| n.is_element() && n.tag_name().name() == "Rel")
                        .and_then(|rel| {
                            rel.attributes()
                                .find(|a| a.name() == "id" && a.namespace().is_some())
                                .map(|a| a.value())
                        })
                });
            let Some(part) = rel_id
                .and_then(|rid| rels.get(rid))
                .map(|rel| rel.target.clone())
            else {
                continue;
            };
            if !package.contains(&part) {
                continue;
            }
            let mut master = load_master(package, id, node.attribute("NameU").unwrap_or(""), &part)?;
            let sheet_cell = |name: &str| {
                node.descendants()
                    .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
                    .and_then(|sheet| sheet.children().find(|c| c.is_element() && c.attribute("N") == Some(name)))
                    .and_then(|cell| cell.attribute("V"))
                    .and_then(|v| v.parse::<f64>().ok())
            };
            if let (Some(paper), Some(drawing)) = (sheet_cell("PageScale"), sheet_cell("DrawingScale")) {
                if paper > 0.0 && drawing > 0.0 {
                    master.page_ratio = paper / drawing;
                }
            }
            masters.insert(id.to_string(), master);
        }
        Ok(Self { masters })
    }

    pub fn get(&self, id: &str) -> Option<&Master> {
        self.masters.get(id)
    }

    pub fn resolve(&self, nodes: &[ShapeNode]) -> Result<Vec<ResolvedShape>> {
        nodes
            .iter()
            .map(|node| resolve_node(node, None, 0, None, self))
            .collect()
    }
}

fn load_master(package: &Package, id: &str, name: &str, part: &str) -> Result<Master> {
    let doc = package.xml(part)?;
    let shapes_container = doc
        .root_element()
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Shapes")
        .or_else(|| {
            doc.descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "Shapes")
        });
    let mut shapes = BTreeMap::new();
    let mut top = None;
    let mut order = 0usize;
    if let Some(container) = shapes_container {
        for child in container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Shape")
        {
            if top.is_none() {
                top = child.attribute("ID").map(str::to_string);
            }
            collect_master_shape(child, None, 0, &mut shapes, &mut order);
        }
    }
    Ok(Master {
        id: id.to_string(),
        name: name.to_string(),
        part: part.to_string(),
        top,
        shapes,
        page_ratio: 0.0,
    })
}

fn collect_master_shape(
    node: Node<'_, '_>,
    parent: Option<&str>,
    depth: usize,
    shapes: &mut BTreeMap<String, MasterShape>,
    order: &mut usize,
) {
    let Some(id) = node.attribute("ID").map(str::to_string) else {
        return;
    };
    let shape = MasterShape {
        id: id.clone(),
        order: *order,
        name: shape_name(node),
        shape_type: node.attribute("Type").map(str::to_string),
        fill_style: style_attr(node, "FillStyle"),
        line_style: style_attr(node, "LineStyle"),
        text_style: style_attr(node, "TextStyle"),
        cells: shape_cells(node),
        sections: strip_deleted(shape_sections(node)),
        foreign: shape_foreign(node),
        has_text: shape_has_text(node),
        text: shape_text(node),
        text_tokens: shape_text_tokens(node),
        parent: parent.map(str::to_string),
        depth,
    };
    *order += 1;
    shapes.insert(id.clone(), shape);
    if let Some(container) = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Shapes")
    {
        for child in container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Shape")
        {
            collect_master_shape(child, Some(&id), depth + 1, shapes, order);
        }
    }
}

impl Package {
    /// Parse the page's shape tree, including nested group members.
    pub fn page_shapes(&self, part: &str) -> Result<Vec<ShapeNode>> {
        let doc = self.xml(part)?;
        let root = doc.root_element();
        let container = root
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "Shapes")
            .or_else(|| {
                root.descendants()
                    .find(|n| n.is_element() && n.tag_name().name() == "Shapes")
            });
        let mut out = Vec::new();
        if let Some(container) = container {
            for child in container
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "Shape")
            {
                out.push(parse_shape_node(child, None, 0));
            }
        }
        Ok(out)
    }

    /// Resolve page/master shape metadata and merge master cells over the page.
    pub fn resolve_page_shapes(&self, part: &str) -> Result<Vec<ResolvedShape>> {
        let catalog = MasterCatalog::load(self)?;
        let shapes = self.page_shapes(part)?;
        catalog.resolve(&shapes)
    }
}

fn parse_shape_node(node: Node<'_, '_>, parent: Option<&str>, depth: usize) -> ShapeNode {
    let id = node.attribute("ID").unwrap_or("").to_string();
    let mut children = Vec::new();
    if let Some(container) = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Shapes")
    {
        for child in container
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Shape")
        {
            children.push(parse_shape_node(child, Some(&id), depth + 1));
        }
    }
    ShapeNode {
        id,
        name: shape_name(node),
        shape_type: node.attribute("Type").map(str::to_string),
        master: node.attribute("Master").map(str::to_string),
        master_shape: node.attribute("MasterShape").map(str::to_string),
        fill_style: style_attr(node, "FillStyle"),
        line_style: style_attr(node, "LineStyle"),
        text_style: style_attr(node, "TextStyle"),
        cells: shape_cells(node),
        sections: shape_sections(node),
        foreign: shape_foreign(node),
        has_text: shape_has_text(node),
        text: shape_text(node),
        text_tokens: shape_text_tokens(node),
        parent: parent.map(str::to_string),
        depth,
        children,
    }
}

fn shape_name(node: Node<'_, '_>) -> Option<String> {
    node.attribute("NameU")
        .or_else(|| node.attribute("Name"))
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn style_attr(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn shape_has_text(node: Node<'_, '_>) -> bool {
    node.children()
        .any(|n| n.is_element() && n.tag_name().name() == "Text")
}

fn raw_text_content(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>()
}

fn shape_text(node: Node<'_, '_>) -> Option<String> {
    let text = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Text")?;
    let mut value = raw_text_content(text)
        .replace('\r', "")
        .replace(['\u{2028}', '\u{2029}'], "\n")
        .replace(['\u{00a0}', '\u{2007}', '\u{202f}'], " ")
        .replace('\t', " ");
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn shape_text_tokens(node: Node<'_, '_>) -> Vec<TextToken> {
    let Some(text) = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Text")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for child in text.children() {
        if child.is_text() {
            let value = child
                .text()
                .unwrap_or("")
                .replace(['\u{2028}', '\u{2029}'], "\n");
            if !value.is_empty() {
                out.push(TextToken::Text(value));
            }
            continue;
        }
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            "cp" => out.push(TextToken::Char(
                child.attribute("IX").unwrap_or("0").to_string(),
            )),
            "pp" => out.push(TextToken::Para(
                child.attribute("IX").unwrap_or("0").to_string(),
            )),
            "fld" => out.push(TextToken::Field {
                ix: child.attribute("IX").unwrap_or("0").to_string(),
                text: raw_text_content(child),
            }),
            _ => {}
        }
    }
    out
}

pub(crate) fn shape_cells(node: Node<'_, '_>) -> BTreeMap<String, Cell> {
    let mut cells = BTreeMap::new();
    for cell in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Cell")
    {
        let Some(name) = cell.attribute("N") else {
            continue;
        };
        cells.insert(
            name.to_string(),
            Cell {
                // Strings with line breaks cannot sit in an XML attribute, so Visio stores
                // them as the element's own text.
                value: cell.attribute("V").map(str::to_string).or_else(|| {
                    cell.text()
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                }),
                formula: cell.attribute("F").map(str::to_string),
                unit: cell.attribute("U").map(str::to_string),
            },
        );
    }
    cells
}

fn shape_foreign(node: Node<'_, '_>) -> Option<ForeignData> {
    let fd = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "ForeignData")?;
    let rel_id = fd
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Rel")
        .and_then(|rel| rel.attribute((REL_NS, "id")))
        .or_else(|| {
            fd.children()
                .find(|n| n.is_element() && n.tag_name().name() == "Rel")
                .and_then(|rel| {
                    rel.attributes()
                        .find(|a| a.name() == "id" && a.namespace().is_some())
                        .map(|a| a.value())
                })
        })
        .map(str::to_string);
    Some(ForeignData {
        rel_id,
        foreign_type: fd.attribute("ForeignType").map(str::to_string),
        compression: fd.attribute("CompressionType").map(str::to_string),
    })
}

pub(crate) fn shape_sections(node: Node<'_, '_>) -> Vec<Section> {
    let mut sections = Vec::new();
    for section in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Section")
    {
        let Some(name) = section.attribute("N") else {
            continue;
        };
        let rows = section
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "Row")
            .map(|row| SectionRow {
                index: row.attribute("IX").map(str::to_string),
                row_type: row.attribute("T").map(str::to_string),
                name: row.attribute("N").map(str::to_string),
                cells: shape_cells(row),
                deleted: row.attribute("Del").is_some_and(|value| value != "0"),
            })
            .collect();
        sections.push(Section {
            name: name.to_string(),
            index: section.attribute("IX").map(str::to_string),
            cells: shape_cells(section),
            rows,
        });
    }
    sections
}

fn master_descendants(master: &Master, ancestor: &str) -> Vec<MasterShape> {
    let mut out: Vec<MasterShape> = master
        .shapes
        .values()
        .filter(|shape| {
            let mut parent = shape.parent.as_deref();
            while let Some(id) = parent {
                if id == ancestor {
                    return true;
                }
                parent = master
                    .shapes
                    .get(id)
                    .and_then(|shape| shape.parent.as_deref());
            }
            false
        })
        .cloned()
        .collect();
    out.sort_by_key(|shape| shape.order);
    out
}

fn row_key(row: &SectionRow) -> Option<String> {
    if let Some(index) = &row.index {
        return Some(format!("i:{index}"));
    }
    row.name.as_ref().map(|name| format!("n:{name}"))
}

/// Drop rows flagged as deleted (they must not be read as rows with default cells).
/// Visio recalculates a master's formulas for the INSTANCE: a master geometry cell `Width*0.5` is half the width of the
/// instance, whatever value the master cached. A resized instance of a master therefore needs the cached values of the
/// geometry and connection-point cells whose formula is just a multiple of `Width` / `Height` (or the closing `Geometry1.X1` / `Y1` of a path)
/// brought up to the instance's size; formulas of any other shape keep their cached value.
pub(crate) fn refresh_geometry_for_size(sections: &mut [Section], width: f64, height: f64) {
    let factor = |formula: &str, name: &str| -> Option<f64> {
        let f: String = formula.chars().filter(|c| !c.is_whitespace()).collect();
        if f == name {
            return Some(1.0);
        }
        f.strip_prefix(&format!("{name}*")).and_then(|k| k.parse::<f64>().ok()).or_else(|| {
            // `Width/2`: written this way by many masters (connection points, geometry)
            f.strip_prefix(&format!("{name}/")).and_then(|k| k.parse::<f64>().ok()).filter(|k| k.abs() > 1e-12).map(|k| 1.0 / k)
        })
    };
    for section in sections.iter_mut().filter(|s| s.name == "Geometry" || s.name == "Connection") {
        for row in section.rows.iter_mut() {
            for cell in row.cells.values_mut() {
                let Some(formula) = cell.formula.as_deref() else { continue };
                let value = factor(formula, "Width").map(|k| k * width).or_else(|| factor(formula, "Height").map(|k| k * height));
                if let Some(v) = value {
                    cell.value = Some(format_size(v));
                }
            }
        }
    }
    // The closing point of a path: `Geometry1.X1` is the X of row 1 of the first geometry section (IX 0).
    let snapshot: Vec<(String, Vec<(String, BTreeMap<String, Cell>)>)> = sections
        .iter()
        .filter(|s| s.name == "Geometry")
        .map(|s| (s.index.clone().unwrap_or_default(), s.rows.iter().map(|r| (r.index.clone().unwrap_or_default(), r.cells.clone())).collect()))
        .collect();
    for (position, section) in sections.iter_mut().filter(|s| s.name == "Geometry").enumerate() {
        let _ = position;
        for row in section.rows.iter_mut() {
            for cell in row.cells.values_mut() {
                let Some(formula) = cell.formula.as_deref() else { continue };
                let f: String = formula.chars().filter(|c| !c.is_whitespace()).collect();
                let Some(rest) = f.strip_prefix("Geometry") else { continue };
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                let after = &rest[digits.len()..];
                let Some(after) = after.strip_prefix('.') else { continue };
                let Some(axis) = after.chars().next().filter(|c| *c == 'X' || *c == 'Y') else { continue };
                let row_no = &after[1..];
                if digits.is_empty() || row_no.is_empty() || !row_no.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let Ok(n) = digits.parse::<usize>() else { continue };
                let Some((_, rows)) = snapshot.get(n.saturating_sub(1)) else { continue };
                if let Some((_, cells)) = rows.iter().find(|(ix, _)| ix == row_no) {
                    if let Some(v) = cells.get(&axis.to_string()).and_then(|c| c.value.clone()) {
                        cell.value = Some(v);
                    }
                }
            }
        }
    }
}

/// The value of a formula that is a multiple of one named quantity (`Width*0.5`, `Height`, `ImgWidth*1`): `k * quantity`.
pub(crate) fn multiple_of(formula: &str, quantities: &BTreeMap<String, f64>) -> Option<f64> {
    let f: String = formula.chars().filter(|c| !c.is_whitespace()).collect();
    for (name, value) in quantities {
        if f == *name {
            return Some(*value);
        }
        if let Some(k) = f.strip_prefix(&format!("{name}*")).and_then(|k| k.parse::<f64>().ok()) {
            return Some(k * value);
        }
        // `Width/2`: connection points and geometry of many masters are written this way
        if let Some(k) = f.strip_prefix(&format!("{name}/")).and_then(|k| k.parse::<f64>().ok()).filter(|k| k.abs() > 1e-12) {
            return Some(value / k);
        }
    }
    None
}

fn format_size(value: f64) -> String {
    let text = format!("{value:.12}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" { "0".to_string() } else { text.to_string() }
}

pub(crate) fn strip_deleted(mut sections: Vec<Section>) -> Vec<Section> {
    for section in &mut sections {
        section.rows.retain(|row| !row.deleted);
    }
    sections
}

fn merge_section(target: &mut Section, page: &Section) {
    for (name, cell) in &page.cells {
        target.cells.insert(name.clone(), cell.clone());
    }
    for page_row in &page.rows {
        let key = row_key(page_row);
        if page_row.deleted {
            if let Some(key) = key.as_ref() {
                target
                    .rows
                    .retain(|row| row_key(row).as_deref() != Some(key.as_str()));
            }
            continue;
        }
        let existing = key.as_ref().and_then(|key| {
            target
                .rows
                .iter_mut()
                .find(|row| row_key(row).as_deref() == Some(key.as_str()))
        });
        if let Some(existing) = existing {
            if page_row.row_type.is_some() {
                existing.row_type = page_row.row_type.clone();
            }
            for (name, cell) in &page_row.cells {
                existing.cells.insert(name.clone(), cell.clone());
            }
        } else {
            target.rows.push(page_row.clone());
        }
    }
}

fn resolve_node(
    node: &ShapeNode,
    parent: Option<&str>,
    depth: usize,
    inherited: Option<&Master>,
    catalog: &MasterCatalog,
) -> Result<ResolvedShape> {
    let master =
        match node.master.as_deref() {
            Some(id) => Some(catalog.get(id).ok_or_else(|| {
                Error(format!("shape {} references missing master {id}", node.id))
            })?),
            None => inherited,
        };
    let selected = if let Some(master_shape_id) = node.master_shape.as_deref() {
        let master = master.ok_or_else(|| {
            Error(format!(
                "shape {} references MasterShape {master_shape_id} without a master context",
                node.id
            ))
        })?;
        Some(master.shapes.get(master_shape_id).ok_or_else(|| {
            Error(format!(
                "shape {} references missing master shape {master_shape_id} in master {}",
                node.id, master.id
            ))
        })?)
    } else if node.master.is_some() {
        master.and_then(|m| m.top.as_deref().and_then(|top| m.shapes.get(top)))
    } else {
        None
    };
    let mut master_children = match (master, selected) {
        (Some(master), Some(selected)) => master_descendants(master, &selected.id),
        _ => Vec::new(),
    };
    // A master can hold a second top-level shape next to its group (a circle group and a
    // separate text shape); the page instance still nests the matching child inside the
    // group. Those shapes belong to the body too.
    if let (Some(master), Some(selected), true) = (master, selected, node.master.is_some()) {
        fn referenced(children: &[ShapeNode], out: &mut Vec<String>) {
            for child in children {
                if child.master.is_some() {
                    continue; // a nested instance of another master
                }
                if let Some(id) = child.master_shape.as_deref() {
                    out.push(id.to_string());
                }
                referenced(&child.children, out);
            }
        }
        let mut ids = Vec::new();
        referenced(&node.children, &mut ids);
        let mut added = false;
        for id in ids {
            if id != selected.id && !master_children.iter().any(|child| child.id == id) {
                if let Some(shape) = master.shapes.get(&id) {
                    master_children.push(shape.clone());
                    added = true;
                }
            }
        }
        if added {
            master_children.sort_by_key(|shape| shape.order);
        }
    }
    let master_width = selected
        .and_then(|shape| shape.cells.get("Width"))
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok());
    let master_height = selected
        .and_then(|shape| shape.cells.get("Height"))
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok());
    let mut cells = selected.map(|s| s.cells.clone()).unwrap_or_default();
    for (name, cell) in &node.cells {
        cells.insert(name.clone(), cell.clone());
    }
    let master_sections = selected.map(|s| s.sections.clone()).unwrap_or_default();
    let own_sections = node.sections.clone();
    let mut sections = master_sections.clone();
    for section in &node.sections {
        if let Some(existing) = sections
            .iter_mut()
            .find(|existing| existing.name == section.name && existing.index == section.index)
        {
            merge_section(existing, section);
        } else {
            sections.push(section.clone());
        }
    }
    let mut sections = strip_deleted(sections);
    // An instance that was resized (its own Width / Height differ from the master's top shape): the geometry follows.
    if let (Some(mw), Some(mh)) = (master_width, master_height) {
        let num = |name: &str| cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
        if let (Some(w), Some(h)) = (num("Width"), num("Height")) {
            if (w - mw).abs() > 1e-9 || (h - mh).abs() > 1e-9 {
                refresh_geometry_for_size(&mut sections, w, h);
            }
        }
    }
    let foreign = node
        .foreign
        .clone()
        .or_else(|| selected.and_then(|s| s.foreign.clone()));
    let name = node
        .name
        .clone()
        .or_else(|| selected.and_then(|s| s.name.clone()));
    let shape_type = node
        .shape_type
        .clone()
        .or_else(|| selected.and_then(|s| s.shape_type.clone()));
    let fill_style = node
        .fill_style
        .clone()
        .or_else(|| selected.and_then(|s| s.fill_style.clone()));
    let line_style = node
        .line_style
        .clone()
        .or_else(|| selected.and_then(|s| s.line_style.clone()));
    let text_style = node
        .text_style
        .clone()
        .or_else(|| selected.and_then(|s| s.text_style.clone()));
    let has_text = node.has_text || selected.is_some_and(|s| s.has_text);
    let text = if node.has_text {
        node.text.clone()
    } else {
        selected.and_then(|s| s.text.clone())
    };
    let text_tokens = if node.has_text {
        node.text_tokens.clone()
    } else {
        selected.map(|s| s.text_tokens.clone()).unwrap_or_default()
    };
    let child_context = if node.master.is_some() {
        master
    } else {
        inherited
    };
    let mut children = Vec::new();
    for child in &node.children {
        children.push(resolve_node(
            child,
            Some(&node.id),
            depth + 1,
            child_context,
            catalog,
        )?);
    }
    let mut master_top_user: BTreeMap<String, f64> = master
        .and_then(|master| master.top.as_ref().and_then(|top| master.shapes.get(top)))
        .filter(|top| selected.is_none_or(|selected| selected.id != top.id))
        .map(|top| {
            top.sections
                .iter()
                .filter(|section| section.name == "User")
                .flat_map(|section| section.rows.iter())
                .filter_map(|row| {
                    let name = row.name.as_deref()?;
                    let value = row.cells.get("Value")?.value.as_deref()?.parse::<f64>().ok()?;
                    Some((format!("User.{name}"), value))
                })
                .collect()
        })
        .unwrap_or_default();
    // The top shape's own ThemeIndex also applies to its nested shapes (a stencil pinned to a
    // built-in theme).
    if let Some(index) = master
        .and_then(|master| master.top.as_ref().and_then(|top| master.shapes.get(top)))
        .and_then(|top| top.cells.get("ThemeIndex"))
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok())
    {
        master_top_user.insert("ThemeIndex".to_string(), index);
    }
    // The top shape's own size and control rows, for formulas such as
    // `IF(Sheet.5!Width<Width-LocPinX,1,0)` or `Sheet.5!Controls.Row_2` that name the group.
    if let Some(top) = master.and_then(|master| master.top.as_ref().and_then(|top| master.shapes.get(top))) {
        for name in ["Width", "Height", "PinX", "PinY", "LocPinX", "LocPinY", "Angle"] {
            if let Some(value) = top
                .cells
                .get(name)
                .and_then(|cell| cell.value.as_deref())
                .and_then(|value| value.parse::<f64>().ok())
            {
                master_top_user.insert(format!("top.{name}"), value);
            }
        }
        for section in top.sections.iter().filter(|section| section.name == "Control") {
            for row in &section.rows {
                let Some(row_name) = row.name.as_deref() else { continue };
                for (cell_name, cell) in &row.cells {
                    if let Some(value) = cell.value.as_deref().and_then(|value| value.parse::<f64>().ok()) {
                        master_top_user.insert(format!("top.Controls.{row_name}.{cell_name}"), value);
                        if cell_name == "X" {
                            master_top_user.insert(format!("top.Controls.{row_name}"), value);
                        }
                    }
                }
            }
        }
    }
    Ok(ResolvedShape {
        id: node.id.clone(),
        name,
        shape_type,
        master_id: master.map(|m| m.id.clone()),
        master_name: master.map(|m| m.name.clone()),
        master_explicit: node.master.is_some(),
        master_shape_id: node
            .master_shape
            .clone()
            .or_else(|| selected.map(|s| s.id.clone())),
        master_part: master.map(|m| m.part.clone()),
        fill_style,
        line_style,
        text_style,
        cells,
        sections,
        own_sections,
        master_sections,
        master_children,
        master_top_id: master.and_then(|master| master.top.clone()),
        master_top_user,
        master_width,
        master_height,
        foreign,
        has_text,
        text,
        text_tokens,
        parent: parent.map(str::to_string),
        depth,
        children,
    })
}
