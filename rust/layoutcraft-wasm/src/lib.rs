//! Spike: the Rust core behind a tiny C ABI so any host (browser, Node, native)
//! can call it without wasm-bindgen. The host allocates input with `vl_alloc`,
//! calls `vl_import`, then pulls results through `vl_out_ptr` / `vl_out_len`.
use std::cell::RefCell;

use layoutcraft_engine::Engine;

/// The editing state of the web host: the shared engine (`layoutcraft-engine`) plus the output buffer the C ABI hands out.
/// The engine's fields and methods are reachable through `Deref`.
#[derive(Default)]
struct State {
    engine: Engine,
    out: Vec<u8>,
}

impl std::ops::Deref for State {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

impl std::ops::DerefMut for State {
    fn deref_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn set_out(text: String) {
    STATE.with(|state| state.borrow_mut().out = text.into_bytes());
}

#[no_mangle]
pub extern "C" fn vl_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// # Safety
/// `ptr`/`len` must come from `vl_alloc` with the same `len`.
#[no_mangle]
pub unsafe extern "C" fn vl_dealloc(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}

/// Import a VSDX from `len` bytes at `ptr`. Returns 0 on success; on failure
/// returns 1 and the error text is in the output buffer.
///
/// # Safety
/// `ptr` must point to `len` initialised bytes.
#[no_mangle]
pub unsafe extern "C" fn vl_import(ptr: *const u8, len: usize) -> i32 {
    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
    match STATE.with(|state| state.borrow_mut().engine.open(bytes)) {
        Ok(()) => 0,
        Err(message) => {
            set_out(message);
            1
        }
    }
}

/// The message for the user when the open drawing was converted from an old format (`.vsd`, `.vdx`, ...) or holds shapes
/// that could not be read, in the output buffer; empty for any other drawing. Returns 0.
#[no_mangle]
pub extern "C" fn vl_conversion() -> i32 {
    let message = STATE.with(|state| state.borrow().engine.open_notice().unwrap_or_default());
    set_out(message);
    0
}

/// Start a NEW, empty drawing: one page of `width` x `height` inches (shown in millimetres when `metric` is
/// non-zero) named by the UTF-8 name at `name_ptr`. Replaces the open drawing (undo history and clipboard are
/// cleared, opened stencils stay). Returns 0, or 1 with the error in the output buffer.
///
/// # Safety
/// `name_ptr` must point to `name_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_new(width: f64, height: f64, metric: i32, name_ptr: *const u8, name_len: usize) -> i32 {
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return fail("the page name is not UTF-8");
    };
    match STATE.with(|state| state.borrow_mut().engine.new_blank(width, height, metric != 0, name)) {
        Ok(()) => 0,
        Err(message) => fail(message),
    }
}

/// Draw a new shape from scratch on `page`, ONE undo step: `kind` 0 rectangle, 1 rounded rectangle, 2 ellipse,
/// 3 diamond, 4 triangle, 5 text box, 6 line, in the box from (`x0`, `y0`) to (`x1`, `y1`) in page inches (y up;
/// a line runs from the first point to the second). The new shape's ID is in the output buffer.
#[no_mangle]
pub extern "C" fn vl_draw(page: i32, kind: i32, x0: f64, y0: f64, x1: f64, y1: f64) -> i32 {
    let result = STATE.with(|state| state.borrow_mut().engine.draw(page.max(0) as usize, kind, x0, y0, x1, y1));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// Insert a picture (PNG, JPEG, GIF or BMP bytes) on `page` in the box from (`x0`, `y0`) to (`x1`, `y1`) in page inches (y up), ONE undo step. The new shape's ID is in the output buffer.
///
/// # Safety
/// `ptr` must point to `len` bytes (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_insert_picture(page: i32, ptr: *const u8, len: usize, x0: f64, y0: f64, x1: f64, y1: f64) -> i32 {
    let data = std::slice::from_raw_parts(ptr, len);
    let result = STATE.with(|state| state.borrow_mut().engine.insert_picture(page.max(0) as usize, data, x0, y0, x1, y1));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// Draw a smooth curve (Visio's spline) through the page points "x,y;x,y;..." (4 to 200, inches, y up) on `page`, ONE undo step. The new shape's ID is in the output buffer.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_draw_curve(page: i32, ptr: *const u8, len: usize) -> i32 {
    let Ok(text) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return fail("the points are not UTF-8");
    };
    let result = layoutcraft_engine::parse_points(text).and_then(|points| STATE.with(|state| state.borrow_mut().engine.draw_curve(page.max(0) as usize, &points)));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// Draw a circular arc on `page` from (`x0`, `y0`) to (`x1`, `y1`) (inches, y up) bowed by `bow` inches (positive = to the right of the direction), ONE undo step.
