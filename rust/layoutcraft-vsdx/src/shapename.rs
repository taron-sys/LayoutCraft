//! Shape names (Shape.Name / Shape.NameU, SHAPE-12). Measured with probe-shapename.ps1 (step 337): a drawn shape has NO name attributes (Visio computes "Sheet.<ID>"); setting `Shape.Name`
//! writes `NameU='X' IsCustomNameU='1' Name='X' IsCustomName='1'` on the `<Shape>` element, a shape given a name another shape has becomes `X.<its ID>` (names are unique on a page), an empty name takes
//! the attributes away again (back to "Sheet.<ID>"), and odd characters (quotes, `&`, `<`, dots) are accepted. This module writes exactly that: the name goes to both `NameU` and `Name`.
use crate::writer::{escape_attr, find_shape_range, with_part};
use crate::{Error, Package, Result};

const MAX_NAME: usize = 255;

fn page_part(package: &Package, page_index: usize) -> Result<String> {
    Ok(package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone())
}

fn unescape(v: &str) -> String {
    v.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The attributes of an opening tag body (`ID='1' NameU='x' ...`) as (key, raw value, quote).
fn parse_attrs(body: &str) -> Vec<(String, String, char)> {
    let b = body.as_bytes();
    let (mut i, mut out) = (0usize, Vec::new());
    while i < b.len() {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        let k = i;
        while i < b.len() && b[i] != b'=' && !(b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            break;
        }
        let key = body[k..i].to_string();
        i += 1;
        if i >= b.len() || (b[i] != b'\'' && b[i] != b'"') {
            break;
        }
        let quote = b[i] as char;
        i += 1;
        let v = i;
        while i < b.len() && b[i] as char != quote {
            i += 1;
        }
        out.push((key, body[v..i].to_string(), quote));
        i += 1;
    }
    out
}

/// The opening tag `<Shape ...>` (or `<Shape .../>`) at the start of `shape`, split into (tag text, attributes, self-closing).
fn open_tag(shape: &str) -> Option<(usize, Vec<(String, String, char)>, bool)> {
    let end = shape.find('>')? + 1;
    let tag = &shape[..end];
    let selfclose = tag.ends_with("/>");
    let body = &tag["<Shape".len()..tag.len() - if selfclose { 2 } else { 1 }];
    Some((end, parse_attrs(body), selfclose))
}

/// The opening tag of `shape` with the name attributes set (`Some`) or taken away (`None`), and everything after it unchanged.
pub(crate) fn edit_shape(shape: &str, name: Option<&str>) -> String {
    let Some((end, attrs, selfclose)) = open_tag(shape) else { return shape.to_string() };
    let mut kept: Vec<(String, String, char)> = attrs.into_iter().filter(|(k, _, _)| !matches!(k.as_str(), "NameU" | "IsCustomNameU" | "Name" | "IsCustomName")).collect();
    if let Some(name) = name {
        let at = kept.iter().position(|(k, _, _)| k == "ID").map_or(0, |p| p + 1);
        let esc = escape_attr(name);
        let new = [("NameU", esc.clone()), ("IsCustomNameU", "1".to_string()), ("Name", esc), ("IsCustomName", "1".to_string())];
        for (n, (k, v)) in new.into_iter().enumerate() {
            kept.insert(at + n, (k.to_string(), v, '\''));
        }
    }
    let mut tag = String::from("<Shape");
    for (k, v, q) in &kept {
        tag.push_str(&format!(" {k}={q}{v}{q}"));
    }
    tag.push_str(if selfclose { "/>" } else { ">" });
    format!("{tag}{}", &shape[end..])
}

/// The `NameU` of every shape element of the page text except the one with ID `except`, lower-cased and unescaped.
fn used_names(xml: &str, except: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = xml[at..].find("<Shape ") {
        let start = at + rel;
        let Some(len) = xml[start..].find('>') else { break };
        let tag = &xml[start..start + len + 1];
        let selfclose = tag.ends_with("/>");
        let attrs = parse_attrs(&tag["<Shape".len()..tag.len() - if selfclose { 2 } else { 1 }]);
        let id = attrs.iter().find(|(k, _, _)| k == "ID").map(|(_, v, _)| v.as_str()).unwrap_or("");
        if id != except {
            if let Some((_, v, _)) = attrs.iter().find(|(k, _, _)| k == "NameU") {
                out.push(unescape(v).to_lowercase());
            }
        }
        at = start + len + 1;
    }
    out
}

/// The names of the shapes of page `page_index` as (shape ID, name), in file order (groups' members included): the `NameU` of every `<Shape>` that has one. A shape without the attribute has Visio's automatic
/// name and is not listed. (The names are NOT part of the drawing model: the golden models of the importer must not change.)
pub fn names(package: &Package, page_index: usize) -> Result<Vec<(String, String)>> {
    let part = page_part(package, page_index)?;
    let xml = package.text(&part)?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = xml[at..].find("<Shape ") {
        let start = at + rel;
        let Some(len) = xml[start..].find('>') else { break };
        let tag = &xml[start..start + len + 1];
        let selfclose = tag.ends_with("/>");
        let attrs = parse_attrs(&tag["<Shape".len()..tag.len() - if selfclose { 2 } else { 1 }]);
        let get = |k: &str| attrs.iter().find(|(key, _, _)| key == k).map(|(_, v, _)| unescape(v));
        if let (Some(id), Some(name)) = (get("ID"), get("NameU")) {
            if !name.is_empty() {
                out.push((id, name));
            }
        }
        at = start + len + 1;
    }
    Ok(out)
}

/// The name Visio gives (measured, step 337: probe-shapename.ps1 and a second probe with five shapes named in a mixed order): `wanted` itself, or `wanted.<ID of the shape>` when another shape of the
/// page has the name already (compared without regard to case: `x` next to `X` becomes `x.2` for shape 2). If that is taken too, `.<ID>.2`, `.<ID>.3` ... (our own tie-break; Visio's is unknown).
pub fn unique_name(wanted: &str, taken: &[String], shape_id: &str) -> String {
    if !taken.iter().any(|t| *t == wanted.to_lowercase()) {
        return wanted.to_string();
    }
    let first = format!("{wanted}.{shape_id}");
    if !taken.iter().any(|t| *t == first.to_lowercase()) {
        return first;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{first}.{n}");
        if !taken.iter().any(|t| *t == candidate.to_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

/// Name shape `shape_id` of page `page_index` (an empty name gives it back its automatic name). Returns the new package bytes and the name the shape has now (which can have
/// a number added to be unique on the page; for an empty name the empty string).
pub fn set_shape_name(package: &Package, page_index: usize, shape_id: &str, name: &str) -> Result<(Vec<u8>, String)> {
    let name = name.trim();
    if name.chars().count() > MAX_NAME {
        return Err(Error(format!("a shape name can have at most {MAX_NAME} characters")));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(Error("a shape name cannot contain control characters".to_string()));
    }
    let part = page_part(package, page_index)?;
    let mut xml = package.text(&part)?.to_string();
    let (start, end) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let final_name = if name.is_empty() { String::new() } else { unique_name(name, &used_names(&xml, shape_id), shape_id) };
    let new_shape = edit_shape(&xml[start..end], if final_name.is_empty() { None } else { Some(&final_name) });
    xml.replace_range(start..end, &new_shape);
    Ok((with_part(package, &part, xml.into_bytes())?, final_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_written_to_both_attributes_after_the_id_like_visio_does() {
        let out = edit_shape("<Shape ID='1' Type='Shape' LineStyle='3'><Cell N='PinX' V='1'/></Shape>", Some("My shape"));
        assert_eq!(out, "<Shape ID='1' NameU='My shape' IsCustomNameU='1' Name='My shape' IsCustomName='1' Type='Shape' LineStyle='3'><Cell N='PinX' V='1'/></Shape>");
    }

    #[test]
    fn changing_and_removing_a_name_keeps_the_other_attributes() {
        let named = edit_shape("<Shape ID='7' NameU='Old' IsCustomNameU='1' Name='Old' IsCustomName='1' Type='Group'><Shapes/></Shape>", Some("New & \"odd\""));
        assert_eq!(named, "<Shape ID='7' NameU='New &amp; &quot;odd&quot;' IsCustomNameU='1' Name='New &amp; &quot;odd&quot;' IsCustomName='1' Type='Group'><Shapes/></Shape>");
        assert_eq!(edit_shape(&named, None), "<Shape ID='7' Type='Group'><Shapes/></Shape>");
        assert_eq!(edit_shape("<Shape ID='7' Type='Shape'/>", Some("x")), "<Shape ID='7' NameU='x' IsCustomNameU='1' Name='x' IsCustomName='1' Type='Shape'/>");
        assert_eq!(edit_shape("<Shape ID=\"7\" Type=\"Shape\">", Some("x")), "<Shape ID=\"7\" NameU='x' IsCustomNameU='1' Name='x' IsCustomName='1' Type=\"Shape\">");
    }

    #[test]
    fn a_taken_name_gets_the_id_of_the_shape_added_as_visio_does() {
        let taken = vec!["my shape".to_string()];
        assert_eq!(unique_name("My Shape", &taken, "5"), "My Shape.5");
        assert_eq!(unique_name("Other", &taken, "5"), "Other");
        assert_eq!(unique_name("Other", &[], "5"), "Other");
        let both = vec!["my shape".to_string(), "my shape.5".to_string()];
        assert_eq!(unique_name("my shape", &both, "5"), "my shape.5.2", "our tie-break when name.<ID> is taken too");
    }

    #[test]
    fn the_names_in_use_skip_the_shape_itself_and_unescape() {
        let xml = "<Shapes><Shape ID='1' NameU='A &amp; B' Name='A &amp; B' Type='Shape'><Shapes><Shape ID='2' NameU='Inner'/></Shapes></Shape><Shape ID='3' Type='Shape'/></Shapes>";
        assert_eq!(used_names(xml, "9"), vec!["a & b".to_string(), "inner".to_string()]);
        assert_eq!(used_names(xml, "1"), vec!["inner".to_string()]);
    }
}
