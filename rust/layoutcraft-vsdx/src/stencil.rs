//! Masters of an external stencil (`.vssx` / `.vstx`): list them, copy one into a drawing's own
//! master list, make a small scratch drawing that shows one (for thumbnails).
//!
//! A stencil is an OPC package like a drawing: `visio/masters/masters.xml` lists the masters, each
//! with a `Rel` to `masterN.xml`, which may relate to pictures. Copying a master into a drawing
//! adds an entry to the drawing's `masters.xml` (new ID, new relationship id), the master part under
//! an unused number, its pictures under unused names and the content types for all of them; a drawing
//! that has no masters at all gets the masters part and its relationship from the document part.
//! A master that the drawing already has (same `UniqueID`, else same `NameU`) is not copied twice.
use std::collections::{BTreeMap, BTreeSet};

use crate::writer::{attr, escape_attr, set_attr, write_package};
use crate::{Error, MasterCatalog, Package, Result};

const MASTERS: &str = "visio/masters/masters.xml";
const DOC_RELS: &str = "visio/_rels/document.xml.rels";
const CONTENT_TYPES: &str = "[Content_Types].xml";
const REL_MASTER: &str = "http://schemas.microsoft.com/visio/2010/relationships/master";
const REL_MASTERS: &str = "http://schemas.microsoft.com/visio/2010/relationships/masters";
const FRESH_MASTERS: &str = "<?xml version='1.0' encoding='utf-8' ?><Masters xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'></Masters>";
const FRESH_RELS: &str = "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'></Relationships>";

/// The start and end of the `<Master ID='id' ...>...</Master>` element in a `masters.xml` text.
fn master_element(xml: &str, id: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    while let Some(relative) = xml[at..].find("<Master ") {
        let start = at + relative;
        let open_end = start + xml[start..].find('>')? + 1;
        if attr(&xml[start..open_end], "ID").as_deref() == Some(id) {
            let end = if xml[start..open_end].ends_with("/>") {
                open_end
            } else {
                open_end + xml[open_end..].find("</Master>")? + "</Master>".len()
            };
            return Some((start, end));
        }
        at = open_end;
    }
    None
}

/// Every `Master` element start tag of a `masters.xml` text.
fn master_tags(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(relative) = xml[at..].find("<Master ") {
        let start = at + relative;
        let Some(close) = xml[start..].find('>') else { break };
        out.push(xml[start..start + close + 1].to_string());
        at = start + close + 1;
    }
    out
}

/// The masters of a stencil (or drawing) as JSON `[{id, name, w, h}]`: size in inches of the master's
/// top shape (0 when unknown), hidden masters left out, in the order of their IDs.
pub fn master_list(package: &Package) -> Result<serde_json::Value> {
    let catalog = MasterCatalog::load(package)?;
    let xml = if package.contains(MASTERS) { package.text(MASTERS)?.to_string() } else { String::new() };
    let hidden: Vec<String> = master_tags(&xml)
        .iter()
        .filter(|tag| attr(tag, "Hidden").as_deref() == Some("1"))
        .filter_map(|tag| attr(tag, "ID"))
        .collect();
    let mut list: Vec<(u64, serde_json::Value)> = catalog
        .masters
        .values()
        .filter(|master| !hidden.contains(&master.id))
        .map(|master| {
            let top = master.top.as_ref().and_then(|top| master.shapes.get(top));
            let size = |cell: &str| {
                top.and_then(|shape| shape.cells.get(cell))
                    .and_then(|cell| cell.value.as_deref())
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            (
                master.id.parse::<u64>().unwrap_or(u64::MAX),
                serde_json::json!({ "id": master.id, "name": master.name, "w": size("Width"), "h": size("Height") }),
            )
        })
        .collect();
    list.sort_by_key(|(id, _)| *id);
    Ok(serde_json::Value::Array(list.into_iter().map(|(_, v)| v).collect()))
}

const STYLE_ATTRS: [&str; 3] = ["LineStyle", "FillStyle", "TextStyle"];

/// Every style sheet id the text refers to (`LineStyle='3'`, `FillStyle="6"`, `TextStyle='7'`).
fn style_references(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    rewrite_style_references(text, &mut |id| {
        found.insert(id.to_string());
        id.to_string()
    });
    found
}

/// The text with every style sheet id reference replaced by `map(id)`.
fn rewrite_style_references(text: &str, map: &mut dyn FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    'scan: while at < text.len() {
        for name in STYLE_ATTRS {
            for quote in ['\'', '"'] {
                let pattern = format!("{name}={quote}");
                if text[at..].starts_with(&pattern) {
                    let digits_start = at + pattern.len();
                    let digits_len = text[digits_start..].chars().take_while(|c| c.is_ascii_digit()).count();
                    if digits_len > 0 && text[digits_start + digits_len..].starts_with(quote) {
                        let id = &text[digits_start..digits_start + digits_len];
                        out.push_str(&pattern);
                        out.push_str(&map(id));
                        at = digits_start + digits_len;
                        continue 'scan;
                    }
                }
            }
        }
        let ch = text[at..].chars().next().unwrap_or(' ');
        out.push(ch);
        at += ch.len_utf8();
    }
    out
}