/// The new shape's ID is in the output buffer.
#[no_mangle]
pub extern "C" fn vl_draw_arc(page: i32, x0: f64, y0: f64, x1: f64, y1: f64, bow: f64) -> i32 {
    let result = STATE.with(|state| state.borrow_mut().engine.draw_arc(page.max(0) as usize, (x0, y0), (x1, y1), bow));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// Draw a freeform shape on `page`, ONE undo step: the text (`len` UTF-8 bytes at `ptr`, from `vl_alloc`) is the page points "x,y;x,y;..." (inches, y up);
/// `closed` 1 makes a polygon, 0 an open polyline. The new shape's ID is in the output buffer.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_draw_polyline(page: i32, closed: i32, ptr: *const u8, len: usize) -> i32 {
    let Ok(text) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return fail("the points are not UTF-8");
    };
    let result = layoutcraft_engine::parse_points(text).and_then(|points| STATE.with(|state| state.borrow_mut().engine.draw_polyline(page.max(0) as usize, &points, closed != 0)));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// The document properties of the open drawing as JSON {"title","subject","creator","keywords","description","category","manager","company"}.
#[no_mangle]
pub extern "C" fn vl_doc_props() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.doc_props_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The ShapeSheet of shape `id` of `page` as JSON (see `GroupEditor::shape_sheet`). Read-only.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_shape_sheet(page: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.shape_sheet_json(page.max(0) as usize, id));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The theme gallery as JSON `{"themes": [{name, index, accents, fonts}], "current": <name of the drawing's theme or null>}` (Design > Themes).
#[no_mangle]
pub extern "C" fn vl_themes() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.themes_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The comments of the drawing as JSON `[{"id","author","initials","page","shape","date","editDate","text"}]` (`page` is the page index or -1, `shape` the shape ID or null for a comment on the page).
#[no_mangle]
pub extern "C" fn vl_comments() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.comments_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The hyperlinks of shape `id` of `page` as JSON `[{"name","description","address","subAddress","extraInfo","frame","newWindow","default","invisible"}]`.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_hyperlinks(page: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.hyperlinks_json(page.max(0) as usize, id));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The ruler guides of `page` as JSON `[{"id","x","y","vertical"}]` (inches, y up; a guide turned a quarter is vertical).
#[no_mangle]
pub extern "C" fn vl_guides(page: i32) -> i32 {
    let result = STATE.with(|state| state.borrow().engine.guides_json(page.max(0) as usize));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The background pages and the background of every shown page as JSON {"backgrounds":[{"id","name"}],"assigned":[id|null,...]}.
#[no_mangle]
pub extern "C" fn vl_background_info() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.background_info_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Preview of a drag: the routes the connectors glued to the shapes `ids` (separated by newlines in the text at `ptr`) would have if those shapes
/// were moved by (`dx`, `dy`) page inches (y up), as JSON `[{"id","route":[[x,y]...]}]`. The drawing is NOT changed. The page's editor is kept
/// between calls while the drawing is unchanged, so a drag costs one parse, not one per mouse move.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_preview_move(page: i32, ptr: *const u8, len: usize, dx: f64, dy: f64) -> i32 {
    let Ok(text) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return fail("the shape ids are not UTF-8");
    };
    let ids: Vec<String> = text.split('\n').filter(|one| !one.is_empty()).map(str::to_string).collect();
    let result = STATE.with(|state| state.borrow_mut().engine.preview_move(page.max(0) as usize, &ids, (dx, dy)));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The names of the shapes of `page` as JSON `{"<shape id>": "<name>", ...}` (only shapes that have a name of their own; see `shapename::names`).
