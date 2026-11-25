//! The fonts the renderer draws with, and what happens when a drawing asks for one that is not installed.
//!
//! Drawings name fonts of the machine that made them (Calibri, Segoe UI). Where such a font is missing the renderer draws the text with the
//! metric-compatible open font (Carlito for Calibri, ...) or, failing that, a sans font; `font_substitution` tells the host so it can warn.
use resvg::usvg::fontdb::Database;
use std::sync::{Arc, OnceLock};

struct Fonts {
    database: Arc<Database>,
    sans: Option<String>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut database = Database::new();
        database.load_system_fonts();
        // fontdb's generic families default to Arial, Times New Roman and Courier New; where those are not installed (Linux) nothing would
        // match and no text would be drawn, so point each generic family at an installed face.
        let installed = |name: &str| has_family(&database, name);
        let first = |names: &[&str]| names.iter().find(|name| installed(name)).map(|name| name.to_string());
        let sans = first(&["Arial", "Liberation Sans", "Carlito", "Noto Sans", "DejaVu Sans", "Adwaita Sans", "Cantarell", "Helvetica"]);
        let serif = first(&["Times New Roman", "Liberation Serif", "Caladea", "Noto Serif", "DejaVu Serif"]);
        let mono = first(&["Courier New", "Liberation Mono", "Noto Sans Mono", "DejaVu Sans Mono"]);
        if let Some(name) = sans.clone() {
            database.set_sans_serif_family(name);
        }
        if let Some(name) = serif {
            database.set_serif_family(name);
        }
        if let Some(name) = mono {
            database.set_monospace_family(name);
        }
        Fonts { database: Arc::new(share_font_files(database)), sans }
    })
}

/// A font file whose bytes are read the first time a face needs them and stay in memory after that.
struct LazyFontFile {
    path: std::path::PathBuf,
    data: OnceLock<Vec<u8>>,
}

impl AsRef<[u8]> for LazyFontFile {
    fn as_ref(&self) -> &[u8] {
        // An unreadable file gives no bytes, so no face can be parsed from it: the same outcome as fontdb failing to read it itself.
        self.data.get_or_init(|| std::fs::read(&self.path).unwrap_or_default())
    }
}

/// The same faces with their files shared in memory. A face that points at a file path makes fontdb open and map the file on every use, and the
/// SVG text code asks for the font data several times per character (has the face this char, shape, outline the glyph): on Windows that is the
/// bulk of the time spent parsing a page with text. Each file is read once, on first use (a file with several faces is shared by all of them),
/// so memory grows only by the fonts a drawing actually uses. Faces keep their order, so every query finds the face it found before.
fn share_font_files(source: Database) -> Database {
    use resvg::usvg::fontdb::{Family, Source};
    let mut shared = Database::new();
    shared.set_serif_family(source.family_name(&Family::Serif).to_string());
    shared.set_sans_serif_family(source.family_name(&Family::SansSerif).to_string());
    shared.set_cursive_family(source.family_name(&Family::Cursive).to_string());
    shared.set_fantasy_family(source.family_name(&Family::Fantasy).to_string());
    shared.set_monospace_family(source.family_name(&Family::Monospace).to_string());
    let mut files: std::collections::HashMap<std::path::PathBuf, Arc<LazyFontFile>> = std::collections::HashMap::new();
    for face in source.faces() {
        let mut face = face.clone();
        if let Source::File(path) = &face.source {
            let file = files.entry(path.clone()).or_insert_with(|| Arc::new(LazyFontFile { path: path.clone(), data: OnceLock::new() })).clone();
            face.source = Source::Binary(file);
        }
        shared.push_face_info(face);
    }
    shared
}

fn has_family(database: &Database, name: &str) -> bool {
    database.faces().any(|face| face.families.iter().any(|(family, _)| family.eq_ignore_ascii_case(name)))
}

/// The shared font database (scanning the system fonts is slow, so it happens once per process).
pub(crate) fn database() -> Arc<Database> {
    fonts().database.clone()
}

/// Open fonts that take the place of a font of Windows, with the same glyph widths.
pub(crate) fn aliases(name: &str) -> &'static [&'static str] {
    match name.to_ascii_lowercase().as_str() {
        "calibri" => &["Carlito"],
        "cambria" => &["Caladea"],
        "arial" | "helvetica" | "segoe ui" | "tahoma" | "verdana" => &["Liberation Sans"],
        "times new roman" | "times" => &["Liberation Serif"],
        "courier new" | "consolas" => &["Liberation Mono"],
        _ => &[],
    }
}

