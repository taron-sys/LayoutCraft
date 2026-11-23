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
        std::sync::Arc::new(database)
    });
    let mut options = resvg::usvg::Options::default();
    options.fontdb = fonts.clone();
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
