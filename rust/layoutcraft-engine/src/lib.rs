//! The editing engine (docs/34-desktop-build.md, step N1): the open drawing with its undo and redo history, the
//! clipboard and the opened stencils, behind a safe Rust API. The wasm crate (`layoutcraft-wasm`) wraps one `Engine`
//! in the C ABI of the web shell; the native app uses it directly. Every method returns `Result<_, String>` (the
//! text is what the wasm build puts into its output buffer), so both hosts report errors the same way.
pub mod borders;
pub mod check;
pub mod compare;
pub mod drawio_edges;
pub mod drawio_export;
pub mod drawio_import;
pub mod drawio_shapes;
pub mod dxf;
pub mod excalidraw_export;
pub mod excalidraw_import;
pub mod inventory;
pub mod pdf;
pub mod svgpdf;
pub mod svgutil;
pub mod webpage;
mod webpage_template;

use std::collections::BTreeMap;

use layoutcraft_model::Document;
use layoutcraft_render::{scene_from_document_with_options, to_svg, SceneOptions};
use layoutcraft_vsdx::groupedit::GroupEditor;
use layoutcraft_vsdx::{import_document, media_bytes, Package};

/// The open drawing and everything an edit session keeps next to it. The fields are public while the wasm crate
/// still holds functions that have not moved into methods yet (they go away as the methods arrive).
#[derive(Default)]
pub struct Engine {
    pub document: Option<Document>,
    /// What the last import numbered (media), so that an edit re-imports only the pages it changed.
    pub import_state: Option<layoutcraft_vsdx::ImportState>,
    pub package: Option<Package>,
    /// The package bytes behind `package` (what `export` returns).
    pub bytes: Vec<u8>,
    /// Earlier package bytes, most recent last (`undo`).
    pub history: Vec<Vec<u8>>,
    /// Undone states for `redo`: (fingerprint of the bytes the undo led to, the bytes that were undone). An entry
    /// only counts while the document still is the state it led to, so any new edit makes it stale.
    pub redo: Vec<(u64, Vec<u8>)>,
    /// Copied shapes (their XML) for paste.
    pub clipboard: Vec<String>,
    /// External stencils opened next to the drawing (`None` = closed); they survive `open`.
    pub stencils: Vec<Option<Package>>,
    /// The index of the built-in "Basic shapes" stencil in `stencils`, once it was opened.
    pub builtin_stencil: Option<usize>,
    /// `Connect` rows (old ids) of the connectors on the clipboard whose ends stay glued.
    pub clipboard_connects: Vec<String>,
    /// The move preview: the page, the fingerprint of the bytes, the editor of that page and its `Connect` rows.
    pub preview: Option<(usize, u64, GroupEditor, Vec<layoutcraft_vsdx::Connect>)>,
    /// Set when the open drawing was converted from an old Visio format (`.vsd`, `.vdx`, ...): what the user has to know.
    /// The drawing is a new `.vsdx` package from then on; the old file is never written.
    pub conversion: Option<Conversion>,
}

/// How the open drawing came out of an old format.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// The format of the file that was opened.
    pub format: &'static str,
    /// What was left behind or is only approximate.
    pub notes: Vec<String>,
}

impl Conversion {
    /// One paragraph for the host to show.
    pub fn message(&self) -> String {
        let mut text = format!("Converted from the {} to a new .vsdx drawing; the original file is not changed.", self.format);
        if !self.notes.is_empty() {
            text.push_str(&format!(" Please check: {}.", self.notes.join("; ")));
        }
        text
    }
}

/// The bytes of a `.vsdx` package: `bytes` themselves, or the conversion of a drawing in an old format.
fn as_package_bytes(bytes: Vec<u8>) -> Result<(Vec<u8>, Option<Conversion>), String> {
    let Some(kind) = layoutcraft_vsdx::legacy::detect(&bytes) else {
        return Ok((bytes, None));
    };
    let converted = layoutcraft_vsdx::legacy::convert(&bytes).map_err(|error| error.to_string())?;
    Ok((converted.vsdx, Some(Conversion { format: kind.describe(), notes: converted.notes })))
}

/// A hash of the package bytes: what ties an undone state to the document it came from.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.len().hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a VSDX / VSTX from its bytes: the undo history, redo states and the clipboard are cleared (opened stencils stay).
    /// A drawing in an old format (binary or XML) is converted first; [`Engine::conversion`] then says so.
    pub fn open(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let (bytes, conversion) = as_package_bytes(bytes)?;
        // Same path as the wasm build always used for a first import (a package without `source`).
        let package = Package::from_reader(std::io::Cursor::new(bytes.clone())).map_err(|error| error.to_string())?;
        let (document, import_state) = layoutcraft_vsdx::import_document_with(&package, None).map_err(|error| error.to_string())?;
        self.install(bytes, package, document, import_state);
        self.conversion = conversion;
        Ok(())
    }

    /// Start a new, empty drawing: one page of `width` x `height` inches (shown in millimetres when `metric`) called `name`.
    pub fn new_blank(&mut self, width: f64, height: f64, metric: bool, name: &str) -> Result<(), String> {
        let options = layoutcraft_vsdx::BlankOptions { width_in: width, height_in: height, metric, name: name.to_string() };
        let bytes = layoutcraft_vsdx::write_blank_document(&options).map_err(|e| e.to_string())?;
        let package = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let (document, import_state) = layoutcraft_vsdx::import_document_with(&package, None).map_err(|e| e.to_string())?;
        self.install(bytes, package, document, import_state);
        Ok(())
    }

    fn install(&mut self, bytes: Vec<u8>, package: Package, document: Document, import_state: layoutcraft_vsdx::ImportState) {
        self.import_state = Some(import_state);
        self.document = Some(document);
        self.package = Some(package);
        self.bytes = bytes;
        self.history.clear();
        self.redo.clear();
        self.clipboard.clear();
        self.clipboard_connects.clear();
        self.conversion = None;
    }

    /// Re-open `bytes` as the current document and make it the export source.
    pub fn adopt(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let package = Package::from_bytes(&bytes).map_err(|error| error.to_string())?;
        self.adopt_package(bytes, package)
    }

    /// `adopt` for bytes that were parsed already (the edit path parses its result once for the connector check).
    pub fn adopt_package(&mut self, bytes: Vec<u8>, package: Package) -> Result<(), String> {
        let previous = match (&self.package, &self.document, &self.import_state) {
            (Some(old_package), Some(old_document), Some(old_state)) => Some((old_package, old_document, old_state)),
            _ => None,
        };
        let (document, import_state) = layoutcraft_vsdx::import_document_with(&package, previous).map_err(|error| error.to_string())?;
        self.import_state = Some(import_state);
        self.document = Some(document);
        self.package = Some(package);
        self.bytes = bytes;
        Ok(())
    }

    /// The current document as VSDX bytes (`None` when nothing is open).
    pub fn export(&self) -> Option<&[u8]> {
        if self.bytes.is_empty() {
            None
        } else {
            Some(&self.bytes)
        }
    }

    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    /// Number of pages of the open drawing (`None` when nothing is open).
    pub fn page_count(&self) -> Option<usize> {
        self.document.as_ref().map(|document| document.pages.len())
    }

    /// The imported document model as JSON (the web shell's `model()`).
    pub fn json(&self) -> Option<String> {
        self.document.as_ref().and_then(|document| serde_json::to_string(document).ok())
    }

    /// One page as SVG; `print` leaves out shapes on non-printing layers. `None` when nothing is open.
    pub fn svg(&self, page: usize, print: bool) -> Option<Result<String, String>> {
        self.document.as_ref().map(|document| {
            let mut media = BTreeMap::new();
            if let Some(package) = self.package.as_ref() {
                for (id, record) in &document.media {
                    if let Some(bytes) = media_bytes(package, record) {
                        media.insert(id.clone(), bytes);
                    }
                }
            }
            scene_from_document_with_options(document, page, &media, SceneOptions { print, fit_text: true })
                .map(|scene| to_svg(&scene))
                .map_err(|error| error.to_string())
        })
    }

    /// Undo the last edit. `Ok(false)` when there is nothing to undo.
    pub fn undo(&mut self) -> Result<bool, String> {
        let Some(previous) = self.history.pop() else {
            return Ok(false);
        };
        let undone = self.bytes.clone();
        self.redo.push((fingerprint(&previous), undone));
        self.adopt(previous)?;
        Ok(true)
    }

    /// Redo the last undone edit (`Ok(false)` when there is nothing to redo; a new edit after the undo makes the undone states stale and drops them).
    pub fn redo(&mut self) -> Result<bool, String> {
        let current = fingerprint(&self.bytes);
        let Some((led_to, bytes)) = self.redo.pop() else {
            return Ok(false);
        };
        if led_to != current {
            self.redo.clear();
            return Ok(false);
        }
        let previous = self.bytes.clone();
        self.adopt(bytes)?;
        self.history.push(previous);
        Ok(true)
    }

    /// Number of edits that can be redone.
    pub fn redo_depth(&mut self) -> usize {
        let current = fingerprint(&self.bytes);
        while self.redo.last().is_some_and(|(led_to, _)| *led_to != current) {
            self.redo.clear();
        }
        self.redo.len()
    }

    /// Number of edits that can be undone.
    pub fn undo_depth(&self) -> usize {
        self.history.len()
    }

    /// Merge the edits made since the undo depth was `depth` into one undo step. Returns the new depth.
    pub fn collapse_history(&mut self, depth: usize) -> usize {
        let keep = depth + 1;
        if self.history.len() > keep {
            self.history.truncate(keep);
        }
        self.history.len()
    }
}

