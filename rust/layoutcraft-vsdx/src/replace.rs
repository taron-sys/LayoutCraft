//! Change Shape: replace a page shape by an instance of another master, keeping its place, size, turn, formats and text.
//!
//! Measured with probe-change-shape2.ps1 / probe-change-shape3.ps1 (Visio `Shape.ReplaceShape`, round 11b): the new shape gets the NEXT shape ID and
//! `OriginalID='<old id>'`, the new master's name and `Master`; it keeps PinX / PinY, Width, Height and Angle of the old shape and every LOCAL
//! format cell the old shape had (FillForegnd, FillPattern, LineWeight, LineColor, ...), its Character section and its text; geometry, connection
//! points and the text block come from the new master. Connectors glued to the old shape are glued to the new one: a connector end on connection
//! point 2 of the old shape lands on the new shape's connection point NEAREST to where that end was (Rectangle X2 -> Ellipse X5 in the probe).
//! The new shape stands where the old one stood in the z-order.
//!
//! Shape Data (Prop) and User rows are carried too (probe-change-shape-data.ps1: Owner / Alice survived, the User row stayed next to the new master's own).

use crate::groupedit::GroupEditor;
use crate::writer::{add_master_instance, find_shape_range, patch_page_cells, with_part, ShapeCellPatch};
use crate::{Error, Package, Result};
use std::collections::BTreeMap;

/// The pieces of the old shape's own XML that the new shape keeps.
#[derive(Default)]
struct Kept {
    cells: String,
    sections: String,
    text: String,
    attrs: String,
}

fn keep_cell(name: &str) -> bool {
    ["Fill", "Line", "Shdw", "Rounding", "Bevel", "Glow", "Reflection", "Soft", "Sketch"].iter().any(|p| name.starts_with(p)) || matches!(name, "FlipX" | "FlipY")
}