#[no_mangle]
pub extern "C" fn vl_shape_names(page: i32) -> i32 {
    let result = STATE.with(|state| state.borrow().engine.shape_names_json(page.max(0) as usize));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The page names of the open drawing as JSON `["Page-1", ...]`, in page order.
#[no_mangle]
pub extern "C" fn vl_page_names() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.page_names_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// A page operation on the open drawing, ONE undo step: `op` 1 add an empty page after page `a` (the name at
/// `name_ptr`, empty = "Page-N"), 2 delete page `a`, 3 rename page `a` to the name, 4 move page `a` to
/// position `b`, 5 duplicate page `a`. The output buffer holds the index of the page to show afterwards (add and
/// duplicate: the new page; delete: the page that took its place; else `a`/`b`), or the error message.
///
/// # Safety
/// `name_ptr` must point to `name_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_page_op(op: i32, a: i32, b: i32, name_ptr: *const u8, name_len: usize) -> i32 {
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return fail("the page name is not UTF-8");
    };
    let name = name.to_string();
    let (a, b) = (a.max(0) as usize, b.max(0) as usize);
    let result = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.engine.page_op(op, a, b, &name)
    });
    match result {
        Ok(show) => {
            set_out(show);
            0
        }
        Err(message) => fail(message),
    }
}

#[no_mangle]
pub extern "C" fn vl_page_count() -> i32 {
    STATE.with(|state| state.borrow().engine.page_count().map_or(-1, |count| count as i32))
}

/// Serialise the imported document model to JSON in the output buffer.
#[no_mangle]
pub extern "C" fn vl_json() -> i32 {
    STATE.with(|state| {
        let json = state.borrow().engine.json();
        match json {
            Some(text) => {
                state.borrow_mut().out = text.into_bytes();
                0
            }
            None => 1,
        }
    })
}

/// Render one page to SVG in the output buffer. Pictures are embedded; EMF
/// pictures are converted to SVG first.
#[no_mangle]
pub extern "C" fn vl_svg(page: i32) -> i32 {
    render_svg(page, false)
}

/// Same as `vl_svg`, but leaves out shapes on non-printing layers (print preview /
/// export, matching Visio's `Page.Export`).
#[no_mangle]
pub extern "C" fn vl_svg_print(page: i32) -> i32 {
    render_svg(page, true)
}

fn render_svg(page: i32, print: bool) -> i32 {
    STATE.with(|state| {
        let svg = state.borrow().engine.svg(page.max(0) as usize, print);
        match svg {
            Some(Ok(text)) => {
                state.borrow_mut().out = text.into_bytes();
                0
            }
            Some(Err(message)) => {
                state.borrow_mut().out = message.into_bytes();
                1
            }
            None => 2,
        }
    })
}

#[no_mangle]
pub extern "C" fn vl_out_ptr() -> *const u8 {
    STATE.with(|state| state.borrow().out.as_ptr())
}

#[no_mangle]
pub extern "C" fn vl_out_len() -> usize {
    STATE.with(|state| state.borrow().out.len())
}

// --- editing -------------------------------------------------------------------------------

fn fail(message: impl ToString) -> i32 {
    set_out(message.to_string());
    1
}

/// The frames of every shape on `page` (see `GroupEditor::describe`) as JSON in the output buffer:
/// `[{id, parentId, isGroup, editable, pin, size, loc, angle}]`, inches, y up, in the parent's
/// coordinates.
#[no_mangle]
pub extern "C" fn vl_group_info(page: i32) -> i32 {
    let result = STATE.with(|state| state.borrow().engine.group_info_json(page.max(0) as usize));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Apply one group edit to `page` and re-import. `op`: 1 resize group (`a`,`b` = new width and
/// height in the group's own axes, `c`,`d` = anchor fractions), 2 move member (`a`,`b` = page delta
/// in inches, y up; `flag` = grow the ancestor frames), 3 resize member (as 1, with `flag`).
/// Returns 0 on success; otherwise 1 and the message is in the output buffer.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_edit(
    page: i32,
    op: i32,
    id_ptr: *const u8,
    id_len: usize,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    flag: i32,
) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let id = id.to_string();
    let page = page.max(0) as usize;
    let result = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.engine.edit(page, op, &id, a, b, c, d, flag)
    });
    match result {
        Ok(()) => 0,
        Err(message) => fail(message),
    }
}

