//! Shape hyperlinks: read and replace the `Hyperlink` section of a shape.
//!
//! Visio (probe-hyperlinks.ps1, Visio 16.0, file visio-test-runs/probe-hyperlinks.vsdx): a link is a row of `<Section N='Hyperlink'>`
//! named `Row_1`, `Row_2` ... with the cells, in this order, `Description`, `Address`, `SubAddress`, `ExtraInfo`, `Frame`,
//! `NewWindow`, `Default`, `Invisible`, `SortKey`. An empty `Address` or `SubAddress` is written `V='' F='No Formula'`. A link to a page
//! of the same drawing has an empty Address and a SubAddress that holds the page name, with the formula
//! `Pages[<page name>]!ThePage!PAGENAME()` (so it follows a rename of the page). The section sits before the shape's `Geometry` section;
//! the page needs no relationship for it. COM reads it back as `Shape.Hyperlinks` (Address, SubAddress, Description, ExtraInfo, Frame,
//! NewWindow, IsDefaultLink, and the `Invisible` cell).

use crate::writer::{escape_attr, find_shape_range, with_part};
use crate::{Error, Package, Result};

/// One hyperlink of a shape.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hyperlink {
    /// The row name (`Row_1` ...); empty for a new link, which gets the next free `Row_<n>`.
    pub name: String,
    pub description: String,
    pub address: String,
    pub sub_address: String,
    pub extra_info: String,
    pub frame: String,
    pub new_window: bool,
    pub default: bool,
    pub invisible: bool,
}

const MAX_LINKS: usize = 64;
const MAX_TEXT: usize = 2048;

fn page_part(package: &Package, page_index: usize) -> Result<String> {
    Ok(package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone())
}

fn cell_text(row: roxmltree::Node, name: &str) -> String {
    row.children().find(|c| c.is_element() && c.attribute("N") == Some(name)).and_then(|c| c.attribute("V")).unwrap_or("").to_string()
}
fn cell_flag(row: roxmltree::Node, name: &str) -> bool {
    let v = cell_text(row, name);
    v.trim().parse::<f64>().map(|n| n != 0.0).unwrap_or(false)
}

/// The hyperlinks a shape of page `page_index` carries itself, in row order. Links a master supplies are not listed.
pub fn read_hyperlinks(package: &Package, page_index: usize, shape_id: &str) -> Result<Vec<Hyperlink>> {
    let part = page_part(package, page_index)?;
    let xml = package.text(&part)?.to_string();
    let (start, end) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let doc = roxmltree::Document::parse(&xml[start..end]).map_err(|e| Error(format!("shape xml: {e}")))?;
    let shape = doc.root_element();
    let mut out = Vec::new();
    for section in shape.children().filter(|n| n.is_element() && n.tag_name().name() == "Section" && n.attribute("N") == Some("Hyperlink")) {
        for row in section.children().filter(|n| n.is_element() && n.tag_name().name() == "Row") {
            if row.attribute("Del") == Some("1") {
                continue;
            }
            out.push(Hyperlink {
                name: row.attribute("N").unwrap_or("").to_string(),
                description: cell_text(row, "Description"),
                address: cell_text(row, "Address"),
                sub_address: cell_text(row, "SubAddress"),
                extra_info: cell_text(row, "ExtraInfo"),
                frame: cell_text(row, "Frame"),
                new_window: cell_flag(row, "NewWindow"),
                default: cell_flag(row, "Default"),
                invisible: cell_flag(row, "Invisible"),
            });
        }
    }
    Ok(out)
}

fn check(link: &Hyperlink) -> Result<()> {
    for (label, text) in [("description", &link.description), ("address", &link.address), ("sub-address", &link.sub_address), ("extra info", &link.extra_info), ("frame", &link.frame)] {
        if text.chars().count() > MAX_TEXT {
            return Err(Error(format!("the {label} of a hyperlink is longer than {MAX_TEXT} characters")));
        }
        if text.chars().any(|c| c.is_control()) {
            return Err(Error(format!("the {label} of a hyperlink has a control character")));
        }
    }
    if link.address.trim().is_empty() && link.sub_address.trim().is_empty() {
        return Err(Error("a hyperlink needs an address or a sub-address".to_string()));
    }
    Ok(())
}

fn cell(name: &str, value: &str, formula: Option<&str>) -> String {
    match formula {
        Some(f) => format!("<Cell N='{name}' V='{}' F='{}'/>", escape_attr(value), escape_attr(f)),
        None => format!("<Cell N='{name}' V='{}'/>", escape_attr(value)),
    }
}

