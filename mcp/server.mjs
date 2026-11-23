#!/usr/bin/env node
// LayoutCraft MCP server (Model Context Protocol, https://modelcontextprotocol.io): lets an AI assistant create and edit drawings.
// It drives the same engine as the web shell (web/engine.js on web/layoutcraft.wasm), so every format the shell handles works here too.
// Transport: stdio, one JSON-RPC 2.0 message per line. No dependencies, no network, no shell.
//   node mcp/server.mjs --root <folder>      files are read and written only below <folder> (default: the current directory)
// Details: docs/32-mcp.md. Logs go to stderr (stdout is the protocol).
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';
import { loadEngine } from '../web/engine.js';
import { importDrawio } from '../web/drawio.js';
import { importExcalidraw } from '../web/excalidraw.js';
import { exportDrawio } from '../web/drawio-export.js';
import { exportExcalidraw } from '../web/excalidraw-export.js';
import { svgToDxf } from '../web/svgdxf.js';
import { checkDiagram, issuesToCsv } from '../web/checkdiagram.js';
import { compareModels, diffsToCsv } from '../web/compare.js';
import { inventoryData, inventoryCsv } from '../web/inventory.js';
import { applyBorder, removeBorder, BORDER_STYLES } from '../web/borders.js';

const SELF = fileURLToPath(import.meta.url);
const HERE = path.dirname(SELF);
const WASM = path.resolve(HERE, '../web/layoutcraft.wasm');
const PROTOCOLS = ['2025-06-18', '2025-03-26', '2024-11-05'];
const PX_PER_INCH = 96;
const KINDS = { rectangle: 0, rounded: 1, ellipse: 2, diamond: 3, triangle: 4, textbox: 5, line: 6 };