/// Parse the points text "x,y;x,y;..." (inches, y up) of the freeform tools.
pub fn parse_points(text: &str) -> Result<Vec<(f64, f64)>, String> {
    text.split(';')
        .filter(|s| !s.trim().is_empty())
        .map(|pair| {
            let mut it = pair.split(',');
            let x = it.next().and_then(|v| v.trim().parse::<f64>().ok());
            let y = it.next().and_then(|v| v.trim().parse::<f64>().ok());
            match (x, y, it.next()) {
                (Some(x), Some(y), None) => Ok((x, y)),
                _ => Err(format!("{pair} is not a point x,y")),
            }
        })
        .collect()
}

// --- drawing tools: each is ONE undo step and returns the new shape's id ---------------------------------
impl Engine {
    fn package_ref(&self) -> Result<&Package, String> {
        self.package.as_ref().ok_or_else(|| "no document".to_string())
    }

    /// Make `bytes` the current document as one undo step (the previous bytes go onto the history).
    pub fn commit(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let previous = self.bytes.clone();
        self.adopt(bytes)?;
        self.history.push(previous);
        Ok(())
    }

    /// Draw a shape from scratch: `kind` 0 rectangle, 1 rounded rectangle, 2 ellipse, 3 diamond, 4 triangle, 5 text box, 6 line, in the box (x0,y0)-(x1,y1) in page inches (y up).
    pub fn draw(&mut self, page: usize, kind: i32, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<String, String> {
        let package = self.package_ref()?;
        let kind = layoutcraft_vsdx::draw::DrawKind::from_code(kind).ok_or_else(|| format!("unknown shape kind {kind}"))?;
        let (bytes, id) = layoutcraft_vsdx::draw::draw_shape(package, page, kind, x0, y0, x1, y1).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(id)
    }

    /// Insert a picture (PNG, JPEG, GIF or BMP bytes) in the box (x0,y0)-(x1,y1).
    pub fn insert_picture(&mut self, page: usize, data: &[u8], x0: f64, y0: f64, x1: f64, y1: f64) -> Result<String, String> {
        let package = self.package_ref()?;
        let (bytes, id) = layoutcraft_vsdx::draw::insert_picture(package, page, data, x0, y0, x1, y1).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(id)
    }

    /// Draw a smooth curve (a spline) through the points (4 to 200).
    pub fn draw_curve(&mut self, page: usize, points: &[(f64, f64)]) -> Result<String, String> {
        let package = self.package_ref()?;
        let (bytes, id) = layoutcraft_vsdx::draw::draw_curve(package, page, points).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(id)
    }

    /// Draw a circular arc from `from` to `to`, bowed by `bow` inches (positive = to the right of the direction).
    pub fn draw_arc(&mut self, page: usize, from: (f64, f64), to: (f64, f64), bow: f64) -> Result<String, String> {
        let package = self.package_ref()?;
        let (bytes, id) = layoutcraft_vsdx::draw::draw_arc(package, page, from, to, bow).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(id)
    }

    /// Draw a freeform shape through the points: a polygon when `closed`, else an open polyline.
    pub fn draw_polyline(&mut self, page: usize, points: &[(f64, f64)], closed: bool) -> Result<String, String> {
        let package = self.package_ref()?;
        let (bytes, id) = layoutcraft_vsdx::draw::draw_freeform(package, page, points, closed).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(id)
    }
}

// --- the edit dispatcher (one undo step) ---------------------------------------------------------------------
use layoutcraft_vsdx::writer::{patch_page, patch_page_cells};

impl Engine {
    /// Apply one edit `op` to the shape(s) `id` on `page` (ops: 1 resize group, 2 move, 3 resize, 4 delete, 5 move several, 6 delete several,
    /// 7 rotate to angle, 8 set text, 10 format, 11 rotate several, 12 flip, 13 format a text range, 14 align, 15 distribute, 16 layer,
    /// 17 / 18 shape data row; see the wasm crate's `vl_edit` for the meaning of `a`..`d` and `flag`) and re-import. Glued connectors follow.
    #[allow(clippy::too_many_arguments)]
    pub fn edit(&mut self, page: usize, op: i32, id: &str, a: f64, b: f64, c: f64, d: f64, flag: i32) -> Result<(), String> {
        let id = id.to_string();
        let package = self.package.as_ref().ok_or("no document")?;
        let editor = match &self.import_state {
            Some(state) => state.open_editor(package, package, page),
            None => GroupEditor::open(package, page),
        }
        .map_err(|e| e.to_string())?;
        // Op 4 deletes the shape: glued connector ends stay where they are (as in Visio) and the
        // shape, its contents and their glue rows go.
        let mut removed: Vec<String> = Vec::new();
        let mut new_font: Option<String> = None;
        let patches = match op {
            4 => {
                let ids = editor.subtree_ids(&id).map_err(|e| e.to_string())?;
                let part = package.pages().map_err(|e| e.to_string())?[page].part.clone();
                let connects = package.connects(&part).map_err(|e| e.to_string())?;
                let patches = editor.detach_patches(&ids, &connects);
                removed.push(id.clone());
                Ok::<_, layoutcraft_vsdx::Error>(patches)
            }
            // Op 5 moves several shapes (ids separated by newlines) by one page delta; op 6 deletes
            // several. Both write one patch and re-import once.
            5 => {
                let mut all = Vec::new();
                for one in id.split('\n').filter(|one| !one.is_empty()) {
                    all.extend(editor.move_member(one, (a, b), false).map_err(|e| e.to_string())?);
                }
                Ok::<_, layoutcraft_vsdx::Error>(all)
            }
            6 => {
                let part = package.pages().map_err(|e| e.to_string())?[page].part.clone();
                let connects = package.connects(&part).map_err(|e| e.to_string())?;
                let mut gone = Vec::new();
                for one in id.split('\n').filter(|one| !one.is_empty()) {
                    gone.extend(editor.subtree_ids(one).map_err(|e| e.to_string())?);
                    removed.push(one.to_string());
                }
                Ok::<_, layoutcraft_vsdx::Error>(editor.detach_patches(&gone, &connects))
            }
            // Op 10 formats shapes: the id parameter is "<id>\n<id>...\n<json>" with the JSON
            // `{fill, line, weight, pattern, fontPt}` (all optional; colours "#rrggbb" or "none").
            10 => {
                let (ids, json) = id.rsplit_once('\n').ok_or_else(|| "expected shape ids and a style".to_string())?;
                let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("bad style: {e}"))?;
                let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
                let edit = layoutcraft_vsdx::groupedit::StyleEdit {
                    fill: text("fill"),
                    line: text("line"),
                    weight_px: value.get("weight").and_then(|v| v.as_f64()),
                    pattern: value.get("pattern").and_then(|v| v.as_u64()).map(|v| v as u32),
                    font_pt: value.get("fontPt").and_then(|v| v.as_f64()),
                    bold: value.get("bold").and_then(|v| v.as_bool()),
                    italic: value.get("italic").and_then(|v| v.as_bool()),
                    underline: value.get("underline").and_then(|v| v.as_bool()),
                    font_family: text("fontFamily"),
                    text_color: text("textColor"),
                    fill_trans: value.get("fillTrans").and_then(|v| v.as_f64()),
                    line_trans: value.get("lineTrans").and_then(|v| v.as_f64()),
                    shadow: value.get("shadow").and_then(|v| v.as_bool()),
                    begin_arrow: value.get("beginArrow").and_then(|v| v.as_u64()).map(|v| v as u32),
                    end_arrow: value.get("endArrow").and_then(|v| v.as_u64()).map(|v| v as u32),
                    begin_arrow_size: value.get("beginArrowSize").and_then(|v| v.as_u64()).map(|v| v as u32),
                    end_arrow_size: value.get("endArrowSize").and_then(|v| v.as_u64()).map(|v| v as u32),
                    fill_pattern: value.get("fillPattern").and_then(|v| v.as_u64()).map(|v| v as u32),
                    // custom gradient: "gradientStops": [[position 0..1, "#rrggbb", transparency 0..1], ...]
                    gradient_stops: value.get("gradientStops").and_then(|v| v.as_array()).map(|stops| {
                        stops
                            .iter()
                            .map(|stop| {
                                (
                                    stop.get(0).and_then(|v| v.as_f64()).unwrap_or(f64::NAN),
                                    stop.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    stop.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0),
                                )
                            })
                            .collect()
                    }),
                    gradient_angle_deg: value.get("gradientAngle").and_then(|v| v.as_f64()),
                    gradient_dir: value.get("gradientDir").and_then(|v| v.as_u64()).map(|v| v as u32),
                    gradient_enabled: value.get("gradientEnabled").and_then(|v| v.as_bool()),
                    glow_px: value.get("glowPx").and_then(|v| v.as_f64()),
                    glow_color: value.get("glowColor").and_then(|v| v.as_str()).map(str::to_string),
                    glow_trans: value.get("glowTrans").and_then(|v| v.as_f64()),
                    soft_edges_px: value.get("softEdgesPx").and_then(|v| v.as_f64()),
                    reflection: value.get("reflection").and_then(|v| v.as_bool()),
                    bevel: value.get("bevel").and_then(|v| v.as_u64()).map(|v| v as u32),
                    bevel_px: value.get("bevelPx").and_then(|v| v.as_f64()),
                    fill_back: text("fillBack"),
                    shadow_color: text("shadowColor"),
                    shadow_dx: value.get("shadowDx").and_then(|v| v.as_f64()),
                    shadow_dy: value.get("shadowDy").and_then(|v| v.as_f64()),
                    shadow_blur: value.get("shadowBlur").and_then(|v| v.as_f64()),
                    shadow_trans: value.get("shadowTrans").and_then(|v| v.as_f64()),
                    shadow_type: value.get("shadowType").and_then(|v| v.as_u64()).map(|v| v as u32),
                    shadow_scale: value.get("shadowScale").and_then(|v| v.as_f64()),
                    shadow_angle: value.get("shadowAngle").and_then(|v| v.as_f64()),
                    line_cap: value.get("lineCap").and_then(|v| v.as_u64()).map(|v| v as u32),
                    rounding_px: value.get("rounding").and_then(|v| v.as_f64()),
                    lock: value.get("lock").and_then(|v| v.as_bool()),
                };
                new_font = edit.font_family.clone();
                let ids: Vec<String> = ids.split('\n').filter(|i| !i.is_empty()).map(str::to_string).collect();
                editor.style_patches(&ids, &edit)
            }
            // Op 8 sets the text of a shape: the id parameter is "<id>\n<new text>"; only the changed
            // characters are replaced so the formatting runs stay.
            8 => {
                let (target, text) = id.split_once('\n').unwrap_or((id.as_str(), ""));
                Ok::<_, layoutcraft_vsdx::Error>(vec![layoutcraft_vsdx::writer::ShapeCellPatch::new(target).edit_text(text)])
            }
            // Op 13 formats a RANGE of a shape's text: the id parameter is "<id> NEWLINE <json>" with
            // `{"start", "end", "cells": {"Strikethru": "1", ...}, "set": bits, "clear": bits, "para": {"HorzAlign": "2", ...}}`
            // (character offsets of the plain text); runs and paragraphs are split at the ends of the range.
            13 => {
                let (target, json) = id.split_once(char::from(10u8)).ok_or_else(|| "text format: expected id, newline, json".to_string())?;
                let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("text format: {e}"))?;
                let pairs = |key: &str| -> Vec<(String, String)> {
                    value.get(key).and_then(|v| v.as_object()).map(|map| map.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect()).unwrap_or_default()
                };
                let char_cells = pairs("cells");
                if let Some((_, font)) = char_cells.iter().find(|(k, _)| k == "Font") {
                    new_font = Some(font.clone());
                }
                let set = value.get("set").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let clear = value.get("clear").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let range = layoutcraft_vsdx::writer::TextRangeFormat {
                    start: value.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                    end: value.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                    char_cells,
                    char_style: (set != 0 || clear != 0).then_some((set, clear)),
                    para_cells: pairs("para"),
                    // what the shape shows now (a master's size/font/colour is not in the page shape): new run rows copy it
                    base_cells: self.document.as_ref()
                        .and_then(|doc| {
                            let shape = doc.pages.get(page)?.shapes.iter().find(|s| s.visio_id.as_deref() == Some(target))?.clone();
                            let mut cells: Vec<(String, String)> = Vec::new();
                            if let Some(px) = shape.font_size {
                                cells.push(("Size".to_string(), (px / 96.0).to_string()));
                            }
                            if let Some(font) = shape.font_family {
                                // the model keeps a CSS stack ("Calibri", Helvetica, ...): the file wants the first face name
                                let face = font.split(',').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                                if !face.is_empty() {
                                    cells.push(("Font".to_string(), face));
                                }
                            }
                            if let Some(color) = shape.text_color.filter(|c| c.starts_with('#') && c.len() == 7) {
                                cells.push(("Color".to_string(), color));
                            }
                            Some(cells)
                        })
                        .unwrap_or_default(),
                };
                Ok::<_, layoutcraft_vsdx::Error>(vec![layoutcraft_vsdx::writer::ShapeCellPatch::new(target).text_range(range)])
            }
            // Op 11 turns the shapes (ids separated by newlines) by `a` radians; one shape about its pin,
            // several as one body about the centre of their joint box. Op 12 mirrors them about the
            // vertical axis (`flag` 0) or the horizontal axis (`flag` 1) through that centre.
            11 | 12 => {
                let ids: Vec<String> = id.split('\n').filter(|one| !one.is_empty()).map(str::to_string).collect();
                if op == 11 { editor.rotate_many(&ids, a) } else { editor.flip_many(&ids, flag != 0) }
            }
            // Op 14 aligns the shapes (ids separated by newlines) to the first one: `flag` is unused, `a` the mode (0 left,
            // 1 centre, 2 right, 3 top, 4 middle, 5 bottom). Op 15 spaces them evenly (`flag` 0 horizontally, 1 vertically).
            14 | 15 => {
                let ids: Vec<String> = id.split('\n').filter(|one| !one.is_empty()).map(str::to_string).collect();
                if op == 14 { editor.align_many(&ids, a.max(0.0) as u32) } else { editor.distribute_many(&ids, flag != 0) }
            }
            // Op 17 writes a Shape Data row: `id`, newline, JSON `{"name","label","value","prompt","format","type","invisible"}`;
            // op 18 removes the row `id` newline name.
            17 | 18 => {
                let (target, rest) = id.split_once(char::from(10u8)).ok_or_else(|| "shape data: expected id, newline, payload".to_string())?;
                let data = if op == 18 {
                    let name = rest.trim().to_string();
                    let inherited = editor.master_supplies_data_row(target, &name);
                    layoutcraft_vsdx::writer::DataEdit { name, remove: true, inherited, ..Default::default() }
                } else {
                    let v: serde_json::Value = serde_json::from_str(rest).map_err(|e| format!("shape data: {e}"))?;
                    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    layoutcraft_vsdx::writer::DataEdit {
                        name: text("name"),
                        remove: false,
                        label: text("label"),
                        value: text("value"),
                        prompt: text("prompt"),
                        format: text("format"),
                        kind: v.get("type").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                        invisible: v.get("invisible").and_then(|x| x.as_bool()).unwrap_or(false),
                        sort_key: text("sortKey"),
                        inherited: false,
                    }
                };
                Ok::<_, layoutcraft_vsdx::Error>(vec![layoutcraft_vsdx::writer::ShapeCellPatch::new(target.to_string()).data_edit(data)])
            }
            // Op 16 puts the shapes (ids separated by newlines) on layer `a` (`flag` 1) or takes them off it (`flag` 0).
            16 => {
                let ids: Vec<String> = id.split('\n').filter(|one| !one.is_empty()).map(str::to_string).collect();
                editor.set_layer_membership(&ids, a.max(0.0) as u32, flag != 0)
            }
            // Op 7 turns a shape about its pin to the absolute angle `a` (radians).
            7 => editor.rotate_member(&id, a),
            1 => editor.resize_group(&id, (a, b), (c, d)),
            2 => editor.move_member(&id, (a, b), flag != 0),
            3 => editor.resize_member(&id, (a, b), (c, d), flag != 0),
            _ => return Err(format!("unknown edit operation {op}")),
        }
        .map_err(|e| e.to_string())?;
        let mut bytes = patch_page(package, page, &patches, &[], &removed).map_err(|e| e.to_string())?;
        if let Some(font) = new_font {
            // A font the document does not list yet is added to its font list, as Visio does.
            let styled = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
            bytes = layoutcraft_vsdx::writer::ensure_face_names(&styled, &[font]).map_err(|e| e.to_string())?;
        }
        // Connectors glued to connection points follow the shapes that moved. The result is parsed once: the package is kept
        // when nothing is rerouted.
        let mut moved = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
        {
            let part = moved.pages().map_err(|e| e.to_string())?[page].part.clone();
            let connects = moved.connects(&part).map_err(|e| e.to_string())?;
            // Only an operation that can change a shape's frame reroutes the glued connectors; text, format, layer and shape data edits and
            // deletes (4, 6: glued ends stay where they are) leave every route alone.
            let reroute = !matches!(op, 4 | 6 | 8 | 10 | 13 | 16 | 17 | 18);
            if reroute && !connects.is_empty() {
                let editor = match &self.import_state {
                    Some(state) => state.open_editor(package, &moved, page),
                    None => GroupEditor::open(&moved, page),
                }
                .map_err(|e| e.to_string())?;
                let jump = moved.pages().map_err(|e| e.to_string())?[page].line_jump.clone().unwrap_or_default();
                let reroutes = editor.reroute_connectors(&connects, &jump);
                if !reroutes.is_empty() {
                    bytes = patch_page_cells(&moved, page, &reroutes).map_err(|e| e.to_string())?;
                    moved = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
                }
            }
        }
        let previous = self.bytes.clone();
        self.adopt_package(bytes, moved)?;
        self.history.push(previous);
        Ok(())
    }
}

