//! Named styles (THEME-07, step 293): list the style sheets of a drawing, apply one to shapes, create or change a custom one.
//!
//! Measured on the installed Visio (probe-styles.ps1, probe-styles2.ps1):
//! * `Shape.Style = name` sets the line, the fill and the text style at once; `LineStyle`, `FillStyle` and `TextStyle` set one each.
//! * Applying a style also CLEARS the shape's own cells of that category, so the shape inherits from the style again: a line style resets
//!   `LineColor`, `LineWeight`, `LinePattern`, `Rounding`, `LineCap` and the arrows; a fill style resets `FillForegnd`, `FillBkgnd`,
//!   `FillPattern`, the transparencies and the shadow cells (`ShdwPattern`); a text style removes the `Character` and `Paragraph` sections and
//!   resets `VerticalAlign` and `TextBkgnd`. Cells of other categories stay.
//! * A style created in Visio is a `StyleSheet` in `visio/document.xml` (`ID` = the next free number, `LineStyle`, `FillStyle`, `TextStyle` =
//!   the id of the style it is based on, `Enable*Props` cells, and the cells it sets itself; the others inherit from the based-on style).
use std::collections::BTreeMap;

use crate::writer::{attr, escape_attr, find_shape_range, set_attr, with_part, write_package_edit};
use crate::{Error, Package, Result};

const DOCUMENT: &str = "visio/document.xml";

/// The styles every Visio drawing has (Visio marks them `IsCustomNameU` too, so the flag does not tell them from the styles a person made).
const BUILT_IN: [&str; 6] = ["No Style", "Text Only", "None", "Normal", "Guide", "Theme"];

/// Cells a line style resets on a shape.
const LINE_CELLS: [&str; 11] = ["LinePattern", "LineWeight", "LineColor", "LineColorTrans", "LineCap", "Rounding", "BeginArrow", "EndArrow", "BeginArrowSize", "EndArrowSize", "CompoundType"];
/// Cells a fill style resets on a shape.
const FILL_CELLS: [&str; 16] = [
    "FillForegnd", "FillBkgnd", "FillPattern", "FillForegndTrans", "FillBkgndTrans", "ShdwForegnd", "ShdwPattern", "ShdwForegndTrans", "ShapeShdwType", "ShapeShdwOffsetX",
    "ShapeShdwOffsetY", "ShapeShdwObliqueAngle", "ShapeShdwScaleFactor", "ShapeShdwBlur", "ShapeShdwShow", "ShdwBkgnd",
];
/// Cells a text style resets on a shape (its `Character` and `Paragraph` sections go as well).
const TEXT_CELLS: [&str; 3] = ["VerticalAlign", "TextBkgnd", "TextBkgndTrans"];

/// One `StyleSheet` element of the document part: `(start, end, opening tag)`.
fn sheets(xml: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = xml[from..].find("<StyleSheet ") {
        let start = from + found;
        let Some(open_end) = xml[start..].find('>').map(|n| n + start) else { break };
        let tag = xml[start..=open_end].to_string();
        let end = if tag.ends_with("/>") { open_end + 1 } else { xml[open_end..].find("</StyleSheet>").map_or(xml.len(), |n| n + open_end + "</StyleSheet>".len()) };
        out.push((start, end, tag));
        from = end;
    }
    out
}

/// The `V` of the cell `name` of a style sheet's XML (its own cell; None when it inherits).
fn cell_value(sheet: &str, name: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let pattern = format!("<Cell N={quote}{name}{quote}");
        if let Some(at) = sheet.find(&pattern) {
            let end = sheet[at..].find('>')? + at;
            if sheet[at..end].contains(" F='Inh'") || sheet[at..end].contains(" F=\"Inh\"") {
                return None;
            }
            return attr(&sheet[at..end], "V");
        }
    }
    None
}