/// The `<StyleSheet ...>...</StyleSheet>` elements of a document part: `(id, NameU, element text)`.
fn style_sheets(document_xml: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(relative) = document_xml[at..].find("<StyleSheet ") {
        let start = at + relative;
        let Some(open_close) = document_xml[start..].find('>') else { break };
        let open_end = start + open_close + 1;
        let end = if document_xml[start..open_end].ends_with("/>") {
            open_end
        } else {
            match document_xml[open_end..].find("</StyleSheet>") {
                Some(n) => open_end + n + "</StyleSheet>".len(),
                None => break,
            }
        };
        let tag = &document_xml[start..open_end];
        out.push((attr(tag, "ID").unwrap_or_default(), attr(tag, "NameU").unwrap_or_default(), document_xml[start..end].to_string()));
        at = end;
    }
    out
}

/// Whether the style sheet `id`, or one it inherits from, takes values from the drawing's theme.
fn takes_theme(sheets: &[(String, String, String)], id: &str) -> bool {
    let mut queue = vec![id.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some((_, _, element)) = sheets.iter().find(|(i, _, _)| *i == id) else { continue };
        if element.contains("THEMEVAL(") || element.contains("THEME(") {
            return true;
        }
        queue.extend(style_references(&element[..element.find('>').map_or(element.len(), |n| n + 1)]));
    }
    false
}

/// What a style sheet element holds between its tags.
fn sheet_body(element: &str) -> &str {
    let from = element.find('>').map_or(element.len(), |n| n + 1);
    &element[from..element.rfind("</StyleSheet>").unwrap_or(from).max(from)]
}

