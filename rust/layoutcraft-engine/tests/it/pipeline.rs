//! What the app and the MCP server do with a file once they have it: open it, read the model, render every page, run the
//! checks, export it in every format, edit it and save it. Results are ignored: only a panic or a hang is a finding.
use layoutcraft_engine::Engine;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// At most this many pages and shapes are walked, so that a sweep case stays fast.
const WALK_PAGES: usize = 6;
const WALK_SHAPES: usize = 6;

/// How many inputs each sweep put through the door: (offered, opened). A sweep whose mutants are all rejected tests nothing.
/// The case threads carry the name of their sweep.
static DOOR: Mutex<BTreeMap<String, (usize, usize)>> = Mutex::new(BTreeMap::new());

fn count(opened: bool) {
    let name = std::thread::current().name().unwrap_or("").to_string();
    if let Ok(mut door) = DOOR.lock() {
        let entry = door.entry(name).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += usize::from(opened);
    }
}

pub fn report(name: &str) {
    if let Ok(door) = DOOR.lock() {
        let (offered, opened) = door.get(name).copied().unwrap_or((0, 0));
        eprintln!("[{name}] {opened} of {offered} inputs opened and went through the pipeline");
    }
}

/// Read and export everything of an open drawing. The slow exports (the PDF rasterises every page) run for one input in eight.
pub fn exercise(engine: &mut Engine, bytes: &[u8]) {
    let deep = layoutcraft_engine::fingerprint(bytes) % 8 == 0;
    let _ = engine.json();
    let _ = engine.masters_json();
    let _ = engine.doc_props_json();
    let _ = engine.themes_json();
    let _ = engine.comments_json();
    let _ = engine.page_names_json();
    let _ = engine.styles_list_json();
    let _ = engine.background_info_json();
    let pages = engine.page_count().unwrap_or(0).min(WALK_PAGES);
    for page in 0..pages {
        let _ = engine.svg(page, false);
        let _ = engine.svg(page, true);
        let _ = engine.connectors_json(page);
        let _ = engine.guides_json(page);
        let _ = engine.shape_names_json(page);
        let _ = engine.group_info_json(page);
        let _ = engine.page_size_json(page);
        for id in shape_ids(engine, page) {
            let _ = engine.shape_sheet_json(page, &id);
            let _ = engine.connection_points_json(page, &id);
            let _ = engine.text_info_json(page, &id);
            let _ = engine.text_state_json(page, &id, 0, 3);
            let _ = engine.style_info_json(page, &id);
            let _ = engine.hyperlinks_json(page, &id);
            let _ = engine.data_sort_json(page, &id);
        }
        let _ = engine.export_excalidraw(page);
        let _ = engine.export_dxf(page);
    }
    let _ = engine.check_diagram(true);
    let _ = engine.export_drawio();
    let _ = engine.export_webpage();
    if !deep {
        return;
    }
    let _ = engine.export_pdf(1.0);
    // the save: the bytes must open again
    if let Some(bytes) = engine.export().map(|b| b.to_vec()) {
        let mut again = Engine::new();
        if again.open(bytes).is_ok() {
            let _ = again.svg(0, false);
            let _ = engine.compare_with(&again);
        }
    }
}

pub fn shape_ids(engine: &Engine, page: usize) -> Vec<String> {
    engine
        .document()
        .and_then(|d| d.pages.get(page))
        .map(|p| p.shapes.iter().filter_map(|s| s.id.clone()).take(WALK_SHAPES).collect())
        .unwrap_or_default()
}