impl Engine {
    /// The new routes of the connectors glued to the shapes `ids` of `page` when they move by `delta` (JSON `[{"id","route":[[x,y]..]}]`). The page's editor is kept between calls while the document does not change.
    pub fn preview_move(&mut self, page: usize, ids: &[String], delta: (f64, f64)) -> Result<String, String> {
        let print = fingerprint(&self.bytes);
        let fresh = !matches!(&self.preview, Some((p, f, _, _)) if *p == page && *f == print);
        if fresh {
            let package = self.package.as_ref().ok_or("no document")?;
            let part = package.pages().map_err(|e| e.to_string())?.get(page).ok_or("no such page")?.part.clone();
            let connects = package.connects(&part).map_err(|e| e.to_string())?;
            let editor = GroupEditor::open(package, page).map_err(|e| e.to_string())?;
            self.preview = Some((page, print, editor, connects));
        }
        let (_, _, editor, connects) = self.preview.as_ref().ok_or("no preview")?;
        let routes = editor.preview_connector_routes(connects, ids, delta);
        Ok(serde_json::Value::Array(routes.into_iter().map(|(id, route)| serde_json::json!({"id": id, "route": route.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>()})).collect()).to_string())
    }
}

impl Engine {
    /// A page, layer, guide, comment, hyperlink, style, theme, connection-point or document-property operation, ONE undo step. `op` is the number
    /// of the wasm `vl_page_op` (1..40), `a` and `b` its numbers, `text` its text.
    /// Returns the page to show afterwards.
    pub fn page_op(&mut self, op: i32, a: usize, b: usize, name: &str) -> Result<String, String> {
        let name = name.to_string();
        let package = self.package.as_ref().ok_or("no document")?;
        let (bytes, show) = match op {
            1 => layoutcraft_vsdx::pageops::add_page(package, a, if name.trim().is_empty() { None } else { Some(name.as_str()) }).map_err(|e| e.to_string())?,
            2 => {
                let bytes = layoutcraft_vsdx::pageops::delete_page(package, a).map_err(|e| e.to_string())?;
                (bytes, a.saturating_sub(1))
            }
            3 => (layoutcraft_vsdx::pageops::rename_page(package, a, &name).map_err(|e| e.to_string())?, a),
            4 => (layoutcraft_vsdx::pageops::move_page(package, a, b).map_err(|e| e.to_string())?, b),
            5 => layoutcraft_vsdx::pageops::duplicate_page(package, a).map_err(|e| e.to_string())?,
            // 7: add a layer called `name` to page `a`; 8: change layer `b` of page `a` from the text "name|visible|print|lock"
            // (an empty field leaves it, flags are 0 or 1); the output holds the page to show (`a`)
            7 => (layoutcraft_vsdx::pageops::add_layer(package, a, &name).map_err(|e| e.to_string())?.0, a),
            8 => {
                let parts: Vec<&str> = name.split('|').collect();
                let flag = |i: usize| parts.get(i).and_then(|v| match v.trim() { "1" => Some(true), "0" => Some(false), _ => None });
                let new_name = parts.first().map(|s| s.trim()).filter(|s| !s.is_empty());
                (layoutcraft_vsdx::pageops::set_layer(package, a, b as u32, new_name, flag(1), flag(2), flag(3)).map_err(|e| e.to_string())?, a)
            }
            // 10: the colour of layer `b` of page `a` from the text `#rrggbb` or `none`
            10 => (layoutcraft_vsdx::pageops::set_layer_color(package, a, b as u32, name.trim()).map_err(|e| e.to_string())?, a),
            // 9: delete layer `b` of page `a` (shapes stay, layer numbers are renumbered)
            9 => (layoutcraft_vsdx::pageops::delete_layer(package, a, b as u32).map_err(|e| e.to_string())?, a),
            // 16: add a guide to page `a` (`b` 1 = vertical, 0 = horizontal) at the position in inches in the text; 17: move the guide
            // with the ID before `|` in the text to the position after it; 18: delete the guide with the ID in the text
            16 => (layoutcraft_vsdx::guides::add_guide(package, a, b == 1, name.trim().parse::<f64>().map_err(|_| "the position is not a number")?).map_err(|e| e.to_string())?.0, a),
            17 => {
                let (id, position) = name.split_once('|').ok_or("expected id|position")?;
                (layoutcraft_vsdx::guides::move_guide(package, a, id.trim(), position.trim().parse::<f64>().map_err(|_| "the position is not a number")?).map_err(|e| e.to_string())?, a)
            }
            18 => (layoutcraft_vsdx::guides::delete_guide(package, a, name.trim()).map_err(|e| e.to_string())?, a),
            // 19: replace all hyperlinks of the shape named before the first `|` in the text by the JSON list after it (see `vl_hyperlinks`)
            19 => {
                let (id, json) = name.split_once('|').ok_or("expected shape id|json")?;
                let list: Vec<serde_json::Value> = serde_json::from_str(json).map_err(|e| format!("the hyperlinks are not valid JSON: {e}"))?;
                let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let flag = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
                let links: Vec<layoutcraft_vsdx::hyperlinks::Hyperlink> = list
                    .iter()
                    .map(|v| layoutcraft_vsdx::hyperlinks::Hyperlink { name: text(v, "name"), description: text(v, "description"), address: text(v, "address"), sub_address: text(v, "subAddress"), extra_info: text(v, "extraInfo"), frame: text(v, "frame"), new_window: flag(v, "newWindow"), default: flag(v, "default"), invisible: flag(v, "invisible") })
                    .collect();
                (layoutcraft_vsdx::hyperlinks::set_hyperlinks(package, a, id.trim(), &links).map_err(|e| e.to_string())?, a)
            }
            // 40: name the shape given before the first `|` in the text with the text after it (empty = automatic name again); the name it got (unique on the page) is read back from the model
            40 => {
                let (id, wanted) = name.split_once('|').ok_or("expected shape id|name")?;
                (layoutcraft_vsdx::shapename::set_shape_name(package, a, id.trim(), wanted).map_err(|e| e.to_string())?.0, a)
            }
            // 39: set the alt text (Shape > Alt Text, User.visAltDescription) of the shape named before the first `|` in the text to the text after it (empty = remove)
            39 => {
                let (id, alt) = name.split_once('|').ok_or("expected shape id|alt text")?;
                (layoutcraft_vsdx::alttext::set_alt_text(package, a, id.trim(), alt).map_err(|e| e.to_string())?, a)
            }
            // 20: add a comment to page `a` from the text "shape id (empty = the page itself)|author|initials|YYYY-MM-DDTHH:MM:SS|comment text"
            20 => {
                let parts: Vec<&str> = name.splitn(5, '|').collect();
                if parts.len() != 5 {
                    return Err("expected shape|author|initials|date|text".to_string());
                }
                let shape = if parts[0].trim().is_empty() { None } else { Some(parts[0].trim()) };
                (layoutcraft_vsdx::comments::add_comment(package, a, shape, parts[1], parts[2], parts[4], parts[3]).map_err(|e| e.to_string())?.0, a)
            }
            // 21: change the text of a comment from "comment id|YYYY-MM-DDTHH:MM:SS|new text"; 22: delete the comment with the ID in the text
            21 => {
                let parts: Vec<&str> = name.splitn(3, '|').collect();
                if parts.len() != 3 {
                    return Err("expected id|date|text".to_string());
                }
                (layoutcraft_vsdx::comments::set_comment_text(package, parts[0].trim(), parts[2], parts[1]).map_err(|e| e.to_string())?, a)
            }
            22 => (layoutcraft_vsdx::comments::delete_comment(package, name.trim()).map_err(|e| e.to_string())?, a),
            // 15: the drawing scale of page `a` from the text "paper,paperUnit,drawing,drawingUnit" (units in ft mm cm m)
            15 => {
                let parts: Vec<&str> = name.split(',').collect();
                if parts.len() != 4 {
                    return Err("expected paper,paperUnit,drawing,drawingUnit".to_string());
                }
                let num = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("{s} is not a number"));
                (layoutcraft_vsdx::pageops::set_drawing_scale(package, a, num(parts[0])?, parts[1], num(parts[2])?, parts[3]).map_err(|e| e.to_string())?, a)
            }
            // 23: the print setup of page `a` from JSON {orientation, margins: [l, r, t, b], grid, scale: [x, y], fit, pages: [x, y], center: [h, v]}
            23 => {
                let v: serde_json::Value = serde_json::from_str(&name).map_err(|e| format!("print setup: {e}"))?;
                let f = |x: &serde_json::Value| x.as_f64().ok_or_else(|| "a print setup number is missing".to_string());
                let pair = |k: &str| -> Result<(serde_json::Value, serde_json::Value), String> {
                    let a = v.get(k).and_then(|x| x.as_array()).filter(|a| a.len() >= 2).ok_or_else(|| format!("print setup {k} needs two values"))?;
                    Ok((a[0].clone(), a[1].clone()))
                };
                let margins = v.get("margins").and_then(|x| x.as_array()).filter(|a| a.len() == 4).ok_or("print setup margins need four values")?;
                let (sx, sy) = pair("scale")?;
                let (px, py) = pair("pages")?;
                let (cx, cy) = pair("center")?;
                let setup = layoutcraft_vsdx::pageops::PrintSetup {
                    orientation: if v.get("orientation").and_then(|x| x.as_u64()) == Some(2) { 2 } else { 1 },
                    margins: [f(&margins[0])?, f(&margins[1])?, f(&margins[2])?, f(&margins[3])?],
                    grid: v.get("grid").and_then(|x| x.as_bool()).unwrap_or(false),
                    scale: (f(&sx)?, f(&sy)?),
                    fit: v.get("fit").and_then(|x| x.as_bool()).unwrap_or(false),
                    pages: (f(&px)? as u32, f(&py)? as u32),
                    center: (cx.as_bool().unwrap_or(false), cy.as_bool().unwrap_or(false)),
                };
                (layoutcraft_vsdx::pageops::set_print_setup(package, a, &setup).map_err(|e| e.to_string())?, a)
            }
            // 24: add a connection point to the shape with the ID before the first `|` of the text, at "fx|fy" (0..1 of its width / height from the bottom left)
            24 => {
                let parts: Vec<&str> = name.split('|').collect();
                if parts.len() != 3 {
                    return Err("expected shape id|fx|fy".to_string());
                }
                let num = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("{s} is not a number"));
                (layoutcraft_vsdx::connpoints::add_connection_point(package, a, parts[0].trim(), num(parts[1])?, num(parts[2])?).map_err(|e| e.to_string())?.0, a)
            }
            // 25: move connection point n of the shape, text "shape id|n|fx|fy" (0..1 of its width / height from the bottom left); the connectors glued to
            // the shape are routed again like after any move
            25 => {
                let parts: Vec<&str> = name.split('|').collect();
                if parts.len() != 4 {
                    return Err("expected shape id|n|fx|fy".to_string());
                }
                let num = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("{s} is not a number"));
                let bytes = layoutcraft_vsdx::connpoints::move_connection_point(package, a, parts[0].trim(), parts[1].trim().parse::<usize>().map_err(|_| "the point number is not a number")?, num(parts[2])?, num(parts[3])?).map_err(|e| e.to_string())?;
                let moved = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
                let part = moved.pages().map_err(|e| e.to_string())?[a].part.clone();
                let connects = moved.connects(&part).map_err(|e| e.to_string())?;
                let editor = GroupEditor::open(&moved, a).map_err(|e| e.to_string())?;
                let jump = moved.pages().map_err(|e| e.to_string())?[a].line_jump.clone().unwrap_or_default();
                let reroutes = editor.reroute_connectors(&connects, &jump);
                (if reroutes.is_empty() { bytes } else { patch_page_cells(&moved, a, &reroutes).map_err(|e| e.to_string())? }, a)
            }
            // 26: delete connection point n of the shape, text "shape id|n"; later points are renumbered (glue follows), connectors glued to the deleted point are unglued
            26 => {
                let parts: Vec<&str> = name.split('|').collect();
                if parts.len() != 2 {
                    return Err("expected shape id|n".to_string());
                }
                (layoutcraft_vsdx::connpoints::delete_connection_point(package, a, parts[0].trim(), parts[1].trim().parse::<usize>().map_err(|_| "the point number is not a number")?).map_err(|e| e.to_string())?, a)
            }
            // 27 / 28 / 29: edit points of a freeform shape (step 251), text "shape id|index|x|y" (move vertex index to x,y; page inches),
            // "shape id|after|x|y" (insert a vertex after vertex `after`), "shape id|index" (delete); glued connectors are routed again
            27 | 28 | 29 | 34 => {
                let parts: Vec<&str> = name.split('|').collect();
                let num = |s: &str| s.trim().parse::<f64>().map_err(|_| format!("{s} is not a number"));
                let idx = |s: &str| s.trim().parse::<usize>().map_err(|_| format!("{s} is not a vertex number"));
                let edit = match (op, parts.len()) {
                    (27, 4) => layoutcraft_vsdx::groupedit::vertices::VertexEdit::Move { index: idx(parts[1])?, to: (num(parts[2])?, num(parts[3])?) },
                    (28, 4) => layoutcraft_vsdx::groupedit::vertices::VertexEdit::Insert { after: idx(parts[1])?, at: (num(parts[2])?, num(parts[3])?) },
                    (29, 2) => layoutcraft_vsdx::groupedit::vertices::VertexEdit::Delete { index: idx(parts[1])? },
                    // 34: "shape id|segment|bow": the signed bow (sagitta, inches) of the arc from vertex `segment` to the next (0 = straight)
                    (34, 3) => layoutcraft_vsdx::groupedit::vertices::VertexEdit::Bulge { segment: idx(parts[1])?, bow: num(parts[2])? },
                    _ => return Err("expected shape id|index|x|y (or shape id|index for a delete)".to_string()),
                };
                let bytes = layoutcraft_vsdx::groupedit::vertices::edit_vertices(package, a, parts[0].trim(), edit).map_err(|e| e.to_string())?;
                let moved = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
                let part = moved.pages().map_err(|e| e.to_string())?[a].part.clone();
                let connects = moved.connects(&part).map_err(|e| e.to_string())?;
                let editor = GroupEditor::open(&moved, a).map_err(|e| e.to_string())?;
                let jump = moved.pages().map_err(|e| e.to_string())?[a].line_jump.clone().unwrap_or_default();
                let reroutes = editor.reroute_connectors(&connects, &jump);
                (if reroutes.is_empty() { bytes } else { patch_page_cells(&moved, a, &reroutes).map_err(|e| e.to_string())? }, a)
            }
            // 36: apply a style: the text is `style id|scope|shape ids separated by commas` (scope all, line, fill or text); 37: create or change the custom style
            // whose JSON is in the text (`name`, `basedOn`, `lineColor`, `lineWeight`, `linePattern`, `fillColor`, `fillPattern`; step 293)
            36 => {
                let mut parts = name.splitn(3, '|');
                let (style, scope, ids) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let ids: Vec<String> = ids.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                if ids.is_empty() {
                    return Err("select a shape first".to_string());
                }
                (layoutcraft_vsdx::styleedit::apply_style(package, a, &ids, style.trim(), scope.trim()).map_err(|e| e.to_string())?, a)
            }
            37 => {
                let spec: serde_json::Value = serde_json::from_str(&name).map_err(|e| format!("the style is not valid JSON: {e}"))?;
                (layoutcraft_vsdx::styleedit::define_style(package, &spec).map_err(|e| e.to_string())?, a)
            }
            // 38: delete the custom style whose id is in the text (step 298)
            38 => (layoutcraft_vsdx::styleedit::delete_style(package, name.trim()).map_err(|e| e.to_string())?, a),
            // 35: Fit Curve: the open polyline with the shape id in the text becomes a smooth spline through its vertices (step 285)
            35 => (layoutcraft_vsdx::groupedit::vertices::fit_curve(package, a, name.trim()).map_err(|e| e.to_string())?, a),
            // 31: choose variant 0..3 (the number in the text) of the drawing's theme (Design > Variants; probe-variants.ps1)
            31 => (layoutcraft_vsdx::pageops::set_theme_variant(package, name.trim().parse::<u32>().map_err(|_| "the variant is a number from 0 to 3")?).map_err(|e| e.to_string())?, a),
            // 30: apply the gallery theme named in the text to the whole drawing (Design > Themes; step 257)
            30 => (layoutcraft_vsdx::pageops::apply_theme(package, name.trim()).map_err(|e| e.to_string())?, a),
            // 32: apply the theme to shown page `a` only (a theme per page); 33: variant 0..3 of page `a` only
            32 => (layoutcraft_vsdx::pageops::apply_theme_page(package, name.trim(), Some(a)).map_err(|e| e.to_string())?, a),
            33 => (layoutcraft_vsdx::pageops::set_theme_variant_page(package, name.trim().parse::<u32>().map_err(|_| "the variant is a number from 0 to 3")?, Some(a)).map_err(|e| e.to_string())?, a),
            // 12: show the background page with the ID in the text (empty = none) beneath page `a`; 13: make page `a` a background
            // page; 14: make the background page with the ID in the text an ordinary page again (the output is its page index)
            12 => (layoutcraft_vsdx::pageops::set_back_page(package, a, if name.trim().is_empty() { None } else { Some(name.trim()) }).map_err(|e| e.to_string())?, a),
            13 => (layoutcraft_vsdx::pageops::make_background(package, a).map_err(|e| e.to_string())?, a),
            14 => layoutcraft_vsdx::pageops::make_foreground(package, name.trim()).map_err(|e| e.to_string())?,
            // 11: the document properties from the JSON text {"title",...} (docprops::DocProps); `a` is ignored, the output is `a`
            11 => {
                let json: serde_json::Value = serde_json::from_str(&name).map_err(|e| format!("bad document properties: {e}"))?;
                let field = |key: &str| json.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let props = layoutcraft_vsdx::docprops::DocProps {
                    title: field("title"),
                    subject: field("subject"),
                    creator: field("creator"),
                    keywords: field("keywords"),
                    description: field("description"),
                    category: field("category"),
                    manager: field("manager"),
                    company: field("company"),
                };
                (layoutcraft_vsdx::docprops::write_doc_props(package, &props).map_err(|e| e.to_string())?, a)
            }
            // 6: the size of page `a` from the text "width,height" (inches)
            6 => {
                let (w, h) = name.split_once(',').ok_or("expected width,height")?;
                let (w, h) = (w.trim().parse::<f64>().map_err(|_| "the width is not a number")?, h.trim().parse::<f64>().map_err(|_| "the height is not a number")?);
                (layoutcraft_vsdx::pageops::set_page_size(package, a, w, h).map_err(|e| e.to_string())?, a)
            }
            _ => return Err(format!("unknown page operation {op}")),
        };
        let previous = self.bytes.clone();
        self.adopt(bytes)?;
        self.history.push(previous);
        Ok(show.to_string())
    }
}

