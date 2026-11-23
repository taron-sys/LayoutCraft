//! Writes a theme part (`visio/theme/themeN.xml`) from our own compact theme data (`theme_data.rs`, and the private extra-theme module, see `themes.rs`).
//!
//! The data is a tree in a small notation: `name(key='value',...){children}` or `name(key='value');` for a leaf.
//! Element names carry their prefix (`a:` = DrawingML, `vt:` = the Visio theme extension namespace of [MS-ODRAWXML]).
//! The generator adds what is not data: the XML declaration, the namespace declarations (`xmlns:a` on the root, `xmlns:vt` on the first `vt:`
//! element below an `a:` element) and the element order. The files Visio itself wrote are no longer part of the source tree.

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_VT: &str = "http://schemas.microsoft.com/office/visio/2012/theme";

fn esc(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

/// Expands one node (and its children) starting at `i`; returns the new position.
fn node(s: &[u8], mut i: usize, parent_vt: bool, q: char, out: &mut String) -> usize {
    let start = i;
    while i < s.len() && !matches!(s[i], b'(' | b'{' | b';' | b'}') {
        i += 1;
    }
    let name = std::str::from_utf8(&s[start..i]).unwrap_or("");
    let is_vt = name.starts_with("vt:");
    out.push('<');
    out.push_str(name);
    if is_vt && !parent_vt {
        out.push_str(&format!(" xmlns:vt={q}{NS_VT}{q}"));
    }
    if i < s.len() && s[i] == b'(' {
        i += 1;
        loop {
            let k = i;
            while s[i] != b'=' {
                i += 1;
            }
            let key = std::str::from_utf8(&s[k..i]).unwrap_or("");
            i += 2; // = and the opening quote
            let v = i;
            while s[i] != b'\'' {
                i += 1;
            }
            let val = std::str::from_utf8(&s[v..i]).unwrap_or("");
            out.push_str(&format!(" {key}={q}{}{q}", esc(val)));
            i += 1; // closing quote
            match s[i] {
                b',' => i += 1,
                _ => break,
            }
        }
        i += 1; // the closing parenthesis
    }
    if i < s.len() && s[i] == b'{' {
        out.push('>');
        i += 1;
        while s[i] != b'}' {
            i = node(s, i, is_vt, q, out);
        }
        i += 1;
        out.push_str(&format!("</{name}>"));
    } else {
        out.push_str("/>");
        i += 1; // the semicolon
    }
    i
}

/// The style sheets of a new drawing (`<StyleSheets>...</StyleSheets>`, single-quoted attributes like the rest of our document part).
pub fn style_sheets_xml() -> String {
    let s = crate::style_data::STYLE_SHEETS_DSL.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        i = node(s, i, false, '\'', &mut out);
    }
    out
}

/// The theme part for the gallery theme `name` (None when the table has no such theme).
pub fn theme_xml(name: &str) -> Option<String> {
    let dsl = crate::themes::dsl(name)?;
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<a:theme xmlns:a=\"{NS_A}\" name=\"{}\">", esc(name));
    let s = dsl.as_bytes();
    let mut i = 0;
    while i < s.len() {
        i = node(s, i, false, '"', &mut out);
    }
    out.push_str("</a:theme>");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every element opened is closed again, in order (the generator never writes a broken part).
    fn well_formed(xml: &str) -> bool {
        let mut stack: Vec<&str> = Vec::new();
        let mut rest = xml;
        while let Some(p) = rest.find('<') {
            let e = rest[p..].find('>').map(|n| p + n).unwrap_or(rest.len() - 1);
            let tag = &rest[p + 1..e];
            if let Some(name) = tag.strip_prefix('/') {
                if stack.pop() != Some(name) {
                    return false;
                }
            } else if !tag.starts_with('?') && !tag.ends_with('/') {
                stack.push(tag.split([' ', '/']).next().unwrap_or(""));
            }
            rest = &rest[e + 1..];
        }
        stack.is_empty()
    }

    #[test]
    fn every_generated_theme_part_is_well_formed_and_carries_its_gallery_index() {
        for (name, index) in crate::themes::all() {
            let xml = theme_xml(name).unwrap();
            assert!(well_formed(&xml), "{name}");
            assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>"), "{name}");
            assert!(xml.contains(&format!("<a:theme xmlns:a=\"{NS_A}\" name=\"{name}\">")), "{name}");
            assert!(xml.contains(&format!("schemeEnum=\"{index}\"")), "{name}: the scheme number is the gallery index");
            assert!(xml.contains("<a:clrScheme") && xml.contains("<a:fontScheme") && xml.contains("<a:fmtScheme") && xml.contains("variationStyleSchemeLst"), "{name}");
            // xmlns:vt is declared where the first vt: element of a branch starts, and nowhere else
            assert_eq!(xml.matches("xmlns:vt=").count(), xml.matches("<a:ext uri=").count(), "{name}");
        }
        assert!(theme_xml("No Such Theme").is_none());
    }

    #[test]
    fn the_generated_style_sheets_are_the_six_styles_of_a_new_drawing() {
        let xml = style_sheets_xml();
        assert!(well_formed(&xml));
        assert!(xml.starts_with("<StyleSheets>") && xml.ends_with("</StyleSheets>"));
        for name in ["No Style", "Text Only", "None", "Normal", "Guide", "Theme"] {
            assert!(xml.contains(&format!("NameU='{name}'")), "{name}");
        }
        assert_eq!(xml.matches("<StyleSheet ").count(), 6);
        assert!(xml.contains("F='THEMEVAL()'"), "the Theme style holds THEMEVAL() formulas");
    }
}