/// Edit an open drawing in every way the editor can, with hostile numbers.
pub fn edit(engine: &mut Engine) {
    let ids = shape_ids(engine, 0);
    let id = ids.first().cloned().unwrap_or_else(|| "1".to_string());
    let numbers = [0.0, -0.0, 1.0, -1.0, 1e308, -1e308, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-320, 4294967296.0];
    for (n, &a) in numbers.iter().enumerate() {
        let b = numbers[(n + 3) % numbers.len()];
        for op in [1, 2, 3, 7, 8, 10, 12, 13, 14, 15, 16, 17, 18] {
            let mut e = Engine::new();
            if let Some(bytes) = engine.export() {
                if e.open(bytes.to_vec()).is_err() {
                    return;
                }
            }
            let _ = e.edit(0, op, &id, a, b, a, b, (n % 3) as i32);
            let _ = e.svg(0, false);
        }
    }
    let mut e = Engine::new();
    if let Some(bytes) = engine.export() {
        if e.open(bytes.to_vec()).is_err() {
            return;
        }
    }
    for kind in 0..8 {
        let _ = e.draw(0, kind, 1.0, 1.0, 3.0, 2.0);
        let _ = e.draw(0, kind, f64::NAN, 1.0, 1e308, -1e308);
    }
    let _ = e.draw_polyline(0, &[(0.0, 0.0), (f64::NAN, 1.0), (1e308, 1e308)], true);
    let _ = e.draw_curve(0, &[(0.0, 0.0), (1.0, 1.0)]);
    let _ = e.draw_arc(0, (0.0, 0.0), (1.0, 1.0), f64::INFINITY);
    let _ = e.copy(0, &ids);
    let _ = e.paste(0, 1.0, 1.0);
    if let [from, to, ..] = &ids[..] {
        let _ = e.add_connector(0, from, Some(0), to, None);
        let _ = e.move_connector_end(0, &id, true, Some((to.as_str(), Some(99))), (f64::NAN, 1.0));
    }
    let _ = e.preview_move(0, &ids, (1e308, f64::NAN));
    let _ = e.insert_picture(0, b"\x89PNG\r\n\x1a\n", 0.0, 0.0, 1.0, 1.0);
    for op in 1..=40 {
        let mut f = Engine::new();
        if let Some(bytes) = engine.export() {
            if f.open(bytes.to_vec()).is_err() {
                return;
            }
        }
        let _ = f.page_op(op, 0, 1, "n");
        let _ = f.page_op(op, 99, usize::MAX, "5");
        let _ = f.svg(0, false);
    }
    for op in 0..12 {
        let _ = e.group_op(0, op, &id);
    }
    let _ = e.undo();
    let _ = e.redo();
    let _ = e.svg(0, true);
    let _ = e.export();
}

/// A package that opened: read it, export it, edit it.
pub fn vsdx(bytes: &[u8]) {
    let mut engine = Engine::new();
    let opened = engine.open(bytes.to_vec()).is_ok();
    count(opened);
    if opened {
        exercise(&mut engine, bytes);
    }
}

/// `vsdx`, plus the edits (slower: used for the unmutated fixtures and a share of the mutants).
pub fn vsdx_and_edit(bytes: &[u8]) {
    let mut engine = Engine::new();
    let opened = engine.open(bytes.to_vec()).is_ok();
    count(opened);
    if opened {
        exercise(&mut engine, bytes);
        edit(&mut engine);
    }
}

pub fn stencil(bytes: &[u8]) {
    let mut engine = Engine::new();
    let _ = engine.new_blank(11.0, 8.5, false, "Page-1");
    if let Ok(text) = engine.stencil_open(bytes.to_vec()) {
        let masters: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let index = masters["index"].as_u64().unwrap_or(0) as usize;
        if let Some(list) = masters["masters"].as_array() {
            for master in list.iter().take(6) {
                let id = master["id"].as_str().map(|s| s.to_string()).unwrap_or_else(|| master["id"].to_string());
                let _ = engine.stencil_thumb(index, &id);
                let _ = engine.stencil_drop(0, index, &id, 2.0, 2.0);
                let _ = engine.add_master(0, &id, 3.0, 3.0);
            }
        }
        let _ = engine.svg(0, false);
        let _ = engine.export();
    }
    let _ = engine.stencil_close(0);
}

pub fn drawio(bytes: &[u8]) {
    let mut engine = Engine::new();
    let opened = engine.import_drawio(bytes).is_ok();
    count(opened);
    if opened {
        exercise(&mut engine, bytes);
    }
}

pub fn excalidraw(bytes: &[u8]) {
    let mut engine = Engine::new();
    let opened = engine.import_excalidraw(&String::from_utf8_lossy(bytes)).is_ok();
    count(opened);
    if opened {
        exercise(&mut engine, bytes);
    }
}

pub fn metafile(bytes: &[u8]) {
    use layoutcraft_vsdx::emf::{emf_natural_size, emf_to_svg_at, is_enhanced_metafile, metafile_to_svg_at, wmf_to_svg_at};
    let _ = is_enhanced_metafile(bytes);
    let _ = emf_natural_size(bytes);
    for width in [None, Some(0.0), Some(f64::NAN), Some(1e-9), Some(200.0)] {
        let _ = emf_to_svg_at(bytes, width);
        let _ = wmf_to_svg_at(bytes, width);
        let _ = metafile_to_svg_at(bytes, width);
    }
}

/// A metafile as the picture of a drawing: the package is opened and rendered.
pub fn picture_in_package(bytes: &[u8]) {
    let mut engine = Engine::new();
    if engine.open(bytes.to_vec()).is_ok() {
        let _ = engine.json();
        let _ = engine.svg(0, false);
        let _ = engine.export_pdf(1.0);
    }
}