export async function createServer({ root = process.cwd(), wasm = WASM } = {}) {
  const base = path.resolve(root);
  const engine = await loadEngine(fs.readFileSync(wasm));
  engine.newDrawing({ width: 11, height: 8.5, name: 'Page-1' });

  /** A path below the root folder (anything else is refused). */
  const inside = (p) => {
    if (typeof p !== 'string' || !p) throw new Error('a path is required');
    const full = path.resolve(base, p);
    const rel = path.relative(base, full);
    if (rel.startsWith('..') || path.isAbsolute(rel)) throw new Error(`the path is outside the root folder: ${p}`);
    return full;
  };
  const page = (a) => { const n = a.page === undefined ? 0 : a.page; if (!Number.isInteger(n) || n < 0 || n >= engine.pageCount()) throw new Error(`no page ${n} (the drawing has ${engine.pageCount()})`); return n; };
  const ids = (a) => { if (!Array.isArray(a.ids) || !a.ids.length) throw new Error('ids must be a list of shape ids'); return a.ids.map(String); };
  const layerIndex = (p, name) => { const layers = (engine.model().pages[p] || {}).layers || []; const at = layers.findIndex((l) => l.name === name); if (at < 0) throw new Error(`no layer named ${name} (layers: ${layers.map((l) => l.name).join(', ') || 'none'})`); return at; };
  const num = (a, k) => { if (typeof a[k] !== 'number' || !Number.isFinite(a[k])) throw new Error(`${k} must be a number`); return a[k]; };
  const json = (v) => JSON.stringify(v, null, 1);
  // the model is in pixels (96 per inch, y down from the top); the tools speak inches with the origin at the lower left corner
  const inch = (v) => Math.round((v / PX_PER_INCH) * 1e6) / 1e6;
  const shapeRow = (s, pageH, names = {}) => ({ id: String(s.visioId), type: s.type, x: inch(s.x), y: inch(pageH * PX_PER_INCH - s.y - s.h), w: inch(s.w), h: inch(s.h), rotation: s.rot ? Math.round(-s.rot * 1e6) / 1e6 : 0, text: s.label || '', name: names[String(s.visioId)] || '', altText: s.altText || '', fill: s.fill, line: s.stroke });
  const pageSvg = (p) => engine.svg(p);

  const S = { type: 'object', properties: {}, additionalProperties: false };
  const props = (properties, required = []) => ({ type: 'object', properties, required, additionalProperties: false });
  const P = { page: { type: 'integer', minimum: 0, description: 'page index, 0 = first (default 0)' } };
  const STYLE = { type: 'object', description: 'fill, line (#rrggbb), weight (pt), pattern, fontPt, fontFamily, textColor, bold, italic, underline, fillTrans, lineTrans, shadow, beginArrow, endArrow', additionalProperties: true };

  const tools = [
    { name: 'new_drawing', description: 'Start a new empty drawing (discards the open one). Sizes are in inches (millimetres are not used; metric only changes the ruler).', inputSchema: props({ width: { type: 'number', default: 11 }, height: { type: 'number', default: 8.5 }, metric: { type: 'boolean' }, name: { type: 'string' } }),
      run(a) { engine.newDrawing({ width: a.width ?? 11, height: a.height ?? 8.5, metric: !!a.metric, name: a.name || 'Page-1' }); return 'new drawing'; } },
    { name: 'open', description: 'Open a .vsdx, .drawio or .excalidraw file below the root folder.', inputSchema: props({ path: { type: 'string' } }, ['path']),
      async run(a) {
        const full = inside(a.path); const bytes = fs.readFileSync(full); const ext = path.extname(full).toLowerCase();
        if (ext === '.drawio') { engine.newDrawing({ width: 11, height: 8.5 }); const r = await importDrawio(engine, new Uint8Array(bytes)); return `opened ${a.path} (draw.io): ${json(r)}`; }
        if (ext === '.excalidraw') { engine.newDrawing({ width: 11, height: 8.5 }); const r = importExcalidraw(engine, bytes.toString('utf8')); return `opened ${a.path} (Excalidraw): ${json(r)}`; }
        engine.import(new Uint8Array(bytes)); return `opened ${a.path}: ${engine.pageCount()} page(s)`;
      } },
    { name: 'save', description: 'Save the drawing as a Visio .vsdx file below the root folder.', inputSchema: props({ path: { type: 'string' } }, ['path']),
      run(a) { const full = inside(a.path); if (path.extname(full).toLowerCase() !== '.vsdx') throw new Error('the file name must end in .vsdx'); fs.mkdirSync(path.dirname(full), { recursive: true }); fs.writeFileSync(full, engine.export()); return `saved ${a.path}`; } },
    { name: 'export', description: 'Export a page as svg or dxf, or the whole drawing as drawio, or a page as excalidraw. With a path the file is written below the root folder; without one the text is returned.', inputSchema: props({ format: { enum: ['svg', 'dxf', 'drawio', 'excalidraw'] }, path: { type: 'string' }, ...P }, ['format']),
      run(a) {
        const p = page(a); let text;
        if (a.format === 'svg') text = pageSvg(p);
        else if (a.format === 'dxf') text = svgToDxf(pageSvg(p), { pxPerInch: PX_PER_INCH, heightIn: engine.pageSize(p).h }).dxf;
        else if (a.format === 'drawio') text = exportDrawio(engine);
        else if (a.format === 'excalidraw') text = JSON.stringify(exportExcalidraw(engine, p), null, 1);
        else throw new Error('format must be svg, dxf, drawio or excalidraw');
        if (!a.path) return text;
        const full = inside(a.path); fs.mkdirSync(path.dirname(full), { recursive: true }); fs.writeFileSync(full, text); return `wrote ${a.path} (${text.length} characters)`;
      } },
    { name: 'list_pages', description: 'The pages of the open drawing with their sizes (inches) and shape counts.', inputSchema: S,
      run() { const names = engine.pageNames(); const model = engine.model(); return json(names.map((n, i) => { const sz = engine.pageSize(i); return { index: i, name: n, width: sz.w, height: sz.h, shapes: (model.pages[i]?.shapes || []).length }; })); } },
    { name: 'list_shapes', description: 'The shapes of a page: id, type, position (inches, y up from the page bottom), size, rotation (degrees, counter-clockwise), text, fill and line colour.', inputSchema: props(P),
      run(a) { const p = page(a); const h = engine.pageSize(p).h, names = engine.shapeNames(p); return json((engine.model().pages[p]?.shapes || []).map((x) => shapeRow(x, h, names))); } },
    { name: 'draw_shape', description: 'Draw a shape from corner (x0, y0) to corner (x1, y1), in inches. kind: rectangle, rounded, ellipse, diamond, triangle, textbox or line. Returns the new shape id. Optional text and style.', inputSchema: props({ kind: { enum: Object.keys(KINDS) }, x0: { type: 'number' }, y0: { type: 'number' }, x1: { type: 'number' }, y1: { type: 'number' }, text: { type: 'string' }, style: STYLE, ...P }, ['kind', 'x0', 'y0', 'x1', 'y1']),
      run(a) {
        const p = page(a); if (!(a.kind in KINDS)) throw new Error(`kind must be one of ${Object.keys(KINDS).join(', ')}`);
        const id = engine.drawShape(p, KINDS[a.kind], num(a, 'x0'), num(a, 'y0'), num(a, 'x1'), num(a, 'y1'));
        if (a.text) engine.setText(p, id, String(a.text));
        if (a.style) engine.styleShapes(p, [id], a.style);
        return json({ id });
      } },
    { name: 'add_connector', description: 'Glue a connector between two shapes. side: 0 whole shape (default), 1 top, 2 right, 3 bottom, 4 left. Returns the connector id.', inputSchema: props({ from: props({ id: { type: 'string' }, side: { type: 'integer', minimum: 0, maximum: 4 } }, ['id']), to: props({ id: { type: 'string' }, side: { type: 'integer', minimum: 0, maximum: 4 } }, ['id']), ...P }, ['from', 'to']),
      run(a) { const p = page(a); const id = engine.addConnector(p, { id: String(a.from.id), n: a.from.side || 0 }, { id: String(a.to.id), n: a.to.side || 0 }); return json({ id }); } },
    { name: 'set_text', description: 'Set the text of a shape.', inputSchema: props({ id: { type: 'string' }, text: { type: 'string' }, ...P }, ['id', 'text']),
      run(a) { engine.setText(page(a), String(a.id), String(a.text)); return 'ok'; } },
    { name: 'style_shapes', description: 'Style shapes: fill, line (#rrggbb), weight (pt), pattern, fontPt, fontFamily, textColor, bold, italic, underline, fillTrans, lineTrans, shadow, beginArrow, endArrow.', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, style: STYLE, ...P }, ['ids', 'style']),
      run(a) { engine.styleShapes(page(a), ids(a), a.style || {}); return 'ok'; } },
    { name: 'delete_shapes', description: 'Delete shapes (and the connectors glued to them follow Visio rules).', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, ...P }, ['ids']),
      run(a) { engine.deleteShapes(page(a), ids(a)); return 'ok'; } },
    { name: 'add_page', description: 'Add a page after the given page (default: after the last).', inputSchema: props({ name: { type: 'string' }, after: { type: 'integer', minimum: 0 } }),
      run(a) { engine.addPage(a.after ?? engine.pageCount() - 1, a.name || ''); return `pages: ${engine.pageCount()}`; } },
    { name: 'move_shapes', description: 'Move shapes by (dx, dy) inches (y up).', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, dx: { type: 'number' }, dy: { type: 'number' }, ...P }, ['ids', 'dx', 'dy']),
      run(a) { engine.moveMembers(page(a), ids(a), num(a, 'dx'), num(a, 'dy')); return 'ok'; } },
    { name: 'rotate_shapes', description: 'Rotate shapes by an angle in degrees, counter-clockwise (the way Visio turns them).', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, degrees: { type: 'number' }, ...P }, ['ids', 'degrees']),
      run(a) { engine.rotateShapes(page(a), ids(a), (num(a, 'degrees') * Math.PI) / 180); return 'ok'; } },
    { name: 'flip_shapes', description: 'Mirror shapes about the vertical axis (or the horizontal one with vertical: true) through the middle of their joint box.', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, vertical: { type: 'boolean' }, ...P }, ['ids']),
      run(a) { engine.flipShapes(page(a), ids(a), !!a.vertical); return 'ok'; } },
    { name: 'align_shapes', description: 'Align shapes to the first one of ids: left, center, right, top, middle or bottom.', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, mode: { enum: ['left', 'center', 'right', 'top', 'middle', 'bottom'] }, ...P }, ['ids', 'mode']),
      run(a) { const m = ['left', 'center', 'right', 'top', 'middle', 'bottom'].indexOf(a.mode); if (m < 0) throw new Error('mode must be left, center, right, top, middle or bottom'); engine.alignShapes(page(a), ids(a), m); return 'ok'; } },
    { name: 'distribute_shapes', description: 'Space three or more shapes evenly, horizontally (default) or vertically.', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, vertical: { type: 'boolean' }, ...P }, ['ids']),
      run(a) { if (ids(a).length < 3) throw new Error('distribute needs at least three shapes'); engine.distributeShapes(page(a), ids(a), !!a.vertical); return 'ok'; } },
    { name: 'group_shapes', description: 'Group shapes; returns the group id.', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, ...P }, ['ids']),
      run(a) { if (ids(a).length < 2) throw new Error('a group needs at least two shapes'); return json({ id: engine.groupShapes(page(a), ids(a)) }); } },
    { name: 'ungroup_shape', description: 'Ungroup a group; its members stay on the page.', inputSchema: props({ id: { type: 'string' }, ...P }, ['id']),
      run(a) { engine.ungroupShape(page(a), String(a.id)); return 'ok'; } },
    { name: 'reorder_shapes', description: 'Change the drawing order: front, back, forward or backward.', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, how: { enum: ['front', 'back', 'forward', 'backward'] }, ...P }, ['ids', 'how']),
      run(a) { engine.reorderShapes(page(a), ids(a), a.how); return 'ok'; } },
    { name: 'list_connectors', description: 'The connectors of a page with the shapes they are glued to.', inputSchema: props(P),
      run(a) { return json(engine.connectors(page(a))); } },
    { name: 'set_shape_name', description: 'Name a shape (Visio: Shape.Name). Names are unique on a page: a taken name gets .<shape id> added (the rule Visio uses); an empty name gives the automatic name back. Returns the name the shape has now.', inputSchema: props({ id: { type: 'string' }, name: { type: 'string' }, ...P }, ['id', 'name']),
      run(a) { return json({ name: engine.setShapeName(page(a), String(a.id), String(a.name)) }); } },
    { name: 'set_alt_text', description: 'Set the alt text of a shape (what it shows, for screen readers; Visio: Shape > Alt Text). An empty text removes it.', inputSchema: props({ id: { type: 'string' }, text: { type: 'string' }, ...P }, ['id', 'text']),
      run(a) { engine.setAltText(page(a), String(a.id), String(a.text)); return 'ok'; } },
    { name: 'set_hyperlinks', description: 'Replace all hyperlinks of a shape. Each link: address (a URL), subAddress, description.', inputSchema: props({ id: { type: 'string' }, links: { type: 'array', items: { type: 'object', properties: { address: { type: 'string' }, subAddress: { type: 'string' }, description: { type: 'string' } }, additionalProperties: true } }, ...P }, ['id', 'links']),
      run(a) { if (!Array.isArray(a.links)) throw new Error('links must be a list'); engine.setHyperlinks(page(a), String(a.id), a.links); return `${engine.hyperlinks(page(a), String(a.id)).length} link(s)`; } },
    { name: 'set_shape_data', description: 'Write a Shape Data row: name, value, optional label, type (0 text, 2 number, 3 flag, 5 date).', inputSchema: props({ id: { type: 'string' }, name: { type: 'string' }, value: { type: 'string' }, label: { type: 'string' }, type: { type: 'integer' }, ...P }, ['id', 'name', 'value']),
      run(a) { engine.setShapeData(page(a), String(a.id), { name: a.name, label: a.label || a.name, value: String(a.value), prompt: '', format: '', type: a.type ?? 0, invisible: false }); return 'ok'; } },
    { name: 'set_page_size', description: 'Set the size of a page in inches.', inputSchema: props({ width: { type: 'number' }, height: { type: 'number' }, ...P }, ['width', 'height']),
      run(a) { if (num(a, 'width') <= 0 || num(a, 'height') <= 0) throw new Error('the size must be positive'); engine.setPageSize(page(a), a.width, a.height); return 'ok'; } },
    { name: 'rename_page', description: 'Rename a page.', inputSchema: props({ name: { type: 'string' }, ...P }, ['name']),
      run(a) { engine.renamePage(page(a), String(a.name)); return 'ok'; } },
    { name: 'duplicate_page', description: 'Duplicate a page (it is inserted after the original).', inputSchema: props(P),
      run(a) { engine.duplicatePage(page(a)); return `pages: ${engine.pageCount()}`; } },
    { name: 'delete_page', description: 'Delete a page (the last page cannot be deleted).', inputSchema: props(P),
      run(a) { if (engine.pageCount() < 2) throw new Error('the last page cannot be deleted'); engine.deletePage(page(a)); return `pages: ${engine.pageCount()}`; } },
    { name: 'add_layer', description: 'Add a layer to a page.', inputSchema: props({ name: { type: 'string' }, ...P }, ['name']),
      run(a) { engine.addLayer(page(a), String(a.name)); return 'ok'; } },
    { name: 'list_layers', description: 'The layers of a page: index, name, visible, print, locked.', inputSchema: props(P),
      run(a) { return json(((engine.model().pages[page(a)] || {}).layers || []).map((l, index) => ({ index, name: l.name, visible: !!l.visible, print: l.print !== false, locked: !!l.locked }))); } },
    { name: 'set_layer', description: 'Change a layer (by name): rename it (newName), show / hide it (visible), print / do not print (print), lock / unlock (locked), colour it (color #rrggbb or none).', inputSchema: props({ layer: { type: 'string' }, newName: { type: 'string' }, visible: { type: 'boolean' }, print: { type: 'boolean' }, locked: { type: 'boolean' }, color: { type: 'string' }, ...P }, ['layer']),
      run(a) {
        const p = page(a), at = layerIndex(p, a.layer);
        const change = {}; for (const k of ['visible', 'print', 'locked']) if (a[k] !== undefined) change[k] = !!a[k];
        if (a.newName !== undefined) change.name = String(a.newName);
        if (Object.keys(change).length) engine.setLayer(p, at, change);
        if (a.color !== undefined) engine.setLayerColor(p, at, String(a.color));
        return 'ok';
      } },
    { name: 'assign_layer', description: 'Put shapes on a layer (member true, default) or take them off it (member false).', inputSchema: props({ ids: { type: 'array', items: { type: 'string' } }, layer: { type: 'string' }, member: { type: 'boolean' }, ...P }, ['ids', 'layer']),
      run(a) { const p = page(a); engine.layerMembers(p, ids(a), layerIndex(p, a.layer), a.member !== false); return 'ok'; } },
    { name: 'delete_layer', description: 'Delete a layer (its shapes stay and lose the layer).', inputSchema: props({ layer: { type: 'string' }, ...P }, ['layer']),
      run(a) { const p = page(a); engine.deleteLayer(p, layerIndex(p, a.layer)); return 'ok'; } },
    { name: 'shape_operation', description: 'Combine shapes into one geometry: union, combine, fragment, intersect, subtract, join or trim (the first shape is the one the others work on). Returns the result.', inputSchema: props({ how: { enum: ['union', 'combine', 'fragment', 'intersect', 'subtract', 'join', 'trim'] }, ids: { type: 'array', items: { type: 'string' } }, ...P }, ['how', 'ids']),
      run(a) { if (ids(a).length < 2) throw new Error('a shape operation needs at least two shapes'); return json(engine.shapeOperation(page(a), a.how, ids(a))); } },
    { name: 'check_diagram', description: 'Look for things that are usually mistakes on a page: loose connector ends, a second connector between the same two shapes in the same direction (duplicate connector), shapes that no connector touches, overlapping filled shapes and shapes outside the page. Returns [{kind, id, message}]; nothing is changed.', inputSchema: props({ altText: { type: 'boolean', description: 'also list shapes with neither text nor alt text' }, allPages: { type: 'boolean', description: 'check every page (each problem then has a page index)' }, csv: { type: 'string', description: 'optional path of a CSV file to write below the root folder' }, ...P }),
      run(a) {
        // one page (default page 0), or with allPages every page: each problem then carries its page index; with csv the list is also written as a CSV file (Page, Problem, Shape, Message, Fix)
        const pages = a.allPages ? [...Array(engine.pageCount()).keys()] : [page(a)];
        const model = engine.model();
        const issues = pages.flatMap((p) => checkDiagram(model, p, engine.pageSize(p), { altText: !!a.altText }).map((i) => (a.allPages ? { ...i, page: p } : i)));
        if (a.csv !== undefined) fs.writeFileSync(inside(a.csv), '\ufeff' + issuesToCsv(issues.map((i) => ({ ...i, page: i.page === undefined ? pages[0] : i.page })), model.pages.map((q, k) => q.name || `Page-${k + 1}`)));
        return json(issues);
      } },
    { name: 'compare_files', description: 'Compare two drawing files below the root folder (.vsdx, .drawio or .excalidraw) without touching the open drawing. `file` is the new one, `other` the old one: added = only in `file`, removed = only in `other`, changed = in both but different (text, alt text, position, size, fill, line, rotation). Pages are matched by name, shapes by id (ids of .drawio / .excalidraw files shift when shapes are added or removed, so compare those only for edits that keep the number of shapes). With `csv` the list is also written as a CSV file below the root folder. Returns [{page, pageName, kind, what, id, label, detail}].', inputSchema: props({ file: { type: 'string' }, other: { type: 'string' }, csv: { type: 'string', description: 'optional path of a CSV file to write' } }, ['file', 'other']),
      async run(a) {
        const read = async (p) => {
          const full = inside(p); const bytes = fs.readFileSync(full); const ext = path.extname(full).toLowerCase(); const e = await loadEngine(fs.readFileSync(wasm));
          if (ext === '.drawio') { e.newDrawing({ width: 11, height: 8.5 }); await importDrawio(e, new Uint8Array(bytes)); } else if (ext === '.excalidraw') { e.newDrawing({ width: 11, height: 8.5 }); importExcalidraw(e, bytes.toString('utf8')); } else e.import(new Uint8Array(bytes));
          return e.model();
        };
        const diffs = compareModels(await read(a.file), await read(a.other));
        if (a.csv !== undefined) fs.writeFileSync(inside(a.csv), '\ufeff' + diffsToCsv(diffs));
        return json(diffs);
      } },
    { name: 'inventory', description: 'The shapes as a table (the Report of the application): one row per shape of a page (every page with allPages) with Page, Shape ID, Text and each Shape Data row as a column. `columns` picks and orders them (keys: page, id, text, or a Shape Data name; default all). With `csv` the table is also written as a CSV file below the root folder. Returns {columns, rows} (rows are arrays in the order of the columns).', inputSchema: props({ allPages: { type: 'boolean' }, columns: { type: 'array', items: { type: 'string' } }, csv: { type: 'string', description: 'optional path of a CSV file to write below the root folder' }, ...P }),
      run(a) {
        const names = engine.pageNames(); const model = engine.model(); const p = a.allPages ? 0 : page(a);
        const { rows, dataCols } = inventoryData(model, names, p, !!a.allPages);
        const columns = a.columns === undefined ? ['page', 'id', 'text', ...dataCols.map((c) => c.key)] : a.columns.map(String);
        if (a.csv !== undefined) fs.writeFileSync(inside(a.csv), '\ufeff' + inventoryCsv(model, names, p, columns, !!a.allPages));
        const cell = (r, k) => (k === 'page' ? r.page : k === 'id' ? r.id : k === 'text' ? r.text : r.values[k] ?? '');
        return json({ columns: columns.map((k) => ({ key: k, label: ({ page: 'Page', id: 'Shape ID', text: 'Text' })[k] || dataCols.find((c) => c.key === k)?.label || k })), rows: rows.map((r) => columns.map((k) => cell(r, k))) });
      } },
    { name: 'find_shapes', description: 'Find shapes: `query` matches (case-insensitive, as a part of the text) the text, alt text, name or any Shape Data value of a shape; `dataName` (with an optional `dataValue`, exact, case-insensitive) matches shapes that have that Shape Data row. At least one of query and dataName is required; both must match when both are given. Searches one page (default 0), or every page with allPages. Returns the rows of list_shapes with the page index added.', inputSchema: props({ query: { type: 'string' }, dataName: { type: 'string' }, dataValue: { type: 'string' }, allPages: { type: 'boolean' }, ...P }),
      run(a) {
        if ((a.query === undefined || a.query === '') && a.dataName === undefined) throw new Error('give a query or a dataName');
        if (a.dataValue !== undefined && a.dataName === undefined) throw new Error('dataValue needs a dataName');
        const q = a.query === undefined ? '' : String(a.query).toLowerCase(); const same = (x, y) => String(x ?? '').toLowerCase() === String(y).toLowerCase();
        const pages = a.allPages ? [...Array(engine.pageCount()).keys()] : [page(a)]; const model = engine.model(); const out = [];
        for (const p of pages) {
          const h = engine.pageSize(p).h, names = engine.shapeNames(p);
          for (const x of model.pages[p]?.shapes || []) {
            const row = shapeRow(x, h, names); const data = x.data || [];
            const textHit = !q || [row.text, row.altText, row.name, ...data.map((d) => d.value)].some((v) => String(v ?? '').toLowerCase().includes(q));
            const dataHit = a.dataName === undefined || data.some((d) => same(d.name, a.dataName) && (a.dataValue === undefined || same(d.value, a.dataValue)));
            if (textHit && dataHit) out.push({ page: p, ...row });
          }
        }
        return json(out);
      } },
    { name: 'add_border', description: 'Design > Borders & Titles: put a border (an own design, not a Visio master) beneath a page as a background page: Frame, Double frame, Frame with title block, Title bar. The title is used by the styles that have a title area. Returns {backgroundPage: id}. One undo step. Note: the drawing model shows the background beneath a page as part of the page, so list_shapes then also lists the border shapes (and the title text).', inputSchema: props({ style: { enum: Object.keys(BORDER_STYLES), description: Object.entries(BORDER_STYLES).map(([k, v]) => `${k}: ${v}`).join('; ') }, title: { type: 'string' }, ...P }, ['style']),
      run(a) { if (!Object.hasOwn(BORDER_STYLES, a.style)) throw new Error(`unknown border style: ${a.style} (one of ${Object.keys(BORDER_STYLES).join(', ')})`); return json({ backgroundPage: applyBorder(engine, page(a), a.style, a.title === undefined ? '' : String(a.title)) }); } },
    { name: 'remove_border', description: 'Take the border (the background page) off a page; the background page stays in the drawing.', inputSchema: props(P),
      run(a) { removeBorder(engine, page(a)); return 'ok'; } },
    { name: 'get_page', description: 'Everything about one page in one answer: index, name, size (inches), number of shapes and connectors, the layers (name, visible, locked) and the background page assigned to it (id, name; null when none). Note: the shape count includes the shapes of an assigned background page (the model shows the background as part of the page).', inputSchema: props(P),
      run(a) {
        const p = page(a); const sz = engine.pageSize(p); const pg = engine.model().pages[p] || {}; const bg = engine.backgroundInfo(); const assigned = bg.assigned[p];
        return json({ index: p, name: engine.pageNames()[p], width: sz.w, height: sz.h, shapes: (pg.shapes || []).length, connectors: (pg.connectors || []).length, layers: (pg.layers || []).map((l) => ({ name: l.name, visible: l.visible !== false, locked: !!l.locked })), background: assigned ? { id: assigned, name: (bg.backgrounds.find((b) => b.id === assigned) || {}).name ?? null } : null });
      } },
    { name: 'insert_picture', description: 'Insert a picture file (PNG, JPEG, GIF or BMP, below the root folder) in the box from (x0, y0) to (x1, y1) inches. Returns the new shape id.', inputSchema: props({ path: { type: 'string' }, x0: { type: 'number' }, y0: { type: 'number' }, x1: { type: 'number' }, y1: { type: 'number' }, ...P }, ['path', 'x0', 'y0', 'x1', 'y1']),
      run(a) { return json({ id: engine.insertPicture(page(a), new Uint8Array(fs.readFileSync(inside(a.path))), num(a, 'x0'), num(a, 'y0'), num(a, 'x1'), num(a, 'y1')) }); } },
    { name: 'open_stencil', description: 'Open a stencil (.vssx / .vsdx file below the root folder), or the built-in Basic shapes stencil when no path is given. Returns the stencil number and its masters (id, name).', inputSchema: props({ path: { type: 'string' } }),
      run(a) {
        const st = a.path ? engine.openStencil(new Uint8Array(fs.readFileSync(inside(a.path)))) : engine.openBuiltinStencil();
        return json({ stencil: st.index, masters: (st.masters || []).map((m) => ({ id: m.id, name: m.name })) });
      } },
    { name: 'drop_master', description: 'Drop a master of an open stencil onto a page with its centre at (x, y) inches. Returns the new shape id.', inputSchema: props({ stencil: { type: 'integer', minimum: 0 }, master: { type: 'string' }, x: { type: 'number' }, y: { type: 'number' }, ...P }, ['stencil', 'master', 'x', 'y']),
      run(a) { return json({ id: engine.dropStencilMaster(page(a), a.stencil, String(a.master), num(a, 'x'), num(a, 'y')) }); } },
    { name: 'list_themes', description: 'The gallery themes (name, accent colours, fonts) and the current theme.', inputSchema: S,
      run() { const t = engine.themes(); return json({ current: t.current, variant: t.variant, themes: t.themes.map((x) => ({ name: x.name, accents: x.accents, fonts: x.fonts })) }); } },
    { name: 'apply_theme', description: 'Apply a gallery theme to the whole drawing, or to one page when page is given.', inputSchema: props({ name: { type: 'string' }, page: P.page }, ['name']),
      run(a) { if (a.page === undefined) engine.applyTheme(0, String(a.name)); else engine.applyThemeToPage(page(a), String(a.name)); return `theme ${engine.themes().current}`; } },
    { name: 'undo', description: 'Undo the last change.', inputSchema: S, run() { return engine.undo() ? 'undone' : 'nothing to undo'; } },
    { name: 'redo', description: 'Redo the last undone change.', inputSchema: S, run() { return engine.redo() ? 'redone' : 'nothing to redo'; } },
  ];
  const byName = new Map(tools.map((t) => [t.name, t]));

  async function callTool(name, args) {
    const tool = byName.get(name);
    if (!tool) return { isError: true, content: [{ type: 'text', text: `unknown tool ${name}` }] };
    try { return { content: [{ type: 'text', text: String(await tool.run(args || {})) }] }; }
    catch (e) { return { isError: true, content: [{ type: 'text', text: String(e && e.message || e) }] }; }
  }

  /** One JSON-RPC message in, the response (or undefined for a notification) out. */
  async function handle(msg) {
    const { id, method, params } = msg || {};
    const ok = (result) => ({ jsonrpc: '2.0', id, result });
    const err = (code, message) => ({ jsonrpc: '2.0', id: id ?? null, error: { code, message } });
    if (method === undefined) return id === undefined ? undefined : err(-32600, 'invalid request');
    if (id === undefined) return undefined; // notifications (initialized, cancelled, ...) need no answer
    switch (method) {
      case 'initialize': {
        const want = params && params.protocolVersion;
        return ok({ protocolVersion: PROTOCOLS.includes(want) ? want : PROTOCOLS[0], capabilities: { tools: { listChanged: false }, resources: { listChanged: false } }, serverInfo: { name: 'layoutcraft', version: '0.1.0' }, instructions: 'Coordinates are inches, the origin is the lower left corner of the page, y grows upward. Draw shapes with draw_shape, join them with add_connector, then save as .vsdx or export.' });
      }
      case 'ping': return ok({});
      case 'tools/list': return ok({ tools: tools.map(({ name, description, inputSchema }) => ({ name, description, inputSchema })) });
      case 'tools/call': {
        if (!params || typeof params.name !== 'string') return err(-32602, 'name is required');
        return ok(await callTool(params.name, params.arguments));
      }
      case 'resources/list': return ok({ resources: engine.pageNames().map((n, i) => ({ uri: `layoutcraft://page/${i}/svg`, name: `${n} (SVG)`, mimeType: 'image/svg+xml' })).concat([{ uri: 'layoutcraft://model', name: 'Drawing model (JSON)', mimeType: 'application/json' }]) });
      case 'resources/read': {
        const uri = params && params.uri; let m;
        if (uri === 'layoutcraft://model') return ok({ contents: [{ uri, mimeType: 'application/json', text: JSON.stringify(engine.model()) }] });
        if ((m = /^layoutcraft:\/\/page\/(\d+)\/svg$/.exec(uri || '')) && Number(m[1]) < engine.pageCount()) return ok({ contents: [{ uri, mimeType: 'image/svg+xml', text: pageSvg(Number(m[1])) }] });
        return err(-32002, `no such resource: ${uri}`);
      }
      case 'prompts/list': return ok({ prompts: [] });
      default: return err(-32601, `method not found: ${method}`);
    }
  }
  return { handle, tools, engine };
}

