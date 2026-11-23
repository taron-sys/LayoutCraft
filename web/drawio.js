// draw.io / diagrams.net import (TODO I, step 302): a .drawio file (<mxfile> with one <diagram> per page, its content either a plain <mxGraphModel> or the compressed form
// base64(raw deflate(url-encoded XML))) or a bare <mxGraphModel> becomes a drawing in the engine, one page per diagram. Written from the public file format only.
// Pixels become inches (96 per inch), y is flipped. Mapped: vertices (rectangle, rounded, ellipse, rhombus, triangle, text), edges (glued connectors, or polylines through their
// waypoints when they have waypoints or a loose end), labels (HTML stripped to text), fill / stroke colour, stroke width, dashed, opacity, rotation, font size / colour, arrowheads,
// containers (children use the parent's origin), layers (a hidden layer and hidden cells are not drawn). Other built-in shapes are drawn as rectangles and listed in `approximated`.
import { parseXmlTree } from './svgpdf.js';
import { shapeGeometry, canonicalName } from './drawio-shapes.js';
import { arrowIndex, arrowSize, sideOf, orthogonalize, exitHorizontal, curveThrough, labelPosition } from './drawio-edges.js';

const PX_PER_IN = 96;
const BASIC = { ellipse: 2, rhombus: 3, triangle: 4 };
const KNOWN_APPROX = new Set(['cylinder', 'cylinder3', 'cloud', 'hexagon', 'parallelogram', 'trapezoid', 'process', 'document', 'actor', 'step', 'callout', 'note', 'card', 'tape', 'folder', 'umlActor', 'swimlane', 'datastore', 'display', 'delay', 'or', 'xor', 'curlyBracket', 'singleArrow', 'doubleArrow', 'image', 'label', 'mxgraph']);
const ARROWS = { classic: 13, classicThin: 13, block: 13, blockThin: 13, open: 13, openThin: 13, oval: 10, diamond: 15, diamondThin: 15, dash: 6, ERone: 6 };

const child = (node, tag) => node.children.filter((c) => c.tag === tag);
const first = (node, tag) => child(node, tag)[0];
const numberOf = (v, d = 0) => { const n = parseFloat(v); return Number.isFinite(n) ? n : d; };

function colour(c) {
  if (typeof c !== 'string') return null;
  if (/^#[0-9a-f]{6}$/i.test(c)) return c.toLowerCase();
  if (/^#[0-9a-f]{3}$/i.test(c)) return '#' + c.slice(1).split('').map((d) => d + d).join('').toLowerCase();
  return null;
}

/** "a=b;c=d;ellipse" -> {a: 'b', c: 'd', shape0: 'ellipse'} (a bare first token is the shape's style name). */
export function parseStyle(text) {
  const out = {};
  for (const [i, part] of String(text || '').split(';').entries()) {
    if (!part) continue;
    const eq = part.indexOf('=');
    if (eq < 0) { if (i === 0 || !out.name) out.name = part; continue; }
    out[part.slice(0, eq)] = part.slice(eq + 1);
  }
  return out;
}

const ENTITIES = { nbsp: ' ', lt: '<', gt: '>', quot: '"', apos: "'", amp: '&' };
const decodeEntities = (t) => t.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (all, e) => (e[0] === '#' ? String.fromCodePoint(e[1] === 'x' || e[1] === 'X' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)) : ENTITIES[e.toLowerCase()] ?? all));

/**
 * An HTML label (html=1) as plain text plus formatted runs: {text, runs: [{start, end, bold, italic, underline, color, sizePx, family}]}. <br> and block ends are line breaks; <b> / <strong>,
 * <i> / <em>, <u>, <font color size face> and <span style="color; font-size; font-weight; font-style; text-decoration; font-family"> format the text inside them (nested tags add up).
 */
