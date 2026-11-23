//! Adding a connection point to a shape (Visio's Connection Point tool, `Shape.AddRow` on the Connection section).
//!
//! Measured with probe-add-connection-point.ps1 (round 11b): a drawn shape gets a new `<Row T='Connection' IX='n'>` in its own Connection
//! section (cells X, Y with the formulas `Width*fx` / `Height*fy`, DirX 0, DirY 0, Type 0, AutoGen 0, Prompt); an INSTANCE of a master that
//! has connection points gets the whole section written out on the instance, every inherited row copied with its formulas, and the new
//! row last. The section stands before the Geometry sections. Glued connectors address the point by its 1-based number (`IX + 1`), so the
//! new point never changes the number of an existing one.

use crate::writer::{find_shape_range, with_part};
use crate::{Error, Package, Result};
use std::fmt::Write as _;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\'', "&apos;").replace('"', "&quot;")
}

fn section_range(own: &str) -> Option<(usize, usize)> {
    let mut search = 0usize;
    while let Some(relative) = own[search..].find("<Section") {
        let start = search + relative;
        let close = own[start..].find('>')? + start;
        let tag = &own[start..=close];
        if tag.contains("N='Connection'") || tag.contains("N=\"Connection\"") {
            if tag.ends_with("/>") {
                return Some((start, close + 1));
            }
            let end = own[close..].find("</Section>")? + close + "</Section>".len();
            return Some((start, end));
        }
        search = close + 1;
    }
    None
}

fn fmt(v: f64) -> String {
    let t = format!("{v:.10}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" { "0".to_string() } else { t.to_string() }
}

enum Edit {
    Add { fx: f64, fy: f64 },
    Move { n: usize, fx: f64, fy: f64 },
    Delete { n: usize },
}

fn in_box(fx: f64, fy: f64) -> Result<()> {
    if !(fx.is_finite() && fy.is_finite()) || !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) {
        return Err(Error("a connection point lies inside the shape's box (0 to 1 of its width and height)".to_string()));
    }
    Ok(())
}

/// Add a connection point at (`fx`, `fy`) of the shape's box (0..1 of Width and Height, from the bottom left) to shape `shape_id` of page
/// `page_index`. Returns the new package bytes and the 1-based number of the new point (as in `Connections.X<n>`).
pub fn add_connection_point(package: &Package, page_index: usize, shape_id: &str, fx: f64, fy: f64) -> Result<(Vec<u8>, usize)> {
    in_box(fx, fy)?;
    rewrite(package, page_index, shape_id, Edit::Add { fx, fy })
}

/// Move connection point `n` (1-based, as in `Connections.X<n>`) of the shape to (`fx`, `fy`) of its box: only that row's X and Y formulas
/// change (`Width*fx`, `Height*fy`), so the numbers of all points, and the glue of connectors to them, stay as they are. The caller routes the
/// glued connectors again.
pub fn move_connection_point(package: &Package, page_index: usize, shape_id: &str, n: usize, fx: f64, fy: f64) -> Result<Vec<u8>> {
    in_box(fx, fy)?;
    Ok(rewrite(package, page_index, shape_id, Edit::Move { n, fx, fy })?.0)
}

fn rewrite(package: &Package, page_index: usize, shape_id: &str, edit: Edit) -> Result<(Vec<u8>, usize)> {
    let editor = crate::groupedit::GroupEditor::open(package, page_index)?;
    let (width, height, rows) = editor.connection_rows(shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found or has no size")))?;
    if let Edit::Add { .. } = edit {
        if rows.len() >= 200 {
            return Err(Error("a shape can have at most 200 connection points".to_string()));
        }
    }
    if let Edit::Move { n, .. } | Edit::Delete { n } = edit {
        if !rows.iter().any(|(ix, _)| ix + 1 == n) {
            return Err(Error(format!("shape {shape_id} has no connection point {n}")));
        }
    }
    let next = rows.iter().map(|(ix, _)| ix + 1).max().unwrap_or(0);
    let xy = |fx: f64, fy: f64| (format!(" V='{}' F='Width*{}'", fmt(width * fx), fmt(fx)), format!(" V='{}' F='Height*{}'", fmt(height * fy), fmt(fy)));
    let mut section = String::from("<Section N='Connection'>");
    let mut kept = 0usize; // Delete: the rows after the deleted one are renumbered without a gap, as Visio does
    for (ix, cells) in &rows {
        let ix = &if let Edit::Delete { n } = edit {
            if ix + 1 == n { continue; }
            kept += 1;
            kept - 1
        } else { *ix };
        let _ = write!(section, "<Row T='Connection' IX='{ix}'>");
        for name in ["X", "Y", "DirX", "DirY", "Type", "AutoGen", "Prompt"] {
            if let Edit::Move { n, fx, fy } = edit {
                if ix + 1 == n && (name == "X" || name == "Y") {
                    let (x, y) = xy(fx, fy);
                    let _ = write!(section, "<Cell N='{name}'{}/>", if name == "X" { x } else { y });
                    continue;
                }
            }
            if let Some(cell) = cells.get(name) {
                let _ = write!(section, "<Cell N='{name}'");
                if let Some(v) = &cell.value { let _ = write!(section, " V='{}'", escape(v)); }
                if let Some(f) = &cell.formula { let _ = write!(section, " F='{}'", escape(f)); }
                if let Some(u) = &cell.unit { let _ = write!(section, " U='{}'", escape(u)); }
                section.push_str("/>");
            }
        }
        section.push_str("</Row>");
    }
    if let Edit::Add { fx, fy } = edit {
        let (x, y) = xy(fx, fy);
        let _ = write!(section, "<Row T='Connection' IX='{next}'><Cell N='X'{x}/><Cell N='Y'{y}/><Cell N='DirX' V='0'/><Cell N='DirY' V='0'/><Cell N='Type' V='0'/><Cell N='AutoGen' V='0'/><Cell N='Prompt' V='' F='No Formula'/></Row>");
    }
    section.push_str("</Section>");
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let mut xml = package.text(&part)?.to_string();
    let (start, end) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let shape = xml[start..end].to_string();
    let own_end = shape.find("<Shapes").unwrap_or(shape.len());
    let own = &shape[..own_end];
    let new_shape = if let Some((s, e)) = section_range(own) {
        format!("{}{}{}", &shape[..s], section, &shape[e..])
    } else if shape.ends_with("/>") && !shape.contains("</Shape>") {
        format!("{}>{}</Shape>", &shape[..shape.len() - 2], section)
    } else {
        let at = ["<Section N='Geometry'", "<Section N=\"Geometry\"", "<Text", "<Shapes"].iter().filter_map(|needle| own.find(needle)).min().unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
        format!("{}{}{}", &shape[..at], section, &shape[at..])
    };
    xml.replace_range(start..end, &new_shape);
    if let Edit::Delete { n } = edit {
        xml = renumber_glue(&xml, shape_id, n);
    }
    Ok((with_part(package, &part, xml.into_bytes())?, match edit { Edit::Add { .. } => next + 1, Edit::Move { n, .. } | Edit::Delete { n } => n }))
}

/// Delete connection point `n` (1-based) of the shape. Measured with probe-delete-connection-point.ps1 (Visio `Shape.DeleteRow`): the rows after it
/// are renumbered, a connector glued to a LATER point of this shape keeps its glue under the new number (its cell formulas and the page's Connect
/// elements say `Connections.X<k-1>` / `Y<k-1>`, ToPart one lower), a connector glued to the deleted point is unglued: its end cells lose their
/// formulas but keep their values (the end stays where it was) and the Connect element goes.
pub fn delete_connection_point(package: &Package, page_index: usize, shape_id: &str, n: usize) -> Result<Vec<u8>> {
    Ok(rewrite(package, page_index, shape_id, Edit::Delete { n })?.0)
}

/// The value of attribute `name` in the tag text `tag` (single quotes, as Visio and this crate write them).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}='");
    let at = tag.find(&key)? + key.len();
    Some(&tag[at..at + tag[at..].find('\'')?])
}