fn flag(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

/// The page names of the drawing (what a sub-address may refer to).
fn page_names(package: &Package) -> Vec<String> {
    package.pages().map(|pages| pages.into_iter().map(|p| p.name).collect()).unwrap_or_default()
}

fn section_xml(links: &[Hyperlink], pages: &[String]) -> String {
    let mut used: Vec<String> = links.iter().filter(|l| !l.name.is_empty()).map(|l| l.name.clone()).collect();
    let mut rows = String::new();
    let mut seen_default = false;
    for link in links {
        let name = if link.name.is_empty() {
            let mut n = 1;
            while used.contains(&format!("Row_{n}")) {
                n += 1;
            }
            let fresh = format!("Row_{n}");
            used.push(fresh.clone());
            fresh
        } else {
            link.name.clone()
        };
        let sub_formula = (link.address.is_empty() && !link.sub_address.contains(']') && pages.iter().any(|p| *p == link.sub_address)).then(|| format!("Pages[{}]!ThePage!PAGENAME()", link.sub_address));
        let default = link.default && !seen_default;
        seen_default |= default;
        rows.push_str(&format!("<Row N='{}'>", escape_attr(&name)));
        rows.push_str(&cell("Description", &link.description, None));
        rows.push_str(&cell("Address", &link.address, link.address.is_empty().then_some("No Formula")));
        rows.push_str(&cell("SubAddress", &link.sub_address, sub_formula.as_deref().or(link.sub_address.is_empty().then_some("No Formula"))));
        rows.push_str(&cell("ExtraInfo", &link.extra_info, None));
        rows.push_str(&cell("Frame", &link.frame, None));
        rows.push_str(&cell("NewWindow", flag(link.new_window), None));
        rows.push_str(&cell("Default", flag(default), None));
        rows.push_str(&cell("Invisible", flag(link.invisible), None));
        rows.push_str(&cell("SortKey", "", None));
        rows.push_str("</Row>");
    }
    format!("<Section N='Hyperlink'>{rows}</Section>")
}

/// Replace ALL hyperlinks of shape `shape_id` on page `page_index` by `links` (an empty list removes the section). Rows that keep a
/// name keep it, new ones get the next free `Row_<n>`; only the first link marked default stays default. Returns the new package bytes.
pub fn set_hyperlinks(package: &Package, page_index: usize, shape_id: &str, links: &[Hyperlink]) -> Result<Vec<u8>> {
    if links.len() > MAX_LINKS {
        return Err(Error(format!("a shape can have at most {MAX_LINKS} hyperlinks")));
    }
    for link in links {
        check(link)?;
    }
    let part = page_part(package, page_index)?;
    let mut xml = package.text(&part)?.to_string();
    let (start, end) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let shape = xml[start..end].to_string();
    // only the shape's own part: a group's members come after `<Shapes`
    let own_end = shape.find("<Shapes").unwrap_or(shape.len());
    let own = &shape[..own_end];
    let section = if links.is_empty() { String::new() } else { section_xml(links, &page_names(package)) };
    let new_shape = if let Some((s, e)) = section_range(own) {
        format!("{}{}{}", &shape[..s], section, &shape[e..])
    } else if links.is_empty() {
        shape.clone()
    } else if shape.ends_with("/>") && !shape.contains("</Shape>") {
        // a self-closing `<Shape .../>` becomes a shape with this one section
        format!("{}>{}</Shape>", &shape[..shape.len() - 2], section)
    } else {
        let at = ["<Section N='Geometry'", "<Section N=\"Geometry\"", "<Text", "<Shapes"].iter().filter_map(|needle| own.find(needle)).min().unwrap_or_else(|| shape.rfind("</Shape>").unwrap_or(shape.len()));
        format!("{}{}{}", &shape[..at], section, &shape[at..])
    };
    xml.replace_range(start..end, &new_shape);
    with_part(package, &part, xml.into_bytes())
}

/// The byte range of the shape's own `Hyperlink` section inside `own`.
fn section_range(own: &str) -> Option<(usize, usize)> {
    let mut search = 0usize;
    while let Some(relative) = own[search..].find("<Section") {
        let start = search + relative;
        let close = own[start..].find('>')? + start;
        let tag = &own[start..=close];
        if tag.contains("N='Hyperlink'") || tag.contains("N=\"Hyperlink\"") {
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