/// Copy the style sheets that `used` (and the sheets they inherit from) name from the stencil's document
/// part into the drawing's. A sheet the drawing has under the same id and name is shared, unless the drawing's takes its
/// values from the theme and the stencil's does not (the shape would lose its own colours: black line art on white turns
/// into one theme colour). Any other is copied under a new id, with a new name when the name is taken. Returns the XML to add before `</StyleSheets>` and the old id -> new id map.
fn import_styles(target_document: &str, stencil_document: &str, used: BTreeSet<String>) -> (String, BTreeMap<String, String>) {
    let theirs = style_sheets(stencil_document);
    let our_sheets = style_sheets(target_document);
    let ours: BTreeMap<String, String> = our_sheets.iter().map(|(id, name, _)| (id.clone(), name.clone())).collect();
    // The sheets to bring: what is used and what those inherit from.
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = used.into_iter().collect();
    while let Some(id) = queue.pop() {
        if !wanted.insert(id.clone()) {
            continue;
        }
        if let Some((_, _, element)) = theirs.iter().find(|(i, _, _)| *i == id) {
            let open = &element[..element.find('>').map_or(element.len(), |n| n + 1)];
            queue.extend(style_references(open));
        }
    }
    let mut next = ours.keys().filter_map(|id| id.parse::<u64>().ok()).max().unwrap_or(0) + 1;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    // Sheets that an earlier drop copied already.
    let mut copied: BTreeSet<String> = BTreeSet::new();
    for id in &wanted {
        let Some((_, name, element)) = theirs.iter().find(|(i, _, _)| i == id) else {
            map.insert(id.clone(), id.clone());
            continue;
        };
        let same_sheet = ours.get(id).is_some_and(|ours_name| ours_name == name);
        if same_sheet && (takes_theme(&theirs, id) || !takes_theme(&our_sheets, id)) {
            map.insert(id.clone(), id.clone());
        } else if let Some((existing, _, _)) = our_sheets.iter().find(|(_, other, sheet)| {
            other.strip_prefix(name.as_str()).is_some_and(|rest| rest.starts_with('.')) && sheet_body(sheet) == sheet_body(element)
        }) {
            copied.insert(id.clone());
            map.insert(id.clone(), existing.clone());
        } else {
            map.insert(id.clone(), next.to_string());
            next += 1;
        }
    }
    let mut add = String::new();
    for id in &wanted {
        let new_id = &map[id];
        if copied.contains(id) || (new_id == id && ours.contains_key(id)) {
            continue;
        }
        let Some((_, name, element)) = theirs.iter().find(|(i, _, _)| i == id) else { continue };
        let open_end = element.find('>').map_or(element.len(), |n| n + 1);
        let mut open = set_attr(&element[..open_end], "ID", new_id);
        // Style names are unique in a drawing.
        if ours.values().any(|other| other == name) {
            let renamed = format!("{name}.{new_id}");
            for attribute in ["NameU", "Name"] {
                if attr(&open, attribute).is_some() {
                    open = set_attr(&open, attribute, &renamed);
                }
            }
        }
        let element = format!("{open}{}", &element[open_end..]);
        add.push_str(&rewrite_style_references(&element, &mut |old| map.get(old).cloned().unwrap_or_else(|| old.to_string())));
    }
    (add, map)
}

fn package_has_document(package: &Package) -> bool {
    package.contains("visio/document.xml")
}

fn largest_number(names: impl Iterator<Item = String>, prefix: &str, suffix: &str) -> u64 {
    names
        .filter_map(|name| name.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(suffix)).and_then(|n| n.parse::<u64>().ok()))
        .max()
        .unwrap_or(0)
}

fn content_type_for(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}


// ---- the document palette and fonts of a stencil ------------------------------------------------------------------

/// The cells that hold a colour (an index into the document palette, or `#rrggbb`).
const COLOUR_CELLS: [&str; 6] = ["FillForegnd", "FillBkgnd", "LineColor", "ShdwForegnd", "ShdwBkgnd", "Color"];

/// The document palette: the `ColorEntry` rows of `document.xml` (`IX` -> `#rrggbb`).
fn custom_palette(document_xml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut at = 0;
    while let Some(relative) = document_xml[at..].find("<ColorEntry") {
        let start = at + relative;
        let end = document_xml[start..].find('>').map_or(document_xml.len(), |n| start + n + 1);
        let tag = &document_xml[start..end];
        if let (Some(ix), Some(rgb)) = (attr(tag, "IX"), attr(tag, "RGB")) {
            let hex = rgb.trim().trim_start_matches('#').to_ascii_lowercase();
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                out.insert(ix, format!("#{hex}"));
            }
        }
        at = end;
    }
    out
}

/// Colour `index` of a document: its own entry, else Visio's built-in palette.
fn palette_colour(custom: &BTreeMap<String, String>, index: &str) -> Option<String> {
    custom.get(index).cloned().or_else(|| index.parse::<usize>().ok().and_then(|n| crate::styles::BUILTIN_COLORS.get(n).map(|c| c.to_string())))
}

