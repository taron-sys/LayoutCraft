// SVG -> PDF page content for the shell's "Export PDF": the drawing's own SVG (what the engine renders) is turned into PDF drawing operators, so
// the PDF holds real paths, text and gradients instead of a picture. No library: a small XML reader, a transform / path / colour parser and a writer
// of content-stream operators.
//
// Handled: svg, g (transform, inherited presentation attributes), rect (also rounded), path (M L H V C S Q T A Z, absolute and relative),
// polygon, polyline, line, circle, ellipse, text / tspan (the 14 standard fonts, textLength honoured), solid fills and strokes with opacity,
// linear gradients in user space (axial shadings), dashes, caps, joins, fill rules.
// Not handled -> `Unsupported` is thrown and the caller keeps the raster page: pattern, filter, mask, clipPath, image, use, symbol, radial gradients,
// gradients in bounding-box units or with transparent stops, group opacity, text outside Latin-1.
export class Unsupported extends Error {}

const num = (n) => { const r = Math.round(n * 10000) / 10000; return Object.is(r, -0) ? '0' : String(r); };

// ---- a small XML reader (the engine's SVG is machine written: elements, attributes, text) ----
function decode(text) {
  return text.replace(/&(#x[0-9a-fA-F]+|#\d+|amp|lt|gt|quot|apos);/g, (_, e) => {
    if (e[0] === '#') return String.fromCodePoint(e[1] === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10));
    return { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" }[e];
  });
}
export function parseXml(source) {
  return parseXmlTree(source).children.find((c) => c.tag === 'svg') || null;
}
/** The whole tree as {tag, attrs, children}; text nodes are {text}. Used by the draw.io import (mxfile / mxGraphModel). */
export function parseXmlTree(source) {
  const root = { tag: '#root', attrs: {}, children: [] };
  const stack = [root];
  const re = /<!--[\s\S]*?-->|<\?[\s\S]*?\?>|<!\[CDATA\[([\s\S]*?)\]\]>|<(\/?)([A-Za-z_][\w:.-]*)((?:\s+[\w:.-]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*(\/?)>|([^<]+)/g;
  let m;
  while ((m = re.exec(source))) {
    const top = stack[stack.length - 1];
    if (m[6] !== undefined) { top.children.push({ text: decode(m[6]) }); continue; }
    if (m[1] !== undefined && m[3] === undefined) { top.children.push({ text: m[1] }); continue; }
    if (m[3] === undefined) continue;
    if (m[2] === '/') { stack.pop(); continue; }
    const attrs = {};
    for (const a of m[4].matchAll(/([\w:.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) attrs[a[1]] = decode(a[2] ?? a[3]);
    const node = { tag: m[3], attrs, children: [] };
    top.children.push(node);
    if (m[5] !== '/') stack.push(node);
  }
  return root;
}

// ---- matrices [a b c d e f]; `mul(A, B)` applies A first, then B ----
const IDENT = [1, 0, 0, 1, 0, 0];
const mul = (A, B) => [A[0] * B[0] + A[1] * B[2], A[0] * B[1] + A[1] * B[3], A[2] * B[0] + A[3] * B[2], A[2] * B[1] + A[3] * B[3], A[4] * B[0] + A[5] * B[2] + B[4], A[4] * B[1] + A[5] * B[3] + B[5]];
function parseTransform(text) {
  let m = IDENT;
  if (!text) return m;
  for (const t of text.matchAll(/(\w+)\s*\(([^)]*)\)/g)) {
    const v = t[2].split(/[\s,]+/).filter(Boolean).map(Number);
    if (v.some((x) => !Number.isFinite(x))) throw new Unsupported(`transform ${t[0]}`);
    let n;
    switch (t[1]) {
      case 'translate': n = [1, 0, 0, 1, v[0], v[1] ?? 0]; break;
      case 'scale': n = [v[0], 0, 0, v[1] ?? v[0], 0, 0]; break;
      case 'rotate': { const a = (v[0] * Math.PI) / 180, c = Math.cos(a), s = Math.sin(a); n = [c, s, -s, c, 0, 0]; if (v.length >= 3) n = mul(mul([1, 0, 0, 1, -v[1], -v[2]], n), [1, 0, 0, 1, v[1], v[2]]); break; }
      case 'matrix': n = v.slice(0, 6); break;
      case 'skewX': n = [1, 0, Math.tan((v[0] * Math.PI) / 180), 1, 0, 0]; break;
      case 'skewY': n = [1, Math.tan((v[0] * Math.PI) / 180), 0, 1, 0, 0]; break;
      default: throw new Unsupported(`transform ${t[1]}`);
    }
    m = mul(n, m);
  }
  return m;
}

// ---- colours ----
const NAMED = { black: [0, 0, 0], white: [1, 1, 1], red: [1, 0, 0], green: [0, 0.5, 0], blue: [0, 0, 1], yellow: [1, 1, 0], gray: [0.5, 0.5, 0.5], grey: [0.5, 0.5, 0.5], silver: [0.75, 0.75, 0.75], orange: [1, 0.647, 0], purple: [0.5, 0, 0.5] };
function parseColour(value) {
  const v = String(value).trim().toLowerCase();
  let m;
  if ((m = /^#([0-9a-f]{6})$/.exec(v))) return [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16) / 255);
  if ((m = /^#([0-9a-f]{3})$/.exec(v))) return [0, 1, 2].map((i) => parseInt(m[1][i] + m[1][i], 16) / 255);
  if ((m = /^rgb\(\s*([\d.]+%?)[ ,]+([\d.]+%?)[ ,]+([\d.]+%?)\s*\)$/.exec(v))) return [m[1], m[2], m[3]].map((x) => (x.endsWith('%') ? parseFloat(x) / 100 : parseFloat(x) / 255));
  if (NAMED[v]) return NAMED[v];
  throw new Unsupported(`colour ${value}`);
}

// ---- paths ----
function arcToBeziers(x1, y1, rx, ry, phiDeg, large, sweep, x2, y2) {
  if (rx === 0 || ry === 0) return [['L', x2, y2]];
  rx = Math.abs(rx); ry = Math.abs(ry);
  const phi = (phiDeg * Math.PI) / 180, cos = Math.cos(phi), sin = Math.sin(phi);
  const dx = (x1 - x2) / 2, dy = (y1 - y2) / 2;
  const x1p = cos * dx + sin * dy, y1p = -sin * dx + cos * dy;
  const lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
  if (lambda > 1) { rx *= Math.sqrt(lambda); ry *= Math.sqrt(lambda); }
  const sign = large === sweep ? -1 : 1;
  const num_ = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
  const den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
  const co = sign * Math.sqrt(Math.max(0, num_ / (den || 1)));
  const cxp = (co * rx * y1p) / ry, cyp = (-co * ry * x1p) / rx;
  const cx = cos * cxp - sin * cyp + (x1 + x2) / 2, cy = sin * cxp + cos * cyp + (y1 + y2) / 2;
  const ang = (ux, uy, vx, vy) => { const a = Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy); return a; };
  const th1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry);
  let dth = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
  if (!sweep && dth > 0) dth -= 2 * Math.PI;
  if (sweep && dth < 0) dth += 2 * Math.PI;
  const n = Math.max(1, Math.ceil(Math.abs(dth) / (Math.PI / 2) - 1e-9));
  const step = dth / n, k = (4 / 3) * Math.tan(step / 4);
  const out = [];
  for (let i = 0; i < n; i++) {
    const a1 = th1 + i * step, a2 = a1 + step;
    const p = (a, d) => [cos * rx * (Math.cos(a) - d * Math.sin(a)) - sin * ry * (Math.sin(a) + d * Math.cos(a)) + cx, sin * rx * (Math.cos(a) - d * Math.sin(a)) + cos * ry * (Math.sin(a) + d * Math.cos(a)) + cy];
    const c1 = p(a1, k), c2 = p(a2, -k), e = p(a2, 0);
    out.push(['C', c1[0], c1[1], c2[0], c2[1], i === n - 1 ? x2 : e[0], i === n - 1 ? y2 : e[1]]);
  }
  return out;
}
const ARGS = { M: 2, L: 2, H: 1, V: 1, C: 6, S: 4, Q: 4, T: 2, A: 7, Z: 0 };
/** Path data -> PDF path operators (absolute coordinates). */
function pathOps(d) {
  const tokens = d.match(/[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g) || [];
  let i = 0, cmd = '', x = 0, y = 0, sx = 0, sy = 0, lc = null, lq = null, out = '';
  const take = (k) => { const v = tokens.slice(i, i + k).map(Number); if (v.length < k || v.some((t) => !Number.isFinite(t))) throw new Unsupported('path data'); i += k; return v; };
  const cubic = (x1, y1, x2, y2, x3, y3) => { out += `${num(x1)} ${num(y1)} ${num(x2)} ${num(y2)} ${num(x3)} ${num(y3)} c\n`; lc = [x2, y2]; lq = null; x = x3; y = y3; };
  while (i < tokens.length) {
    if (/[A-Za-z]/.test(tokens[i])) cmd = tokens[i++];
    else if (cmd === 'M') cmd = 'L'; else if (cmd === 'm') cmd = 'l';
    const up = cmd.toUpperCase(), rel = cmd !== up;
    if (!(up in ARGS)) throw new Unsupported(`path command ${cmd}`);
    const a = take(ARGS[up]);
    const ox = rel ? x : 0, oy = rel ? y : 0;
    switch (up) {
      case 'M': x = a[0] + ox; y = a[1] + oy; sx = x; sy = y; out += `${num(x)} ${num(y)} m\n`; lc = lq = null; break;
      case 'L': x = a[0] + ox; y = a[1] + oy; out += `${num(x)} ${num(y)} l\n`; lc = lq = null; break;
      case 'H': x = a[0] + ox; out += `${num(x)} ${num(y)} l\n`; lc = lq = null; break;
      case 'V': y = a[0] + oy; out += `${num(x)} ${num(y)} l\n`; lc = lq = null; break;
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
          if (seg[0] === 'L') { out += `${num(seg[1])} ${num(seg[2])} l\n`; } else out += `${seg.slice(1).map(num).join(' ')} c\n`;
        }
        x = ex; y = ey; lc = lq = null; break;
      }
      case 'Z': out += 'h\n'; x = sx; y = sy; lc = lq = null; break;
      default: break;
    }
  }
  return out;
}
const ellipsePath = (cx, cy, rx, ry) => {
  const k = 0.5522847498;
  return `${num(cx + rx)} ${num(cy)} m\n${num(cx + rx)} ${num(cy + k * ry)} ${num(cx + k * rx)} ${num(cy + ry)} ${num(cx)} ${num(cy + ry)} c\n${num(cx - k * rx)} ${num(cy + ry)} ${num(cx - rx)} ${num(cy + k * ry)} ${num(cx - rx)} ${num(cy)} c\n${num(cx - rx)} ${num(cy - k * ry)} ${num(cx - k * rx)} ${num(cy - ry)} ${num(cx)} ${num(cy - ry)} c\n${num(cx + k * rx)} ${num(cy - ry)} ${num(cx + rx)} ${num(cy - k * ry)} ${num(cx + rx)} ${num(cy)} c\nh\n`;
};
function roundedRect(x, y, w, h, rx, ry) {
  rx = Math.min(rx, w / 2); ry = Math.min(ry, h / 2);
  const k = 0.5522847498;
  return `${num(x + rx)} ${num(y)} m\n${num(x + w - rx)} ${num(y)} l\n${num(x + w - rx + k * rx)} ${num(y)} ${num(x + w)} ${num(y + ry - k * ry)} ${num(x + w)} ${num(y + ry)} c\n${num(x + w)} ${num(y + h - ry)} l\n${num(x + w)} ${num(y + h - ry + k * ry)} ${num(x + w - rx + k * rx)} ${num(y + h)} ${num(x + w - rx)} ${num(y + h)} c\n${num(x + rx)} ${num(y + h)} l\n${num(x + rx - k * rx)} ${num(y + h)} ${num(x)} ${num(y + h - ry + k * ry)} ${num(x)} ${num(y + h - ry)} c\n${num(x)} ${num(y + ry)} l\n${num(x)} ${num(y + ry - k * ry)} ${num(x + rx - k * rx)} ${num(y)} ${num(x + rx)} ${num(y)} c\nh\n`;
}

// ---- Helvetica widths (1/1000 em) for the characters 32..126, used to fit text to its textLength and to place centred / right-aligned text ----
const HELV = [278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584];
const TIMES = [250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541];
const CP1252 = { 0x20ac: 0x80, 0x201a: 0x82, 0x192: 0x83, 0x201e: 0x84, 0x2026: 0x85, 0x2020: 0x86, 0x2021: 0x87, 0x2c6: 0x88, 0x2030: 0x89, 0x160: 0x8a, 0x2039: 0x8b, 0x152: 0x8c, 0x17d: 0x8e, 0x2018: 0x91, 0x2019: 0x92, 0x201c: 0x93, 0x201d: 0x94, 0x2022: 0x95, 0x2013: 0x96, 0x2014: 0x97, 0x2dc: 0x98, 0x2122: 0x99, 0x161: 0x9a, 0x203a: 0x9b, 0x153: 0x9c, 0x17e: 0x9e, 0x178: 0x9f };
function winAnsi(text) {
  const codes = [];
  for (const ch of text.replace(/ /g, ' ')) {
    const c = ch.codePointAt(0);
    if (c >= 32 && c < 127) codes.push(c);
    else if (c >= 160 && c < 256) codes.push(c);
    else if (CP1252[c]) codes.push(CP1252[c]);
    else if (c === 9 || c === 10 || c === 13) codes.push(32);
    else throw new Unsupported(`character U+${c.toString(16)}`);
  }
  return codes;
}
/** Width of the text in the standard font `base` (its regular metrics; bold faces are about 6 % wider). */
function textWidth(codes, size, base) {
  const table = base.startsWith('Times') ? TIMES : base.startsWith('Courier') ? null : HELV;
  const wide = base.includes('Bold') ? 1.06 : 1;
  const total = codes.reduce((sum, c) => sum + (table === null ? 600 : c >= 32 && c < 127 ? table[c - 32] : table === TIMES ? 500 : 556), 0);
  return (total * size * wide) / 1000;
}
const hex = (codes) => `<${codes.map((c) => c.toString(16).padStart(2, '0')).join('')}>`;
function standardFont(family, weight, style) {
  const f = String(family || '').toLowerCase();
  const bold = /bold|[6-9]00/.test(String(weight || '')), italic = /italic|oblique/.test(String(style || ''));
  if (/courier|consolas|mono/.test(f)) return `Courier${bold || italic ? '-' : ''}${bold ? 'Bold' : ''}${italic ? 'Oblique' : ''}`;
  if (/times|georgia|cambria|garamond|serif|palatino|book/.test(f) && !/sans/.test(f)) return bold && italic ? 'Times-BoldItalic' : bold ? 'Times-Bold' : italic ? 'Times-Italic' : 'Times-Roman';
  return `Helvetica${bold || italic ? '-' : ''}${bold ? 'Bold' : ''}${italic ? 'Oblique' : ''}`;
}

const INHERITED = ['fill', 'stroke', 'stroke-width', 'stroke-linecap', 'stroke-linejoin', 'stroke-dasharray', 'stroke-miterlimit', 'fill-opacity', 'stroke-opacity', 'fill-rule', 'font-family', 'font-size', 'font-weight', 'font-style', 'text-anchor'];
const UNSUPPORTED_TAGS = new Set(['pattern', 'filter', 'mask', 'clipPath', 'image', 'use', 'symbol', 'radialGradient', 'foreignObject', 'marker', 'style', 'script']);

/**
 * svgText -> { content, fonts: Map(name -> resource), gstates: Map(key -> resource name + dict), patterns: [{name, dict}], widthPt, heightPt }.
 * Throws `Unsupported` when the drawing uses something the converter does not draw faithfully.
 */
export function svgToPdfPage(svgText, { paper = null, fontSource = null } = {}) {
  const root = parseXml(svgText);
  if (!root) throw new Unsupported('no svg element');
  const vb = (root.attrs.viewBox || '').split(/[\s,]+/).map(Number);
  const wPx = parseFloat(root.attrs.width), hPx = parseFloat(root.attrs.height);
  if (vb.length !== 4 || vb.some((v) => !Number.isFinite(v)) || !(wPx > 0) || !(hPx > 0)) throw new Unsupported('viewBox');
  const widthPt = wPx * 0.75, heightPt = hPx * 0.75;
  const sx = widthPt / vb[2], sy = heightPt / vb[3];
  const base = [sx, 0, 0, -sy, -vb[0] * sx, (vb[1] + vb[3]) * sy];
  const gradients = new Map();
  const walkDefs = (node) => {
    if (UNSUPPORTED_TAGS.has(node.tag) && node.tag !== 'radialGradient') { if (node.tag !== 'image') throw new Unsupported(`<${node.tag}>`); }
    if (node.tag === 'linearGradient' && node.attrs.id) gradients.set(node.attrs.id, node);
    for (const c of node.children || []) if (c.tag) walkDefs(c);
  };
  walkDefs(root);
  const fonts = new Map(), gstates = new Map(), patterns = [];
  const embedded = new Map(); // font key -> EmbeddedFont (ttf.js): the fonts of this page that the PDF embeds as subsets
  // the paper colour first, in page space
  let out = paper ? `q ${parseColour(paper).map(num).join(' ')} rg 0 0 ${num(widthPt)} ${num(heightPt)} re f Q\n` : '';
  out += `q ${base.map(num).join(' ')} cm\n`;
  const gstate = (fo, so) => {
    if (fo >= 0.9999 && so >= 0.9999) return null;
    const key = `${num(fo)}/${num(so)}`;
    if (!gstates.has(key)) gstates.set(key, { name: `GS${gstates.size + 1}`, dict: `<< /Type /ExtGState /ca ${num(fo)} /CA ${num(so)} >>` });
    return gstates.get(key).name;
  };
  const shading = (id, ctm) => {
    const g = gradients.get(id);
    if (!g) throw new Unsupported(`gradient ${id}`);
    if (g.attrs.href || g.attrs['xlink:href']) throw new Unsupported('gradient href');
    if (g.attrs.gradientUnits !== 'userSpaceOnUse') throw new Unsupported('gradient units');
    const c = ['x1', 'y1', 'x2', 'y2'].map((k) => Number(g.attrs[k] ?? 0));
    if (c.some((v) => !Number.isFinite(v))) throw new Unsupported('gradient coordinates');
    const stops = g.children.filter((s) => s.tag === 'stop').map((s) => {
      if (Number(s.attrs['stop-opacity'] ?? 1) < 0.9999) throw new Unsupported('transparent gradient stop');
      return { at: Math.min(1, Math.max(0, parseFloat(s.attrs.offset ?? 0) / (String(s.attrs.offset).endsWith('%') ? 100 : 1))), rgb: parseColour(s.attrs['stop-color'] ?? '#000000') };
    });
    if (stops.length < 2) throw new Unsupported('gradient stops');
    for (let i = 1; i < stops.length; i++) stops[i].at = Math.max(stops[i].at, stops[i - 1].at);
    const fn = (a, b) => `<< /FunctionType 2 /Domain [0 1] /C0 [${a.map(num).join(' ')}] /C1 [${b.map(num).join(' ')}] /N 1 >>`;
    let func;
    if (stops.length === 2 && stops[0].at === 0 && stops[1].at === 1) func = fn(stops[0].rgb, stops[1].rgb);
    else {
      const all = [{ at: 0, rgb: stops[0].rgb }, ...stops, { at: 1, rgb: stops[stops.length - 1].rgb }];
      const segs = [], bounds = [];
      for (let i = 0; i + 1 < all.length; i++) { segs.push(fn(all[i].rgb, all[i + 1].rgb)); if (i + 2 < all.length) bounds.push(all[i + 1].at); }
      func = `<< /FunctionType 3 /Domain [0 1] /Functions [${segs.join(' ')}] /Bounds [${bounds.map(num).join(' ')}] /Encode [${segs.map(() => '0 1').join(' ')}] >>`;
    }
    const m = mul(parseTransform(g.attrs.gradientTransform), ctm);
    const name = `P${patterns.length + 1}`;
    patterns.push({ name, dict: `<< /Type /Pattern /PatternType 2 /Matrix [${m.map(num).join(' ')}] /Shading << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [${c.map(num).join(' ')}] /Function ${func} /Extend [true true] >> >>` });
    return name;
  };
  const fontOf = (st) => {
    const name = standardFont(st['font-family'], st['font-weight'], st['font-style']);
    if (!fonts.has(name)) fonts.set(name, `F${fonts.size + 1}`);
    return fonts.get(name);
  };
  const paint = (pathStr, node, st, ctm, canFill = true) => {
    const fillV = canFill ? (st.fill ?? '#000000') : 'none', strokeV = st.stroke ?? 'none';
    const doFill = fillV !== 'none' && fillV !== 'transparent', doStroke = strokeV !== 'none' && strokeV !== 'transparent';
    if (!doFill && !doStroke) return;
    out += 'q\n';
    const gs = gstate(doFill ? Number(st['fill-opacity'] ?? 1) : 1, doStroke ? Number(st['stroke-opacity'] ?? 1) : 1);
    if (gs) out += `/${gs} gs\n`;
    if (doFill) {
      const url = /^url\(#([^)]+)\)$/.exec(fillV);
      if (url) out += `/Pattern cs /${shading(url[1], ctm)} scn\n`; else out += `${parseColour(fillV).map(num).join(' ')} rg\n`;
    }
    if (doStroke) {
      if (/^url\(/.test(strokeV)) throw new Unsupported('gradient stroke');
      out += `${parseColour(strokeV).map(num).join(' ')} RG\n${num(Number(st['stroke-width'] ?? 1))} w\n`;
      const cap = { butt: 0, round: 1, square: 2 }[st['stroke-linecap'] ?? 'butt'], join = { miter: 0, round: 1, bevel: 2 }[st['stroke-linejoin'] ?? 'miter'];
      out += `${cap ?? 0} J ${join ?? 0} j\n`;
      if (st['stroke-miterlimit']) out += `${num(Number(st['stroke-miterlimit']))} M\n`;
      const dash = String(st['stroke-dasharray'] ?? 'none');
      if (dash !== 'none') { const v = dash.split(/[\s,]+/).filter(Boolean).map(Number); if (v.length && v.every((x) => Number.isFinite(x) && x >= 0) && v.some((x) => x > 0)) out += `[${(v.length % 2 ? v.concat(v) : v).map(num).join(' ')}] 0 d\n`; }
    }
    const evenodd = st['fill-rule'] === 'evenodd';
    out += `${pathStr}${doFill && doStroke ? (evenodd ? 'B*' : 'B') : doFill ? (evenodd ? 'f*' : 'f') : 'S'}\nQ\n`;
  };
  // text in an embedded TrueType font (fontSource.pick(family, bold, italic) gives an EmbeddedFont or null): the glyph ids are shown through an Identity-H Type0
  // font; `null` when the font has no glyph for a character (the run then takes the standard font path below)
  const embeddedRun = (str, x, y, st, node, ctm) => {
    const bold = /bold|[6-9]00/.test(String(st['font-weight'] || '')), italic = /italic|oblique/.test(String(st['font-style'] || ''));
    const ef = fontSource.pick(st['font-family'], bold, italic);
    if (!ef) return null;
    const cps = [...str.replace(/[\t\n\r\u00a0]/g, ' ')].map((ch) => ch.codePointAt(0));
    if (!cps.length) return 0;
    const gids = cps.map((cp) => ef.gidOf(cp));
    if (gids.some((g) => g === 0)) return null;
    const size = Number(st['font-size'] ?? 16);
    const fillV = st.fill ?? '#000000';
    if (fillV === 'none') return 0;
    const natural = (gids.reduce((sum, g) => sum + ef.width(g), 0) * size) / 1000;
    const want = node.attrs.textLength !== undefined ? Number(node.attrs.textLength) : natural;
    const width = Number.isFinite(want) && want > 0 ? want : natural;
    const anchor = st['text-anchor'] ?? 'start';
    const x0 = anchor === 'middle' ? x - width / 2 : anchor === 'end' ? x - width : x;
    const tz = natural > 0 ? Math.round((width / natural) * 10000) / 100 : 100;
    gids.forEach((g, i) => ef.use(g, cps[i]));
    const key = `emb:${ef.name}`;
    if (!fonts.has(key)) fonts.set(key, `F${fonts.size + 1}`);
    embedded.set(key, ef);
    out += 'q\n';
    const gs = gstate(Number(st['fill-opacity'] ?? 1), 1);
    if (gs) out += `/${gs} gs\n`;
    const url = /^url\(#([^)]+)\)$/.exec(fillV);
    if (url) out += `/Pattern cs /${shading(url[1], ctm)} scn\n`; else out += `${parseColour(fillV).map(num).join(' ')} rg\n`;
    out += `BT /${fonts.get(key)} ${num(size)} Tf ${num(tz)} Tz 1 0 0 -1 ${num(x0)} ${num(y)} Tm <${gids.map((g) => g.toString(16).padStart(4, '0')).join('')}> Tj ET\nQ\n`;
    return width;
  };
  const textRun = (str, x, y, st, node, ctm) => {
    if (fontSource) { const done = embeddedRun(str, x, y, st, node, ctm); if (done !== null) return done; }
    const codes = winAnsi(str);
    if (!codes.length) return 0;
    const size = Number(st['font-size'] ?? 16);
    const fillV = st.fill ?? '#000000';
    if (fillV === 'none') return 0;
    const base = standardFont(st['font-family'], st['font-weight'], st['font-style']);
    const natural = textWidth(codes, size, base);
    const want = node.attrs.textLength !== undefined ? Number(node.attrs.textLength) : natural;
    const width = Number.isFinite(want) && want > 0 ? want : natural;
    const anchor = st['text-anchor'] ?? 'start';
    const x0 = anchor === 'middle' ? x - width / 2 : anchor === 'end' ? x - width : x;
    const tz = natural > 0 ? Math.round((width / natural) * 10000) / 100 : 100;
    out += 'q\n';
    const gs = gstate(Number(st['fill-opacity'] ?? 1), 1);
    if (gs) out += `/${gs} gs\n`;
    const url = /^url\(#([^)]+)\)$/.exec(fillV);
    if (url) out += `/Pattern cs /${shading(url[1], ctm)} scn\n`; else out += `${parseColour(fillV).map(num).join(' ')} rg\n`;
    out += `BT /${fontOf(st)} ${num(size)} Tf ${num(tz)} Tz 1 0 0 -1 ${num(x0)} ${num(y)} Tm ${hex(codes)} Tj ET\nQ\n`;
    return width;
  };
  const draw = (node, parentStyle, ctm) => {
    if (!node.tag) return;
    if (['title', 'desc', 'defs', 'linearGradient', 'stop'].includes(node.tag)) return;
    if (UNSUPPORTED_TAGS.has(node.tag)) throw new Unsupported(`<${node.tag}>`);
    if (node.attrs.display === 'none' || node.attrs.visibility === 'hidden') return;
    if (node.attrs.opacity !== undefined && Number(node.attrs.opacity) < 0.9999) throw new Unsupported('group opacity');
    if (node.attrs.filter || node.attrs.mask || node.attrs['clip-path']) throw new Unsupported('filter / mask / clip');
    const st = { ...parentStyle };
    for (const k of INHERITED) if (node.attrs[k] !== undefined) st[k] = node.attrs[k];
    const local = parseTransform(node.attrs.transform);
    const here = mul(local, ctm);
    const hasTransform = node.attrs.transform !== undefined;
    if (hasTransform) out += `q ${local.map(num).join(' ')} cm\n`;
    const n = (k, d = 0) => { const v = Number(node.attrs[k] ?? d); if (!Number.isFinite(v)) throw new Unsupported(`attribute ${k}`); return v; };
    switch (node.tag) {
      case 'svg': case 'g': for (const c of node.children) draw(c, st, here); break;
      case 'rect': {
        const w = n('width'), h = n('height');
        if (w <= 0 || h <= 0) break;
        let rx = node.attrs.rx !== undefined ? n('rx') : n('ry'), ry = node.attrs.ry !== undefined ? n('ry') : rx;
        paint(rx > 0 || ry > 0 ? roundedRect(n('x'), n('y'), w, h, rx, ry) : `${num(n('x'))} ${num(n('y'))} ${num(w)} ${num(h)} re\n`, node, st, here);
        break;
      }
      case 'path': paint(pathOps(node.attrs.d || ''), node, st, here); break;
      case 'polygon': case 'polyline': {
        const v = (node.attrs.points || '').split(/[\s,]+/).filter(Boolean).map(Number);
        if (v.length < 4 || v.some((q) => !Number.isFinite(q))) break;
        let p = `${num(v[0])} ${num(v[1])} m\n`;
        for (let i = 2; i + 1 < v.length; i += 2) p += `${num(v[i])} ${num(v[i + 1])} l\n`;
        paint(node.tag === 'polygon' ? `${p}h\n` : p, node, st, here, node.tag === 'polygon');
        break;
      }
      case 'line': paint(`${num(n('x1'))} ${num(n('y1'))} m\n${num(n('x2'))} ${num(n('y2'))} l\n`, node, st, here, false); break;
      case 'circle': paint(ellipsePath(n('cx'), n('cy'), n('r'), n('r')), node, st, here); break;
      case 'ellipse': paint(ellipsePath(n('cx'), n('cy'), n('rx'), n('ry')), node, st, here); break;
      case 'text': {
        const x = n('x'), y = n('y');
        const own = node.children.filter((c) => c.text !== undefined).map((c) => c.text).join('');
        const spans = node.children.filter((c) => c.tag);
        for (const c of spans) {
          if (c.tag !== 'tspan') throw new Unsupported(`<${c.tag}> in text`);
          if (c.attrs.dx || c.attrs.dy) throw new Unsupported('tspan dx / dy');
          if (c.children.some((t) => t.tag)) throw new Unsupported('nested tspan');
        }
        const textOf = (c) => c.children.filter((t) => t.text !== undefined).map((t) => t.text).join('');
        const placed = spans.some((c) => c.attrs.x !== undefined || c.attrs.y !== undefined);
        if (!spans.length) { if (own.trim()) textRun(own, x, y, st, node, here); break; }
        if (!placed) {
          // the spans are pieces of one line of text (styles can differ); the parent's textLength fits the whole line
          if (spans.every((c) => INHERITED.every((k) => c.attrs[k] === undefined))) { textRun(own + spans.map(textOf).join(''), x, y, st, node, here); break; }
          let cursor = x;
          for (const c of spans) {
            const cs = { ...st };
            for (const k of INHERITED) if (c.attrs[k] !== undefined) cs[k] = c.attrs[k];
            cursor += textRun(textOf(c), cursor, y, { ...cs, 'text-anchor': 'start' }, c, here);
          }
          break;
        }
        let cx = x, cy = y;
        for (const c of spans) {
          const cs = { ...st };
          for (const k of INHERITED) if (c.attrs[k] !== undefined) cs[k] = c.attrs[k];
          if (c.attrs.x !== undefined) cx = Number(c.attrs.x);
          if (c.attrs.y !== undefined) cy = Number(c.attrs.y);
          if (!Number.isFinite(cx) || !Number.isFinite(cy)) throw new Unsupported('tspan position');
          textRun(textOf(c), cx, cy, cs, c, here);
        }
        break;
      }
      default: throw new Unsupported(`<${node.tag}>`);
    }
    if (hasTransform) out += 'Q\n';
  };
  draw(root, {}, base);
  out += 'Q\n';
  return { content: out, fonts, embedded, gstates, patterns, widthPt, heightPt };
}

export { parseTransform, mul, parseColour, arcToBeziers, IDENT };