/// The style sheets as JSON `[{id, name, line, fill, text, custom, lineColor, lineWeight, fillColor, fillPattern}]` (`line` / `fill` / `text` = the ids of the styles it is based on).
pub fn list_styles(package: &Package) -> Result<String> {
    let xml = package.text(DOCUMENT)?;
    let mut rows = Vec::new();
    for (start, end, tag) in sheets(xml) {
        let sheet = &xml[start..end];
        let name = attr(&tag, "NameU").or_else(|| attr(&tag, "Name")).unwrap_or_default();
        let get = |n: &str| cell_value(sheet, n).map(serde_json::Value::from).unwrap_or(serde_json::Value::Null);
        rows.push(serde_json::json!({
            "id": attr(&tag, "ID").unwrap_or_default(),
            "name": name,
            "line": attr(&tag, "LineStyle"), "fill": attr(&tag, "FillStyle"), "text": attr(&tag, "TextStyle"),
            "custom": !BUILT_IN.contains(&name.as_str()),
            "lineColor": get("LineColor"), "lineWeight": get("LineWeight"), "fillColor": get("FillForegnd"), "fillPattern": get("FillPattern"),
            "textColor": get("Color"), "textSize": get("Size"), "textStyle": get("Style"),
            "paraAlign": get("HorzAlign"), "indentFirst": get("IndFirst"), "indentLeft": get("IndLeft"), "indentRight": get("IndRight"), "lineSpacing": get("SpLine"), "spaceBefore": get("SpBefore"), "spaceAfter": get("SpAfter"),
        }));
    }
    Ok(serde_json::Value::Array(rows).to_string())
}

/// Remove the cell `name` (a direct cell of the shape: self-closing or with content) from `xml`; true when one went.
fn remove_cell(xml: &mut String, name: &str) -> bool {
    for quote in ['\'', '"'] {
        let pattern = format!("<Cell N={quote}{name}{quote}");
        if let Some(at) = xml.find(&pattern) {
            let Some(close) = xml[at..].find('>').map(|n| n + at) else { return false };
            let end = if xml[..close].ends_with('/') { close + 1 } else { xml[close..].find("</Cell>").map_or(close + 1, |n| n + close + "</Cell>".len()) };
            xml.replace_range(at..end, "");
            return true;
        }
    }
    false
}

/// Remove the section `name` (`Character`, `Paragraph`) of a shape's own XML.
fn remove_section(xml: &mut String, name: &str) {
    for quote in ['\'', '"'] {
        let pattern = format!("<Section N={quote}{name}{quote}");
        while let Some(at) = xml.find(&pattern) {
            let Some(close) = xml[at..].find('>').map(|n| n + at) else { return };
            let end = if xml[..close].ends_with('/') { close + 1 } else { xml[close..].find("</Section>").map_or(close + 1, |n| n + close + "</Section>".len()) };
            xml.replace_range(at..end, "");
        }
    }
}

/// Apply the style `style_id` to the shapes `ids` of page `page_index`. `scope`: `all` (line, fill and text, like `Shape.Style`), `line`, `fill` or `text`.
pub fn apply_style(package: &Package, page_index: usize, ids: &[String], style_id: &str, scope: &str) -> Result<Vec<u8>> {
    let (line, fill, text) = match scope {
        "all" => (true, true, true),
        "line" => (true, false, false),
        "fill" => (false, true, false),
        "text" => (false, false, true),
        other => return Err(Error(format!("unknown style scope '{other}' (all, line, fill, text)"))),
    };
    let document = package.text(DOCUMENT)?;
    if !sheets(document).iter().any(|(_, _, tag)| attr(tag, "ID").as_deref() == Some(style_id)) {
        return Err(Error(format!("the drawing has no style {style_id}")));
    }
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let mut xml = package.text(&part)?.to_string();
    for id in ids {
        let (start, end) = find_shape_range(&xml, id).ok_or_else(|| Error(format!("shape {id} not found")))?;
        // the shape's own part: up to its child shapes
        let own_end = xml[start..end].find("<Shapes>").map_or(end, |n| start + n);
        let mut own = xml[start..own_end].to_string();
        let open_end = own.find('>').ok_or_else(|| Error(format!("shape {id} is broken")))?;
        let mut tag = own[..=open_end].to_string();
        let self_closing = tag.ends_with("/>");
        if self_closing {
            // a shape without content: only the attributes change
            let mut t = tag.clone();
            for (flag, name) in [(line, "LineStyle"), (fill, "FillStyle"), (text, "TextStyle")] {
                if flag {
                    t = set_attr(&t, name, style_id);
                }
            }
            xml.replace_range(start..own_end, &t);
            continue;
        }
        for (flag, name) in [(line, "LineStyle"), (fill, "FillStyle"), (text, "TextStyle")] {
            if flag {
                tag = set_attr(&tag, name, style_id);
            }
        }
        let mut body = own[open_end + 1..].to_string();
        if line {
            LINE_CELLS.iter().for_each(|c| { remove_cell(&mut body, c); });
        }
        if fill {
            FILL_CELLS.iter().for_each(|c| { remove_cell(&mut body, c); });
        }
        if text {
            TEXT_CELLS.iter().for_each(|c| { remove_cell(&mut body, c); });
            remove_section(&mut body, "Character");
            remove_section(&mut body, "Paragraph");
        }
        own = format!("{tag}{body}");
        xml.replace_range(start..own_end, &own);
    }
    with_part(package, &part, xml.into_bytes())
}

