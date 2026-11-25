//! Time every stage of drawing a page: scene build, SVG string, usvg parse, rasterize, PNG encode, PNG decode (the path `raster_scene` used to take),
//! and the direct RGBA path (`render_rgba`), on every public fixture page and on a generated stress drawing, at 1x and 2x. Medians over several runs.
//!
//! usage: cargo run --release -p layoutcraft-render --example raster_bench -- [--runs N] [--no-stress] [--only <substring of the page name>] [--hash]
//!
//! `--hash` prints one FNV-1a hash of the straight RGBA pixels per page and scale instead of timings: run it before and after a change to see that
//! no pixel moved.
//!
//! Timing code lives here, not in the library (AGENTS.md rule 9). The page's raster scale follows the app (`raster_scale_for`): the wanted scale is
//! clamped so the long side stays within 8192 px and the picture within about 24 million pixels.
use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use layoutcraft_model::Document;
use layoutcraft_render::{render_png, render_rgba, scene_from_document_with_media, to_svg_scaled, usvg_options, Scene};
use layoutcraft_vsdx::{import_document, media_bytes, Package};

/// The app's `raster_scale_for` (rust/layoutcraft-app/src/main.rs): what scale a page of w x h scene px gets for a wanted scale.
fn app_scale(w: f64, h: f64, wanted: f64) -> f64 {
    let longest = w.max(h).max(1.0);
    let base = (8192.0 / longest).min(1.0);
    let cap = (8192.0 / longest).min((24.0e6 / (w.max(1.0) * h.max(1.0))).sqrt()).max(base);
    wanted.clamp(base, cap)
}

fn median_ms<T>(runs: usize, mut f: impl FnMut() -> T) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            black_box(f());
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    times[times.len() / 2]
}

struct Page {
    name: String,
    document: Document,
    media: BTreeMap<String, Vec<u8>>,
    index: usize,
}

fn fixture_pages() -> Vec<Page> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/public/sources");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures/public/sources")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "vsdx"))
        .collect();
    paths.sort();
    let mut pages = Vec::new();
    for path in paths {
        let package = Package::open(&path).expect("open fixture");
        let document = import_document(&package).expect("import fixture");
        let mut media = BTreeMap::new();
        for (id, record) in &document.media {
            if let Some(bytes) = media_bytes(&package, record) {
                media.insert(id.clone(), bytes);
            }
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        for index in 0..document.pages.len() {
            pages.push(Page { name: format!("{stem}#{index}"), document: document.clone(), media: media.clone(), index });
        }
    }
    pages
}

/// 2,000 shapes on a 50 x 40 grid (rectangles, rounded rectangles and ellipses with a label each) and 1,000 connectors (elbow, straight and curved)
/// between neighbours, built as model JSON (the same data an imported page has).
fn stress_document() -> Document {
    let (columns, rows) = (50usize, 40usize);
    let (cell_w, cell_h) = (60.0, 40.0);
    let (shape_w, shape_h) = (44.0, 26.0);
    let fonts = ["Calibri", "Cambria", "Arial", "Segoe UI"];
    let fills = ["#4472c4", "#ed7d31", "#a5a5a5", "#ffc000", "#5b9bd5", "#70ad47"];
    let mut shapes = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let n = row * columns + column;
            let id = (n + 1).to_string();
            let x = column as f64 * cell_w + 8.0;
            let y = row as f64 * cell_h + 7.0;
            let (w, h) = (shape_w, shape_h);
            let d = match n % 3 {
                // rectangle
                0 | 1 => format!("M0 {h}L{w} {h}L{w} 0L0 0L0 {h}Z"),
                // ellipse from four arcs
                _ => format!("M0 {}A{} {} 0 0 1 {} 0A{} {} 0 0 1 {w} {}A{} {} 0 0 1 {} {h}A{} {} 0 0 1 0 {}Z", h / 2.0, w / 2.0, h / 2.0, w / 2.0, w / 2.0, h / 2.0, h / 2.0, w / 2.0, h / 2.0, w / 2.0, w / 2.0, h / 2.0, h / 2.0),
            };
            let fill = fills[n % fills.len()];
            let font = fonts[n % fonts.len()];
            let label = format!("Node {n}");
            let mut shape = serde_json::json!({
                "type": "custom", "visioId": id, "x": x, "y": y, "w": w, "h": h, "z": n as f64,
                "label": label, "fill": fill, "fillOpacity": 1.0, "stroke": "#1f3864", "strokeWidth": 1.0,
                "fontFamily": font, "fontSize": 8.0, "textColor": "#000000", "align": "center", "vAlign": "middle",
                "textBox": {"x": 2.0, "y": 2.0, "w": w - 4.0, "h": h - 4.0},
                "parts": [{"d": d, "fill": fill, "fillOpacity": 1.0, "stroke": "#1f3864", "strokeWidth": 1.0}],
                "rich": {"paragraphs": [{"align": "center", "runs": [{"text": label, "style": {"fontFamily": font, "fontSize": 8.0, "textColor": "#000000"}}], "style": {"align": "center"}}]}
            });
            if n % 3 == 1 {
                shape["rounding"] = serde_json::json!(5.0);
            }
            shapes.push(shape);
        }
    }
    let mut connectors = Vec::new();
    let routes = ["elbow", "straight", "curved"];
    for k in 0..1000usize {
        // every other shape of a row to its right neighbour, then every other one downwards: 1,000 in all
        let (from, to, from_side, to_side) = if k < 500 {
            let row = k / 25;
            let column = (k % 25) * 2;
            let a = row * columns + column;
            (a, a + 1, "e", "w")
        } else {
            let j = k - 500;
            let row = (j / 50) * 2;
            let column = j % 50;
            let a = row * columns + column;
            (a, a + columns, "s", "n")
        };
        connectors.push(serde_json::json!({
            "visioId": (3000 + k).to_string(), "route": routes[k % 3], "routeStyle": 1, "label": "",
            "from": {"shapeId": format!("shape:visio:{}", from + 1), "side": from_side},
            "to": {"shapeId": format!("shape:visio:{}", to + 1), "side": to_side},
            "stroke": "#000000", "strokeWidth": 1.0, "strokeStyle": "solid", "arrowEnd": "filled", "arrowStart": "none", "z": (2000 + k) as f64
        }));
    }
    let document = serde_json::json!({"pages": [{"name": "stress", "shapes": shapes, "connectors": connectors}]});
    Document::from_json(&document.to_string()).expect("stress document")
}