impl Engine {
    /// Group, ungroup, z-order and shape operations on the shapes in `text` (ids separated by newlines), ONE undo step. `op`: 0 group (returns the new
    /// group's id), 1 ungroup (returns the JSON array of the freed ids), 2 to front, 3 to back, 4 forward, 5 backward, 6 union, 7 combine, 8 fragment,
    /// 9 intersect, 10 subtract, 11 join, 12 trim (JSON array of the new shapes' ids).
    pub fn group_op(&mut self, page: usize, op: i32, text: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let (bytes, out) = match op {
            0 => {
                let ids: Vec<String> = text.split('\n').filter(|one| !one.is_empty()).map(str::to_string).collect();
                layoutcraft_vsdx::groupedit::group_shapes(package, page, &ids).map_err(|e| e.to_string())?
            }
            1 => {
                let (bytes, ids) = layoutcraft_vsdx::groupedit::ungroup_shape(package, page, text.trim()).map_err(|e| e.to_string())?;
                (bytes, serde_json::json!(ids).to_string())
            }
            // 2 to the front, 3 to the back, 4 one forward, 5 one backward: the drawing order of the shapes (ids separated by newlines)
            2..=5 => {
                use layoutcraft_vsdx::groupedit::{reorder_shapes, ZOrder};
                let ids: Vec<String> = text.split(char::from(10u8)).filter(|one| !one.is_empty()).map(str::to_string).collect();
                let order = match op { 2 => ZOrder::ToFront, 3 => ZOrder::ToBack, 4 => ZOrder::Forward, _ => ZOrder::Backward };
                (reorder_shapes(package, page, &ids, order).map_err(|e| e.to_string())?, "[]".to_string())
            }
            // 6 Union, 7 Combine, 8 Fragment, 9 Intersect, 10 Subtract, 11 Join (step 286), 12 Trim (step 287) of the shapes (ids separated by newlines, the first one gives the look);
            // the output buffer holds the JSON array of the new shapes' ids
            6..=12 => {
                use layoutcraft_vsdx::groupedit::shapeops::{shape_operation, ShapeOp};
                let ids: Vec<String> = text.split(char::from(10u8)).filter(|one| !one.is_empty()).map(str::to_string).collect();
                let which = match op { 6 => ShapeOp::Union, 7 => ShapeOp::Combine, 8 => ShapeOp::Fragment, 9 => ShapeOp::Intersect, 11 => ShapeOp::Join, 12 => ShapeOp::Trim, _ => ShapeOp::Subtract };
                let (bytes, new_ids) = shape_operation(package, page, which, &ids).map_err(|e| e.to_string())?;
                (bytes, serde_json::json!(new_ids).to_string())
            }
            _ => return Err(format!("unknown group operation {op}")),
        };
        // Ungrouping a turned or mirrored group moves the ends of its connectors: the glued ones are routed again.
        let bytes = if op == 1 {
            let moved = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
            let part = moved.pages().map_err(|e| e.to_string())?[page].part.clone();
            let connects = moved.connects(&part).map_err(|e| e.to_string())?;
            let editor = GroupEditor::open(&moved, page).map_err(|e| e.to_string())?;
            let jump = moved.pages().map_err(|e| e.to_string())?[page].line_jump.clone().unwrap_or_default();
            let reroutes = editor.reroute_connectors(&connects, &jump);
            if reroutes.is_empty() { bytes } else { patch_page_cells(&moved, page, &reroutes).map_err(|e| e.to_string())? }
        } else {
            bytes
        };
        let previous = self.bytes.clone();
        self.adopt(bytes)?;
        self.history.push(previous);
        Ok(out)
    }
}