/// A font a drawing asks for and the one drawn in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSubstitution {
    pub requested: String,
    pub used: String,
}

/// The installed font families, sorted and without duplicates.
pub fn installed_font_families() -> Vec<String> {
    let mut names: Vec<String> = fonts().database.faces().filter_map(|face| face.families.first().map(|(name, _)| name.clone())).collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// The first face name of a CSS font stack (`"Calibri", Helvetica, sans-serif` gives `Calibri`).
pub fn first_font_name(stack: &str) -> String {
    stack.split(',').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string()
}

/// `Some` when the first font of `stack` is not installed: what is drawn instead. `None` when it is installed or generic (sans-serif, ...).
pub fn font_substitution(stack: &str) -> Option<FontSubstitution> {
    let requested = first_font_name(stack);
    if requested.is_empty() || matches!(requested.to_ascii_lowercase().as_str(), "sans-serif" | "serif" | "monospace" | "cursive" | "fantasy") {
        return None;
    }
    let fonts = fonts();
    if has_family(&fonts.database, &requested) {
        return None;
    }
    let used = aliases(&requested).iter().find(|name| has_family(&fonts.database, name)).map(|name| name.to_string()).or_else(|| fonts.sans.clone())?;
    Some(FontSubstitution { requested, used })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_nobody_has_is_reported_as_replaced() {
        let found = font_substitution("'No Such Face 7', Helvetica, sans-serif").expect("replaced");
        assert_eq!(found.requested, "No Such Face 7");
        assert!(!found.used.is_empty());
        assert_eq!(font_substitution("sans-serif"), None);
    }

    /// Sharing the font files in memory is a speed change only: text must come out with the same pixels as with the database fontdb loaded.
    #[test]
    fn sharing_font_files_does_not_change_what_text_looks_like() {
        let mut plain = Database::new();
        plain.load_system_fonts();
        let shared = share_font_files(plain.clone());
        assert_eq!(plain.len(), shared.len());
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="360" height="120">
            <text x="4" y="22" font-size="20" font-family="Arial, sans-serif">Hello Node 12 Wg</text>
            <text x="4" y="52" font-size="18" font-family="Times New Roman, serif" font-style="italic">Hello Node 12 Wg</text>
            <text x="4" y="82" font-size="16" font-family="Courier New, monospace" font-weight="bold">Hello Node 12 Wg</text>
            <text x="4" y="110" font-size="14" font-family="No Such Face 7">Hello Node 12 Wg</text></svg>"#;
        let draw = |database: Database| {
            let mut options = resvg::usvg::Options::default();
            options.fontdb = Arc::new(database);
            let tree = resvg::usvg::Tree::from_str(svg, &options).unwrap();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(360, 120).unwrap();
            resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
            pixmap.take()
        };
        let (a, b) = (draw(plain.clone()), draw(shared));
        assert!(a == b, "text pixels changed");
        if !plain.is_empty() {
            assert!(a.chunks(4).any(|px| px[3] > 0), "the reference drew no text, so the comparison proves nothing");
        }
    }

    #[test]
    fn an_installed_font_is_not_reported() {
        let Some(name) = installed_font_families().into_iter().next() else { return };
        assert_eq!(font_substitution(&format!("\"{name}\"")), None);
    }
}

/// The fonts of a scene that are replaced on this machine, each requested font once.
pub fn scene_font_substitutions(scene: &crate::scene::Scene) -> Vec<FontSubstitution> {
    use crate::scene::SceneItem;
    let mut stacks: Vec<&str> = Vec::new();
    for item in &scene.items {
        if let SceneItem::Shape(shape) = item {
            for text in shape.text.iter().chain(shape.texts.iter()) {
                stacks.push(&text.font_family);
                for run in text.paragraphs.iter().flat_map(|paragraph| paragraph.runs.iter()) {
                    if let Some(family) = &run.font_family {
                        stacks.push(family);
                    }
                }
            }
        }
    }
    let mut found: Vec<FontSubstitution> = Vec::new();
    for stack in stacks {
        if let Some(substitution) = font_substitution(stack) {
            if !found.iter().any(|known| known.requested.eq_ignore_ascii_case(&substitution.requested)) {
                found.push(substitution);
            }
        }
    }
    found
}
