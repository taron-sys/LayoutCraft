//! Rasterize a scene through the shared SVG path using `resvg`.
use crate::scene::Scene;
use crate::svg::to_svg_scaled;

/// The usvg options with the system fonts loaded. Scanning the fonts costs tens to hundreds of milliseconds, so it happens once per process and
/// every render shares the font database (an `Arc`).
fn shared_options() -> resvg::usvg::Options<'static> {
    let fonts = crate::fonts::database();
    let mut options = resvg::usvg::Options::default();
    options.fontdb = fonts;
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
                    let alias = crate::fonts::aliases(name);
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

/// The usvg options every raster uses (shared system fonts, Windows font substitutes). Public so a benchmark can time the SVG parse on its own.
pub fn usvg_options() -> resvg::usvg::Options<'static> {
    shared_options()
}

/// Draw the whole page at `scale` into a premultiplied pixmap (the one place that parses the SVG and runs the rasterizer).
fn render_pixmap(scene: &Scene, scale: f64) -> Result<resvg::tiny_skia::Pixmap, String> {
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
    Ok(pixmap)
}

/// Straight (not premultiplied) RGBA bytes of a pixmap, row by row. The same per-pixel conversion `Pixmap::encode_png` applies, so this is
/// byte for byte what a PNG round trip returns, without the encode, the decode or the extra copy.
pub(crate) fn straight_rgba(pixmap: &resvg::tiny_skia::Pixmap) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    rgba
}

pub fn render_png(scene: &Scene, scale: f64) -> Result<Vec<u8>, String> {
    render_pixmap(scene, scale)?
        .encode_png()
        .map_err(|error| format!("PNG encode failed: {error}"))
}

/// Rasterize the whole page at `scale` device pixels per scene pixel. Returns (width, height, RGBA bytes, straight alpha, not premultiplied): the
/// layout `egui::ColorImage::from_rgba_unmultiplied` and the PNG format take. This is exactly what `render_png` followed by a PNG decode gives
/// (the pixels are the same bytes), without the encode and decode.
pub fn render_rgba(scene: &Scene, scale: f64) -> Result<(u32, u32, Vec<u8>), String> {
    let pixmap = render_pixmap(scene, scale)?;
    Ok((pixmap.width(), pixmap.height(), straight_rgba(&pixmap)))
}

/// Rasterize one window of the page at `scale` device pixels per scene pixel: `region` is (x, y, width, height) in scene pixels counted from the
/// top-left corner of the page picture. Returns (width, height, RGBA bytes, not premultiplied). Only the window gets pixels, so a tiny shape
/// can be drawn sharp at a zoom where the whole page would exceed the raster size limit.
pub fn render_region_rgba(scene: &Scene, scale: f64, region: [f64; 4]) -> Result<(u32, u32, Vec<u8>), String> {
    if !scale.is_finite() || scale <= 0.0 || region.iter().any(|v| !v.is_finite()) || region[2] <= 0.0 || region[3] <= 0.0 {
        return Err("scale and region must be positive finite numbers".to_string());
    }
    let svg = to_svg_scaled(scene, scale);
    let options = shared_options();
    let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|error| format!("SVG parse failed: {error}"))?;
    let width = ((region[2] * scale).round() as u32).max(1);
    let height = ((region[3] * scale).round() as u32).max(1);
    if u64::from(width) * u64::from(height) > 64_000_000 {
        return Err("the window is too large to draw".to_string());
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| "could not allocate raster pixmap".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_row(scale as f32, 0.0, 0.0, scale as f32, (-region[0] * scale) as f32, (-region[1] * scale) as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok((width, height, straight_rgba(&pixmap)))
}

/// Rasterize a scene to 8 bit RGB rows (width, height, bytes) composited on the paper colour `paper` (the scene's own background is part of the picture).
pub fn render_rgb(scene: &Scene, scale: f64, paper: [u8; 3]) -> Result<(u32, u32, Vec<u8>), String> {
    let pixmap = render_pixmap(scene, scale)?;
    let (width, height) = (pixmap.width(), pixmap.height());
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

    /// A window of the page drawn at a large scale shows the same picture as that part of the whole page, and keeps detail the whole-page raster lacks.
    #[test]
    fn a_region_is_the_matching_part_of_the_page_at_the_same_scale() {
        use crate::scene::Scene;
        let scene = Scene { fit_text: false, title: "t".into(), min_x: 0.0, min_y: 0.0, width: 100.0, height: 50.0, background: "#ffffff".into(), items: vec![] };
        let (w, h, rgba) = super::render_region_rgba(&scene, 4.0, [10.0, 5.0, 20.0, 10.0]).unwrap();
        assert_eq!((w, h), (80, 40));
        assert_eq!(rgba.len(), 80 * 40 * 4);
        assert!(rgba.chunks(4).all(|px| px == [255, 255, 255, 255]));
        assert!(super::render_region_rgba(&scene, 4.0, [0.0, 0.0, 0.0, 10.0]).is_err());
    }
}