/// Split the own part of a shape (before its `<Shapes>`) into what is kept.
fn scan_own(shape: &str) -> Kept {
    let mut kept = Kept::default();
    let own_end = shape.find("<Shapes").unwrap_or(shape.len());
    let own = &shape[..own_end];
    let Some(open_end) = own.find('>') else { return kept };
    let open_tag = &own[..=open_end];
    for style in ["LineStyle", "FillStyle", "TextStyle"] {
        let key = format!(" {style}='");
        if let Some(at) = open_tag.find(&key) {
            let value_start = at + key.len();
            if let Some(len) = open_tag[value_start..].find('\'') {
                kept.attrs.push_str(&format!(" {style}='{}'", &open_tag[value_start..value_start + len]));
            }
        }
    }
    let mut at = open_end + 1;
    while at < own.len() {
        let Some(rel) = own[at..].find('<') else { break };
        let start = at + rel;
        let rest = &own[start..];
        if rest.starts_with("<Cell ") {
            let Some(end) = rest.find("/>").map(|e| e + 2).or_else(|| rest.find("</Cell>").map(|e| e + 7)) else { break };
            let tag = &rest[..end];
            let name = tag.find(" N='").and_then(|i| tag[i + 4..].find('\'').map(|j| &tag[i + 4..i + 4 + j])).unwrap_or("");
            if keep_cell(name) {
                kept.cells.push_str(tag);
            }
            at = start + end;
        } else if rest.starts_with("<Section ") {
            let Some(end) = rest.find("</Section>").map(|e| e + "</Section>".len()) else { break };
            let tag = &rest[..end];
            let name = tag.find(" N='").and_then(|i| tag[i + 4..].find('\'').map(|j| &tag[i + 4..i + 4 + j])).unwrap_or("");
            if matches!(name, "Character" | "Paragraph" | "Tabs" | "Property" | "User" | "Hyperlink") {
                kept.sections.push_str(tag);
            }
            at = start + end;
        } else if rest.starts_with("<Text>") || rest.starts_with("<Text ") {
            let Some(end) = rest.find("</Text>").map(|e| e + "</Text>".len()) else { break };
            kept.text = rest[..end].to_string();
            at = start + end;
        } else {
            // some other element (a self-closing tag or an element with a body): skip over it
            let Some(gt) = rest.find('>') else { break };
            at = if rest[..gt].ends_with('/') { start + gt + 1 } else {
                let name: String = rest[1..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                let close = format!("</{name}>");
                rest.find(&close).map_or(start + gt + 1, |e| start + e + close.len())
            };
        }
    }
    kept
}

/// Replace shape `shape_id` of page `page_index` by an instance of master `master_id` (a master of the drawing). Returns the new package bytes and the
/// ID of the new shape. The caller routes the connectors again (`GroupEditor::reroute_connectors`).
pub fn replace_shape(package: &Package, page_index: usize, shape_id: &str, master_id: &str) -> Result<(Vec<u8>, String)> {
    let editor = GroupEditor::open(package, page_index)?;
    let frame = editor.replaceable_frame(shape_id).ok_or_else(|| Error(format!("shape {shape_id} is not a plain shape of the page (connectors and members of groups cannot be changed)")))?;
    let old_points = editor.connection_points(shape_id);
    drop(editor);
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let xml0 = package.text(&part)?.to_string();
    let (os, oe) = find_shape_range(&xml0, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let kept = scan_own(&xml0[os..oe]);

    // 1. a fresh instance of the master at the old pin
    let (bytes, new_id) = add_master_instance(package, page_index, master_id, frame.pin.0, frame.pin.1)?;
    let mut work = Package::from_reader(std::io::Cursor::new(bytes))?;
    // 2. the old size, pin and turn (children of a group master scale with the box)
    let editor = GroupEditor::open(&work, page_index)?;
    let mut patches = Vec::new();
    let natural = editor.replaceable_frame(&new_id);
    if let Some(n) = &natural {
        if (n.size.0 - frame.size.0).abs() > 1e-9 || (n.size.1 - frame.size.1).abs() > 1e-9 {
            patches.extend(editor.resize_member(&new_id, frame.size, (0.5, 0.5), false)?);
        }
    }
    drop(editor);
    if !patches.is_empty() {
        work = Package::from_reader(std::io::Cursor::new(patch_page_cells(&work, page_index, &patches)?))?;
    }
    let mut cells = BTreeMap::new();
    cells.insert("PinX".to_string(), fmt(frame.pin.0));
    cells.insert("PinY".to_string(), fmt(frame.pin.1));
    cells.insert("Width".to_string(), fmt(frame.size.0));
    cells.insert("Height".to_string(), fmt(frame.size.1));
    if frame.angle.abs() > 1e-12 {
        cells.insert("Angle".to_string(), fmt(frame.angle));
    }
    let placement = ShapeCellPatch { visio_id: new_id.clone(), cells, ..Default::default() };
    work = Package::from_reader(std::io::Cursor::new(patch_page_cells(&work, page_index, &[placement])?))?;

    // 3. the kept formats, sections and text go into the new shape, which takes the old one's place in the z-order
    let mut xml = work.text(&part)?.to_string();
    let (ns, ne) = find_shape_range(&xml, &new_id).ok_or_else(|| Error("the new shape was not written".to_string()))?;
    let mut shape = xml[ns..ne].to_string();
    let open_end = shape.find('>').ok_or_else(|| Error("malformed shape".to_string()))?;
    let open_tag = shape[..open_end].to_string();
    let selfclosed = open_tag.ends_with('/');
    if selfclosed {
        return Err(Error("the new shape has no cells".to_string()));
    }
    let new_open = format!("{open_tag} OriginalID='{shape_id}'{}", kept.attrs);
    // the shape's own cells and sections come first, then its Text, then its children (`<Shapes>` of a group master)
    let own_tail = shape.find("<Shapes").unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
    let tail = &shape[own_tail..];
    let body = format!("{}{}{}{}", kept.cells, &shape[open_end + 1..own_tail], format!("{}{}", kept.sections, kept.text), tail);
    shape = format!("{new_open}>{body}");
    xml.replace_range(ns..ne, "");
    let (os2, oe2) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    xml.replace_range(os2..oe2, &shape);
    let staged = Package::from_reader(std::io::Cursor::new(with_part(&work, &part, xml.clone().into_bytes())?))?;

    // 4. connectors glued to the old shape follow to the new one (nearest connection point)
    let editor = GroupEditor::open(&staged, page_index)?;
    let new_points = editor.connection_points(&new_id);
    drop(editor);
    let nearest = |n: usize| -> Option<usize> {
        let old = old_points.iter().find(|p| p.0 == n)?;
        new_points.iter().min_by(|a, b| ((a.1 - old.1).powi(2) + (a.2 - old.2).powi(2)).total_cmp(&((b.1 - old.1).powi(2) + (b.2 - old.2).powi(2)))).map(|p| p.0)
    };
    let xml = rewrite_glue(&xml, shape_id, &new_id, &nearest);
    Ok((with_part(&staged, &part, xml.into_bytes())?, new_id))
}

fn fmt(v: f64) -> String {
    let t = format!("{v:.10}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" { "0".to_string() } else { t.to_string() }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}='");
    let at = tag.find(&key)? + key.len();
    Some(&tag[at..at + tag[at..].find('\'')?])
}

/// Point the Connect elements and the connector cell formulas that name `old` at `new`, with connection point `n` of the old shape becoming
/// `nearest(n)` of the new one (`None`: the new shape has no point, the end is glued to the whole shape).
fn rewrite_glue(xml: &str, old: &str, new: &str, nearest: &dyn Fn(usize) -> Option<usize>) -> String {
    let marker = format!("Sheet.{old}!");
    let mut out = String::with_capacity(xml.len());
    let mut at = 0usize;
    while let Some(rel) = xml[at..].find('<') {
        let start = at + rel;
        out.push_str(&xml[at..start]);
        let Some(close) = xml[start..].find('>') else { out.push_str(&xml[start..]); return out };
        let tag = &xml[start..=start + close];
        at = start + close + 1;
        if tag.starts_with("<Connect ") && attr(tag, "ToSheet") == Some(old) {
            let to_cell = attr(tag, "ToCell").unwrap_or("");
            let rewritten = if let Some(n) = to_cell.strip_prefix("Connections.X").and_then(|d| d.parse::<usize>().ok()) {
                match nearest(n) {
                    Some(k) => tag
                        .replace(&format!("ToSheet='{old}'"), &format!("ToSheet='{new}'"))
                        .replace(&format!("ToCell='Connections.X{n}'"), &format!("ToCell='Connections.X{k}'"))
                        .replace(&format!("ToPart='{}'", 100 + n - 1), &format!("ToPart='{}'", 100 + k - 1)),
                    None => tag.replace(&format!("ToSheet='{old}'"), &format!("ToSheet='{new}'")).replace(&format!("ToCell='{to_cell}'"), "ToCell='PinX'").replace(&format!(" ToPart='{}'", attr(tag, "ToPart").unwrap_or("")), " ToPart='3'"),
                }
            } else {
                tag.replace(&format!("ToSheet='{old}'"), &format!("ToSheet='{new}'"))
            };
            out.push_str(&rewritten);
        } else if tag.starts_with("<Cell ") && tag.contains(&marker) {
            out.push_str(&rewrite_formula(tag, &marker, new, nearest));
        } else {
            out.push_str(tag);
        }
    }
    out.push_str(&xml[at..]);
    out
}

/// `Sheet.<old>!Connections.X<n>` / `Y<n>` in a formula -> `Sheet.<new>!Connections.X<k>` / `Y<k>`; `Sheet.<old>!PinX` -> `Sheet.<new>!PinX`.
fn rewrite_formula(tag: &str, marker: &str, new: &str, nearest: &dyn Fn(usize) -> Option<usize>) -> String {
    let target = format!("Sheet.{new}!");
    let mut out = String::new();
    let mut from = 0usize;
    while let Some(i) = tag[from..].find(marker) {
        let p = from + i + marker.len();
        out.push_str(&tag[from..from + i]);
        out.push_str(&target);
        let rest = &tag[p..];
        if let Some(after) = rest.strip_prefix("Connections.") {
            let letter = &after[..1];
            let digits: String = after[1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let n = digits.parse::<usize>().unwrap_or(0);
            let k = nearest(n).unwrap_or(n);
            out.push_str(&format!("Connections.{letter}{k}"));
            from = p + "Connections.".len() + 1 + digits.len();
        } else {
            from = p;
        }
    }
    out.push_str(&tag[from..]);
    out
}