// --- clipboard and connectors ---------------------------------------------------------------------------------
impl Engine {
    /// Copy the shapes `ids` of `page` to the clipboard (only page-level shapes that are not connectors). Returns the number copied.
    pub fn copy(&mut self, page: usize, ids: &[String]) -> Result<usize, String> {
        if ids.is_empty() {
            return Err("nothing to copy".to_string());
        }
        let package = self.package_ref()?;
        let (copied, rows) = layoutcraft_vsdx::groupedit::copy_shapes(package, page, ids).map_err(|e| e.to_string())?;
        let count = copied.len();
        self.clipboard = copied;
        self.clipboard_connects = rows;
        Ok(count)
    }

    /// Paste the clipboard onto `page`, offset by (`dx`, `dy`) inches (y up), ONE undo step. Returns the new ids of the pasted top-level shapes.
    pub fn paste(&mut self, page: usize, dx: f64, dy: f64) -> Result<Vec<String>, String> {
        if self.clipboard.is_empty() {
            return Err("the clipboard is empty".to_string());
        }
        let package = self.package_ref()?;
        let (bytes, ids) = layoutcraft_vsdx::writer::paste_shapes_with_connects(package, page, &self.clipboard, &self.clipboard_connects, dx, dy).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(ids)
    }

    /// Number of shapes on the clipboard.
    pub fn clipboard_count(&self) -> usize {
        self.clipboard.len()
    }

