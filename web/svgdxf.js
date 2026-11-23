// The shell's "Export DXF": the page's SVG (the engine's own drawing, the same one the PNG / PDF export use) becomes an ASCII DXF of release 12 (AC1009), the version every CAD
// program and Visio read: outlines as POLYLINE / VERTEX / SEQEND (curves flattened), circles as CIRCLE, text as TEXT. Units are inches, y points up (the SVG's y points down), one layer.
// Fills, gradients, shadows and images are not exported (DXF has no fill without HATCH): a shape is its outline, drawn in its line colour (its fill colour when it has no line).
import { parseXml, parseTransform, mul, parseColour, arcToBeziers, IDENT, Unsupported } from './svgpdf.js';

const f = (n) => { const r = Math.round(n * 1e6) / 1e6; return Object.is(r, -0) ? '0' : String(r); };
const INHERITED = ['fill', 'stroke', 'stroke-width', 'font-size', 'font-family', 'text-anchor'];

/** The nearest of the AutoCAD colour indexes 1..7 (and the greys) to an [r, g, b] colour (0..1). */
function aci([r, g, b]) {
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  if (max - min < 0.2) return max > 0.85 ? 7 : max < 0.12 ? 7 : 8; // white / black are both 7 (shown dark on a light screen, light on a dark one), mid greys 8
  const hue = (() => {
    const d = max - min;
    let h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h *= 60; return h < 0 ? h + 360 : h;
  })();
  const slots = [[0, 1], [60, 2], [120, 3], [180, 4], [240, 5], [300, 6], [360, 1]];
  return slots.reduce((best, s) => (Math.abs(s[0] - hue) < Math.abs(best[0] - hue) ? s : best), slots[0])[1];
}

