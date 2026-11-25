// Thin JS bridge over the Rust core (layoutcraft-wasm, raw C ABI). Works in browsers and Node.
// `await loadEngine(wasmBytesOrUrl)` -> engine with import/svg/groups/edit/undo/export.
const STUBS = {
  __wbindgen_placeholder__: { __wbindgen_describe: () => {} },
  __wbindgen_externref_xform__: { __wbindgen_externref_table_set_null: () => {}, __wbindgen_externref_table_grow: () => 0 },
};

export async function loadEngine(source) {
  let instance;
  if (typeof source === 'string') {
    const response = await fetch(source);
    ({ instance } = await WebAssembly.instantiate(await response.arrayBuffer(), STUBS));
  } else {
    ({ instance } = await WebAssembly.instantiate(source, STUBS));
  }
  const x = instance.exports;
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  const output = () => decoder.decode(new Uint8Array(x.memory.buffer, x.vl_out_ptr(), x.vl_out_len()));
  const check = (status) => { if (status !== 0) throw new Error(output() || `engine error ${status}`); };
  const text = (status) => { check(status); return output(); };

  function pageOp(op, a, b, name) {
    const bytes = encoder.encode(String(name));
    const ptr = x.vl_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
    const status = x.vl_page_op(op, a, b, ptr, bytes.length);
    x.vl_dealloc(ptr, bytes.length);
    return Number(text(status));
  }
  function edit(page, op, id, a, b, anchor, flag) {
    const bytes = encoder.encode(String(id));
    const ptr = x.vl_alloc(bytes.length);
    new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
    const status = x.vl_edit(page, op, ptr, bytes.length, a, b, anchor[0], anchor[1], flag ? 1 : 0);
    x.vl_dealloc(ptr, bytes.length);
    check(status);
  }

  return {
    /** Open a VSDX/VSSX/VSTX file (Uint8Array / ArrayBuffer). Clears the undo history. */
    import(bytes) {
      const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
      const ptr = x.vl_alloc(data.length);
      new Uint8Array(x.memory.buffer, ptr, data.length).set(data);
      const status = x.vl_import(ptr, data.length);
      x.vl_dealloc(ptr, data.length);
      check(status);
    },
    /**
     * Open an external stencil (.vssx / .vstx, or any drawing used as a source of masters) next to the
     * drawing. Returns {index, masters: [{id, name, w, h}]}; the stencil stays open until closeStencil.
     */
    openStencil(bytes) {
      const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
      const ptr = x.vl_alloc(data.length);
      new Uint8Array(x.memory.buffer, ptr, data.length).set(data);
      const status = x.vl_stencil_open(ptr, data.length);
      x.vl_dealloc(ptr, data.length);
      return JSON.parse(text(status));
    },
    /** The built-in "Basic shapes" stencil (no file): same answer as openStencil; opened once, then the same index. */
    openBuiltinStencil() {
      return JSON.parse(text(x.vl_stencil_builtin()));
    },
    /** SVG text of master `masterId` of the open DRAWING on its own (a thumbnail for "This drawing"). */
    masterThumb(masterId) {
      const bytes = encoder.encode(String(masterId));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_master_thumb(ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    closeStencil: (index) => x.vl_stencil_close(index) === 0,
    /** SVG text of one master of an open stencil, drawn on its own (a thumbnail). */
    stencilThumb(index, masterId) {
      const bytes = encoder.encode(String(masterId));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_stencil_thumb(index, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    /**
     * Drop an instance of a stencil master with its pin at (x, y) inches (y up): the master is copied into
     * the drawing first (once). One undo step; returns the new shape's id.
     */
    dropStencilMaster(page, index, masterId, px, py) {
      const bytes = encoder.encode(String(masterId));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_stencil_drop(page, index, ptr, bytes.length, px, py);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    /**
     * Start a NEW, empty drawing: one page of width x height inches (stored in inches; `metric` shows it in
     * millimetres) called `name`. Replaces the open drawing and clears the undo history.
     */
    newDrawing({ width, height, metric = false, name = 'Page-1' }) {
      const bytes = encoder.encode(String(name));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_new(width, height, metric ? 1 : 0, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      check(status);
    },
    /** The background pages and the background of each shown page: {backgrounds: [{id, name}], assigned: [id|null, ...]}. */
    backgroundInfo: () => JSON.parse(text(x.vl_background_info())),
    /** Show the background page `id` (null = none) beneath page `index`. One undo step. */
    setBackPage: (index, id) => pageOp(12, index, 0, id == null ? '' : String(id)),
    /** Make page `index` a background page (it leaves the page tabs); returns the index to show. One undo step. */
    makeBackground: (index) => pageOp(13, index, 0, ''),
    /** Make the background page `id` an ordinary page again; returns its page index. One undo step. */
    makeForeground: (id) => pageOp(14, 0, 0, String(id)),
    /** The document properties {title, subject, creator, keywords, description, category, manager, company}. */
    docProps: () => JSON.parse(text(x.vl_doc_props())),
    /** Set the document properties (all eight fields; an empty string clears one). One undo step. */
    setDocProps: (props) => pageOp(11, 0, 0, JSON.stringify(props)),
    /** The page names, in page order. */
    pageNames: () => JSON.parse(text(x.vl_page_names())),
    /** Add an empty page after page `after` (name optional); returns the new page's index. One undo step. */
    addPage: (after, name = '') => pageOp(1, after, 0, name),
    /** Delete page `index` (not the last one); returns the index of the page to show. One undo step. */
    deletePage: (index) => pageOp(2, index, 0, ''),
    /** Set the size of page `index` (inches). The shapes keep their positions (origin lower left). One undo step. */
    setPageSize: (index, width, height) => pageOp(6, index, 0, `${width},${height}`),
    /** Preview of a move: the routes [{id, route: [[x, y]...]}] (page inches) the connectors glued to the shapes `ids` would have if they were moved by (dx, dy) (page inches, y up). Changes nothing. */
    previewMove: (page, ids, dx, dy) => {
      const bytes = encoder.encode(ids.join(String.fromCharCode(10)));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_preview_move(page, ptr, bytes.length, dx, dy);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** The ruler guides of page `index`: [{id, x, y, vertical}] (inches, y up). */
    /** The ShapeSheet of shape `id` (read only): {id, name, type, master, cells: [{name, value, formula, unit}], sections: [{name, index, fromMaster, cells, rows: [{name, index, type, cells}]}]}. */
    shapeSheet(index, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_shape_sheet(index, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** The comments of the drawing: [{id, author, initials, page (index or -1), shape (id or null = the page), date, editDate, text}]. */
    comments: () => JSON.parse(text(x.vl_comments())),
    /** Add a comment to page `index` (on shape `shapeId`, or the page itself when null) by `author` (`initials`); `now` is local time YYYY-MM-DDTHH:MM:SS. One undo step. */
    /** Add a connection point to shape `id` at (fx, fy), 0..1 of its width / height from the bottom left (Visio's Connection Point tool). One undo step; returns the new point's number (1-based). */
    addConnectionPoint(index, id, fx, fy) { const before = this.connectionPoints(index, id).length; pageOp(24, index, 0, [id, fx, fy].join('|')); return this.connectionPoints(index, id).length > before ? Math.max(...this.connectionPoints(index, id).map((p) => p.n)) : 0; },
    /** Move connection point `n` of shape `id` to (fx, fy), 0..1 of its width / height from the bottom left; connectors glued to the shape are routed again. One undo step. */
    moveConnectionPoint: (index, id, n, fx, fy) => { pageOp(25, index, 0, [id, n, fx, fy].join('|')); },
    /** Delete connection point `n` of shape `id` (later points are renumbered, glue follows; connectors glued to it are unglued, their ends stay). One undo step. */
    deleteConnectionPoint: (index, id, n) => { pageOp(26, index, 0, [id, n].join('|')); },
    /** The theme gallery of Design > Themes: {themes: [{name, index, accents: [hex x 6], fonts: {major, minor}}], current: name of the drawing's theme or null}. */
    themes: () => JSON.parse(text(x.vl_themes())),
    /** Apply the gallery theme `name` to the whole drawing (the theme part and the page sheets' scheme cells; shapes keep their theme formulas). One undo step. */
    applyTheme: (index, name) => { pageOp(30, index, 0, String(name)); },
    /** Apply a gallery theme to shown page `index` ONLY (Visio's Page.SetTheme / Design > Themes on the current page): a theme per page; the theme part is added when no page has it yet. One undo step. */
    applyThemeToPage: (index, name) => { pageOp(32, index, 0, String(name)); },
    /** Variant 0..3 of one page's theme only (the page must have a theme). One undo step. */
    setThemeVariantOnPage: (index, variant) => { pageOp(33, index, 0, String(variant)); },
    /** Choose variant 0..3 of the drawing's theme (Design > Variants; the drawing must have a theme). One undo step. `themes().variant` says which one is on. */
    setThemeVariant: (index, variant) => { pageOp(31, index, 0, String(variant)); },
    /** Edit points of a freeform shape (GEOM-15; page inches, y up; the box is refitted; glued connectors are routed again; one undo step each). */
    moveVertex: (index, id, v, x, y) => { pageOp(27, index, 0, [id, v, x, y].join('|')); },
    insertVertex: (index, id, after, x, y) => { pageOp(28, index, 0, [id, after, x, y].join('|')); },
    deleteVertex: (index, id, v) => { pageOp(29, index, 0, [id, v].join('|')); },
    setSegmentBow: (index, id, seg, bow) => { pageOp(34, index, 0, [id, seg, bow].join('|')); }, // the bow (signed sagitta, inches) of the arc from vertex seg to the next; 0 = straight
    /**
     * Change Shape: replace shape `id` by an instance of master `masterId` (a master of the drawing, or - with `stencilIndex` - of an opened stencil,
     * imported first). Place, size, turn, formats and text stay; glued connectors follow. One undo step; returns the new shape's id.
     */
    replaceShape(page, id, masterId, stencilIndex = -1) {
      const bytes = encoder.encode(`${id}|${masterId}`);
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_replace_shape(page, stencilIndex, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    addComment: (index, shapeId, author, initials, commentText, now) => pageOp(20, index, 0, [shapeId ?? '', author, initials, now, commentText].join('|')),
    /** Change the text of comment `id` (page `index` stays shown). */
    setCommentText: (index, id, commentText, now) => pageOp(21, index, 0, [id, now, commentText].join('|')),
    deleteComment: (index, id) => pageOp(22, index, 0, String(id)),
    /** The hyperlinks of shape `id` of page `index`: [{name, description, address, subAddress, extraInfo, frame, newWindow, default, invisible}]. */
    hyperlinks(index, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_hyperlinks(index, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** Replace ALL hyperlinks of shape `id` by the list (same fields; a new link has no name). One undo step. */
    setHyperlinks: (index, id, links) => pageOp(19, index, 0, `${id}|${JSON.stringify(links)}`),
    /** Set the alt text (Shape > Alt Text) of shape `id` on page `index`; an empty text removes it. One undo step. */
    setAltText: (index, id, alt) => pageOp(39, index, 0, `${id}|${alt}`),
    /** Name shape `id` of page `index` (Shape.Name; an empty name gives the automatic name back). Names are unique on a page: a taken name gets .<shape id> (Visio's rule); returns the name the shape has now ("" for none). One undo step. */
    setShapeName(index, id, name) { pageOp(40, index, 0, `${id}|${name}`); return this.shapeNames(index)[String(id)] || ''; },
    /** The names of the shapes of page `index`: {shape id: name} (only shapes that have a name of their own). Not part of model(). */
    shapeNames: (index) => JSON.parse(text(x.vl_shape_names(index))),
    guides: (index) => JSON.parse(text(x.vl_guides(index))),
    /** Add a guide to page `index`: vertical at x = `position`, or horizontal at y = `position` (inches). One undo step; the new guide is the last of model().pages[index].guides. */
    addGuide: (index, vertical, position) => pageOp(16, index, vertical ? 1 : 0, String(position)),
    /** Move the guide `id` of page `index` to `position` (inches: x of a vertical guide, y of a horizontal one). One undo step. */
    moveGuide: (index, id, position) => pageOp(17, index, 0, `${id}|${position}`),
    /** Delete the guide `id` of page `index`. One undo step. */
    deleteGuide: (index, id) => pageOp(18, index, 0, String(id)),
    /** Set the drawing scale of page `index`: `paper` `paperUnit` = `drawing` `drawingUnit` (units in, ft, mm, cm, m). pageSize(index).scale tells the current one. One undo step. */
    /** Print setup of a page: {orientation 1|2, margins [l, r, t, b] in, grid, scale [x, y], fit, pages [x, y], center [h, v]}; one undo step. */
    setPrintSetup: (index, setup) => pageOp(23, index, 0, JSON.stringify(setup)),
    setDrawingScale: (index, paper, paperUnit, drawing, drawingUnit) => pageOp(15, index, 0, [paper, paperUnit, drawing, drawingUnit].join(',')),
    /** Write a Shape Data row of shape `id`: `{name, label, value, prompt, format, type, invisible}` (types 0 text, 1 list, 2 number, 3 flag, 4 variable list, 5 date). One undo step. */
    setShapeData: (page, id, row) => edit(page, 17, `${id}${String.fromCharCode(10)}${JSON.stringify(row)}`, 0, 0, [0, 0], false),
    /** Remove the Shape Data row `name` of shape `id` (a row of the shape's own, not one only its master has). One undo step. */
    removeShapeData: (page, id, name) => edit(page, 18, `${id}${String.fromCharCode(10)}${name}`, 0, 0, [0, 0], false),
    /** Add a layer to page `index`. One undo step. */
    addLayer: (index, name) => pageOp(7, index, 0, name),
    /** Change layer `layer` (its index) of page `index`: `{name, visible, print, locked}` (any of them). One undo step. */
    setLayer: (index, layer, { name = '', visible, print, locked } = {}) => pageOp(8, index, layer, [name, visible === undefined ? '' : visible ? 1 : 0, print === undefined ? '' : print ? 1 : 0, locked === undefined ? '' : locked ? 1 : 0].join('|')),
    /** Colour layer `layer` of page `index` (`#rrggbb`, or `none`): its shapes draw lines and text in it with a white fill. One undo step. */
    setLayerColor: (index, layer, color) => pageOp(10, index, layer, color),
    /** Delete layer `layer` of page `index`; its shapes stay (and lose the layer). One undo step. */
    deleteLayer: (index, layer) => pageOp(9, index, layer, ''),
    /** Put the shapes on layer `layer` (`member` true) or take them off it. One undo step. */
    layerMembers: (page, ids, layer, member = true) => edit(page, 16, ids.join(String.fromCharCode(10)), layer, 0, [0, 0], member),
    /** Rename page `index`. One undo step. */
    renamePage: (index, name) => pageOp(3, index, 0, name),
    /** Move page `from` to position `to`. One undo step. */
    movePage: (from, to) => pageOp(4, from, to, ''),
    /** Copy page `index` right after it; returns the copy's index. One undo step. */
    duplicatePage: (index) => pageOp(5, index, 0, ''),
    /**
     * Draw a new shape from scratch: `kind` 0 rectangle, 1 rounded rectangle, 2 ellipse, 3 diamond, 4 triangle, 5 text
     * box, 6 line, in the box from (x0, y0) to (x1, y1) in page inches (y up; a line runs from the first point to the
     * second). Returns the new shape's id. One undo step.
     */
    drawShape: (page, kind, x0, y0, x1, y1) => text(x.vl_draw(page, kind, x0, y0, x1, y1)),
    /** Insert a picture (PNG, JPEG, GIF or BMP bytes) in the box (x0, y0)-(x1, y1) of page `page` (inches, y up); returns the new shape's id. One undo step. */
    insertPicture(page, bytes, x0, y0, x1, y1) {
      const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
      const ptr = x.vl_alloc(data.length);
      new Uint8Array(x.memory.buffer, ptr, data.length).set(data);
      const status = x.vl_insert_picture(page, ptr, data.length, x0, y0, x1, y1);
      x.vl_dealloc(ptr, data.length);
      return text(status);
    },
    /**
     * Draw a freeform shape through `points` ([[x, y], ...] in page inches, y up): an open polyline, or a closed polygon when `closed`. Returns the new
     * shape's id; the shape has Edit Points like any freeform. One undo step.
     */
    /** Draw a circular arc from (x0, y0) to (x1, y1) bowed by `bow` inches (positive bulges to the right of the direction); returns the shape's id. One undo step. */
    drawArc: (page, x0, y0, x1, y1, bow) => text(x.vl_draw_arc(page, x0, y0, x1, y1, bow)),
    /** Draw a smooth curve (Visio's spline) through `points` ([[x, y], ...] in page inches, 4 to 200); returns the shape's id. One undo step. */
    drawCurve: (page, points) => {
      const bytes = encoder.encode(points.map((p) => `${p[0]},${p[1]}`).join(';'));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_draw_curve(page, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    /** Fit Curve: the open polyline `id` becomes a smooth spline through its vertices. One undo step. */
    fitCurve: (index, id) => { pageOp(35, index, 0, String(id)); },
    /** The style sheets of the drawing: [{id, name, line, fill, text (ids of the styles it is based on), custom, lineColor, lineWeight (inches), fillColor, fillPattern}]. */
    listStyles: () => JSON.parse(text(x.vl_styles_list())),
    /** Apply style `styleId` to shapes `ids` of page `index` (scope 'all' = Shape.Style, or 'line' / 'fill' / 'text'); the shapes' own cells of that category go. One undo step. */
    applyStyle: (index, ids, styleId, scope = 'all') => { pageOp(36, index, 0, `${styleId}|${scope}|${ids.join(',')}`); },
    /** Create the custom style `spec.name` or change it: {name, basedOn, lineColor '#rrggbb', lineWeight (pt), linePattern, fillColor, fillPattern}. One undo step. */
    defineStyle: (spec) => { pageOp(37, 0, 0, JSON.stringify(spec)); },
    /** Delete the custom style `styleId`: shapes that used it and styles based on it fall back to No Style (as Visio does); built-in styles are refused. One undo step. */
    deleteStyle: (styleId) => { pageOp(38, 0, 0, String(styleId)); },
    drawPolyline: (page, points, closed) => {
      const bytes = encoder.encode(points.map((p) => `${p[0]},${p[1]}`).join(';'));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_draw_polyline(page, closed ? 1 : 0, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    pageCount: () => x.vl_page_count(),
    /** The imported document model. */
    model: () => JSON.parse(text(x.vl_json())),
    /** SVG of a page; `print` leaves out non-printing layers as Visio's export does. */
    svg: (page, { print = false } = {}) => text(print ? x.vl_svg_print(page) : x.vl_svg(page)),
    /** Frame of every shape on the page: [{id, parentId, isGroup, editable, pin, size, loc, angle (radians)}]. */
    groups: (page) => JSON.parse(text(x.vl_group_info(page))),
    /** Resize a group to (w,h) inches in its own axes, keeping the anchor fraction (0..1) fixed. */
    resizeGroup: (page, id, w, h, anchor = [0.5, 0.5]) => edit(page, 1, id, w, h, anchor, false),
    /** Move a group member by a page delta (inches, y up); `refit` grows the ancestor frames. */
    moveMember: (page, id, dx, dy, refit = false) => edit(page, 2, id, dx, dy, [0, 0], refit),
    resizeMember: (page, id, w, h, anchor = [0.5, 0.5], refit = false) => edit(page, 3, id, w, h, anchor, refit),
    /** Delete a shape and everything inside it; connector ends glued to it stay where they are. */
    deleteShape: (page, id) => edit(page, 4, id, 0, 0, [0, 0], false),
    /**
     * Format shapes (one undo step) and everything inside them: {fill, line} as "#rrggbb" or "none",
     * weight in px (96 per inch), pattern 0..23, fontPt in points; every field optional.
     */
    /**
     * Format a RANGE of the text of shape `id` (the shape that owns the text, see textInfo): `spec` =
     * {start, end (character offsets of the plain text), cells: {Strikethru: '1', Case: '1', Letterspace: inches,
     * Color: '#rrggbb', Size: inches, Font: name}, set / clear (Style bits: 1 bold, 2 italic, 4 underline),
     * para: {HorzAlign: '0'-'3', Bullet: '1'}}. The runs and paragraphs are split at the ends of the range
     * (new Character / Paragraph rows). One undo step.
     */
    formatText: (page, id, spec) => edit(page, 13, `${id}${String.fromCharCode(10)}${JSON.stringify(spec)}`, 0, 0, [0, 0], false),
    styleShapes: (page, ids, style) => edit(page, 10, `${ids.join('\n')}\n${JSON.stringify(style)}`, 0, 0, [0, 0], false),
    /** What the cells state for a shape: {linePattern, lineWeight (in), fillPattern, fontPt, hasText}. */
    styleInfo(page, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_style_info(page, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** Copy page-level, non-connector shapes to the clipboard; returns how many were copied. */
    copy(page, ids) {
      const bytes = encoder.encode(ids.join('\n'));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_copy(page, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return Number(text(status));
    },
    /** Paste the clipboard (one undo step), offset by (dx, dy) inches, y up; returns the new ids. */
    paste: (page, dx = 0, dy = 0) => JSON.parse(text(x.vl_paste(page, dx, dy))),
    clipboardCount: () => x.vl_clipboard_count(),
    /** Connection points of a shape: [{n, x, y}] (n as in Connections.X<n>, page inches, y up). */
    connectionPoints(page, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_connection_points(page, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /**
     * Add a glued right-angle connector; each end is {id, n} with n = connection point (1-based) or
     * 0/undefined for the whole shape. Returns the new shape's id.
     */
    addConnector(page, from, to) {
      const bytes = encoder.encode(`${from.id}\n${to.id}`);
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_add_connector(page, ptr, bytes.length, from.n || 0, to.n || 0);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    /**
     * AutoConnect (CONN-13, probe-autoconnect.ps1): place a new shape next to shape `id` in direction `dir` ('right', 'left', 'up', 'down') and glue a connector from `id` to it
     * (whole shapes). The new shape is one avenue (the page's AvenueSize, pageSize(page).avenue) away from the edge of the box of `id` and centred on its axis. `pick` is null
     * (a copy of the shape) or {size: [w, h], place: (x, y) => id of the new shape, centred at (x, y)}. One undo step; returns the id of the new shape.
     */
    autoConnect(page, id, dir, pick = null) {
      const f = this.groups(page).find((g) => g.id === id);
      const d = { right: [1, 0], left: [-1, 0], up: [0, 1], down: [0, -1] }[dir];
      if (!f || !d) throw new Error(`cannot add and connect a shape next to ${id} (${dir})`);
      const [s, c] = [Math.sin(f.angle), Math.cos(f.angle)];
      const corners = [[0, 0], [f.size[0], 0], [f.size[0], f.size[1]], [0, f.size[1]]].map(([u, v]) => {
        let p = [u - f.loc[0], v - f.loc[1]];
        if (f.flipX) p[0] = -p[0];
        if (f.flipY) p[1] = -p[1];
        return [f.pin[0] + c * p[0] - s * p[1], f.pin[1] + s * p[0] + c * p[1]];
      });
      const box = [Math.min(...corners.map((p) => p[0])), Math.min(...corners.map((p) => p[1])), Math.max(...corners.map((p) => p[0])), Math.max(...corners.map((p) => p[1]))];
      const [ax, ay] = this.pageSize(page).avenue;
      const size = pick ? pick.size : [box[2] - box[0], box[3] - box[1]];
      const mid = [(box[0] + box[2]) / 2, (box[1] + box[3]) / 2];
      const target = [mid[0] + d[0] * ((box[2] - box[0]) / 2 + ax + size[0] / 2), mid[1] + d[1] * ((box[3] - box[1]) / 2 + ay + size[1] / 2)];
      let created;
      this.batch(() => {
        if (pick) created = pick.place(target[0], target[1]);
        else { this.copy(page, [id]); created = this.paste(page, target[0] - mid[0], target[1] - mid[1])[0]; }
        this.addConnector(page, { id }, { id: created });
      });
      return created;
    },
    /** The page-level connectors: [{id, route: [[x, y]...], begin: {x, y, glue}, end: {x, y, glue}}], glue = {sheet, n} | null. */
    connectors: (page) => JSON.parse(text(x.vl_connectors(page))),
    /**
     * Move one end (`end` 0 = begin, 1 = end) of a connector: onto a shape {id, n} (n = connection point
     * or 0 for the whole shape) so it is glued there, or to a loose page position {x, y} (inches, y up).
     * One undo step.
     */
    moveConnectorEnd(page, id, end, target) {
      const bytes = encoder.encode([id, target.id ?? ''].join(String.fromCharCode(10)));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_move_connector_end(page, ptr, bytes.length, end, target.n || 0, target.x ?? 0, target.y ?? 0);
      x.vl_dealloc(ptr, bytes.length);
      text(status); // throws the engine's message on failure
    },
    /**
     * Reverse ends (Visio: Shape > Reverse Ends, command 1870, probed with visio-test-harness/probe-reverse-ends.ps1): the begin and the end of a connector
     * trade places, glue included; the arrowheads stay with their end (the End arrow then points at the shape that was the start). One undo step.
     */
    reverseEnds(page, id) {
      const c = JSON.parse(text(x.vl_connectors(page))).find((w) => w.id === id);
      if (!c) throw new Error(`shape ${id} is not a connector`);
      const target = (e) => (e.glue ? { id: e.glue.sheet, n: e.glue.n } : { x: e.x, y: e.y });
      const [fromBegin, fromEnd] = [target(c.begin), target(c.end)];
      // the end is let go first: a connector cannot have both ends on one shape while the ends trade places
      this.batch(() => { this.moveConnectorEnd(page, id, 1, { x: c.end.x, y: c.end.y }); this.moveConnectorEnd(page, id, 0, fromEnd); this.moveConnectorEnd(page, id, 1, fromBegin); });
    },
    /** The masters of the open document: [{id, name, w, h}] (sizes in inches). */
    masters: () => JSON.parse(text(x.vl_masters())),
    /** Drop an instance of a master with its pin at (x, y) inches (y up); returns the new shape's id. */
    addMaster(page, masterId, px, py) {
      const bytes = encoder.encode(String(masterId));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_add_master(page, ptr, bytes.length, px, py);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    /** The shape that owns the text shown for `id` and its plain text: {owner, text}. */
    textInfo(page, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_text_info(page, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /**
     * The formatting of characters `start..end` (a caret when equal; offsets as a text box counts them) of the text of
     * shape `id` (the `owner` of textInfo): {char: {Style, Case, Strikethru, Letterspace, Size, Font, Color, ...},
     * para: {HorzAlign, Bullet, IndLeft, ...}}; a value is "mixed" when the range differs, a cell is missing when the
     * shape's style supplies it.
     */
    textState(page, id, start, end) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_text_state(page, ptr, bytes.length, start, end);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** The Shape Data rows written on shape `id` with their sort keys: [{name, sortKey}] in file order (rows a master supplies are not listed). */
    dataSortKeys(page, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_data_sort(page, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** Replace the text of shape `id` (typically the `owner` from textInfo); formatting runs stay. */
    setText: (page, id, newText) => edit(page, 8, `${id}\n${newText}`, 0, 0, [0, 0], false),
    /** Turn a shape about its pin to the absolute page angle (radians, counter-clockwise). */
    rotateShape: (page, id, angle) => edit(page, 7, id, angle, 0, [0, 0], false),
    /** Turn page-level shapes by `delta` radians (counter-clockwise): one about its pin, several as one body about the centre of their joint box. One undo step. */
    rotateShapes: (page, ids, delta) => edit(page, 11, ids.join(String.fromCharCode(10)), delta, 0, [0, 0], false),
    /** Mirror page-level shapes about the vertical (or `vertical`: horizontal) axis through the centre of their joint box. One undo step. */
    /** Align the shapes to the first of `ids`: mode 0 left, 1 centre, 2 right, 3 top, 4 middle, 5 bottom. One undo step. */
    alignShapes: (page, ids, mode) => edit(page, 14, ids.join(String.fromCharCode(10)), mode, 0, [0, 0], false),
    /** Space the shapes evenly (needs three): horizontally, or `vertical`. One undo step. */
    distributeShapes: (page, ids, vertical = false) => edit(page, 15, ids.join(String.fromCharCode(10)), 0, 0, [0, 0], vertical),
    /** Group page-level shapes into a new group; returns its id. One undo step. */
    groupShapes(page, ids) {
      const bytes = encoder.encode(ids.join(String.fromCharCode(10)));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_group(page, 0, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    /** Ungroup a plain group of the page; returns the ids of the freed shapes. One undo step. */
    ungroupShape(page, id) {
      const bytes = encoder.encode(String(id));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_group(page, 1, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /**
     * Shape operations (Visio: Home > Arrange > Operations; rules measured with visio-test-harness/probe-shape-ops.ps1): `how` is 'union',
     * 'combine', 'fragment', 'intersect' or 'subtract' on page-level shapes of straight lines or ellipses. The first id gives the look and the
     * text of the result (Subtract: the shape the others are cut from). Returns the ids of the new shapes. One undo step.
     */
    shapeOperation(page, how, ids) {
      const op = { union: 6, combine: 7, fragment: 8, intersect: 9, subtract: 10, join: 11, trim: 12 }[how];
      if (!op) throw new Error(`unknown shape operation ${how}`);
      const bytes = encoder.encode(ids.join(String.fromCharCode(10)));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_group(page, op, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return JSON.parse(text(status));
    },
    /** Change the drawing order of shapes (connectors included) among their siblings: `how` is 'front', 'back', 'forward' or 'backward'. One undo step. */
    reorderShapes(page, ids, how) {
      const op = { front: 2, back: 3, forward: 4, backward: 5 }[how];
      if (!op) throw new Error(`unknown drawing order ${how}`);
      const bytes = encoder.encode(ids.join(String.fromCharCode(10)));
      const ptr = x.vl_alloc(bytes.length);
      new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
      const status = x.vl_group(page, op, ptr, bytes.length);
      x.vl_dealloc(ptr, bytes.length);
      return text(status);
    },
    flipShapes: (page, ids, vertical = false) => edit(page, 12, ids.join(String.fromCharCode(10)), 0, 0, [0, 0], vertical),
    /** Page size in inches; the SVG maps page (x, y) to user units (96 x, 96 (h - y)). */
    pageSize: (page) => JSON.parse(text(x.vl_page_size(page))),
    /** Move several shapes by the same page delta (inches, y up) as one undo step. */
    moveMembers(page, ids, dx, dy, refit = false) {
      if (ids.length === 1) return edit(page, 2, ids[0], dx, dy, [0, 0], refit);
      if (refit) return this.batch(() => { for (const id of ids) edit(page, 2, id, dx, dy, [0, 0], true); });
      return edit(page, 5, ids.join('\n'), dx, dy, [0, 0], false);
    },
    /** Delete several shapes (none inside another) as one undo step. */
    deleteShapes: (page, ids) => edit(page, 6, ids.join('\n'), 0, 0, [0, 0], false),
    /** Run several edits as ONE undo step (even when one of them throws). */
    batch(fn) {
      const depth = x.vl_undo_depth();
      try { return fn(); } finally { x.vl_collapse_history(depth); }
    },
    undo() { return x.vl_undo() === 0; },
    /** Redo the last undone edit (false when there is none, or when an edit came after the undo). */
    redo() { return x.vl_redo() === 0; },
    redoDepth: () => x.vl_redo_depth(),
    undoDepth: () => x.vl_undo_depth(),
    /** The current document as VSDX bytes. */
    export() { check(x.vl_export()); return new Uint8Array(x.memory.buffer, x.vl_out_ptr(), x.vl_out_len()).slice(); },
  };
}