/// Renumber the glue to shape `shape_id`'s connection points after point `n` was deleted (see `delete_connection_point`).
fn renumber_glue(xml: &str, shape_id: &str, n: usize) -> String {
    let marker = format!("Sheet.{shape_id}!Connections.");
    let mut out = String::with_capacity(xml.len());
    let mut at = 0usize;
    while let Some(relative) = xml[at..].find('<') {
        let start = at + relative;
        out.push_str(&xml[at..start]);
        let Some(close) = xml[start..].find('>') else { out.push_str(&xml[start..]); return out };
        let tag = &xml[start..=start + close];
        at = start + close + 1;
        if tag.starts_with("<Cell ") && tag.contains(&marker) {
            out.push_str(&rewrite_cell(tag, &marker, n));
        } else if tag.starts_with("<Connect ") && attr(tag, "ToSheet") == Some(shape_id) {
            let to_cell = attr(tag, "ToCell").unwrap_or("");
            let k = to_cell.strip_prefix("Connections.X").and_then(|d| d.parse::<usize>().ok());
            match k {
                Some(k) if k == n => {} // unglued: the Connect element goes
                Some(k) if k > n => {
                    let part = attr(tag, "ToPart").and_then(|p| p.parse::<usize>().ok()).map_or(String::new(), |p| format!(" ToPart='{}'", p.saturating_sub(1)));
                    let rewritten = tag.replace(&format!("ToCell='Connections.X{k}'"), &format!("ToCell='Connections.X{}'", k - 1));
                    match attr(tag, "ToPart") {
                        Some(p) => out.push_str(&rewritten.replace(&format!(" ToPart='{p}'"), &part)),
                        None => out.push_str(&rewritten),
                    }
                }
                _ => out.push_str(tag),
            }
        } else {
            out.push_str(tag);
        }
    }
    out.push_str(&xml[at..]);
    out
}

/// A `<Cell .../>` whose formula mentions `Sheet.<id>!Connections.X<k>` / `Y<k>`: points above `n` move down by one; a formula that mentions the
/// deleted point is dropped (the cached value stays).
fn rewrite_cell(tag: &str, marker: &str, n: usize) -> String {
    let mut ks = Vec::new();
    let mut from = 0usize;
    while let Some(i) = tag[from..].find(marker) {
        let p = from + i + marker.len();
        let digits: String = tag[p + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(k) = digits.parse::<usize>() { ks.push(k); }
        from = p;
    }
    if ks.contains(&n) {
        let Some(f) = tag.find(" F='") else { return tag.to_string() };
        let end = tag[f + 4..].find('\'').map_or(tag.len(), |e| f + 4 + e + 1);
        return format!("{}{}", &tag[..f], &tag[end..]);
    }
    let mut out = String::new();
    let mut from = 0usize;
    while let Some(i) = tag[from..].find(marker) {
        let p = from + i + marker.len();
        out.push_str(&tag[from..p]);
        let letter = &tag[p..p + 1];
        let digits: String = tag[p + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
        let k = digits.parse::<usize>().unwrap_or(0);
        out.push_str(letter);
        out.push_str(&(if k > n { k - 1 } else { k }).to_string());
        from = p + 1 + digits.len();
    }
    out.push_str(&tag[from..]);
    out
}