    /// Add a glued right-angle connector from shape `from` to shape `to`; `from_site` / `to_site` are 1-based connection points or `None` for the whole shape. ONE undo step; returns the id.
    pub fn add_connector(&mut self, page: usize, from: &str, from_site: Option<usize>, to: &str, to_site: Option<usize>) -> Result<String, String> {
        let package = self.package_ref()?;
        let (bytes, id) = layoutcraft_vsdx::groupedit::add_connector(package, page, (from, from_site), (to, to_site)).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(id)
    }

    /// The page-level connectors of `page` as JSON (route, ends, what each end is glued to).
    pub fn connectors_json(&self, page: usize) -> Result<String, String> {
        let package = self.package_ref()?;
        let part = package.pages().map_err(|e| e.to_string())?.get(page).ok_or("no such page")?.part.clone();
        let connects = package.connects(&part).map_err(|e| e.to_string())?;
        let editor = GroupEditor::open(package, page).map_err(|e| e.to_string())?;
        Ok(editor.connectors(&connects).to_string())
    }

    /// Move one end of `connector` (`begin` = the begin end): glued to `target` (connection point `site`, or the whole shape) or, with `None`, loose at `free`. ONE undo step.
    pub fn move_connector_end(&mut self, page: usize, connector: &str, begin: bool, target: Option<(&str, Option<usize>)>, free: (f64, f64)) -> Result<(), String> {
        let package = self.package_ref()?;
        let target = match target {
            Some((shape, site)) => layoutcraft_vsdx::groupedit::EndTarget::Glue(shape.to_string(), site),
            None => {
                if !free.0.is_finite() || !free.1.is_finite() {
                    return Err("the position is not a number".to_string());
                }
                layoutcraft_vsdx::groupedit::EndTarget::Free(free.0, free.1)
            }
        };
        let bytes = layoutcraft_vsdx::groupedit::move_connector_end(package, page, connector, begin, target).map_err(|e| e.to_string())?;
        self.commit(bytes)
    }
}

// --- stencils and masters ---------------------------------------------------------------------------------------
use std::io::Cursor;