/// Undo the last `vl_edit`. Returns 0 on success, 1 when there is nothing to undo.
#[no_mangle]
pub extern "C" fn vl_undo() -> i32 {
    let result = STATE.with(|state| state.borrow_mut().engine.undo());
    match result {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(message) => fail(message),
    }
}

/// Redo the last undone edit. Returns 0 on success, 1 when there is nothing to redo (a new edit after the undo
/// makes the undone states stale and drops them).
#[no_mangle]
pub extern "C" fn vl_redo() -> i32 {
    let result = STATE.with(|state| state.borrow_mut().engine.redo());
    match result {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(message) => fail(message),
    }
}

/// Number of edits that can be redone.
#[no_mangle]
pub extern "C" fn vl_redo_depth() -> i32 {
    STATE.with(|state| state.borrow_mut().engine.redo_depth() as i32)
}

/// Copy the shapes `ids` (newline-separated) of `page` to the clipboard. Only shapes that sit
/// directly on the page and are not connectors can be copied. Returns 0 and leaves the number of
/// copied shapes in the output buffer.
///
/// # Safety
/// `ids_ptr` must point to `ids_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_copy(page: i32, ids_ptr: *const u8, ids_len: usize) -> i32 {
    let Ok(ids) = std::str::from_utf8(std::slice::from_raw_parts(ids_ptr, ids_len)) else {
        return fail("shape ids are not UTF-8");
    };
    let ids: Vec<String> = ids.split('\n').filter(|id| !id.is_empty()).map(str::to_string).collect();
    let result = STATE.with(|state| state.borrow_mut().engine.copy(page.max(0) as usize, &ids)).map(|count| count.to_string());
    match result {
        Ok(count) => {
            set_out(count);
            0
        }
        Err(message) => fail(message),
    }
}

/// Paste the clipboard onto `page`, offset by (`dx`, `dy`) inches (y up); one undo step. The new
/// IDs of the pasted top-level shapes are in the output buffer as JSON `["..", ..]`.
#[no_mangle]
pub extern "C" fn vl_paste(page: i32, dx: f64, dy: f64) -> i32 {
    let result = STATE.with(|state| state.borrow_mut().engine.paste(page.max(0) as usize, dx, dy)).map(|ids| serde_json::to_string(&ids).unwrap_or_default());
    match result {
        Ok(ids) => {
            set_out(ids);
            0
        }
        Err(message) => fail(message),
    }
}

/// Number of shapes on the clipboard.
#[no_mangle]
pub extern "C" fn vl_clipboard_count() -> i32 {
    STATE.with(|state| state.borrow().engine.clipboard_count() as i32)
}

