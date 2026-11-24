//! Rasterize a scene through the shared SVG path using `resvg`.
use crate::scene::Scene;
use crate::svg::to_svg_scaled;

/// The usvg options with the system fonts loaded. Scanning the fonts costs tens to hundreds of milliseconds, so it happens once per process and
/// every render shares the font database (an `Arc`).
fn shared_options() -> resvg::usvg::Options<'static> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> = std::sync::OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut database = resvg::usvg::fontdb::Database::new();
        database.load_system_fonts();
        // fontdb's generic families default to Arial, Times New Roman and Courier New; where those are not installed (Linux) nothing would
        // match and no text would be drawn, so point each generic family at an installed face.
        let installed = |name: &str| database.faces().any(|face| face.families.iter().any(|(family, _)| family.eq_ignore_ascii_case(name)));
        let first = |names: &[&str]| names.iter().find(|name| installed(name)).map(|name| name.to_string());
        let sans = first(&["Arial", "Liberation Sans", "Carlito", "Noto Sans", "DejaVu Sans", "Adwaita Sans", "Cantarell", "Helvetica"]);
        let serif = first(&["Times New Roman", "Liberation Serif", "Caladea", "Noto Serif", "DejaVu Serif"]);
        let mono = first(&["Courier New", "Liberation Mono", "Noto Sans Mono", "DejaVu Sans Mono"]);
        if let Some(name) = sans {
            database.set_sans_serif_family(name);
        }
        if let Some(name) = serif {
            database.set_serif_family(name);
        }
        if let Some(name) = mono {
            database.set_monospace_family(name);
        }
        std::sync::Arc::new(database)
    });
    let mut options = resvg::usvg::Options::default();
    options.fontdb = fonts.clone();
    // Visio files name Windows fonts; try the open, metric-compatible faces after each of them, and fall back to sans-serif last.
    options.font_resolver.select_font = Box::new(|font, db| {
        use resvg::usvg::fontdb::{Family, Query, Stretch, Style, Weight};
        use resvg::usvg::{FontFamily, FontStretch, FontStyle};
        let mut names: Vec<String> = Vec::new();
        let mut order: Vec<Option<usize>> = Vec::new(); // Some(index into names) for a named family, None for a generic marker
        let mut generics: Vec<FontFamily> = Vec::new();
        for family in font.families() {
            match family {
                FontFamily::Named(name) => {
                    order.push(Some(names.len()));
                    names.push(name.clone());
                    let alias: &[&str] = match name.to_ascii_lowercase().as_str() {
                        "calibri" => &["Carlito"],
                        "cambria" => &["Caladea"],
                        "arial" | "helvetica" | "segoe ui" | "tahoma" | "verdana" => &["Liberation Sans"],
                        "times new roman" | "times" => &["Liberation Serif"],
                        "courier new" | "consolas" => &["Liberation Mono"],
                        _ => &[],
                    };
                    for a in alias {
                        order.push(Some(names.len()));
                        names.push(a.to_string());
                    }
                }
                other => {
                    order.push(None);
                    generics.push(other.clone());
                }
            }
        }
        let mut generic = generics.into_iter();
        let mut list: Vec<Family> = order
            .iter()
            .map(|slot| match slot {
                Some(i) => Family::Name(&names[*i]),
                None => match generic.next() {
                    Some(FontFamily::Serif) => Family::Serif,
                    Some(FontFamily::Cursive) => Family::Cursive,
                    Some(FontFamily::Fantasy) => Family::Fantasy,
                    Some(FontFamily::Monospace) => Family::Monospace,
                    _ => Family::SansSerif,
                },
            })
            .collect();
        list.push(Family::SansSerif);
        let query = Query {
            families: &list,
            weight: Weight(font.weight()),
            stretch: match font.stretch() {
                FontStretch::UltraCondensed => Stretch::UltraCondensed,
                FontStretch::ExtraCondensed => Stretch::ExtraCondensed,
                FontStretch::Condensed => Stretch::Condensed,
                FontStretch::SemiCondensed => Stretch::SemiCondensed,
                FontStretch::Normal => Stretch::Normal,
                FontStretch::SemiExpanded => Stretch::SemiExpanded,
                FontStretch::Expanded => Stretch::Expanded,
                FontStretch::ExtraExpanded => Stretch::ExtraExpanded,
                FontStretch::UltraExpanded => Stretch::UltraExpanded,
            },
            style: match font.style() {
                FontStyle::Normal => Style::Normal,
                FontStyle::Italic => Style::Italic,
                FontStyle::Oblique => Style::Oblique,
            },
        };
        db.query(&query)
    });
    options
}

pub fn render_png(scene: &Scene, scale: f64) -> Result<Vec<u8>, String> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err("scale must be a positive finite number".to_string());
    }
    let svg = to_svg_scaled(scene, scale);
    let options = shared_options();
    let tree = resvg::usvg::Tree::from_str(&svg, &options)
        .map_err(|error| format!("SVG parse failed: {error}"))?;
    let size = tree.size();
    let width = ((size.width() as f64 * scale).round() as u32).max(1);
    let height = ((size.height() as f64 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "could not allocate raster pixmap".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|error| format!("PNG encode failed: {error}"))
}

/// Rasterize a scene to 8 bit RGB rows (width, height, bytes) composited on the paper colour `paper` (the scene's own background is part of the picture).
pub fn render_rgb(scene: &Scene, scale: f64, paper: [u8; 3]) -> Result<(u32, u32, Vec<u8>), String> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err("scale must be a positive finite number".to_string());
    }
    let svg = to_svg_scaled(scene, scale);
    let options = shared_options();
    let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|error| format!("SVG parse failed: {error}"))?;
    let size = tree.size();
    let width = ((size.width() as f64 * scale).round() as u32).max(1);
    let height = ((size.height() as f64 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| "could not allocate raster pixmap".to_string())?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32), &mut pixmap.as_mut());
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for px in pixmap.pixels() {
        // premultiplied colour over the paper: c + paper * (1 - alpha)
        let a = px.alpha() as u32;
        let over = |c: u8, p: u8| (c as u32 + (p as u32 * (255 - a) + 127) / 255).min(255) as u8;
        rgb.extend_from_slice(&[over(px.red(), paper[0]), over(px.green(), paper[1]), over(px.blue(), paper[2])]);
    }
    Ok((width, height, rgb))
}

#[cfg(test)]
mod tests {
    use super::shared_options;

    fn inked_pixels(family: &str) -> usize {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40"><text x="5" y="28" font-size="24" font-family="{family}">Hello</text></svg>"#);
        let tree = resvg::usvg::Tree::from_str(&svg, &shared_options()).unwrap();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(200, 40).unwrap();
        resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
        pixmap.pixels().iter().filter(|pixel| pixel.alpha() > 0).count()
    }

    /// Visio files name Windows fonts that other systems lack; text must still be drawn with a substitute.
    #[test]
    fn text_in_an_uninstalled_font_is_still_drawn() {
        assert!(inked_pixels("'No Such Face 7'") > 0);
        assert!(inked_pixels("Calibri") > 0);
    }
}