struct Row {
    page: String,
    scale: f64,
    px: (u32, u32),
    items: usize,
    scene: f64,
    options: f64,
    svg: f64,
    svg_bytes: usize,
    parse: f64,
    raster: f64,
    encode: f64,
    decode: f64,
    png_bytes: usize,
    old_total: f64,
    direct: f64,
}

fn measure(page: &Page, wanted: f64, runs: usize) -> Row {
    let build = || scene_from_document_with_media(&page.document, page.index, &page.media).expect("scene");
    let scene: Scene = build();
    let (w, h) = scene.raster_size();
    let scale = app_scale(f64::from(w), f64::from(h), wanted);
    let scene_ms = median_ms(runs, build);
    let options_ms = median_ms(runs, usvg_options);
    let svg = to_svg_scaled(&scene, scale);
    if let Ok(path) = std::env::var("RASTER_BENCH_DUMP_SVG") {
        std::fs::write(path, &svg).expect("dump svg");
    }
    let svg_ms = median_ms(runs, || to_svg_scaled(&scene, scale));
    let options = usvg_options();
    let parse_ms = median_ms(runs, || resvg::usvg::Tree::from_str(&svg, &options).expect("parse"));
    let tree = resvg::usvg::Tree::from_str(&svg, &options).expect("parse");
    let size = tree.size();
    let (pw, ph) = (((f64::from(size.width()) * scale).round() as u32).max(1), ((f64::from(size.height()) * scale).round() as u32).max(1));
    let draw = || {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(pw, ph).expect("pixmap");
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32), &mut pixmap.as_mut());
        pixmap
    };
    let raster_ms = median_ms(runs, draw);
    let pixmap = draw();
    let encode_ms = median_ms(runs, || pixmap.encode_png().expect("encode"));
    let png = pixmap.encode_png().expect("encode");
    let decode_ms = median_ms(runs, || image::load_from_memory_with_format(&png, image::ImageFormat::Png).expect("decode").to_rgba8());
    // the whole old path as the app ran it on the raster thread (render_png, then the decode), and the direct path (straight RGBA from the pixmap)
    let old_total = median_ms(runs, || {
        let png = render_png(&scene, scale).expect("png");
        image::load_from_memory_with_format(&png, image::ImageFormat::Png).expect("decode").to_rgba8()
    });
    let direct = direct_ms(&scene, scale, runs);
    Row { page: page.name.clone(), scale, px: (pw, ph), items: scene.items.len(), scene: scene_ms, options: options_ms, svg: svg_ms, svg_bytes: svg.len(), parse: parse_ms, raster: raster_ms, encode: encode_ms, decode: decode_ms, png_bytes: png.len(), old_total, direct }
}

/// The whole direct path: parse, rasterize and convert to straight RGBA (what `raster_scene` runs now).
fn direct_ms(scene: &Scene, scale: f64, runs: usize) -> f64 {
    median_ms(runs, || render_rgba(scene, scale).expect("rgba"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runs = args.iter().position(|a| a == "--runs").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(7usize).max(1);
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
    let mut pages = fixture_pages();
    if !args.iter().any(|a| a == "--no-stress") {
        pages.push(Page { name: "stress (2000 shapes + 1000 connectors)".into(), document: stress_document(), media: BTreeMap::new(), index: 0 });
    }
    if let Some(only) = only {
        pages.retain(|p| p.name.contains(&only));
    }
    if args.iter().any(|a| a == "--hash") {
        for page in &pages {
            for wanted in [1.0, 2.0] {
                let scene = scene_from_document_with_media(&page.document, page.index, &page.media).expect("scene");
                let (w, h) = scene.raster_size();
                let scale = app_scale(f64::from(w), f64::from(h), wanted);
                let (pw, ph, rgba) = render_rgba(&scene, scale).expect("rgba");
                let hash = rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3));
                println!("{} {wanted}x {pw}x{ph} {hash:016x}", page.name);
            }
        }
        return;
    }
    // warm the shared font database once, so no row pays for the system font scan
    black_box(usvg_options());
    println!("runs per stage: {runs} (median, ms)\n");
    println!("| page | scale | px | items | scene | options | svg | svg KB | parse | raster | png enc | png dec | png KB | old total | direct |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for page in &pages {
        for wanted in [1.0, 2.0] {
            let r = measure(page, wanted, runs);
            println!(
                "| {} | {:.2} | {}x{} | {} | {:.2} | {:.3} | {:.2} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {:.2} | {:.2} |",
                r.page, r.scale, r.px.0, r.px.1, r.items, r.scene, r.options, r.svg, r.svg_bytes / 1024, r.parse, r.raster, r.encode, r.decode, r.png_bytes / 1024, r.old_total, r.direct
            );
        }
    }
}