/// Set the cells `cells` (`(name, xml)`) in the `Character` section row 0 of a style sheet's XML (the section is created when the style has none).
fn set_character_cells(sheet: &mut String, cells: &[(&str, String)]) {
    set_section_cells(sheet, "Character", cells);
}

/// Set own cells in row 0 of the section `section` (`Character`, `Paragraph`) of a style sheet; the section and the row are created when missing, cells of other names stay.
fn set_section_cells(sheet: &mut String, section_name: &str, cells: &[(&str, String)]) {
    if cells.is_empty() {
        return;
    }
    if let Some(section) = sheet.find(&format!("<Section N='{section_name}'")) {
        for (name, _) in cells {
            // only inside the Character section (a cell of that name elsewhere in the sheet stays)
            let end = sheet[section..].find("</Section>").map_or(sheet.len(), |n| n + section);
            let mut part = sheet[section..end].to_string();
            remove_cell(&mut part, name);
            sheet.replace_range(section..end, &part);
        }
        let end = sheet[section..].find("</Section>").map_or(sheet.len(), |n| n + section);
        let row_end = sheet[section..end].find("</Row>").map(|n| n + section);
        let xml: String = cells.iter().map(|(_, x)| x.as_str()).collect();
        match row_end {
            Some(at) => sheet.insert_str(at, &xml),
            None => sheet.insert_str(end, &format!("<Row IX='0'>{xml}</Row>")),
        }
    } else if let Some(close) = sheet.rfind("</StyleSheet>") {
        let xml: String = cells.iter().map(|(_, x)| x.as_str()).collect();
        sheet.insert_str(close, &format!("<Section N='{section_name}'><Row IX='0'>{xml}</Row></Section>"));
    }
}

/// The cell XML of a style: `name` = (cell, value, unit).
fn style_cell(name: &str, value: &str, unit: &str) -> String {
    if unit.is_empty() { format!("<Cell N='{name}' V='{}'/>", escape_attr(value)) } else { format!("<Cell N='{name}' V='{}' U='{unit}'/>", escape_attr(value)) }
}