/// SVG text of master `master` of `source` (a stencil, or the drawing itself) drawn on its own: a scratch drawing
/// with one instance of it; a very thin or very flat picture (a line) is padded so that it stays visible.
pub fn thumb_svg(source: &Package, master: &str) -> Result<String, String> {
    let (bytes, _) = layoutcraft_vsdx::stencil::scratch_drawing_with_master(source, master).map_err(|e| e.to_string())?;
    let package = Package::from_reader(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let document = import_document(&package).map_err(|e| e.to_string())?;
    let mut media = BTreeMap::new();
    for (id, record) in &document.media {
        if let Some(bytes) = media_bytes(&package, record) {
            media.insert(id.clone(), bytes);
        }
    }
    let svg = scene_from_document_with_options(&document, 0, &media, SceneOptions { print: false, fit_text: true })
        .map(|scene| to_svg(&scene))
        .map_err(|e| e.to_string())?;
    Ok(pad_thumb(&svg))
}

/// Grow the `viewBox` (and `width` / `height`) of a thumbnail SVG to at least 24 x 24 around its centre.
pub fn pad_thumb(svg: &str) -> String {
    const MIN: f64 = 24.0;
    let Some(at) = svg.find("viewBox=\"") else { return svg.to_string() };
    let from = at + "viewBox=\"".len();
    let Some(len) = svg[from..].find('"') else { return svg.to_string() };
    let numbers: Vec<f64> = svg[from..from + len].split_whitespace().filter_map(|n| n.parse().ok()).collect();
    if numbers.len() != 4 || (numbers[2] >= MIN && numbers[3] >= MIN) {
        return svg.to_string();
    }
    let (x, y, w, h) = (numbers[0], numbers[1], numbers[2].max(MIN), numbers[3].max(MIN));
    let (x, y) = (x - (w - numbers[2]) / 2.0, y - (h - numbers[3]) / 2.0);
    let mut out = svg.to_string();
    out.replace_range(from..from + len, &format!("{x} {y} {w} {h}"));
    // the pixel size follows the view box (one pixel per unit, as the original did)
    let width = out.find(" width=\"").map(|n| n + " width=\"".len());
    if let Some(start) = width {
        if let Some(end) = out[start..].find('"') {
            out.replace_range(start..start + end, &format!("{}", w.round()));
        }
    }
    let height = out.find(" height=\"").map(|n| n + " height=\"".len());
    if let Some(start) = height {
        if let Some(end) = out[start..].find('"') {
            out.replace_range(start..start + end, &format!("{}", h.round()));
        }
    }
    out
}

impl Engine {
    /// The masters of the open document as JSON `[{id, name, w, h}]` (size in inches of the master's top shape; 0 when unknown), in document order of their ids.
    pub fn masters_json(&self) -> Result<String, String> {
        let package = self.package_ref()?;
        let catalog = layoutcraft_vsdx::MasterCatalog::load(package).map_err(|e| e.to_string())?;
        let mut list: Vec<(u64, serde_json::Value)> = catalog
            .masters
            .values()
            .map(|master| {
                let top = master.top.as_ref().and_then(|top| master.shapes.get(top));
                let size = |cell: &str| {
                    top.and_then(|shape| shape.cells.get(cell))
                        .and_then(|cell| cell.value.as_deref())
                        .and_then(|value| value.parse::<f64>().ok())
                        .unwrap_or(0.0)
                };
                (
                    master.id.parse::<u64>().unwrap_or(u64::MAX),
                    serde_json::json!({ "id": master.id, "name": master.name, "w": size("Width"), "h": size("Height") }),
                )
            })
            .collect();
        list.sort_by_key(|(id, _)| *id);
        Ok(serde_json::Value::Array(list.into_iter().map(|(_, v)| v).collect()).to_string())
    }

    /// Open an external stencil (`.vssx` / `.vstx` bytes, or `.vss` / `.vsx` and the other old formats) next to the drawing.
    /// Returns JSON `{index, masters: [..]}`, with `converted` (a message for the user) for an old format.
    pub fn stencil_open(&mut self, bytes: Vec<u8>) -> Result<String, String> {
        let (bytes, conversion) = as_package_bytes(bytes)?;
        let package = Package::from_reader(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let masters = layoutcraft_vsdx::stencil::master_list(&package).map_err(|e| e.to_string())?;
        if masters.as_array().is_none_or(|list| list.is_empty()) {
            return Err("this file has no masters (is it a stencil?)".to_string());
        }
        self.stencils.push(Some(package));
        let index = self.stencils.len() - 1;
        let mut answer = serde_json::json!({ "index": index, "masters": masters });
        if let Some(conversion) = conversion {
            answer["converted"] = conversion.message().into();
        }
        Ok(answer.to_string())
    }

    /// Open the built-in "Basic shapes" stencil the first time and answer with it every time (JSON like `stencil_open`).
    pub fn stencil_builtin(&mut self) -> Result<String, String> {
        if let Some(index) = self.builtin_stencil {
            if let Some(Some(package)) = self.stencils.get(index) {
                let masters = layoutcraft_vsdx::stencil::master_list(package).map_err(|e| e.to_string())?;
                return Ok(serde_json::json!({ "index": index, "masters": masters }).to_string());
            }
        }
        let bytes = layoutcraft_vsdx::builtin::builtin_stencil().map_err(|e| e.to_string())?;
        let package = Package::from_reader(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let masters = layoutcraft_vsdx::stencil::master_list(&package).map_err(|e| e.to_string())?;
        self.stencils.push(Some(package));
        let index = self.stencils.len() - 1;
        self.builtin_stencil = Some(index);
        Ok(serde_json::json!({ "index": index, "masters": masters }).to_string())
    }

    /// Forget stencil `index` (the others keep their numbers). `false` when there is no such slot.
    pub fn stencil_close(&mut self, index: usize) -> bool {
        match self.stencils.get_mut(index) {
            Some(slot) => {
                *slot = None;
                true
            }
            None => false,
        }
    }

    /// SVG of master `master` of stencil `index` on its own.
    pub fn stencil_thumb(&self, index: usize, master: &str) -> Result<String, String> {
        let stencil = self.stencils.get(index).and_then(|s| s.as_ref()).ok_or("no such stencil")?;
        thumb_svg(stencil, master)
    }

    /// SVG of master `master` of the open drawing on its own.
    pub fn master_thumb(&self, master: &str) -> Result<String, String> {
        thumb_svg(self.package_ref()?, master)
    }

    /// Drop an instance of master `master` of stencil `index` on `page`, pin at (`x`, `y`); the master is copied into the drawing first (once). ONE undo step; returns the new shape's id.
    pub fn stencil_drop(&mut self, page: usize, index: usize, master: &str, x: f64, y: f64) -> Result<String, String> {
        let package = self.package_ref()?;
        let stencil = self.stencils.get(index).and_then(|s| s.as_ref()).ok_or("no such stencil")?;
        let (with_master, new_master) = layoutcraft_vsdx::stencil::import_master(package, stencil, master).map_err(|e| e.to_string())?;
        let with_master = Package::from_reader(Cursor::new(with_master)).map_err(|e| e.to_string())?;
        let (bytes, shape) = layoutcraft_vsdx::writer::add_master_instance(&with_master, page, &new_master, x, y).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(shape)
    }

    /// Change Shape: replace a shape by an instance of a master; `text` is `shape id|master id`; `index` < 0 means the master is one of the drawing, else it is imported from stencil `index`. Glued connectors are routed again. ONE undo step; returns the new id.
    pub fn replace_shape(&mut self, page: usize, index: i32, text: &str) -> Result<String, String> {
        let package = self.package_ref()?;
        let (shape, master) = text.split_once('|').ok_or("expected shape id|master id")?;
        let (work, master) = if index >= 0 {
            let stencil = self.stencils.get(index as usize).and_then(|s| s.as_ref()).ok_or("no such stencil")?;
            let (with_master, new_master) = layoutcraft_vsdx::stencil::import_master(package, stencil, master.trim()).map_err(|e| e.to_string())?;
            (Package::from_reader(Cursor::new(with_master)).map_err(|e| e.to_string())?, new_master)
        } else {
            (Package::from_bytes(&self.bytes).map_err(|e| e.to_string())?, master.trim().to_string())
        };
        let (bytes, new_id) = layoutcraft_vsdx::replace::replace_shape(&work, page, shape.trim(), &master).map_err(|e| e.to_string())?;
        let moved = Package::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let part = moved.pages().map_err(|e| e.to_string())?[page].part.clone();
        let connects = moved.connects(&part).map_err(|e| e.to_string())?;
        let editor = GroupEditor::open(&moved, page).map_err(|e| e.to_string())?;
        let jump = moved.pages().map_err(|e| e.to_string())?[page].line_jump.clone().unwrap_or_default();
        let reroutes = editor.reroute_connectors(&connects, &jump);
        let bytes = if reroutes.is_empty() { bytes } else { patch_page_cells(&moved, page, &reroutes).map_err(|e| e.to_string())? };
        self.commit(bytes)?;
        Ok(new_id)
    }

    /// Drop an instance of master `master` of the drawing on `page`, pin at (`x`, `y`). ONE undo step; returns the new id.
    pub fn add_master(&mut self, page: usize, master: &str, x: f64, y: f64) -> Result<String, String> {
        let package = self.package_ref()?;
        let (bytes, new_id) = layoutcraft_vsdx::writer::add_master_instance(package, page, master, x, y).map_err(|e| e.to_string())?;
        self.commit(bytes)?;
        Ok(new_id)
    }
}

// --- read-only information about the open drawing (JSON text) ----------------------------------------------
fn page_index_of(pages: &[layoutcraft_vsdx::PageRef], part: &str) -> usize {
    pages.iter().position(|p| p.part == part).unwrap_or(0)
}

impl Engine {
    /// The document properties as JSON. Read-only.
    pub fn doc_props_json(&self) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let p = layoutcraft_vsdx::docprops::read_doc_props(package);
        Ok(serde_json::json!({
            "title": p.title, "subject": p.subject, "creator": p.creator, "keywords": p.keywords,
            "description": p.description, "category": p.category, "manager": p.manager, "company": p.company,
        }).to_string())
    }

    /// The ShapeSheet of shape `id` as JSON. Read-only.
    pub fn shape_sheet_json(&self, page: usize, id: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let editor = layoutcraft_vsdx::groupedit::GroupEditor::open(package, page).map_err(|e| e.to_string())?;
        editor.shape_sheet(id).map(|v| v.to_string()).ok_or_else(|| format!("no shape {id}"))
    }

    /// The theme gallery with the drawing's current theme, variant and per-page themes as JSON. Read-only.
    pub fn themes_json(&self) -> Result<String, String> {
        let current = self.package.as_ref().and_then(|package| {
            let part = package.names().find(|n| { let l = n.to_ascii_lowercase(); l.starts_with("visio/theme/theme") && l.ends_with(".xml") })?.to_string();
            let text = package.text(&part).ok()?;
            let at = text.find("<a:theme ")?;
            let tag = &text[at..at + text[at..].find('>')?];
            let key = tag.find("name=")? + "name=".len();
            let quote = tag[key..].chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let n = key + 1;
            let raw = tag[n..n + tag[n..].find(quote)?].to_string();
            Some(layoutcraft_vsdx::themes::own_name(&raw).map(str::to_string).unwrap_or(raw))
        });
        let variant = self.package.as_ref().map(layoutcraft_vsdx::pageops::theme_variant).unwrap_or(0);
        // per shown page: the gallery index of the page's theme (0 = none; null = not stated, the page follows the drawing's theme) and its variant
        let per_page: Vec<serde_json::Value> = self.package.as_ref().map(layoutcraft_vsdx::pageops::page_themes).unwrap_or_default().into_iter().map(|(theme, variant)| serde_json::json!({ "theme": theme, "variant": variant })).collect();
        Ok(serde_json::json!({ "themes": layoutcraft_vsdx::themes::gallery(), "current": current, "variant": variant, "pages": per_page }).to_string())
    }

    /// The comments of the drawing as JSON. Read-only.
    pub fn comments_json(&self) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let list = layoutcraft_vsdx::comments::list_comments(package).map_err(|e| e.to_string())?;
        Ok(
            serde_json::Value::Array(
                list.into_iter()
                    .map(|c| serde_json::json!({"id": c.id, "author": c.author, "initials": c.initials, "page": c.page_index.map(|i| i as i64).unwrap_or(-1), "shape": c.shape_id, "date": c.date, "editDate": c.edit_date, "text": c.text}))
                    .collect(),
            )
            .to_string(),
        )
    }

    /// The hyperlinks of shape `id` as JSON. Read-only.
    pub fn hyperlinks_json(&self, page: usize, id: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let links = layoutcraft_vsdx::hyperlinks::read_hyperlinks(package, page, id).map_err(|e| e.to_string())?;
        Ok(
            serde_json::Value::Array(
                links
                    .into_iter()
                    .map(|l| serde_json::json!({"name": l.name, "description": l.description, "address": l.address, "subAddress": l.sub_address, "extraInfo": l.extra_info, "frame": l.frame, "newWindow": l.new_window, "default": l.default, "invisible": l.invisible}))
                    .collect(),
            )
            .to_string(),
        )
    }

    /// The ruler guides of `page` as JSON. Read-only.
    pub fn guides_json(&self, page: usize) -> Result<String, String> {
        let document = self.document.as_ref().ok_or("no document")?;
        let page = document.pages.get(page).ok_or("no such page")?;
        let list: Vec<serde_json::Value> = page
            .guides
            .iter()
            .map(|g| serde_json::json!({"id": g.visio_id, "x": g.x_in, "y": g.y_in, "vertical": (g.angle - std::f64::consts::FRAC_PI_2).abs() < 0.1 || (g.angle + std::f64::consts::FRAC_PI_2).abs() < 0.1}))
            .collect();
        Ok(serde_json::Value::Array(list).to_string())
    }

    /// The background pages and each page's background as JSON. Read-only.
    pub fn background_info_json(&self) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let info = layoutcraft_vsdx::pageops::background_info(package).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "backgrounds": info.backgrounds.iter().map(|(id, name)| serde_json::json!({"id": id, "name": name})).collect::<Vec<_>>(),
            "assigned": info.assigned,
        }).to_string())
    }

    /// The names of the shapes of `page` as JSON `{id: name}`. Read-only.
    pub fn shape_names_json(&self, page: usize) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let list = layoutcraft_vsdx::shapename::names(package, page).map_err(|e| e.to_string())?;
        let map: serde_json::Map<String, serde_json::Value> = list.into_iter().map(|(id, name)| (id, serde_json::Value::String(name))).collect();
        Ok(serde_json::Value::Object(map).to_string())
    }

    /// The page names as a JSON array. Read-only.
    pub fn page_names_json(&self) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let names: Vec<String> = package.pages().map_err(|e| e.to_string())?.into_iter().map(|p| p.name).collect();
        Ok(serde_json::json!(names).to_string())
    }

    /// The frames of every shape on `page` as JSON. Read-only.
    pub fn group_info_json(&self, page: usize) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let editor = GroupEditor::open(package, page).map_err(|e| e.to_string())?;
        Ok(editor.describe().to_string())
    }

    /// The format values of shape `id` as JSON. Read-only.
    pub fn style_info_json(&self, page: usize, id: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let editor = GroupEditor::open(package, page).map_err(|e| e.to_string())?;
        editor.style_info(id).map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    /// The style sheets of the drawing as JSON. Read-only.
    pub fn styles_list_json(&self) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        layoutcraft_vsdx::styleedit::list_styles(package).map_err(|e| e.to_string())
    }

    /// The connection points of shape `id` as JSON. Read-only.
    pub fn connection_points_json(&self, page: usize, id: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let editor = GroupEditor::open(package, page).map_err(|e| e.to_string())?;
        let points: Vec<serde_json::Value> = editor
            .connection_points(id)
            .into_iter()
            .map(|(n, x, y)| serde_json::json!({ "n": n, "x": x, "y": y }))
            .collect();
        Ok(serde_json::Value::Array(points).to_string())
    }

    /// Which shape owns the text shown for `id` and what it says, as JSON. Read-only.
    pub fn text_info_json(&self, page: usize, id: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let editor = GroupEditor::open(package, page).map_err(|e| e.to_string())?;
        let (owner, text) = editor.text_owner(package, page, id).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "owner": owner, "text": text }).to_string())
    }

    /// The Shape Data rows of shape `id` with their sort keys as JSON. Read-only.
    pub fn data_sort_json(&self, page: usize, id: &str) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let keys = layoutcraft_vsdx::writer::data_sort_keys(package, page, id).map_err(|e| e.to_string())?;
        Ok(serde_json::Value::Array(keys.into_iter().map(|(name, key)| serde_json::json!({ "name": name, "sortKey": key })).collect()).to_string())
    }

    /// The formatting of characters `start..end` of the text of shape `id` as JSON. Read-only.
    pub fn text_state_json(&self, page: usize, id: &str, start: usize, end: usize) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        layoutcraft_vsdx::writer::text_state(package, page, id, start, end).map_err(|e| e.to_string())
    }

    /// Width, height, scale and print setup of `page` as JSON. Read-only.
    pub fn page_size_json(&self, page: usize) -> Result<String, String> {
        let package = self.package.as_ref().ok_or("no document")?;
        let pages = package.pages().map_err(|e| e.to_string())?;
        let page = pages.get(page).ok_or("no such page")?;
        // The page is shown in millimetres when its paper unit (the unit of the PageScale cell) is a metric one.
        let scale = layoutcraft_vsdx::pageops::page_scale(package, page_index_of(&pages, &page.part)).map_err(|e| e.to_string())?;
        let metric = layoutcraft_vsdx::pageops::is_metric_unit(&scale.paper_unit);
        let print = layoutcraft_vsdx::pageops::print_setup(package, page_index_of(&pages, &page.part)).ok();
        let paper = layoutcraft_vsdx::pageops::paper_size(package, page_index_of(&pages, &page.part)).ok().flatten();
        let unit_name = |code: &str| match code.to_ascii_uppercase().as_str() { "IN_F" => "in".to_string(), "F_I" => "ft".to_string(), other => other.to_ascii_lowercase() };
        // the space between a shape and the one AutoConnect places next to it: the page's AvenueSize cells, else 7.5 mm = 0.2953 in (measured on Visio: the same default on a Letter and on an A4 page written without the cell)
        let own = layoutcraft_vsdx::pageops::avenue_size(package, page_index_of(&pages, &page.part)).ok().flatten();
        let default_avenue = 0.2952755905511811;
        let avenue = own.map(|a| [a.0, a.1]).unwrap_or([default_avenue, default_avenue]);
        let avenue_own = own.is_some();
        Ok(serde_json::json!({
            "w": page.width.unwrap_or(8.5), "h": page.height.unwrap_or(11.0), "metric": metric,
            "avenue": avenue, "avenueOwn": avenue_own,
            "scale": {"paper": scale.paper, "paperUnit": unit_name(&scale.paper_unit), "drawing": scale.drawing, "drawingUnit": unit_name(&scale.drawing_unit), "type": scale.scale_type},
            "print": print.as_ref().map(|p| serde_json::json!({"orientation": p.orientation, "margins": p.margins, "grid": p.grid, "scale": [p.scale.0, p.scale.1], "fit": p.fit, "pages": [p.pages.0, p.pages.1], "center": [p.center.0, p.center.1], "paper": paper.map(|s| [s.0, s.1])})),
        }).to_string())
    }
}