/// The format values of shape `id` that the cells state: JSON `{linePattern, lineWeight, fillPattern,
/// fontPt, hasText}` (see `GroupEditor::style_info`).
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_style_info(page: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.style_info_json(page.max(0) as usize, id));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The style sheets of the drawing as JSON `[{id, name, line, fill, text, custom, lineColor, lineWeight, fillColor, fillPattern}]` (step 293).
#[no_mangle]
pub extern "C" fn vl_styles_list() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.styles_list_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Connection points of shape `id` on `page` as JSON `[{n, x, y}]` (`n` as in `Connections.X<n>`,
/// page inches, y up); empty when the shape defines none.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_connection_points(page: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.connection_points_json(page.max(0) as usize, id));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Add a glued right-angle connector on `page`. `ids` is "<from id>\n<to id>"; `from_n` / `to_n`
/// are connection point numbers (1-based) or 0 for the whole shape. One undo step; the new
/// shape's ID is in the output buffer.
///
/// # Safety
/// `ids_ptr` must point to `ids_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_add_connector(page: i32, ids_ptr: *const u8, ids_len: usize, from_n: i32, to_n: i32) -> i32 {
    let Ok(ids) = std::str::from_utf8(std::slice::from_raw_parts(ids_ptr, ids_len)) else {
        return fail("shape ids are not UTF-8");
    };
    let Some((from, to)) = ids.split_once('\n') else {
        return fail("expected two shape ids");
    };
    let (from, to) = (from.to_string(), to.to_string());
    let site = |n: i32| (n > 0).then_some(n as usize);
    let result = STATE.with(|state| state.borrow_mut().engine.add_connector(page.max(0) as usize, &from, site(from_n), &to, site(to_n)));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// The page-level connectors of `page` as JSON (see `GroupEditor::connectors`): route, ends and
/// what each end is glued to.
#[no_mangle]
pub extern "C" fn vl_connectors(page: i32) -> i32 {
    let result = STATE.with(|state| state.borrow().engine.connectors_json(page.max(0) as usize));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Move one end of connector `ids` ("<connector id>\n<target shape id>") on `page`: `end` 0 is the
/// begin end, 1 the end. With a target shape the end is glued to its connection point `n` (1-based)
/// or, with `n` = 0, to the whole shape; with an empty target it becomes loose at (`x`, `y`) page
/// inches. One undo step.
///
/// # Safety
/// `ids_ptr` must point to `ids_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_move_connector_end(page: i32, ids_ptr: *const u8, ids_len: usize, end: i32, n: i32, x: f64, y: f64) -> i32 {
    let Ok(ids) = std::str::from_utf8(std::slice::from_raw_parts(ids_ptr, ids_len)) else {
        return fail("shape ids are not UTF-8");
    };
    let (connector, target) = ids.split_once('\n').unwrap_or((ids, ""));
    let (connector, target) = (connector.to_string(), target.to_string());
    let result = STATE.with(|state| {
        let target = (!target.is_empty()).then_some((target.as_str(), (n > 0).then_some(n as usize)));
        state.borrow_mut().engine.move_connector_end(page.max(0) as usize, &connector, end == 0, target, (x, y))
    });
    match result {
        Ok(()) => 0,
        Err(message) => fail(message),
    }
}

/// The masters of the open document as JSON `[{id, name, w, h}]` (size in inches of the master's
/// top shape; 0 when unknown), in document order of their IDs.
#[no_mangle]
pub extern "C" fn vl_masters() -> i32 {
    let result = STATE.with(|state| state.borrow().engine.masters_json());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Open an external stencil (`.vssx` / `.vstx` bytes) next to the drawing. On success the output
/// buffer holds JSON `{index, masters: [{id, name, w, h}]}`; the stencil stays in memory (see
/// `vl_stencil_close`) and its masters can be drawn (`vl_stencil_thumb`) and dropped (`vl_stencil_drop`).
///
/// # Safety
/// `ptr` must point to `len` initialised bytes.
#[no_mangle]
pub unsafe extern "C" fn vl_stencil_open(ptr: *const u8, len: usize) -> i32 {
    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
    match STATE.with(|state| state.borrow_mut().engine.stencil_open(bytes)) {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Open the built-in "Basic shapes" stencil (shapes of the draw tools and flowchart blocks; no file needed) the
/// first time, and answer with it every time: JSON `{index, masters: [{id, name, w, h}]}` like `vl_stencil_open`.
#[no_mangle]
pub extern "C" fn vl_stencil_builtin() -> i32 {
    let result = STATE.with(|state| state.borrow_mut().engine.stencil_builtin());
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Forget stencil `index` (frees its memory); the other stencils keep their numbers.
#[no_mangle]
pub extern "C" fn vl_stencil_close(index: i32) -> i32 {
    STATE.with(|state| if state.borrow_mut().engine.stencil_close(index.max(0) as usize) { 0 } else { 1 })
}

/// SVG of master `id` of stencil `index` on its own (the thumbnail), in the output buffer.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_stencil_thumb(index: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(master) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("master id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.stencil_thumb(index.max(0) as usize, master));
    match result {
        Ok(svg) => {
            set_out(svg);
            0
        }
        Err(message) => fail(message),
    }
}

/// Draw master `id` of the open DRAWING on its own (a thumbnail for the Shapes panel's "This drawing" list).
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_master_thumb(id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(master) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("master id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.master_thumb(master));
    match result {
        Ok(svg) => {
            set_out(svg);
            0
        }
        Err(message) => fail(message),
    }
}

/// Drop an instance of master `id` of stencil `index` on `page` with its pin at (`x`, `y`) inches
/// (y up). The master is copied into the drawing's own masters first (once; the same master is not
/// copied twice). One undo step; the new shape's ID is in the output buffer.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_stencil_drop(page: i32, index: i32, id_ptr: *const u8, id_len: usize, x: f64, y: f64) -> i32 {
    let Ok(master) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("master id is not UTF-8");
    };
    let master = master.to_string();
    let result = STATE.with(|state| state.borrow_mut().engine.stencil_drop(page.max(0) as usize, index.max(0) as usize, &master, x, y));
    match result {
        Ok(shape) => {
            set_out(shape);
            0
        }
        Err(message) => fail(message),
    }
}

/// Replace (Change Shape) shape `shape` of `page` by an instance of master `master` and route its glued connectors again; `index` < 0: the master is
/// one of the drawing, otherwise it is imported from stencil `index` first. The text is `shape id|master id`; one undo step; the output buffer holds
/// the new shape's ID.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_replace_shape(page: i32, index: i32, ptr: *const u8, len: usize) -> i32 {
    let Ok(text) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return fail("text is not UTF-8");
    };
    let text = text.to_string();
    let result = STATE.with(|state| state.borrow_mut().engine.replace_shape(page.max(0) as usize, index, &text));
    match result {
        Ok(id) => {
            set_out(id);
            0
        }
        Err(message) => fail(message),
    }
}

/// Drop an instance of master `id` on `page` with its pin at (`x`, `y`) inches (y up); one undo
/// step. The ID of the new shape is in the output buffer.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_add_master(page: i32, id_ptr: *const u8, id_len: usize, x: f64, y: f64) -> i32 {
    let Ok(master) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("master id is not UTF-8");
    };
    let master = master.to_string();
    let result = STATE.with(|state| state.borrow_mut().engine.add_master(page.max(0) as usize, &master, x, y));
    match result {
        Ok(new_id) => {
            set_out(new_id);
            0
        }
        Err(message) => fail(message),
    }
}