/// Whether the two documents show some palette index in a different colour.
fn palettes_differ(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> bool {
    let mut keys: BTreeSet<String> = a.keys().chain(b.keys()).cloned().collect();
    keys.extend((0..24).map(|n| n.to_string()));
    keys.iter().any(|k| palette_colour(a, k) != palette_colour(b, k))
}

/// Replace the palette INDEXES in the colour cells of `xml` by the colours they have in `palette` (a cell with a theme
/// formula keeps its value: it follows the theme of the drawing, not a palette).
fn resolve_palette_indexes(xml: &str, palette: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut at = 0;
    while let Some(relative) = xml[at..].find("<Cell ") {
        let start = at + relative;
        let end = xml[start..].find('>').map_or(xml.len(), |n| start + n + 1);
        out.push_str(&xml[at..start]);
        let tag = &xml[start..end];
        let replaced = match (attr(tag, "N"), attr(tag, "V")) {
            (Some(name), Some(value))
                if COLOUR_CELLS.contains(&name.as_str())
                    && !value.is_empty()
                    && value.chars().all(|c| c.is_ascii_digit())
                    && !attr(tag, "F").is_some_and(|f| f.to_ascii_uppercase().contains("THEME")) =>
            {
                palette_colour(palette, &value).map(|hex| set_attr(tag, "V", &hex))
            }
            _ => None,
        };
        out.push_str(replaced.as_deref().unwrap_or(tag));
        at = end;
    }
    out.push_str(&xml[at..]);
    out
}

/// The font names the `Font` cells of `xml` use (not the themed ones).
fn fonts_used(xml: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(relative) = xml[at..].find("<Cell ") {
        let start = at + relative;
        let end = xml[start..].find('>').map_or(xml.len(), |n| start + n + 1);
        let tag = &xml[start..end];
        if attr(tag, "N").as_deref() == Some("Font") {
            if let Some(value) = attr(tag, "V") {
                if !value.is_empty() && value != "Themed" && !value.chars().all(|c| c.is_ascii_digit()) && !out.contains(&value) {
                    out.push(value);
                }
            }
        }
        at = end;
    }
    out
}

/// The `<FaceName .../>` element of `document_xml` that lists font `name` (whole, with its attributes), if any.
fn face_element(document_xml: &str, name: &str) -> Option<String> {
    let mut at = 0;
    while let Some(relative) = document_xml[at..].find("<FaceName") {
        let start = at + relative;
        let end = document_xml[start..].find('>').map_or(document_xml.len(), |n| start + n + 1);
        let tag = &document_xml[start..end];
        if attr(tag, "NameU").as_deref() == Some(name) || attr(tag, "Name").as_deref() == Some(name) {
            return Some(tag.to_string());
        }
        at = end;
    }
    None
}

/// `document_xml` listing the fonts `names` (the stencil's own `FaceName` element when it has one).
fn with_faces(document_xml: &str, stencil_document: &str, names: &[String]) -> Option<String> {
    let mut add = String::new();
    for name in names {
        let escaped = escape_attr(name);
        let listed = |text: &str| text.contains(&format!("NameU='{escaped}'")) || text.contains(&format!("NameU=\"{escaped}\""));
        if listed(document_xml) || listed(&add) {
            continue;
        }
        add.push_str(&face_element(stencil_document, name).unwrap_or_else(|| format!("<FaceName NameU='{escaped}'/>")));
    }
    if add.is_empty() {
        return None;
    }
    let mut xml = document_xml.to_string();
    if let Some(at) = xml.find("</FaceNames>") {
        xml.insert_str(at, &add);
    } else if let Some(at) = xml.find("<FaceNames/>") {
        xml.replace_range(at..at + "<FaceNames/>".len(), &format!("<FaceNames>{add}</FaceNames>"));
    } else {
        let at = xml.find("<StyleSheets").or_else(|| xml.find("</Colors>").map(|n| n + "</Colors>".len()))?;
        xml.insert_str(at, &format!("<FaceNames>{add}</FaceNames>"));
    }
    Some(xml)
}

/// Copy master `master_id` of `stencil` into `target`'s masters; returns the new package bytes and the
/// ID the master has in `target` (an existing master that is the same one is reused, nothing changes).
pub fn import_master(target: &Package, stencil: &Package, master_id: &str) -> Result<(Vec<u8>, String)> {
    if !stencil.contains(MASTERS) {
        return Err(Error("the stencil has no masters".to_string()));
    }
    let stencil_xml = stencil.text(MASTERS)?;
    let (start, end) = master_element(stencil_xml, master_id).ok_or_else(|| Error(format!("the stencil has no master {master_id}")))?;
    let element = &stencil_xml[start..end];
    let open_end = element.find('>').map_or(0, |n| n + 1);
    let open_tag = &element[..open_end];
    let unique = attr(open_tag, "UniqueID").filter(|u| !u.is_empty());
    let name_u = attr(open_tag, "NameU");

    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let have_masters = target.contains(MASTERS);
    let mut masters_xml = if have_masters { target.text(MASTERS)?.to_string() } else { FRESH_MASTERS.to_string() };
    // The drawing has this master already.
    for tag in master_tags(&masters_xml) {
        let same = match (&unique, attr(&tag, "UniqueID")) {
            (Some(u), Some(t)) => *u == t,
            _ => name_u.is_some() && attr(&tag, "NameU") == name_u,
        };
        if same {
            if let Some(id) = attr(&tag, "ID") {
                return Ok((write_package(target, extras)?, id));
            }
        }
    }
    // Which part holds the master, and what it relates to.
    let rel_tag_start = element.find("<Rel ").ok_or_else(|| Error(format!("master {master_id} has no Rel")))?;
    let rel_tag_end = rel_tag_start + element[rel_tag_start..].find('>').ok_or_else(|| Error("bad Rel element".to_string()))? + 1;
    let rel_tag = &element[rel_tag_start..rel_tag_end];
    let rel_id = attr(rel_tag, "r:id").ok_or_else(|| Error(format!("master {master_id} has no relationship id")))?;
    let stencil_rels = stencil.relationships(MASTERS)?;
    let part = stencil_rels.get(&rel_id).ok_or_else(|| Error(format!("the stencil has no relationship {rel_id}")))?.target.clone();
    let mut part_text = String::from_utf8(stencil.entry(&part).ok_or_else(|| Error(format!("the stencil has no part {part}")))?.to_vec())
        .map_err(|_| Error(format!("{part} is not UTF-8")))?;
    // The style sheets the master's shapes (and its page sheet) use come along, renumbered when they differ.
    let mut used = style_references(&part_text);
    used.extend(style_references(element));
    let (style_xml, style_map) = if package_has_document(target) && stencil.contains("visio/document.xml") {
        import_styles(target.text("visio/document.xml")?, stencil.text("visio/document.xml")?, used)
    } else {
        (String::new(), BTreeMap::new())
    };
    if style_map.iter().any(|(old, new)| old != new) {
        part_text = rewrite_style_references(&part_text, &mut |old| style_map.get(old).cloned().unwrap_or_else(|| old.to_string()));
    }
    // The palette of the stencil's document and its fonts come along: a palette INDEX in a colour cell would show
    // another colour in a drawing with a different palette, so the indexes are replaced by the colours they stand for.
    let (palette, faces): (Option<BTreeMap<String, String>>, Vec<String>) = if package_has_document(target) && stencil.contains("visio/document.xml") {
        let from = custom_palette(stencil.text("visio/document.xml")?);
        let to = custom_palette(target.text("visio/document.xml")?);
        (palettes_differ(&from, &to).then_some(from), fonts_used(&part_text))
    } else {
        (None, Vec::new())
    };
    let mut style_xml = style_xml;
    let mut element_override: Option<String> = None;
    if let Some(palette) = &palette {
        part_text = resolve_palette_indexes(&part_text, palette);
        style_xml = resolve_palette_indexes(&style_xml, palette);
        element_override = Some(resolve_palette_indexes(element, palette));
    }
    let part_bytes = part_text.into_bytes();

    // New numbers.
    let all_names: Vec<String> = target.names().map(str::to_string).collect();
    let new_part_no = largest_number(all_names.iter().cloned(), "visio/masters/master", ".xml") + 1;
    let new_part = format!("visio/masters/master{new_part_no}.xml");
    let new_id = master_tags(&masters_xml)
        .iter()
        .filter_map(|tag| attr(tag, "ID").and_then(|id| id.parse::<u64>().ok()))
        .max()
        .map_or(1, |m| m + 1)
        .to_string();
    let existing_rels = if have_masters { target.relationships(MASTERS)? } else { BTreeMap::new() };
    let new_rel_no = largest_number(existing_rels.keys().cloned(), "rId", "") + 1;
    let new_rel_id = format!("rId{new_rel_no}");

    // The master part's pictures.
    let mut content_types = target.text(CONTENT_TYPES)?.to_string();
    let master_dir = part.rsplit_once('/').map_or("", |(dir, _)| dir).to_string();
    let rels_part = format!("{}/_rels/{}.rels", master_dir, part.rsplit('/').next().unwrap_or(""));
    if stencil.contains(&rels_part) {
        let resolved = stencil.relationships(&part)?;
        let rels_doc = stencil.xml(&rels_part)?;
        let mut rels_out = String::from("<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>");
        let mut taken: Vec<String> = all_names.clone();
        for node in rels_doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Relationship") {
            let id = node.attribute("Id").unwrap_or("");
            let kind = node.attribute("Type").unwrap_or("");
            if node.attribute("TargetMode") == Some("External") {
                rels_out.push_str(&format!(
                    "<Relationship Id='{}' Type='{}' Target='{}' TargetMode='External'/>",
                    escape_attr(id),
                    escape_attr(kind),
                    escape_attr(node.attribute("Target").unwrap_or(""))
                ));
                continue;
            }
            let Some(source) = resolved.get(id).map(|r| r.target.clone()) else { continue };
            let Some(bytes) = stencil.entry(&source) else { continue };
            let file = source.rsplit('/').next().unwrap_or("media").to_string();
            let (stem, extension) = file.rsplit_once('.').map_or((file.as_str(), ""), |(s, e)| (s, e));
            let mut k = 1;
            let new_name = loop {
                let candidate = format!("visio/media/stencil{k}_{stem}.{extension}");
                if !taken.contains(&candidate) && !extras.contains_key(&candidate) {
                    break candidate;
                }
                k += 1;
            };
            taken.push(new_name.clone());
            extras.insert(new_name.clone(), bytes.to_vec());
            if !extension.is_empty() && !content_types.to_ascii_lowercase().contains(&format!("extension=\"{}\"", extension.to_ascii_lowercase())) && !content_types.to_ascii_lowercase().contains(&format!("extension='{}'", extension.to_ascii_lowercase())) {
                let at = content_types.rfind("</Types>").ok_or_else(|| Error("bad [Content_Types].xml".to_string()))?;
                content_types.insert_str(at, &format!("<Default Extension='{}' ContentType='{}'/>", escape_attr(extension), content_type_for(extension)));
            }
            rels_out.push_str(&format!(
                "<Relationship Id='{}' Type='{}' Target='../media/{}'/>",
                escape_attr(id),
                escape_attr(kind),
                escape_attr(new_name.rsplit('/').next().unwrap_or(""))
            ));
        }
        rels_out.push_str("</Relationships>");
        extras.insert(format!("visio/masters/_rels/master{new_part_no}.xml.rels"), rels_out.into_bytes());
    }
    extras.insert(new_part.clone(), part_bytes);
    // Content type of the master part (and of the masters part when it is new).
    let at = content_types.rfind("</Types>").ok_or_else(|| Error("bad [Content_Types].xml".to_string()))?;
    let mut overrides = format!("<Override PartName='/{new_part}' ContentType='application/vnd.ms-visio.master+xml'/>");
    if !have_masters {
        overrides.push_str("<Override PartName='/visio/masters/masters.xml' ContentType='application/vnd.ms-visio.masters+xml'/>");
    }
    content_types.insert_str(at, &overrides);
    extras.insert(CONTENT_TYPES.to_string(), content_types.into_bytes());

    // masters.xml: the new Master element.
    let element = element_override.as_deref().unwrap_or(element);
    let open_tag = &element[..element.find('>').map_or(0, |n| n + 1)];
    let mut new_element = element.replacen(open_tag, &set_attr(open_tag, "ID", &new_id), 1);
    if style_map.iter().any(|(old, new)| old != new) {
        new_element = rewrite_style_references(&new_element, &mut |old| style_map.get(old).cloned().unwrap_or_else(|| old.to_string()));
    }
    if let Some(at) = new_element.find("<Rel ") {
        let close = at + new_element[at..].find('>').unwrap_or(0) + 1;
        let tag = new_element[at..close].to_string();
        new_element.replace_range(at..close, &set_attr(&tag, "r:id", &new_rel_id));
    }
    let at = masters_xml.rfind("</Masters>").ok_or_else(|| Error("masters.xml has no end".to_string()))?;
    masters_xml.insert_str(at, &new_element);
    // The new Rel element uses the `r:` prefix: the root must declare it.
    if let Some(root_start) = masters_xml.find("<Masters") {
        let root_end = root_start + masters_xml[root_start..].find('>').unwrap_or(0);
        if !masters_xml[root_start..root_end].contains("xmlns:r") {
            masters_xml.insert_str(root_start + "<Masters".len(), " xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'");
        }
    }
    extras.insert(MASTERS.to_string(), masters_xml.into_bytes());
    if !style_xml.is_empty() {
        let mut document = target.text("visio/document.xml")?.to_string();
        let at = document.rfind("</StyleSheets>").ok_or_else(|| Error("document.xml has no style sheet list".to_string()))?;
        document.insert_str(at, &style_xml);
        extras.insert("visio/document.xml".to_string(), document.into_bytes());
    }
    if !faces.is_empty() {
        let current = match extras.get("visio/document.xml") {
            Some(bytes) => String::from_utf8_lossy(bytes).to_string(),
            None => target.text("visio/document.xml")?.to_string(),
        };
        if let Some(document) = with_faces(&current, stencil.text("visio/document.xml")?, &faces) {
            extras.insert("visio/document.xml".to_string(), document.into_bytes());
        }
    }

    // masters.xml.rels: the relationship to the new part (and the document's relationship to masters.xml).
    let masters_rels_part = "visio/masters/_rels/masters.xml.rels";
    let mut masters_rels = if target.contains(masters_rels_part) { target.text(masters_rels_part)?.to_string() } else { FRESH_RELS.to_string() };
    let at = masters_rels.rfind("</Relationships>").ok_or_else(|| Error("bad masters.xml.rels".to_string()))?;
    masters_rels.insert_str(at, &format!("<Relationship Id='{new_rel_id}' Type='{REL_MASTER}' Target='master{new_part_no}.xml'/>"));
    extras.insert(masters_rels_part.to_string(), masters_rels.into_bytes());
    if !have_masters {
        let mut doc_rels = target.text(DOC_RELS)?.to_string();
        let doc_numbers = target.relationships("visio/document.xml")?;
        let doc_rel_no = largest_number(doc_numbers.keys().cloned(), "rId", "") + 1;
        let at = doc_rels.rfind("</Relationships>").ok_or_else(|| Error("bad document.xml.rels".to_string()))?;
        doc_rels.insert_str(at, &format!("<Relationship Id='rId{doc_rel_no}' Type='{REL_MASTERS}' Target='masters/masters.xml'/>"));
        extras.insert(DOC_RELS.to_string(), doc_rels.into_bytes());
    }
    Ok((write_package(target, extras)?, new_id))
}

/// A one-page scratch drawing that holds one instance of stencil master `master_id`, for drawing its
/// thumbnail: returns the package bytes and the instance's shape ID. The page is empty otherwise, so the
/// drawing of the page is the master alone.
pub fn scratch_drawing_with_master(stencil: &Package, master_id: &str) -> Result<(Vec<u8>, String)> {
    let document: layoutcraft_model::Document = serde_json::from_value(serde_json::json!({
        "pages": [{ "name": "Page-1", "shapes": [], "connectors": [] }]
    }))
    .map_err(|e| Error(e.to_string()))?;
    let fresh = Package::from_reader(std::io::Cursor::new(crate::write_fresh_document(&document)?))?;
    let (bytes, new_id) = import_master(&fresh, stencil, master_id)?;
    let with_master = Package::from_reader(std::io::Cursor::new(bytes))?;
    crate::writer::add_master_instance(&with_master, 0, &new_id, 4.25, 5.5)
}