impl Engine {
    /// Whether `redo` would do something (an undone state that still leads from the current document).
    pub fn can_redo(&self) -> bool {
        self.redo.last().is_some_and(|(led_to, _)| *led_to == fingerprint(&self.bytes))
    }
}

impl Engine {
    /// Process > Check Diagram on every page (see `check`): the issues with their page index; `alt_text` also lists shapes without text and alt text.
    pub fn check_diagram(&self, alt_text: bool) -> Result<Vec<check::Issue>, String> {
        let model: serde_json::Value = serde_json::from_str(&self.json().ok_or("no document")?).map_err(|e| e.to_string())?;
        let pages = model["pages"].as_array().map_or(0, Vec::len);
        let mut issues = Vec::new();
        for page in 0..pages {
            let size = self.page_size_json(page).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|s| Some((s["w"].as_f64()?, s["h"].as_f64()?)));
            issues.extend(check::check_page(&model, page, size, alt_text).into_iter().map(|mut issue| {
                issue.page = Some(page);
                issue
            }));
        }
        Ok(issues)
    }
}

impl Engine {
    /// Review > Compare Documents: the open drawing (new) against another drawing (old), see `compare`.
    pub fn compare_with(&self, other: &Engine) -> Result<Vec<compare::Diff>, String> {
        let current: serde_json::Value = serde_json::from_str(&self.json().ok_or("no document")?).map_err(|e| e.to_string())?;
        let old: serde_json::Value = serde_json::from_str(&other.json().ok_or("no document in the other file")?).map_err(|e| e.to_string())?;
        Ok(compare::compare_models(&current, &old))
    }
}

impl Engine {
    /// The page rendered to 8 bit RGB rows on white paper at `scale` times 96 dpi (print view), for the picture pages of the PDF export.
    pub(crate) fn render_page_rgb(&self, page: usize, scale: f64) -> Result<(u32, u32, Vec<u8>), String> {
        let document = self.document.as_ref().ok_or("no document")?;
        let mut media = BTreeMap::new();
        if let Some(package) = self.package.as_ref() {
            for (id, record) in &document.media {
                if let Some(bytes) = media_bytes(package, record) {
                    media.insert(id.clone(), bytes);
                }
            }
        }
        let scene = scene_from_document_with_options(document, page, &media, SceneOptions { print: true, fit_text: true }).map_err(|e| e.to_string())?;
        layoutcraft_render::render_rgb(&scene, scale, [255, 255, 255])
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::Engine;

    /// An old-format drawing opens as a `.vsdx` drawing that says it was converted; opening anything else clears that.
    #[test]
    fn an_old_format_drawing_opens_as_vsdx_and_says_so() {
        let old = br#"<VisioDocument xmlns="http://schemas.microsoft.com/visio/2003/core"><Pages><Page ID="0"><PageSheet><PageProps><PageWidth>8.5</PageWidth><PageHeight>11</PageHeight></PageProps></PageSheet><Shapes/></Page></Pages></VisioDocument>"#;
        let mut engine = Engine::new();
        engine.open(old.to_vec()).unwrap();
        assert!(engine.conversion.as_ref().is_some_and(|c| c.message().contains(".vsdx")));
        let saved = engine.export().unwrap().to_vec();
        assert!(saved.starts_with(b"PK"));
        engine.open(saved).unwrap();
        assert!(engine.conversion.is_none());
    }
}