/// Which shape owns the text shown for `id` and what it says, as JSON `{"owner":"..","text":".."}`:
/// the shape itself when it has text of its own, else the first member with some (a group's label
/// lives in a member). Text with fields is reported as an error.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_text_info(page: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.text_info_json(page.max(0) as usize, id));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The Shape Data rows of shape `id` on `page` with their sort keys, as JSON `[{"name","sortKey"}]` in file order. Read-only.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_data_sort(page: i32, id_ptr: *const u8, id_len: usize) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.data_sort_json(page.max(0) as usize, id));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// The formatting of characters `start..end` (a caret when equal) of the text of shape `id` (the owner from
/// `vl_text_info`), as JSON `{"char":{..},"para":{..}}` (see `writer::text_state`). Read-only.
///
/// # Safety
/// `id_ptr` must point to `id_len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_text_state(page: i32, id_ptr: *const u8, id_len: usize, start: i32, end: i32) -> i32 {
    let Ok(id) = std::str::from_utf8(std::slice::from_raw_parts(id_ptr, id_len)) else {
        return fail("shape id is not UTF-8");
    };
    let result = STATE.with(|state| state.borrow().engine.text_state_json(page.max(0) as usize, id, start.max(0) as usize, end.max(0) as usize));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Width and height (inches) of `page` as JSON `{"w":..,"h":..,"metric":bool}` in the output buffer. The SVG
