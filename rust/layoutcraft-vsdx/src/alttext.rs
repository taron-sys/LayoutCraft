//! Alt text of a shape (Shape > Alt Text, `Shape.AlternativeText`): Visio keeps it in the shape's `User` section, in the row `visAltDescription` (probe-alttext.ps1, step 335: setting
//! `Shape.AlternativeText` on a new rectangle wrote `<Section N='User'><Row N='visAltDescription'><Cell N='Value' V='...' U='STR'/><Cell N='Prompt' V=''/></Row>
//! <Row N='visAltText'><Cell N='Value' V='0'/><Cell N='Prompt' V=''/></Row></Section>`). The importer reads `visAltDescription` (import.rs); this writes it. Other rows of the
//! User section are left alone; an empty text removes the row.
use crate::writer::{escape_attr, find_shape_range, with_part};
use crate::{Error, Package, Result};

const MAX_ALT_TEXT: usize = 4096;
const ROW: &str = "visAltDescription";

fn page_part(package: &Package, page_index: usize) -> Result<String> {
    Ok(package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone())
}

fn row_xml(text: &str) -> String {
    format!("<Row N='{ROW}'><Cell N='Value' V='{}' U='STR'/><Cell N='Prompt' V=''/></Row>", escape_attr(text))
}

/// The byte range of the shape's own `User` section inside `own` (None when there is none).
fn user_section(own: &str) -> Option<(usize, usize)> {
    let mut search = 0usize;
    while let Some(relative) = own[search..].find("<Section") {
        let start = search + relative;
        let close = own[start..].find('>')? + start;
        let tag = &own[start..=close];
        if tag.contains("N='User'") || tag.contains("N=\"User\"") {
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

/// The byte range of the row `ROW` inside the section text.
fn alt_row(section: &str) -> Option<(usize, usize)> {
    let mut search = 0usize;
    while let Some(relative) = section[search..].find("<Row") {
        let start = search + relative;
        let close = section[start..].find('>')? + start;
        let tag = &section[start..=close];
        if tag.contains(&format!("N='{ROW}'")) || tag.contains(&format!("N=\"{ROW}\"")) {
            if tag.ends_with("/>") {
                return Some((start, close + 1));
            }
            let end = section[close..].find("</Row>")? + close + "</Row>".len();
            return Some((start, end));
        }
        search = close + 1;
    }
    None
}

/// The shape element `shape` (from `<Shape` to `</Shape>`, members of a group included) with its alt text set (an empty text removes it).
pub(crate) fn edit_shape(shape: &str, text: &str) -> String {
    let own_end = shape.find("<Shapes").unwrap_or(shape.len()); // only the shape's own part: a group's members come after `<Shapes`
    let own = &shape[..own_end];
    if let Some((s, e)) = user_section(own) {
        let section = &shape[s..e];
        let new_section = if section.ends_with("/>") {
            if text.is_empty() { section.to_string() } else { format!("{}>{}</Section>", &section[..section.len() - 2], row_xml(text)) }
        } else if let Some((rs, re)) = alt_row(section) {
            let row = if text.is_empty() { String::new() } else { row_xml(text) };
            format!("{}{}{}", &section[..rs], row, &section[re..])
        } else if text.is_empty() {
            section.to_string()
        } else {
            let at = section.rfind("</Section>").unwrap_or(section.len());
            format!("{}{}{}", &section[..at], row_xml(text), &section[at..])
        };
        format!("{}{}{}", &shape[..s], new_section, &shape[e..])
    } else if text.is_empty() {
        shape.to_string()
    } else {
        // Visio writes both rows when it makes the section (visAltText is a flag it keeps at 0)
        let section = format!("<Section N='User'>{}<Row N='visAltText'><Cell N='Value' V='0'/><Cell N='Prompt' V=''/></Row></Section>", row_xml(text));
        if shape.ends_with("/>") && !shape.contains("</Shape>") {
            format!("{}>{}</Shape>", &shape[..shape.len() - 2], section)
        } else {
            let at = ["<Section N='Geometry'", "<Section N=\"Geometry\"", "<Text"].iter().filter_map(|needle| own.find(needle)).min().unwrap_or_else(|| if own_end < shape.len() { own_end } else { shape.rfind("</Shape>").unwrap_or(shape.len()) }); // a group without geometry: before its members
            format!("{}{}{}", &shape[..at], section, &shape[at..])
        }
    }
}

/// Set the alt text of shape `shape_id` on page `page_index` (an empty text removes it). Returns the new package bytes.
pub fn set_alt_text(package: &Package, page_index: usize, shape_id: &str, text: &str) -> Result<Vec<u8>> {
    if text.chars().count() > MAX_ALT_TEXT {
        return Err(Error(format!("the alt text can have at most {MAX_ALT_TEXT} characters")));
    }
    let part = page_part(package, page_index)?;
    let mut xml = package.text(&part)?.to_string();
    let (start, end) = find_shape_range(&xml, shape_id).ok_or_else(|| Error(format!("shape {shape_id} not found")))?;
    let new_shape = edit_shape(&xml[start..end], text);
    xml.replace_range(start..end, &new_shape);
    with_part(package, &part, xml.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::edit_shape;

    #[test]
    fn a_shape_without_a_user_section_gets_both_rows_before_its_geometry() {
        let out = edit_shape("<Shape ID='1'><Cell N='PinX' V='1'/><Section N='Geometry' IX='0'></Section></Shape>", "hi");
        assert_eq!(out, "<Shape ID='1'><Cell N='PinX' V='1'/><Section N='User'><Row N='visAltDescription'><Cell N='Value' V='hi' U='STR'/><Cell N='Prompt' V=''/></Row><Row N='visAltText'><Cell N='Value' V='0'/><Cell N='Prompt' V=''/></Row></Section><Section N='Geometry' IX='0'></Section></Shape>");
    }

    #[test]
    fn other_rows_of_the_user_section_are_kept() {
        let shape = "<Shape ID='1'><Section N='User'><Row N='mine'><Cell N='Value' V='keep me' U='STR'/></Row></Section><Section N='Geometry' IX='0'></Section></Shape>";
        let with = edit_shape(shape, "alt");
        assert!(with.contains("<Row N='mine'><Cell N='Value' V='keep me' U='STR'/></Row><Row N='visAltDescription'>"), "{with}");
        let without = edit_shape(&with, "");
        assert_eq!(without, shape, "removing the row gives the original back");
    }

    #[test]
    fn a_self_closing_shape_and_a_self_closing_section_are_opened() {
        assert_eq!(edit_shape("<Shape ID='1'/>", "x"), "<Shape ID='1'><Section N='User'><Row N='visAltDescription'><Cell N='Value' V='x' U='STR'/><Cell N='Prompt' V=''/></Row><Row N='visAltText'><Cell N='Value' V='0'/><Cell N='Prompt' V=''/></Row></Section></Shape>");
        assert_eq!(edit_shape("<Shape ID='1'><Section N='User'/></Shape>", "x"), "<Shape ID='1'><Section N='User'><Row N='visAltDescription'><Cell N='Value' V='x' U='STR'/><Cell N='Prompt' V=''/></Row></Section></Shape>");
        assert_eq!(edit_shape("<Shape ID='1'><Section N='User'/></Shape>", ""), "<Shape ID='1'><Section N='User'/></Shape>");
    }

    #[test]
    fn only_the_shapes_own_user_section_counts_not_a_members() {
        let group = "<Shape ID='1' Type='Group'><Cell N='PinX' V='1'/><Shapes><Shape ID='2'><Section N='User'><Row N='visAltDescription'><Cell N='Value' V='member' U='STR'/></Row></Section></Shape></Shapes></Shape>";
        let out = edit_shape(group, "group alt");
        assert!(out.contains("member") && out.contains("group alt"), "{out}");
        assert!(out.find("group alt").unwrap() < out.find("<Shapes>").unwrap(), "the group's own section comes before its members");
    }
}