fn colour_ok(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Create the custom style `name` (or change it when a style of that name already exists and is custom). `spec`: JSON with optional `basedOn` (a style id; the
/// default is the style named Normal), `lineColor` / `fillColor` (`#rrggbb`), `lineWeight` (points), `linePattern` (0-23), `fillPattern` (0-40), and for the text `textColor` (`#rrggbb`), `fontSize` (points), `bold`, `italic`.
pub fn define_style(package: &Package, spec: &serde_json::Value) -> Result<Vec<u8>> {
    let name = spec.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() || name.len() > 64 || name.contains(['<', '>', '&', '\'', '"']) {
        return Err(Error("a style needs a name of up to 64 characters without < > & or quotes".to_string()));
    }
    let mut cells = String::new();
    let mut line_cells = Vec::new();
    if let Some(c) = spec.get("lineColor").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
        if !colour_ok(c) {
            return Err(Error(format!("'{c}' is not a colour #rrggbb")));
        }
        line_cells.push(style_cell("LineColor", &c.to_ascii_lowercase(), ""));
    }
    if let Some(w) = spec.get("lineWeight").and_then(|v| v.as_f64()) {
        if !(0.0..=144.0).contains(&w) {
            return Err(Error("the line weight is 0 to 144 points".to_string()));
        }
        line_cells.push(style_cell("LineWeight", &format!("{}", w / 72.0), "PT"));
    }
    if let Some(p) = spec.get("linePattern").and_then(|v| v.as_u64()) {
        if p > 23 {
            return Err(Error("the line pattern is 0 to 23".to_string()));
        }
        line_cells.push(style_cell("LinePattern", &p.to_string(), ""));
    }
    let mut fill_cells = Vec::new();
    if let Some(c) = spec.get("fillColor").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
        if !colour_ok(c) {
            return Err(Error(format!("'{c}' is not a colour #rrggbb")));
        }
        fill_cells.push(style_cell("FillForegnd", &c.to_ascii_lowercase(), ""));
    }
    if let Some(p) = spec.get("fillPattern").and_then(|v| v.as_u64()) {
        if p > 40 {
            return Err(Error("the fill pattern is 0 to 40".to_string()));
        }
        fill_cells.push(style_cell("FillPattern", &p.to_string(), ""));
    }
    // text: size in points, colour, bold / italic (the Character row 0 of the style)
    let mut char_cells: Vec<(&str, String)> = Vec::new();
    if let Some(c) = spec.get("textColor").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
        if !colour_ok(c) {
            return Err(Error(format!("'{c}' is not a colour #rrggbb")));
        }
        char_cells.push(("Color", style_cell("Color", &c.to_ascii_lowercase(), "")));
    }
    if let Some(size) = spec.get("fontSize").and_then(|v| v.as_f64()) {
        if !(1.0..=999.0).contains(&size) {
            return Err(Error("the font size is 1 to 999 points".to_string()));
        }
        char_cells.push(("Size", style_cell("Size", &format!("{}", size / 72.0), "PT")));
    }
    if spec.get("bold").is_some() || spec.get("italic").is_some() {
        let bits = u32::from(spec.get("bold").and_then(|v| v.as_bool()).unwrap_or(false)) + 2 * u32::from(spec.get("italic").and_then(|v| v.as_bool()).unwrap_or(false));
        char_cells.push(("Style", style_cell("Style", &bits.to_string(), "")));
    }
    // paragraph (Visio, probe-styles5.ps1: a style's Paragraph section row 0 holds its own cells, the others are `F='Inh'`): alignment 0-3, indents in inches, line spacing as a multiple of the
    // line (stored negative: Visio's Normal style has -1.2), space before / after in points (stored in inches, unit PT)
    let mut para_cells: Vec<(&str, String)> = Vec::new();
    if let Some(a) = spec.get("align").and_then(|v| v.as_u64()) {
        if a > 3 {
            return Err(Error("the alignment is 0 (left), 1 (centre), 2 (right) or 3 (justified)".to_string()));
        }
        para_cells.push(("HorzAlign", style_cell("HorzAlign", &a.to_string(), "")));
    }
    for (key, cell) in [("indentFirst", "IndFirst"), ("indentLeft", "IndLeft"), ("indentRight", "IndRight")] {
        if let Some(v) = spec.get(key).and_then(|v| v.as_f64()) {
            if !(-100.0..=100.0).contains(&v) {
                return Err(Error("an indent is -100 to 100 inches".to_string()));
            }
            para_cells.push((cell, style_cell(cell, &format!("{v}"), "IN")));
        }
    }
    if let Some(v) = spec.get("lineSpacing").and_then(|v| v.as_f64()) {
        if !(0.0..=100.0).contains(&v) {
            return Err(Error("the line spacing is 0 to 100 lines".to_string()));
        }
        // Visio stores a multiple of the line height as a NEGATIVE number (the Normal style has -1.2; a positive SpLine is an exact distance)
        para_cells.push(("SpLine", style_cell("SpLine", &format!("{}", -v), "")));
    }
    for (key, cell) in [("spaceBefore", "SpBefore"), ("spaceAfter", "SpAfter")] {
        if let Some(v) = spec.get(key).and_then(|v| v.as_f64()) {
            if !(0.0..=1584.0).contains(&v) {
                return Err(Error("the space before and after a paragraph is 0 to 1584 points".to_string()));
            }
            para_cells.push((cell, style_cell(cell, &format!("{}", v / 72.0), "PT")));
        }
    }
    cells.push_str(&line_cells.concat());
    cells.push_str(&fill_cells.concat());

    let mut xml = package.text(DOCUMENT)?.to_string();
    let all = sheets(&xml);
    let name_of = |tag: &str| attr(tag, "NameU").or_else(|| attr(tag, "Name")).unwrap_or_default();
    if let Some((start, end, tag)) = all.iter().find(|(_, _, t)| name_of(t) == name).cloned() {
        let _ = &tag;
        if BUILT_IN.contains(&name.as_str()) {
            return Err(Error(format!("'{name}' is a built-in style: it cannot be changed (create a style with another name)")));
        }
        // change: the cells given replace the style's own cells of that name, the others stay
        let mut sheet = xml[start..end].to_string();
        for (cell_name, _) in line_cells.iter().chain(fill_cells.iter()).map(|c| (c.split("N='").nth(1).and_then(|r| r.split('\'').next()).unwrap_or("").to_string(), ())) {
            remove_cell(&mut sheet, &cell_name);
        }
        let close = sheet.rfind("</StyleSheet>").ok_or_else(|| Error("the style has no content to change".to_string()))?;
        sheet.insert_str(close, &cells);
        set_character_cells(&mut sheet, &char_cells);
        set_section_cells(&mut sheet, "Paragraph", &para_cells);
        // changing the style a style is based on (Visio: BasedOn / LineBasedOn / FillBasedOn / TextBasedOn, probe-styles5.ps1; the sheet's LineStyle / FillStyle / TextStyle attributes)
        let own_id = attr(&tag, "ID").unwrap_or_default();
        let mut open_tag = sheet[..sheet.find('>').map_or(sheet.len(), |n| n + 1)].to_string();
        let old_open = open_tag.clone();
        for (key, attr_name) in [("lineBasedOn", "LineStyle"), ("fillBasedOn", "FillStyle"), ("textBasedOn", "TextStyle")] {
            let Some(base) = spec.get(key).or_else(|| spec.get("basedOn")).and_then(|v| v.as_str()).filter(|v| !v.is_empty()) else { continue };
            if !all.iter().any(|(_, _, t)| attr(t, "ID").as_deref() == Some(base)) {
                return Err(Error(format!("the drawing has no style {base} to base the style on")));
            }
            // no loop: walking up the chain of the new base must not reach this style
            let mut at = base.to_string();
            for _ in 0..all.len() + 1 {
                if at == own_id {
                    return Err(Error(format!("'{name}' cannot be based on itself or on a style that is based on it")));
                }
                match all.iter().find(|(_, _, t)| attr(t, "ID").as_deref() == Some(at.as_str())).and_then(|(_, _, t)| attr(t, attr_name)) {
                    Some(next) if next != at && next != "0" => at = next,
                    _ => break,
                }
            }
            open_tag = set_attr(&open_tag, attr_name, base);
        }
        if open_tag != old_open {
            sheet.replace_range(..old_open.len(), &open_tag);
        }
        xml.replace_range(start..end, &sheet);
    } else {
        let based = match spec.get("basedOn").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
            Some(id) => {
                if !all.iter().any(|(_, _, t)| attr(t, "ID").as_deref() == Some(id)) {
                    return Err(Error(format!("the drawing has no style {id} to base the new style on")));
                }
                id.to_string()
            }
            None => all.iter().find(|(_, _, t)| name_of(t) == "Normal").and_then(|(_, _, t)| attr(t, "ID")).unwrap_or_else(|| "0".to_string()),
        };
        let next = all.iter().filter_map(|(_, _, t)| attr(t, "ID").and_then(|v| v.parse::<u32>().ok())).max().unwrap_or(0) + 1;
        let sheet = format!(
            "<StyleSheet ID='{next}' NameU='{n}' IsCustomNameU='1' Name='{n}' IsCustomName='1' LineStyle='{based}' FillStyle='{based}' TextStyle='{based}'><Cell N='EnableLineProps' V='1'/><Cell N='EnableFillProps' V='1'/><Cell N='EnableTextProps' V='1'/><Cell N='HideForApply' V='0'/>{cells}</StyleSheet>",
            n = escape_attr(&name)
        );
        let mut sheet = sheet;
        set_character_cells(&mut sheet, &char_cells);
        set_section_cells(&mut sheet, "Paragraph", &para_cells);
        let at = xml.rfind("</StyleSheets>").ok_or_else(|| Error("the document has no style sheets".to_string()))?;
        xml.insert_str(at, &sheet);
    }
    with_part(package, DOCUMENT, xml.into_bytes())
}