/// maps page point (x, y) to user units `(96 x, 96 (h - y))`.
#[no_mangle]
pub extern "C" fn vl_page_size(page: i32) -> i32 {
    let result = STATE.with(|state| state.borrow().engine.page_size_json(page.max(0) as usize));
    match result {
        Ok(text) => {
            set_out(text);
            0
        }
        Err(message) => fail(message),
    }
}

/// Merge the edits made since the undo depth was `depth` into one undo step (a multi-shape edit
/// is applied as several single-shape edits). Returns the new depth.
#[no_mangle]
pub extern "C" fn vl_collapse_history(depth: i32) -> i32 {
    STATE.with(|state| state.borrow_mut().engine.collapse_history(depth.max(0) as usize) as i32)
}

/// Number of edits that can be undone.
#[no_mangle]
pub extern "C" fn vl_undo_depth() -> i32 {
    STATE.with(|state| state.borrow().engine.undo_depth() as i32)
}

/// The current document as VSDX bytes in the output buffer.
#[no_mangle]
pub extern "C" fn vl_export() -> i32 {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(bytes) = state.engine.export().map(|bytes| bytes.to_vec()) else {
            return 1;
        };
        state.out = bytes;
        0
    })
}

/// Group or ungroup page-level shapes, ONE undo step: `op` 0 groups the shapes whose ids (separated by newlines) are at
/// `ptr` (the output buffer holds the new group's id), `op` 1 ungroups the group whose id is at `ptr` (the output buffer
/// holds the JSON array of the freed shapes' ids).
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 (from `vl_alloc`).
#[no_mangle]
pub unsafe extern "C" fn vl_group(page: i32, op: i32, ptr: *const u8, len: usize) -> i32 {
    let Ok(text) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return fail("the shape ids are not UTF-8");
    };
    let text = text.to_string();
    let result = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.engine.group_op(page.max(0) as usize, op, &text)
    });
    match result {
        Ok(out) => {
            set_out(out);
            0
        }
        Err(message) => fail(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_XML: &str = "<VisioDocument xmlns='http://schemas.microsoft.com/visio/2003/core'><Pages><Page ID='0' NameU='Plan'><PageSheet><PageProps><PageWidth>8</PageWidth><PageHeight>6</PageHeight></PageProps></PageSheet></Page></Pages></VisioDocument>";

    fn import(bytes: &[u8]) -> i32 {
        unsafe { vl_import(bytes.as_ptr(), bytes.len()) }
    }

    fn output() -> String {
        STATE.with(|state| String::from_utf8_lossy(&state.borrow().out).into_owned())
    }

    #[test]
    fn a_converted_drawing_says_so_and_another_one_does_not() {
        assert_eq!(import(OLD_XML.as_bytes()), 0, "{}", output());
        assert_eq!(vl_conversion(), 0);
        assert!(output().contains("original file is not changed"), "{}", output());
        assert_eq!(unsafe { vl_new(8.0, 6.0, 0, "Page".as_ptr(), 4) }, 0);
        assert_eq!(vl_conversion(), 0);
        assert_eq!(output(), "");
    }

    /// The browser shell must offer what the engine opens, in both file pickers and for dropped files.
    #[test]
    fn the_shell_offers_the_old_formats() {
        let page = include_str!("../../../web/shell.html");
        let script = include_str!("../../../web/shell.js");
        let accepted = |id: &str| {
            let tag = page.split(&format!("id=\"{id}\" accept=\"")).nth(1).unwrap_or_else(|| panic!("no file input {id}"));
            tag.split('"').next().unwrap_or("").split(',').collect::<Vec<_>>()
        };
        let dropped = script.split("const DRAWING_FILE = /\\.(").nth(1).and_then(|rest| rest.split(')').next()).expect("DRAWING_FILE");
        for extension in ["vsd", "vss", "vst", "vdx", "vsx", "vtx"] {
            for id in ["file", "stencilFile"] {
                assert!(accepted(id).contains(&format!(".{extension}").as_str()), "{id} does not offer .{extension}");
            }
            assert!(dropped.split('|').any(|e| e == extension), "a dropped .{extension} is refused");
        }
    }
}