export function labelRuns(value) {
  const src = String(value ?? '');
  let text = '';
  const runs = [];
  const stack = [{}];
  const emit = (chunk) => {
    if (!chunk) return;
    const st = stack[stack.length - 1];
    const start = text.length;
    text += chunk;
    if (st.bold || st.italic || st.underline || st.color || st.sizePx || st.family) runs.push({ start, end: text.length, bold: !!st.bold, italic: !!st.italic, underline: !!st.underline, color: st.color, sizePx: st.sizePx, family: st.family });
  };
  const FONT_SIZES = { 1: 10, 2: 13, 3: 16, 4: 18, 5: 24, 6: 32, 7: 48 };
  for (const m of src.matchAll(/<(\/?)([a-z0-9]+)((?:\s+[^>]*?)?)\s*(\/?)>|([^<]+)|</gi)) {
    if (m[5] !== undefined) { emit(decodeEntities(m[5])); continue; }
    if (m[2] === undefined) { emit('<'); continue; }
    const tag = m[2].toLowerCase(), closing = m[1] === '/', attrs = m[3] || '';
    if (tag === 'br') { emit('\n'); continue; }
    if (['div', 'p', 'li', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6'].includes(tag)) { if (closing) emit('\n'); continue; }
    if (closing) { if (stack.length > 1) stack.pop(); continue; }
    if (m[4] === '/') continue;
    const next = { ...stack[stack.length - 1] };
    if (tag === 'b' || tag === 'strong') next.bold = true;
    else if (tag === 'i' || tag === 'em') next.italic = true;
    else if (tag === 'u') next.underline = true;
    else if (tag === 'font' || tag === 'span') {
      const get = (n) => { const q = attrs.match(new RegExp(`${n}\\s*=\\s*"([^"]*)"|${n}\\s*=\\s*'([^']*)'`, 'i')); return q ? q[1] ?? q[2] : null; };
      const c = colour(get('color')); if (c) next.color = c;
      const sz = get('size'); if (sz && FONT_SIZES[sz]) next.sizePx = FONT_SIZES[sz];
      const face = get('face'); if (face) next.family = face.split(',')[0].trim();
      const css = get('style') || '';
      for (const decl of css.split(';')) {
        const [k, v] = decl.split(':').map((q) => (q || '').trim().toLowerCase());
        if (k === 'color') { const cc = colour(v) || null; if (cc) next.color = cc; }
        else if (k === 'font-size') { const n = parseFloat(v); if (n > 0) next.sizePx = v.endsWith('pt') ? n / 0.75 : n; }
        else if (k === 'font-weight' && (v === 'bold' || parseInt(v, 10) >= 600)) next.bold = true;
        else if (k === 'font-style' && v === 'italic') next.italic = true;
        else if (k === 'text-decoration' && v.includes('underline')) next.underline = true;
        else if (k === 'font-family' && v) next.family = v.split(',')[0].replace(/['"]/g, '').trim();
      }
    }
    stack.push(next);
  }
  text = text.replace(/\n+$/, '');
  return { text, runs: runs.filter((r) => r.start < text.length).map((r) => ({ ...r, end: Math.min(r.end, text.length) })) };
}

/** The text of a label: HTML (html=1) loses its tags, <br> and block ends become line breaks, entities are decoded. */
export function labelText(value, html) {
  return html ? labelRuns(value).text : String(value ?? '');
}

async function inflateRaw(bytes) {
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** The <mxGraphModel> nodes of a file, one per diagram: [{name, model}]. Compressed diagrams are inflated here. Throws a plain Error for anything else. */
export async function readDiagrams(input) {
  const source = typeof input === 'string' ? input : new TextDecoder().decode(input);
  if (!/<(mxfile|mxGraphModel)[\s>]/.test(source)) throw new Error('This is not a draw.io file (no mxfile or mxGraphModel)');
  const tree = parseXmlTree(source);
  const top = tree.children.find((c) => c.tag === 'mxfile' || c.tag === 'mxGraphModel');
  if (!top) throw new Error('This is not a draw.io file (the XML cannot be read)');
  if (top.tag === 'mxGraphModel') return [{ name: 'Page-1', model: top }];
  const out = [];
  for (const d of child(top, 'diagram')) {
    let model = first(d, 'mxGraphModel');
    if (!model) {
      const packed = d.children.map((c) => c.text || '').join('').trim();
      if (!packed) continue;
      let xml;
      try {
        const bin = atob(packed.replace(/\s+/g, ''));
        const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
        xml = decodeURIComponent(new TextDecoder().decode(await inflateRaw(bytes)));
      } catch { throw new Error(`The diagram "${d.attrs.name || ''}" is compressed in a way that cannot be read`); }
      model = parseXmlTree(xml).children.find((c) => c.tag === 'mxGraphModel');
      if (!model) throw new Error(`The diagram "${d.attrs.name || ''}" has no model`);
    }
    out.push({ name: d.attrs.name || `Page-${out.length + 1}`, model });
  }
  if (!out.length) throw new Error('This draw.io file has no diagram');
  return out;
}

/**
 * Build the drawing in `engine` (a new document; one page per diagram). Returns {pages, shapes, connectors, approximated: {shapeName: count}, skipped: {what: count}, map: [{cellId: shapeId}]}.
 */
/** The bytes of a picture style `image=data:image/png,BASE64` (draw.io leaves out ";base64" because ";" separates the style entries, so the long form cannot occur in a style), or null (a web address cannot be fetched, other types are not pictures we can place). */
function pictureOf(spec, skip) {
  if (!spec) return null;
  const m = /^data:image\/(png|jpe?g|gif|bmp),(.+)$/is.exec(spec);
  if (!m) { skip(/^https?:/i.test(spec) ? 'image from a web address' : 'image of an unsupported kind'); return null; }
  try { const bin = atob(m[2].replace(/\s+/g, '')); const bytes = new Uint8Array(bin.length); for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i); return bytes; } catch { skip('image that cannot be read'); return null; }
}

export async function importDrawio(engine, input, { margin = 0.5 } = {}) {
  const diagrams = await readDiagrams(input);
  const approximated = {}, skipped = {}, maps = [];
  let shapes = 0, connectors = 0;
  for (const [pi, { name, model }] of diagrams.entries()) {
    const root = first(model, 'root');
    const cells = root ? child(root, 'mxCell').concat(child(root, 'object').map((o) => { const c = first(o, 'mxCell'); return c ? { ...c, attrs: { ...c.attrs, id: o.attrs.id, value: o.attrs.label ?? o.attrs.value ?? c.attrs.value } } : null; }).filter(Boolean)) : [];
    const byId = Object.fromEntries(cells.map((c) => [c.attrs.id, c]));
    const geo = (c) => first(c, 'mxGeometry');
    // layers (step 327): the cells whose parent is the root (0). With two or more they become the page's layers; a hidden layer is imported as a hidden layer, not dropped
    const layerCells = cells.filter((c) => c.attrs.parent === '0' && c.attrs.vertex !== '1' && c.attrs.edge !== '1' && c.attrs.id !== '0');
    const layerOf = (c) => { let k = c; for (let n = 0; k && n < 50; n++) { if (k.attrs.parent === '0') return k.attrs.id; k = byId[k.attrs.parent]; } return null; };
    const hidden = (c) => { for (let k = c, n = 0; k && n < 50; k = byId[k.attrs.parent], n++) if (k.attrs.visible === '0' && (layerCells.length < 2 || k.attrs.parent !== '0')) return true; return false; };
    const origin = (c) => { // the absolute top left of a vertex: its own geometry plus those of its container ancestors
      let x = 0, y = 0;
      for (let k = c, n = 0; k && n < 50; k = byId[k.attrs.parent], n++) { const g = k.attrs.vertex === '1' ? geo(k) : null; if (g) { x += numberOf(g.attrs.x); y += numberOf(g.attrs.y); } }
      return [x, y];
    };
    const vertices = cells.filter((c) => c.attrs.vertex === '1' && geo(c) && !hidden(c));
    const edges = cells.filter((c) => c.attrs.edge === '1' && !hidden(c));
    const box = (c) => { const [x, y] = origin(c), g = geo(c); return { x, y, w: numberOf(g.attrs.width), h: numberOf(g.attrs.height) }; };
    const centre = (c) => { const b = box(c); return [b.x + b.w / 2, b.y + b.h / 2]; };
    const pointOf = (g, role) => { const p = g.children.find((n) => n.tag === 'mxPoint' && n.attrs.as === role); return p ? [numberOf(p.attrs.x), numberOf(p.attrs.y)] : null; };
    const edgePoints = (e) => {
      const g = geo(e) || { children: [] }, pts = [];
      const s = byId[e.attrs.source], t = byId[e.attrs.target];
      pts.push(pointOf(g, 'sourcePoint') || (s && geo(s) ? centre(s) : null));
      const arr = g.children.find((n) => n.tag === 'Array' && n.attrs.as === 'points');
      if (arr) for (const p of arr.children.filter((n) => n.tag === 'mxPoint')) pts.push([numberOf(p.attrs.x), numberOf(p.attrs.y)]);
      pts.push(pointOf(g, 'targetPoint') || (t && geo(t) ? centre(t) : null));
      return pts.filter(Boolean);
    };
    // page size from every vertex box and edge point
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    const add = (x, y) => { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); };
    for (const v of vertices) { const b = box(v); add(b.x, b.y); add(b.x + b.w, b.y + b.h); }
    for (const e of edges) for (const p of edgePoints(e)) add(p[0], p[1]);
    if (!Number.isFinite(x0)) { x0 = y0 = 0; x1 = y1 = 0; }
    const width = Math.max(2, Math.round(((x1 - x0) / PX_PER_IN + 2 * margin) * 1000) / 1000), height = Math.max(2, Math.round(((y1 - y0) / PX_PER_IN + 2 * margin) * 1000) / 1000);
    if (pi === 0) engine.newDrawing({ width, height, metric: false, name });
    else { engine.addPage(pi - 1, name); engine.setPageSize(pi, width, height); }
    const X = (x) => (x - x0) / PX_PER_IN + margin, Y = (y) => height - ((y - y0) / PX_PER_IN + margin);
    const map = {}; maps.push(map);
    const skip = (what) => { skipped[what] = (skipped[what] || 0) + 1; };

    const paint = (id, st, { vertex }) => {
      const s = {};
      const text = st.name === 'text' || st.shape === 'text';
      const stroke = st.strokeColor === 'none' ? 'none' : colour(st.strokeColor) || (text && vertex ? 'none' : '#000000');
      const fill = st.fillColor === 'none' ? 'none' : colour(st.fillColor) || (text || !vertex ? 'none' : '#ffffff');
      s.line = stroke; s.fill = fill;
      if (st.strokeWidth) s.weight = numberOf(st.strokeWidth, 1);
      if (st.dashed === '1') s.pattern = 2;
      const op = numberOf(st.opacity, 100);
      if (op < 100) { s.lineTrans = Math.round((1 - op / 100) * 100) / 100; if (fill !== 'none') s.fillTrans = s.lineTrans; }
      // gradient (gradientColor, gradientDirection: south = from the fill colour at the top to the gradient colour at the bottom) and shadow
      const gc = colour(st.gradientColor);
      if (gc && fill !== 'none' && colour(st.fillColor)) { s.gradientStops = [[0, fill, 0], [1, gc, 0]]; s.gradientAngle = { south: 90, north: 270, east: 0, west: 180 }[st.gradientDirection || 'south'] ?? 90; }
      if (st.shadow === '1') s.shadow = true;
      return { id, style: s, text };
    };
    const writeLabel = (id, st, rawValue) => {
      const html = st.html === '1';
      const { text, runs } = html ? labelRuns(rawValue) : { text: String(rawValue ?? ''), runs: [] };
      if (!text) return;
      engine.setText(pi, id, text);
      engine.styleShapes(pi, [id], font(st));
      for (const r of runs) {
        const cells = {};
        if (r.color) cells.Color = r.color;
        if (r.sizePx) cells.Size = String((r.sizePx * 0.75) / 72);
        if (r.family) cells.Font = r.family;
        const spec = { start: r.start, end: r.end, set: (r.bold ? 1 : 0) | (r.italic ? 2 : 0) | (r.underline ? 4 : 0) };
        if (Object.keys(cells).length) spec.cells = cells;
        try { engine.formatText(pi, id, spec); } catch { skip('label run format'); }
      }
      const al = { left: '0', center: '1', right: '2' }[st.align];
      if (al !== undefined) { try { engine.formatText(pi, id, { start: 0, end: text.length, para: { HorzAlign: al } }); } catch { skip('label alignment'); } }
    };
    const font = (st) => ({ fontPt: Math.round(numberOf(st.fontSize, 12) * 0.75 * 10) / 10, textColor: colour(st.fontColor) || '#000000', bold: (numberOf(st.fontStyle) & 1) === 1 || undefined, italic: (numberOf(st.fontStyle) & 2) === 2 || undefined });

    for (const v of vertices) {
      const st = parseStyle(v.attrs.style), b = box(v);
      const shapeName = st.shape || st.name || '';
      const canon = canonicalName(shapeName);
      const isText = canon === 'text';
      const img = canon === 'image' || st.image ? pictureOf(st.image, skip) : null; // a picture of the cell (a data: URI in the style)
      let kind = isText ? 5 : BASIC[canon] ?? (st.rounded === '1' || canon === 'rounded' ? 1 : 0);
      const geo = isText ? null : shapeGeometry(shapeName, st, b.w, b.h);
      if (!geo && !isText && !img && !(canon in BASIC) && canon && canon !== 'rect' && canon !== 'rounded' && !/^(whiteSpace|html)$/.test(canon)) approximated[shapeName] = (approximated[shapeName] || 0) + 1;
      const at = (pt) => [X(b.x + pt[0]), Y(b.y + pt[1])];
      let id;
      const extras = [];
      const p = paint(0, st, { vertex: true });
      const label = labelText(v.attrs.value, st.html === '1');
      if (img) { try { id = engine.insertPicture(pi, img, X(b.x), Y(b.y + b.h), X(b.x + b.w), Y(b.y)); } catch { skip('image that cannot be read'); } }
      if (id !== undefined) {
        if (label) writeLabel(id, st, v.attrs.value);
      } else if (geo && canon === 'swimlane') {
        // a container: the body (no fill unless swimlaneFillColor) and a header bar that carries the label
        const hs = Math.max(8, Math.min(numberOf(st.startSize, 23), b.h * 0.9));
        id = engine.drawShape(pi, 0, X(b.x), Y(b.y + b.h), X(b.x + b.w), Y(b.y));
        engine.styleShapes(pi, [id], { ...p.style, fill: colour(st.swimlaneFillColor) || 'none' });
        const header = engine.drawShape(pi, 0, X(b.x), Y(b.y + hs), X(b.x + b.w), Y(b.y));
        engine.styleShapes(pi, [header], p.style);
        writeLabel(header, st, v.attrs.value);
        extras.push(header);
      } else if (geo) {
        id = geo.invisibleMain ? engine.drawShape(pi, 0, X(b.x), Y(b.y + b.h), X(b.x + b.w), Y(b.y)) : engine.drawPolyline(pi, geo.outline.map(at), true);
        engine.styleShapes(pi, [id], geo.invisibleMain ? { ...p.style, fill: 'none', line: 'none' } : p.style);
        for (const d of geo.details || []) {
          // a perfectly vertical line has no width and is lost by the freeform tool: lean it by half a pixel (invisible)
          const dpts = d.pts.map(at);
          if (dpts.every((q) => Math.abs(q[0] - dpts[0][0]) < 1e-9)) dpts[dpts.length - 1][0] += 0.5 / PX_PER_IN;
          const did = engine.drawPolyline(pi, dpts, !!d.closed);
          engine.styleShapes(pi, [did], { ...p.style, fill: d.closed ? p.style.fill : 'none', gradientStops: undefined });
          extras.push(did);
        }
        writeLabel(id, st, v.attrs.value);
      } else {
        id = engine.drawShape(pi, kind, X(b.x), Y(b.y + b.h), X(b.x + b.w), Y(b.y));
        engine.styleShapes(pi, [id], p.style);
        writeLabel(id, st, v.attrs.value);
      }
      let top = id;
      if (extras.length) top = engine.groupShapes(pi, [id, ...extras]); // the details move with the shape
      map[v.attrs.id] = top; shapes++;
      const rot = numberOf(st.rotation);
      if (rot) engine.rotateShape(pi, top, (((-rot * Math.PI) / 180) % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI));
    }
    for (const e of edges) {
      const st = parseStyle(e.attrs.style);
      const s = map[e.attrs.source], t = map[e.attrs.target];
      const g = geo(e);
      const waypoints = g && g.children.some((n) => n.tag === 'Array' && n.attrs.as === 'points' && n.children.length);
      const curved = st.curved === '1';
      const orthogonal = /orthogonal|elbow|entityRelation/.test(st.edgeStyle || '');
      const frac = (k) => (st[k] === undefined || !Number.isFinite(parseFloat(st[k])) ? undefined : parseFloat(st[k]));
      // the connection point (side) the edge leaves / enters by (exitX / exitY, entryX / entryY); shapes without the four standard points (drawn outlines) are glued as a whole
      const sideN = (shapeId, fx, fy) => {
        if (fx === undefined || fy === undefined) return 0;
        try { return engine.connectionPoints(pi, shapeId).length >= 4 ? sideOf(fx, fy) : 0; } catch { return 0; }
      };
      let id, routePx = null;
      if (s && t && s !== t && !waypoints) {
        id = engine.addConnector(pi, { id: s, n: sideN(s, frac('exitX'), frac('exitY')) }, { id: t, n: sideN(t, frac('entryX'), frac('entryY')) });
        connectors++;
      } else {
        let ptsPx = edgePoints(e);
        if (ptsPx.length < 2) { skip('edge without two ends'); continue; }
        if (orthogonal && !curved) ptsPx = orthogonalize(ptsPx, exitHorizontal(frac('exitX'), frac('exitY')));
        const pts = ptsPx.map((q) => [X(q[0]), Y(q[1])]);
        if (curved) { try { id = engine.drawCurve(pi, curveThrough(pts)); } catch { id = engine.drawPolyline(pi, pts, false); } }
        else id = engine.drawPolyline(pi, pts, false);
        routePx = ptsPx;
      }
      map[e.attrs.id] = id; shapes++;
      const style = paint(id, st, { vertex: false }).style;
      style.fill = 'none';
      // arrowheads: draw.io's endArrow defaults to classic, startArrow to none; endFill / startFill = 0 are the hollow forms; endSize / startSize
      const endName = st.endArrow === undefined ? 'classic' : st.endArrow;
      let end = arrowIndex(endName, st.endFill !== '0'), begin = arrowIndex(st.startArrow, st.startFill !== '0');
      if (end === null) { end = 4; skip('arrow type'); }
      if (begin === null) { begin = 4; skip('arrow type'); }
      if (end) { style.endArrow = end; style.endArrowSize = arrowSize(st.endSize ?? 6); }
      if (begin) { style.beginArrow = begin; style.beginArrowSize = arrowSize(st.startSize ?? 6); }
      engine.styleShapes(pi, [id], style);
      // the label: at the middle of the edge, or (mxGeometry x / y / offset) at its own place as a text box
      const labelValue = e.attrs.value;
      const gx = g ? numberOf(g.attrs.x) : 0, gy = g ? numberOf(g.attrs.y) : 0;
      const offPoint = g && g.children.find((n) => n.tag === 'mxPoint' && n.attrs.as === 'offset');
      const off = offPoint ? [numberOf(offPoint.attrs.x), numberOf(offPoint.attrs.y)] : [0, 0];
      const text = labelText(labelValue, st.html === '1');
      if (text && (gx !== 0 || gy !== 0 || off[0] !== 0 || off[1] !== 0)) {
        try {
          if (!routePx) { // a glued connector: its route as the engine made it (inches, y up) back to draw.io pixels
            const c = engine.connectors(pi).find((q) => q.id === id);
            routePx = c && c.route.length ? c.route.map(([rx, ry]) => [(rx - margin) * PX_PER_IN + x0, (height - ry - margin) * PX_PER_IN + y0]) : [[x0, y0], [x0 + 1, y0]];
          }
          const [lx, ly] = labelPosition(routePx, gx, gy, off);
          const fs = numberOf(st.fontSize, 12), lines = text.split('\n');
          const lw = Math.max(...lines.map((l) => l.length)) * fs * 0.6 + 8, lh = lines.length * fs * 1.25 + 4;
          const box = engine.drawShape(pi, 5, X(lx - lw / 2), Y(ly + lh / 2), X(lx + lw / 2), Y(ly - lh / 2));
          engine.styleShapes(pi, [box], { fill: 'none', line: 'none' });
          writeLabel(box, st, labelValue);
          shapes++;
        } catch { skip('edge label'); }
      } else { try { writeLabel(id, st, labelValue); } catch { skip('edge label'); } }
    }
    // the layers: one engine layer per layer cell, the shapes and edges on the layer of their cell, hidden / locked layers as such
    if (layerCells.length >= 2) {
      const index = {};
      layerCells.forEach((lc, k) => { engine.addLayer(pi, String(lc.attrs.value || `Layer ${k + 1}`)); index[lc.attrs.id] = k; });
      const byLayer = {};
      for (const [cellId, shapeId] of Object.entries(map)) { const c = byId[cellId]; const lid = c && layerOf(c); if (lid !== null && lid in index) (byLayer[lid] ||= []).push(shapeId); }
      for (const [lid, list] of Object.entries(byLayer)) { try { engine.layerMembers(pi, list, index[lid], true); } catch { skip('layer membership'); } }
      for (const lc of layerCells) {
        const change = {};
        if (lc.attrs.visible === '0') change.visible = false;
        if (parseStyle(lc.attrs.style).locked === '1') change.locked = true;
        if (Object.keys(change).length) { try { engine.setLayer(pi, index[lc.attrs.id], change); } catch { skip('layer state'); } }
      }
    }
  }
  return { pages: diagrams.length, shapes, connectors, approximated, skipped, map: maps };
}