/** Reads one JSON message per line from `input`, writes the answers to `output`. */
export async function serve({ input = process.stdin, output = process.stdout, root } = {}) {
  const server = await createServer({ root });
  const rl = readline.createInterface({ input });
  const send = (m) => output.write(`${JSON.stringify(m)}\n`);
  let chain = Promise.resolve();
  rl.on('line', (line) => {
    if (!line.trim()) return;
    chain = chain.then(async () => {
      let msg;
      try { msg = JSON.parse(line); } catch { send({ jsonrpc: '2.0', id: null, error: { code: -32700, message: 'parse error' } }); return; }
      const batch = Array.isArray(msg) ? msg : [msg];
      for (const one of batch) { try { const r = await server.handle(one); if (r) send(r); } catch (e) { send({ jsonrpc: '2.0', id: one && one.id !== undefined ? one.id : null, error: { code: -32603, message: String(e && e.message || e) } }); } }
    });
  });
  await new Promise((done) => rl.on('close', done));
  await chain;
}

const real = (p) => { try { return fs.realpathSync(p); } catch { return path.resolve(p); } }; // a symlink or junction to the file still counts as running it
if (process.argv[1] && real(process.argv[1]) === real(SELF)) {
  const at = process.argv.indexOf('--root');
  serve({ root: at > 0 ? process.argv[at + 1] : process.cwd() }).catch((e) => { console.error(e); process.exit(1); });
}
