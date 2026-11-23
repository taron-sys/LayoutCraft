//! The theme gallery: the public themes plus, in a maintainer build, the private extra themes; each with colours, fonts and effects.
//!
//! Measured with probe-theme-apply.ps1 (step 257): `Page.SetTheme(name)` in Visio changes NO shape cell (fills, lines and fonts stay `THEMEVAL()`
//! formulas); it adds the theme as a DrawingML part (`visio/theme/theme1.xml`: `a:clrScheme`, `a:fontScheme`, `a:fmtScheme` plus Visio's own
//! extension lists) and writes the cells `ColorSchemeIndex`, `EffectSchemeIndex`, `ConnectorSchemeIndex`, `FontSchemeIndex` and `ThemeIndex`
//! (all the same number, the gallery's index) to the page sheet. The theme parts we write are generated from our own tables by `themegen.rs`.

/// Every gallery theme as (name, gallery index): the extra themes when the private module is built in (`private/themes-extra/`,
/// see `build.rs`), then the themes of the public source (`theme_data.rs`).
pub fn all() -> Vec<(&'static str, u32)> {
    let mut v: Vec<(&'static str, u32)> = Vec::new();
    #[cfg(private_themes)]
    v.extend(crate::theme_data_private::THEMES.iter().copied());
    v.extend(crate::theme_data::OWN_THEMES.iter().map(|(name, index, _, _)| (*name, *index)));
    v
}

/// Whether the private extra themes are part of this build (false in the public source build).
pub const fn has_private_themes() -> bool {
    cfg!(private_themes)
}

/// (our name, the name written in a file) pairs of the private extra themes, kept only to recognise theme names found in files from other applications and to map
/// the name a script passes to `Page.SetTheme(name)` to ours. Empty in the public build.
fn alias_names() -> &'static [(&'static str, &'static str)] {
    #[cfg(private_themes)]
    {
        &crate::theme_data_private::ALIAS_NAMES
    }
    #[cfg(not(private_themes))]
    {
        &[]
    }
}

/// Our name of a theme given either our name or a name found in a file (case-insensitive); None when it is neither.
pub fn own_name(name: &str) -> Option<&'static str> {
    let n = name.trim();
    all().into_iter().map(|(own, _)| own).find(|own| own.eq_ignore_ascii_case(n)).or_else(|| alias_names().iter().find(|(_, visio)| visio.eq_ignore_ascii_case(n)).map(|(own, _)| *own))
}

/// The theme tree (compact notation of `themegen.rs`) of the gallery theme `name`.
pub(crate) fn dsl(name: &str) -> Option<String> {
    #[cfg(private_themes)]
    if let Some((_, tree)) = crate::theme_data_private::THEME_DSL.iter().find(|(n, _)| *n == name) {
        return Some((*tree).to_string());
    }
    crate::theme_data::OWN_THEMES.iter().find(|(n, ..)| *n == name).map(|(n, index, accents, font)| crate::theme_data::own_dsl(n, *index, accents, font))
}

/// The generated DrawingML theme part of gallery theme `name`.
pub fn part(name: &str) -> String {
    crate::themegen::theme_xml(own_name(name).unwrap_or(name)).unwrap_or_default()
}

/// The six accent colours (hex RGB, no `#`) of a theme part, for a gallery preview.
pub fn accents(xml: &str) -> Vec<String> {
    (1..=6)
        .filter_map(|n| {
            let open = format!("<a:accent{n}><a:srgbClr val=\"");
            let at = xml.find(&open)? + open.len();
            Some(xml[at..at + xml[at..].find('"')?].to_string())
        })
        .collect()
}

/// The first two colours (`varColor1`, `varColor2`, hex RGB) of each of the theme's colour variations (Design > Variants), for a preview: `[[c1, c2], ...]` (up to 4).
pub fn variants(xml: &str) -> Vec<Vec<String>> {
    let Some(at) = xml.find("<vt:variationClrSchemeLst") else { return Vec::new() };
    let end = xml[at..].find("</vt:variationClrSchemeLst>").map_or(xml.len(), |n| at + n);
    xml[at..end]
        .split("<vt:variationClrScheme")
        .skip(1)
        .filter(|part| part.starts_with('>') || part.starts_with(' ')) // not `variationClrSchemeLst`; some entries carry `monotone="1"`
        .take(4)
        .map(|part| {
            (1..=2)
                .filter_map(|n| {
                    let open = format!("<vt:varColor{n}><a:srgbClr val=\"");
                    let p = part.find(&open)? + open.len();
                    Some(part[p..p + part[p..].find('"')?].to_string())
                })
                .collect()
        })
        .collect()
}

/// The gallery as JSON: `[{name, index, accents: [...], fonts: {major, minor}}]`.
pub fn gallery() -> serde_json::Value {
    let font = |xml: &str, tag: &str| -> Option<String> {
        let open = xml.find(&format!("<a:{tag}>"))?;
        let key = "<a:latin typeface=\"";
        let at = xml[open..].find(key)? + open + key.len();
        Some(xml[at..at + xml[at..].find('"')?].to_string())
    };
    serde_json::Value::Array(
        all()
            .iter()
            .map(|(name, index)| {
                let xml = part(name);
                serde_json::json!({ "name": name, "index": index, "accents": accents(&xml), "variants": variants(&xml), "fonts": { "major": font(&xml, "majorFont"), "minor": font(&xml, "minorFont") } })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_part_is_a_theme_with_six_accents_and_its_own_name() {
        assert!(all().len() >= 3);
        for (name, index) in all() {
            let xml = part(name);
            let xml = xml.as_str();
            assert!(xml.contains("<a:clrScheme") && xml.contains("<a:fontScheme") && xml.contains("<a:fmtScheme"), "{name}");
            assert!(xml.contains(&format!("name=\"{name}\"")) || name == "Standard", "{name}");
            assert_eq!(accents(xml).len(), 6, "{name}");
            assert_eq!(variants(xml).len(), 4, "{name}: four colour variants");
            assert!((30..200).contains(&index), "{name}: {index}");
        }
        let g = gallery();
        assert_eq!(g.as_array().unwrap().len(), all().len());
        assert!(g[0]["fonts"]["minor"].as_str().is_some());
    }

    #[test]
    fn the_public_themes_are_always_there_and_have_their_own_colours() {
        assert_eq!(accents(&part("Daylight"))[0], "3A7BD5");
        assert_eq!(variants(&part("Dusk"))[1][0], "C94C7B");
        assert_eq!(own_name("meadow"), Some("Meadow"));
        assert_eq!(own_name("No Such Theme"), None);
    }

    #[cfg(private_themes)]
    #[test]
    fn the_private_themes_are_built_in() {
        assert!(has_private_themes());
        assert_eq!(all().len(), 14);
        assert_eq!(own_name("Office 2016"), Some("Standard"));
        assert_eq!(accents(&part("Essential"))[0], "1CADE4", "Essential accent 1");
        assert_eq!(variants(&part("Essential")).len(), 4, "four colour variants");
        assert_eq!(variants(&part("Essential"))[1][0], "9CBEBD", "Essential variant 1");
    }

    #[cfg(not(private_themes))]
    #[test]
    fn the_public_build_has_no_private_themes() {
        assert!(!has_private_themes());
        assert_eq!(all().len(), 3);
        assert_eq!(own_name("Office 2016"), None);
    }
}