/** Flatten SVG path data into subpaths: [{pts: [[x, y]...], closed}] in the path's own coordinates. */
function flattenPath(d) {
  const tokens = d.match(/[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g) || [];
  const ARGS = { M: 2, L: 2, H: 1, V: 1, C: 6, S: 4, Q: 4, T: 2, A: 7, Z: 0 };
  const subs = [];
  let cur = null, i = 0, cmd = '', x = 0, y = 0, sx = 0, sy = 0, lc = null, lq = null;
  const take = (k) => { const v = tokens.slice(i, i + k).map(Number); if (v.length < k || v.some((t) => !Number.isFinite(t))) throw new Unsupported('path data'); i += k; return v; };
  const add = (px, py) => { if (!cur) { cur = { pts: [[x, y]], closed: false }; subs.push(cur); } cur.pts.push([px, py]); x = px; y = py; };
  const cubic = (x1, y1, x2, y2, x3, y3) => {
    const [x0, y0] = [x, y];
    for (let k = 1; k <= 16; k++) {
      const t = k / 16, u = 1 - t;
      add(u * u * u * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3, u * u * u * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3);
    }
    lc = [x2, y2]; lq = null;
  };
  while (i < tokens.length) {
    if (/[A-Za-z]/.test(tokens[i])) cmd = tokens[i++];
    else if (cmd === 'M') cmd = 'L'; else if (cmd === 'm') cmd = 'l';
    const up = cmd.toUpperCase(), rel = cmd !== up;
    if (!(up in ARGS)) throw new Unsupported(`path command ${cmd}`);
    const a = take(ARGS[up]);
    const ox = rel ? x : 0, oy = rel ? y : 0;
    switch (up) {
      case 'M': x = a[0] + ox; y = a[1] + oy; sx = x; sy = y; cur = null; lc = lq = null; break;
      case 'L': add(a[0] + ox, a[1] + oy); lc = lq = null; break;
      case 'H': add(a[0] + ox, y); lc = lq = null; break;
      case 'V': add(x, a[0] + oy); lc = lq = null; break;
      case 'C': cubic(a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy, a[4] + ox, a[5] + oy); break;
      case 'S': { const c1 = lc ? [2 * x - lc[0], 2 * y - lc[1]] : [x, y]; cubic(c1[0], c1[1], a[0] + ox, a[1] + oy, a[2] + ox, a[3] + oy); break; }
      case 'Q': case 'T': {
        const q = up === 'Q' ? [a[0] + ox, a[1] + oy] : (lq ? [2 * x - lq[0], 2 * y - lq[1]] : [x, y]);
        const e = up === 'Q' ? [a[2] + ox, a[3] + oy] : [a[0] + ox, a[1] + oy];
        cubic(x + (2 / 3) * (q[0] - x), y + (2 / 3) * (q[1] - y), e[0] + (2 / 3) * (q[0] - e[0]), e[1] + (2 / 3) * (q[1] - e[1]), e[0], e[1]);
        lq = q; break;
      }
      case 'A': {
        const ex = a[5] + ox, ey = a[6] + oy;
        for (const seg of arcToBeziers(x, y, a[0], a[1], a[2], a[3] !== 0, a[4] !== 0, ex, ey)) {
          if (seg[0] === 'L') add(seg[1], seg[2]); else cubic(seg[1], seg[2], seg[3], seg[4], seg[5], seg[6]);
        }
        add(ex, ey); lc = lq = null; break;
      }
      case 'Z': if (cur) { cur.closed = true; const a0 = cur.pts[0], z0 = cur.pts[cur.pts.length - 1]; if (cur.pts.length > 2 && Math.abs(a0[0] - z0[0]) < 1e-9 && Math.abs(a0[1] - z0[1]) < 1e-9) cur.pts.pop(); } x = sx; y = sy; cur = null; lc = lq = null; break;
      default: break;
    }
  }
  return subs.filter((s) => s.pts.length >= 2);
}
const ellipsePoints = (cx, cy, rx, ry) => Array.from({ length: 48 }, (_, k) => { const t = (Math.PI * 2 * k) / 48; return [cx + rx * Math.cos(t), cy + ry * Math.sin(t)]; });

/**
 * svgText: the SVG of one page (viewBox in px, `pxPerInch` px per inch, `heightIn` the page height in inches). Returns {dxf, polylines, circles, texts}.
 * `layers` (step 307): {shapeId: layer name} for the `<g data-vid>` shapes; the entities of a shape go on its DXF layer (the name is made DXF safe: capitals, digits, $ _ -, at most 31
 * characters) and the LAYER table lists the layers used with `layerFlags` ({name: 1 frozen | 4 locked}). Line weight: a stroked outline gets the polyline's start and end width
 * (group codes 40 / 41, inches) = the stroke width; CAD programs draw it, a program that ignores widths draws a thin line.
 * Throws Unsupported for markup the engine does not write.
 */
export function svgToDxf(svgText, { pxPerInch = 96, heightIn = null, layers = {}, layerFlags = {} } = {}) {
  const root = parseXml(svgText);
  if (!root) throw new Unsupported('no svg element');
  const vb = (root.attrs.viewBox || '').split(/[\s,]+/).map(Number);
  if (vb.length !== 4 || vb.some((v) => !Number.isFinite(v))) throw new Unsupported('viewBox');
  // SVG px -> DXF inches in PAGE coordinates, y up: x' = x / ppi, y' = pageHeight - y / ppi (the viewBox may be cropped to the content: its origin does not matter)
  const base = [1 / pxPerInch, 0, 0, -1 / pxPerInch, 0, heightIn ?? (vb[1] + vb[3]) / pxPerInch];
  const out = [];
  let box = [Infinity, Infinity, -Infinity, -Infinity];
  const stat = { polylines: 0, circles: 0, texts: 0 };
  const usedLayers = new Map(); // DXF layer name -> flags
  const dxfName = (name) => { const t = String(name || '').toUpperCase().replace(/[^A-Z0-9$_-]/g, '_').slice(0, 31); return t || '0'; };
  const layerOf = (st) => { const n = st.dxfLayer || '0'; if (n !== '0' && !usedLayers.has(n)) usedLayers.set(n, layerFlags[st.dxfLayerRaw] ?? 0); return n; };
  const grow = (x, y) => { box = [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)]; };
  const put = (code, value) => { out.push(String(code), String(value)); };
  const colourOf = (st, outline) => {
    const v = outline ? (st.stroke && st.stroke !== 'none' && st.stroke !== 'transparent' ? st.stroke : st.fill) : st.fill;
    if (!v || v === 'none' || v === 'transparent' || /^url\(/.test(v)) return 7;
    try { return aci(parseColour(v)); } catch { return 7; }
  };
  const polyline = (pts, closed, colour, m, st = {}) => {
    if (pts.length < 2) return;
    const layer = layerOf(st);
    const sw = Number(st['stroke-width'] ?? 0), strokeOn = st.stroke && st.stroke !== 'none' && st.stroke !== 'transparent';
    const width = strokeOn && Number.isFinite(sw) && sw > 0 ? sw * Math.hypot(m[0], m[1]) : 0;
    put(0, 'POLYLINE'); put(8, layer); put(62, colour); put(66, 1); put(70, closed ? 1 : 0); put(10, 0); put(20, 0); put(30, 0);
    if (width > 0.0005) { put(40, f(width)); put(41, f(width)); }
    for (const [px, py] of pts) {
      const x = px * m[0] + py * m[2] + m[4], y = px * m[1] + py * m[3] + m[5];
      grow(x, y);
      put(0, 'VERTEX'); put(8, layer); put(10, f(x)); put(20, f(y)); put(30, 0);
    }
    put(0, 'SEQEND'); put(8, layer);
    stat.polylines++;
  };
  const visible = (st, canFill) => (st.stroke && st.stroke !== 'none' && st.stroke !== 'transparent') || (canFill && st.fill && st.fill !== 'none' && st.fill !== 'transparent');
  const walk = (node, parent, ctm) => {
    if (!node.tag) return;
    if (['title', 'desc', 'defs', 'linearGradient', 'radialGradient', 'stop', 'filter', 'clipPath', 'mask', 'image', 'style', 'pattern', 'marker'].includes(node.tag)) return;
    if (node.attrs.display === 'none' || node.attrs.visibility === 'hidden') return;
    const st = { ...parent };
    for (const k of INHERITED) if (node.attrs[k] !== undefined) st[k] = node.attrs[k];
    if (node.tag === 'g' && node.attrs['data-vid'] !== undefined && layers[node.attrs['data-vid']]) { st.dxfLayerRaw = layers[node.attrs['data-vid']]; st.dxfLayer = dxfName(st.dxfLayerRaw); }
    const m = mul(parseTransform(node.attrs.transform), ctm);
    const n = (k, d = 0) => { const v = Number(node.attrs[k] ?? d); return Number.isFinite(v) ? v : d; };
    switch (node.tag) {
      case 'svg': case 'g': case 'a': for (const c of node.children) walk(c, st, m); break;
      case 'rect': {
        const w = n('width'), h = n('height');
        if (w <= 0 || h <= 0 || !visible(st, true)) break;
        const x = n('x'), y = n('y');
        polyline([[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true, colourOf(st, true), m, st);
        break;
      }
      case 'path': if (visible(st, true)) for (const s of flattenPath(node.attrs.d || '')) polyline(s.pts, s.closed, colourOf(st, true), m, st); break;
      case 'polygon': case 'polyline': {
        const v = (node.attrs.points || '').split(/[\s,]+/).filter(Boolean).map(Number);
        if (v.length < 4 || v.some((q) => !Number.isFinite(q)) || !visible(st, node.tag === 'polygon')) break;
        const pts = []; for (let i = 0; i + 1 < v.length; i += 2) pts.push([v[i], v[i + 1]]);
        polyline(pts, node.tag === 'polygon', colourOf(st, true), m, st);
        break;
      }
      case 'line': if (visible(st, false)) polyline([[n('x1'), n('y1')], [n('x2'), n('y2')]], false, colourOf(st, true), m, st); break;
      case 'circle': case 'ellipse': {
        const rx = node.tag === 'circle' ? n('r') : n('rx'), ry = node.tag === 'circle' ? n('r') : n('ry');
        if (rx <= 0 || ry <= 0 || !visible(st, true)) break;
        const cx = n('cx'), cy = n('cy');
        const sxs = Math.hypot(m[0], m[1]), sys = Math.hypot(m[2], m[3]);
        if (rx === ry && Math.abs(sxs - sys) < 1e-9 * sxs) { // a circle under a similarity transform stays a circle
          const x = cx * m[0] + cy * m[2] + m[4], y = cx * m[1] + cy * m[3] + m[5];
          put(0, 'CIRCLE'); put(8, layerOf(st)); put(62, colourOf(st, true)); put(10, f(x)); put(20, f(y)); put(30, 0); put(40, f(rx * sxs));
          grow(x - rx * sxs, y - rx * sxs); grow(x + rx * sxs, y + rx * sxs);
          stat.circles++;
        } else polyline(ellipsePoints(cx, cy, rx, ry), true, colourOf(st, true), m, st);
        break;
      }
      case 'text': {
        const x = n('x'), y = n('y');
        const own = node.children.filter((c) => c.text !== undefined).map((c) => c.text).join('');
        const spans = node.children.filter((c) => c.tag === 'tspan');
        const content = (own + spans.map((c) => c.children.filter((t) => t.text !== undefined).map((t) => t.text).join('')).join('')).replace(/[\t\n\r ]/g, ' ').trim();
        if (!content || !st.fill || st.fill === 'none') break;
        const size = Number(st['font-size'] ?? 16);
        const px = x * m[0] + y * m[2] + m[4], py = x * m[1] + y * m[3] + m[5];
        const scale = Math.hypot(m[0], m[1]);
        const rotation = (Math.atan2(m[1], m[0]) * 180) / Math.PI;
        const anchor = st['text-anchor'] ?? 'start';
        put(0, 'TEXT'); put(8, layerOf(st)); put(62, colourOf(st, false)); put(10, f(px)); put(20, f(py)); put(30, 0); put(40, f(Math.max(size * scale, 0.01)));
        put(1, content.replace(/[^\x20-\x7e]/g, '?')); put(50, f(rotation)); put(7, 'STANDARD');
        if (anchor !== 'start') { put(72, anchor === 'middle' ? 1 : 2); put(11, f(px)); put(21, f(py)); put(31, 0); }
        grow(px, py);
        stat.texts++;
        break;
      }
      default: break;
    }
  };
  walk(root, { fill: '#000000' }, base);
  if (!Number.isFinite(box[0])) box = [0, 0, vb[2] / pxPerInch, vb[3] / pxPerInch];
  const head = [];
  const h = (code, value) => { head.push(String(code), String(value)); };
  h(0, 'SECTION'); h(2, 'HEADER'); h(9, '$ACADVER'); h(1, 'AC1009');
  h(9, '$MEASUREMENT'); h(70, 0); h(9, '$INSUNITS'); h(70, 1); // inches: without them Visio reads the units as millimetres and imports a tiny drawing
  h(9, '$EXTMIN'); h(10, f(box[0])); h(20, f(box[1])); h(30, 0);
  h(9, '$EXTMAX'); h(10, f(box[2])); h(20, f(box[3])); h(30, 0);
  h(0, 'ENDSEC');
  h(0, 'SECTION'); h(2, 'TABLES');
  h(0, 'TABLE'); h(2, 'LTYPE'); h(70, 1); h(0, 'LTYPE'); h(2, 'CONTINUOUS'); h(70, 0); h(3, 'Solid line'); h(72, 65); h(73, 0); h(40, 0); h(0, 'ENDTAB');
  h(0, 'TABLE'); h(2, 'LAYER'); h(70, 1 + usedLayers.size); h(0, 'LAYER'); h(2, '0'); h(70, 0); h(62, 7); h(6, 'CONTINUOUS');
  for (const [name, flags] of usedLayers) { h(0, 'LAYER'); h(2, name); h(70, flags); h(62, 7); h(6, 'CONTINUOUS'); }
  h(0, 'ENDTAB');
  h(0, 'TABLE'); h(2, 'STYLE'); h(70, 1); h(0, 'STYLE'); h(2, 'STANDARD'); h(70, 0); h(40, 0); h(41, 1); h(50, 0); h(71, 0); h(42, 0.2); h(3, 'txt'); h(4, ''); h(0, 'ENDTAB');
  h(0, 'ENDSEC');
  h(0, 'SECTION'); h(2, 'ENTITIES');
  const tail = ['0', 'ENDSEC', '0', 'EOF'];
  return { dxf: [...head, ...out, ...tail].join('\r\n') + '\r\n', ...stat, layers: [...usedLayers.keys()], extents: box };
}