/// Replace `name='old'` by `name='new'` in every tag of `xml` (the style attributes `LineStyle`, `FillStyle`, `TextStyle`); the number of replacements.
fn replace_attr_value(xml: &mut String, name: &str, old: &str, new: &str) -> usize {
    let mut count = 0;
    for quote in ['\'', '"'] {
        let pattern = format!(" {name}={quote}{old}{quote}");
        let with = format!(" {name}={quote}{new}{quote}");
        count += xml.matches(&pattern).count();
        *xml = xml.replace(&pattern, &with);
    }
    count
}

/// Delete the custom style `style_id`. Measured on the installed Visio (probe-styles4.ps1): a built-in style cannot be deleted ("Built-in styles cannot be deleted."); styles that were
/// based on the deleted one become based on "No Style" (id 0); shapes (and masters) that used it get "No Style" (0) for the line, fill and text style they had it for. The shapes' own cells stay.
pub fn delete_style(package: &Package, style_id: &str) -> Result<Vec<u8>> {
    let mut xml = package.text(DOCUMENT)?.to_string();
    let all = sheets(&xml);
    let (start, end, tag) = all
        .iter()
        .find(|(_, _, t)| attr(t, "ID").as_deref() == Some(style_id))
        .cloned()
        .ok_or_else(|| Error(format!("the drawing has no style {style_id}")))?;
    let name = attr(&tag, "NameU").or_else(|| attr(&tag, "Name")).unwrap_or_default();
    if BUILT_IN.contains(&name.as_str()) {
        return Err(Error(format!("'{name}' is a built-in style: it cannot be deleted")));
    }
    xml.replace_range(start..end, "");
    for key in ["LineStyle", "FillStyle", "TextStyle"] {
        replace_attr_value(&mut xml, key, style_id, "0");
    }
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    extras.insert(DOCUMENT.to_string(), xml.into_bytes());
    let parts: Vec<String> = package.names().filter(|n| { let l = n.to_ascii_lowercase(); (l.starts_with("visio/pages/page") || l.starts_with("visio/masters/master")) && l.ends_with(".xml") }).map(str::to_string).collect();
    for part in parts {
        let mut text = package.text(&part)?.to_string();
        let mut changed = 0;
        for key in ["LineStyle", "FillStyle", "TextStyle"] {
            changed += replace_attr_value(&mut text, key, style_id, "0");
        }
        if changed > 0 {
            extras.insert(part, text.into_bytes());
        }
    }
    write_package_edit(package, extras, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Package {
        let blank = crate::fresh::write_blank_document(&crate::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".to_string() }).unwrap();
        Package::from_reader(std::io::Cursor::new(blank)).unwrap()
    }

    fn styles(package: &Package) -> Vec<serde_json::Value> {
        serde_json::from_str(&list_styles(package).unwrap()).unwrap()
    }

    #[test]
    fn a_new_drawing_lists_the_visio_styles() {
        let package = blank();
        let names: Vec<String> = styles(&package).iter().map(|s| s["name"].as_str().unwrap().to_string()).collect();
        for expected in ["Normal", "Text Only", "None", "Guide"] {
            assert!(names.iter().any(|n| n == expected), "{expected} missing in {names:?}");
        }
    }

    #[test]
    fn defining_a_style_adds_a_custom_sheet_based_on_normal_and_changing_it_keeps_the_id() {
        let package = blank();
        let normal = styles(&package).iter().find(|s| s["name"] == "Normal").unwrap()["id"].as_str().unwrap().to_string();
        let bytes = define_style(&package, &serde_json::json!({"name": "Red", "lineColor": "#FF0000", "lineWeight": 3.0, "fillColor": "#00ff00"})).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let red = styles(&package).into_iter().find(|s| s["name"] == "Red").expect("Red exists");
        assert_eq!(red["custom"], true);
        assert_eq!(red["line"].as_str().unwrap(), normal);
        assert_eq!(red["lineColor"], "#ff0000");
        assert_eq!(red["fillColor"], "#00ff00");
        let w: f64 = red["lineWeight"].as_str().unwrap().parse().unwrap();
        assert!((w * 72.0 - 3.0).abs() < 1e-9);
        let id = red["id"].clone();
        let bytes = define_style(&package, &serde_json::json!({"name": "Red", "lineColor": "#0000ff"})).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let again: Vec<_> = styles(&package).into_iter().filter(|s| s["name"] == "Red").collect();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0]["id"], id);
        assert_eq!(again[0]["lineColor"], "#0000ff");
        assert_eq!(again[0]["fillColor"], "#00ff00", "cells that were not given stay");
    }

    #[test]
    fn a_style_can_carry_text_cells_and_changing_them_keeps_the_others() {
        let package = blank();
        let bytes = define_style(&package, &serde_json::json!({"name": "Loud", "textColor": "#FF0000", "fontSize": 20.0, "bold": true})).unwrap();
        let package = Package::from_bytes(&bytes).unwrap();
        let loud = styles(&package).into_iter().find(|s| s["name"] == "Loud").unwrap();
        assert_eq!(loud["textColor"], "#ff0000");
        assert_eq!(loud["textStyle"], "1");
        let size: f64 = loud["textSize"].as_str().unwrap().parse().unwrap();
        assert!((size * 72.0 - 20.0).abs() < 1e-9);
        let bytes = define_style(&package, &serde_json::json!({"name": "Loud", "fontSize": 10.0, "italic": true})).unwrap();
        let package = Package::from_bytes(&bytes).unwrap();
        let loud = styles(&package).into_iter().find(|s| s["name"] == "Loud").unwrap();
        assert_eq!(loud["textColor"], "#ff0000", "the colour that was not given stays");
        assert_eq!(loud["textStyle"], "2", "bold off, italic on");
        let size: f64 = loud["textSize"].as_str().unwrap().parse().unwrap();
        assert!((size * 72.0 - 10.0).abs() < 1e-9);
        assert!(define_style(&package, &serde_json::json!({"name": "Loud", "fontSize": 0.0})).is_err());
        assert!(define_style(&package, &serde_json::json!({"name": "Loud", "textColor": "red"})).is_err());
        let xml = package.text("visio/document.xml").unwrap();
        let (start, end, _) = sheets(xml).into_iter().find(|(_, _, tag)| tag.contains("NameU='Loud'")).unwrap();
        assert_eq!(xml[start..end].matches("<Section N='Character'").count(), 1, "one Character section in the style");
    }

    #[test]
    fn a_style_can_carry_paragraph_cells_and_be_rebased() {
        let package = blank();
        let bytes = define_style(&package, &serde_json::json!({"name": "Para", "align": 2, "indentLeft": 0.5, "indentFirst": 0.25, "lineSpacing": 1.5, "spaceBefore": 6.0, "spaceAfter": 3.0})).unwrap();
        let package = Package::from_bytes(&bytes).unwrap();
        let para = styles(&package).into_iter().find(|s| s["name"] == "Para").unwrap();
        assert_eq!(para["paraAlign"], "2");
        assert_eq!(para["indentLeft"], "0.5");
        assert_eq!(para["lineSpacing"], "-1.5", "1.5 lines is stored as -1.5, like Visio's own Normal style (-1.2)");
        let before: f64 = para["spaceBefore"].as_str().unwrap().parse().unwrap();
        assert!((before * 72.0 - 6.0).abs() < 1e-9, "stored in inches: {before}");
        // changing one cell keeps the others, one Paragraph section only
        let bytes = define_style(&package, &serde_json::json!({"name": "Para", "align": 1})).unwrap();
        let package = Package::from_bytes(&bytes).unwrap();
        let para = styles(&package).into_iter().find(|s| s["name"] == "Para").unwrap();
        assert_eq!((para["paraAlign"].as_str(), para["indentLeft"].as_str()), (Some("1"), Some("0.5")));
        let xml = package.text("visio/document.xml").unwrap();
        let (start, end, _) = sheets(xml).into_iter().find(|(_, _, tag)| tag.contains("NameU='Para'")).unwrap();
        assert_eq!(xml[start..end].matches("<Section N='Paragraph'").count(), 1);
        assert!(define_style(&package, &serde_json::json!({"name": "Para", "align": 9})).is_err());
        // rebase: Kid on Para (all three), then Para on Kid is a loop and is refused; an unknown id is refused
        let para_id = para["id"].as_str().unwrap().to_string();
        let package = Package::from_bytes(&define_style(&package, &serde_json::json!({"name": "Kid", "fillColor": "#ffcc00"})).unwrap()).unwrap();
        let kid_id = styles(&package).into_iter().find(|s| s["name"] == "Kid").unwrap()["id"].as_str().unwrap().to_string();
        let package = Package::from_bytes(&define_style(&package, &serde_json::json!({"name": "Kid", "basedOn": para_id})).unwrap()).unwrap();
        let kid = styles(&package).into_iter().find(|s| s["name"] == "Kid").unwrap();
        assert_eq!((kid["line"].as_str(), kid["fill"].as_str(), kid["text"].as_str()), (Some(para_id.as_str()), Some(para_id.as_str()), Some(para_id.as_str())));
        assert_eq!(kid["fillColor"], "#ffcc00", "its own cells stay");
        let package = Package::from_bytes(&define_style(&package, &serde_json::json!({"name": "Kid", "lineBasedOn": "1"})).unwrap()).unwrap();
        let kid = styles(&package).into_iter().find(|s| s["name"] == "Kid").unwrap();
        assert_eq!((kid["line"].as_str(), kid["fill"].as_str()), (Some("1"), Some(para_id.as_str())), "one category only");
        assert!(define_style(&package, &serde_json::json!({"name": "Para", "basedOn": kid_id})).is_err(), "Para based on a style that is based on Para is a loop");
        assert!(define_style(&package, &serde_json::json!({"name": "Kid", "basedOn": "9999"})).is_err());
        assert!(define_style(&package, &serde_json::json!({"name": "Kid", "basedOn": kid_id})).is_err(), "a style cannot be based on itself");
    }

    #[test]
    fn deleting_a_style_frees_the_shapes_and_the_styles_based_on_it() {
        let package = blank();
        let (bytes, id) = crate::draw::draw_shape(&package, 0, crate::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).expect("draw");
        let package = Package::from_bytes(&bytes).unwrap();
        let package = Package::from_bytes(&define_style(&package, &serde_json::json!({"name": "Base", "lineColor": "#ff0000"})).unwrap()).unwrap();
        let base_id = styles(&package).into_iter().find(|s| s["name"] == "Base").unwrap()["id"].as_str().unwrap().to_string();
        let package = Package::from_bytes(&define_style(&package, &serde_json::json!({"name": "Kid", "basedOn": base_id, "fillColor": "#00ff00"})).unwrap()).unwrap();
        let package = Package::from_bytes(&apply_style(&package, 0, &[id.to_string()], &base_id, "all").unwrap()).unwrap();
        let bytes = delete_style(&package, &base_id).unwrap();
        let package = Package::from_bytes(&bytes).unwrap();
        let now = styles(&package);
        assert!(!now.iter().any(|s| s["name"] == "Base"), "the style is gone");
        let kid = now.iter().find(|s| s["name"] == "Kid").unwrap();
        assert_eq!(kid["line"], "0", "the style based on it is based on No Style now");
        let page = package.text(&package.pages().unwrap()[0].part.clone()).unwrap().to_string();
        let (s, e) = find_shape_range(&page, &id.to_string()).unwrap();
        assert!(page[s..e].contains("LineStyle='0'") && page[s..e].contains("FillStyle='0'") && page[s..e].contains("TextStyle='0'"), "{}", &page[s..e.min(s + 300)]);
        // built-in styles and unknown ids are refused
        let normal = now.iter().find(|s| s["name"] == "Normal").unwrap()["id"].as_str().unwrap().to_string();
        assert!(delete_style(&package, &normal).is_err());
        assert!(delete_style(&package, "9999").is_err());
    }

    #[test]
    fn built_in_styles_and_bad_values_are_refused() {
        let package = blank();
        assert!(define_style(&package, &serde_json::json!({"name": "Normal", "lineColor": "#ff0000"})).is_err());
        assert!(define_style(&package, &serde_json::json!({"name": "X", "lineColor": "red"})).is_err());
        assert!(define_style(&package, &serde_json::json!({"name": "X", "lineWeight": 500.0})).is_err());
        assert!(define_style(&package, &serde_json::json!({"name": "", "lineColor": "#ff0000"})).is_err());
        assert!(define_style(&package, &serde_json::json!({"name": "X", "basedOn": "9999"})).is_err());
    }

    #[test]
    fn applying_a_style_sets_the_attributes_and_clears_the_cells_of_that_category_only() {
        let package = blank();
        let (bytes, id) = crate::draw::draw_shape(&package, 0, crate::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).expect("draw");
        let package = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let bytes = define_style(&package, &serde_json::json!({"name": "Red", "lineColor": "#ff0000"})).unwrap();
        let package = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let red = styles(&package).into_iter().find(|s| s["name"] == "Red").unwrap()["id"].as_str().unwrap().to_string();
        // give the shape local line and fill cells first
        let part = package.pages().unwrap()[0].part.clone();
        let mut xml = package.text(&part).unwrap().to_string();
        let (start, _) = find_shape_range(&xml, &id.to_string()).unwrap();
        let open_end = xml[start..].find('>').unwrap() + start;
        xml.insert_str(open_end + 1, "<Cell N='LineColor' V='#010203'/><Cell N='FillForegnd' V='#040506'/>");
        let package = Package::from_reader(std::io::Cursor::new(with_part(&package, &part, xml.into_bytes()).unwrap())).unwrap();
        let bytes = apply_style(&package, 0, &[id.to_string()], &red, "line").unwrap();
        let package = Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
        let after = package.text(&package.pages().unwrap()[0].part.clone()).unwrap().to_string();
        let (s, e) = find_shape_range(&after, &id.to_string()).unwrap();
        let shape = &after[s..e];
        assert!(shape.contains(&format!("LineStyle='{red}'")), "{shape}");
        assert!(!shape.contains("N='LineColor'"), "the local line colour must go: {shape}");
        assert!(shape.contains("N='FillForegnd'"), "the fill cell stays when only the line style is applied");
        assert!(!shape.contains(&format!("FillStyle='{red}'")));
        assert!(apply_style(&package, 0, &[id.to_string()], "9999", "all").is_err());
        assert!(apply_style(&package, 0, &["424242".to_string()], &red, "all").is_err());
        assert!(apply_style(&package, 0, &[id.to_string()], &red, "nonsense").is_err());
    }
}
