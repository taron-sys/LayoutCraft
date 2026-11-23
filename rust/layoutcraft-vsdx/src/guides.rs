//! Ruler guides: add, move and delete the `Type='Guide'` page shapes.
//!
//! Evidence: make-guides.mjs and check-guides-in-visio.ps1 (the files we write are opened and read back in Visio over COM).
//!
//! Visio (a Visio-authored fixture, test/corpus/layers-guides.vsdx): a horizontal ruler guide is a page-level `Shape Type='Guide'` with
//! `PinY` = its height, `PinX` 0, `Angle` 0 and `Width` / `Height` 0; a vertical one has `PinX` = its position and
//! `Angle` = 90 degrees (V='1.5707963267949' U='DEG'). The shape uses the document's "Guide" style when there is one.

use crate::writer::{next_shape_id, num_text, with_part};
use crate::{Error, Package, Result};

const QUARTER_TURN: &str = "1.5707963267949";

fn guide_style(package: &Package) -> String {
    // the StyleSheet called Guide in document.xml (Visio writes style 4), else the default style 0
    let Ok(document) = package.text("visio/document.xml") else { return "0".to_string() };
    for key in ["NameU='Guide'", "NameU=\"Guide\""] {
        if let Some(at) = document.find(key) {
            let start = document[..at].rfind("<StyleSheet ").unwrap_or(0);
            let tag = &document[start..at];
            for quote in ['\'', '"'] {
                if let Some(i) = tag.find(&format!("ID={quote}")) {
                    let rest = &tag[i + 4..];
                    if let Some(e) = rest.find(quote) {
                        return rest[..e].to_string();
                    }
                }
            }
        }
    }
    "0".to_string()
}

fn guide_xml(id: u64, style: &str, vertical: bool, position: f64) -> String {
    let (pin_x, pin_y, angle) = if vertical { (position, 0.0, format!("<Cell N='Angle' V='{QUARTER_TURN}' U='DEG'/>")) } else { (0.0, position, "<Cell N='Angle' V='0'/>".to_string()) };
    format!(
        "<Shape ID='{id}' Type='Guide' LineStyle='{style}' FillStyle='{style}' TextStyle='{style}'><Cell N='PinX' V='{}'/><Cell N='PinY' V='{}'/><Cell N='Width' V='0'/><Cell N='Height' V='0'/><Cell N='LocPinX' V='0' F='Width*0.5'/><Cell N='LocPinY' V='0' F='Height*0.5'/>{angle}<Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/><Cell N='ResizeMode' V='0'/><Cell N='TxtPinX' V='0' F='Width*0.5'/><Cell N='TxtPinY' V='0' F='Height*0.5'/><Cell N='TxtWidth' V='0'/><Cell N='TxtHeight' V='0' F='Height*1'/><Cell N='TxtLocPinX' V='0' F='TxtWidth*0.5'/><Cell N='TxtLocPinY' V='0' F='TxtHeight*0.5'/><Cell N='TxtAngle' V='0'/></Shape>",
        num_text(pin_x),
        num_text(pin_y)
    )
}

fn check_position(position: f64) -> Result<()> {
    if !position.is_finite() || position.abs() > 10_000.0 {
        return Err(Error("the guide's position is not a usable number".to_string()));
    }
    Ok(())
}

/// Add a guide to page `page_index`: a vertical one at x = `position`, or a horizontal one at y = `position` (inches, y up).
/// Returns the new package bytes and the new guide's shape ID.
pub fn add_guide(package: &Package, page_index: usize, vertical: bool, position: f64) -> Result<(Vec<u8>, String)> {
    check_position(position)?;
    let id = next_shape_id(package, page_index)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut xml = package.text(&page.part)?.to_string();
    let shape = guide_xml(id, &guide_style(package), vertical, position);
    if let Some(at) = xml.rfind("</Shapes>") {
        xml.insert_str(at, &shape);
    } else if let Some(at) = xml.find("<Shapes/>") {
        xml.replace_range(at..at + "<Shapes/>".len(), &format!("<Shapes>{shape}</Shapes>"));
    } else if let Some(at) = xml.rfind("</PageContents>") {
        xml.insert_str(at, &format!("<Shapes>{shape}</Shapes>"));
    } else if let Some(at) = xml.rfind("/>") {
        xml.replace_range(at..at + 2, &format!("><Shapes>{shape}</Shapes></PageContents>"));
    } else {
        return Err(Error("page has no root Shapes element".to_string()));
    }
    Ok((with_part(package, &page.part, xml.into_bytes())?, id.to_string()))
}

/// The byte range of the guide shape with ID `id` in a page part.
fn guide_range(xml: &str, id: &str) -> Result<(usize, usize)> {
    for quote in ['\'', '"'] {
        let mut from = 0;
        let key = format!("<Shape ID={quote}{id}{quote}");
        while let Some(found) = xml[from..].find(&key) {
            let start = from + found;
            let open_end = start + xml[start..].find('>').ok_or_else(|| Error("bad shape tag".to_string()))? + 1;
            if xml[start..open_end].contains("Type='Guide'") || xml[start..open_end].contains("Type=\"Guide\"") {
                let end = if xml[start..open_end].ends_with("/>") { open_end } else { open_end + xml[open_end..].find("</Shape>").ok_or_else(|| Error("unterminated guide".to_string()))? + "</Shape>".len() };
                if xml[open_end..end].contains("<Shape ") {
                    return Err(Error(format!("shape {id} is not a plain guide")));
                }
                return Ok((start, end));
            }
            from = open_end;
        }
    }
    Err(Error(format!("there is no guide with ID {id}")))
}

/// Delete guide `id` of page `page_index`.
pub fn delete_guide(package: &Package, page_index: usize, id: &str) -> Result<Vec<u8>> {
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut xml = package.text(&page.part)?.to_string();
    let (start, end) = guide_range(&xml, id)?;
    xml.replace_range(start..end, "");
    with_part(package, &page.part, xml.into_bytes())
}

/// Move guide `id` of page `page_index` to `position` (inches): its x for a vertical guide, its y for a horizontal one.
pub fn move_guide(package: &Package, page_index: usize, id: &str, position: f64) -> Result<Vec<u8>> {
    check_position(position)?;
    let pages = package.pages()?;
    let page = pages.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?;
    let mut xml = package.text(&page.part)?.to_string();
    let (start, end) = guide_range(&xml, id)?;
    let mut shape = xml[start..end].to_string();
    let cell_v = |shape: &str, name: &str| -> Option<f64> {
        let at = shape.find(&format!("<Cell N='{name}'"))?;
        let tag = &shape[at..at + shape[at..].find('>')? + 1];
        let v = tag.find(" V='")?;
        tag[v + 4..].split('\'').next()?.parse::<f64>().ok()
    };
    let vertical = (cell_v(&shape, "Angle").unwrap_or(0.0) - std::f64::consts::FRAC_PI_2).abs() < 0.1;
    let name = if vertical { "PinX" } else { "PinY" };
    let at = shape.find(&format!("<Cell N='{name}'")).ok_or_else(|| Error(format!("the guide has no {name}")))?;
    let close = at + shape[at..].find('>').ok_or_else(|| Error("bad cell".to_string()))? + 1;
    shape.replace_range(at..close, &format!("<Cell N='{name}' V='{}'/>", num_text(position)));
    xml.replace_range(start..end, &shape);
    with_part(package, &page.part, xml.into_bytes())
}
