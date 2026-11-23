// draw.io export (TODO I, second half, step 304): every page of the open drawing becomes one <diagram> of an uncompressed .drawio file (<mxfile><diagram><mxGraphModel>), built from the
// engine's document model (engine.model()), like web/excalidraw-export.js. Public format only.
// Mapped: closed four-corner outlines to rectangles (rounded when the shape has rounding), ellipses, diamonds (rhombus); shape text as the cell's label; glued connectors to edges with
// source / target cells, the route's bends as waypoints and the arrowheads; fill / line colour, line weight, dashes, transparency, rotation, font size / colour / family.
// Every other outline (open lines, polygons, curves) becomes an edge without ends through its points (draw.io has no free polygon vertex): a filled polygon loses its fill.
import { classify } from './excalidraw-export.js';
import { shapeGeometry, KNOWN_SHAPES } from './drawio-shapes.js';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const num = (n) => String(Math.round(n * 100) / 100);
const colour = (c) => (typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c) ? c : null);
// Visio's arrowhead numbers (measured, docs/27) back to draw.io names: [name, filled]
const FROM_INDEX = { 1: ['open', 1], 2: ['block', 1], 3: ['open', 1], 4: ['block', 1], 5: ['classic', 1], 6: ['classic', 1], 7: ['open', 1], 8: ['classic', 1], 9: ['dash', 1], 10: ['oval', 1], 11: ['box', 1], 12: ['open', 1], 13: ['block', 1], 14: ['block', 0], 15: ['block', 0], 16: ['block', 0], 17: ['classic', 0], 18: ['classic', 0], 19: ['classic', 0], 20: ['oval', 0], 21: ['box', 0], 22: ['diamond', 0], 23: ['cross', 1], 24: ['dash', 1], 25: ['ERmandOne', 1], 26: ['ERmandOne', 1], 27: ['ERmany', 1], 28: ['ERoneToMany', 1], 29: ['ERzeroToMany', 1], 30: ['ERzeroToOne', 1], 31: ['ERzeroToOne', 1], 32: ['ERzeroToMany', 1], 33: ['ERzeroToMany', 1], 34: ['diamond', 0], 35: ['ERzeroToOne', 1] };
const SIZE_PX = { 0: 3, 1: 4, 2: 6, 3: 10, 4: 14, 5: 18, 6: 24 };

/** The draw.io arrow styles of a shape / connector from the engine's own numbers (style `endArrow`, `startArrow`, with fill and size), '' when it has no arrowheads. */
function arrowStyles(engine, pageIndex, id) {
  let info;
  try { info = engine.styleInfo(pageIndex, id); } catch { return null; }
  const one = (index, size, prefix) => {
    if (!index) return `${prefix}Arrow=none;`;
    const [name, fill] = FROM_INDEX[index] || ['classic', 1];
    return `${prefix}Arrow=${name};${fill ? '' : `${prefix}Fill=0;`}${prefix}Size=${SIZE_PX[size ?? 2] ?? 6};`;
  };
  return one(info.endArrow, info.endArrowSize, 'end') + one(info.beginArrow, info.beginArrowSize, 'start');
}

const ARROW = { none: 'none', filled: 'classic', open: 'open', triangle: 'block', dot: 'oval', circle: 'oval', diamond: 'diamond', bar: 'dash' };

function styleOf(s, extra = '') {
  const parts = [];
  if (extra) parts.push(extra);
  parts.push('whiteSpace=wrap', 'html=1');
  const fill = colour(s.fill);
  parts.push(fill ? `fillColor=${fill}` : 'fillColor=none');
  const line = colour(s.stroke);
  parts.push(s.strokeWidth === 0 || !line ? 'strokeColor=none' : `strokeColor=${line}`);
  if (s.strokeWidth > 0) parts.push(`strokeWidth=${num(s.strokeWidth)}`);
  const dash = String(s.strokeStyle || '');
  if (/pat[2-9]|pat1\d/.test(dash)) parts.push('dashed=1');
  if (typeof s.fillOpacity === 'number' && s.fillOpacity < 1) parts.push(`opacity=${Math.round(s.fillOpacity * 100)}`);
  if (s.rot) parts.push(`rotation=${num(s.rot)}`);
  if (s.fontSize) parts.push(`fontSize=${Math.max(1, Math.round(s.fontSize))}`);
  if (colour(s.textColor)) parts.push(`fontColor=${s.textColor}`);
  if (s.fontFamily) parts.push(`fontFamily=${String(s.fontFamily).replace(/[;=]/g, '')}`);
  const bits = (s.bold ? 1 : 0) | (s.italic ? 2 : 0) | (s.underline ? 4 : 0);
  if (bits) parts.push(`fontStyle=${bits}`);
  return parts.join(';') + ';';
}

const label = (s) => esc(String(s.label || '').replace(/\r?\n/g, '<br>'));

/** The draw.io shape (a name of drawio-shapes.js) whose outline and inner lines are those of `main` and the group members `others`, or null (step 310: shapes the importer drew come back as shapes). */
const SKIP_NAMES = new Set(['swimlane', 'bpmnGateway', 'bpmnEvent']); // the BPMN ones carry our own names (draw.io writes mxgraph.bpmn.* with symbol styles), so they stay plain shapes on export
const bboxOf = (pts) => { const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]); return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)]; };
function recognize(main, cls, others) {
  const outlineOf = () => (cls.kind === 'rectangle' ? null : (() => { const q = cls.paths[0]; let pts = q.points; if (pts.length > 2 && Math.hypot(pts[0][0] - pts[pts.length - 1][0], pts[0][1] - pts[pts.length - 1][1]) < 0.5) pts = pts.slice(0, -1); return pts; })());
  const pts = outlineOf();
  if (cls.kind === 'polyline' && !(cls.paths.length === 1 && cls.paths[0].closed)) return null;
  for (const name of KNOWN_SHAPES) {
    if (SKIP_NAMES.has(name)) continue;
    const g = shapeGeometry(name, {}, main.w, main.h);
    if (!g) continue;
    if (cls.kind === 'rectangle') {
      if (g.outline.length !== 4 || !g.outline.every((q) => (Math.abs(q[0]) < 0.5 || Math.abs(q[0] - main.w) < 0.5) && (Math.abs(q[1]) < 0.5 || Math.abs(q[1] - main.h) < 0.5))) continue;
    } else {
      if (!pts || g.outline.length !== pts.length || g.outline.some((q, i) => Math.hypot(q[0] - pts[i][0], q[1] - pts[i][1]) > 0.75)) continue;
    }
    // the inner lines: one other member for each, with the same box
    const rest = [...others];
    const ok = (g.details || []).every((d) => {
      const [x0, y0, x1, y1] = bboxOf(d.pts);
      const at = rest.findIndex((o) => Math.abs(o.x - (main.x + x0)) < 1.2 && Math.abs(o.y - (main.y + y0)) < 1.2 && Math.abs(o.w - Math.max(x1 - x0, 2)) < 1.2 && Math.abs(o.h - Math.max(y1 - y0, 2)) < 1.2); // a flat line is drawn 2 px wide at least
      if (at < 0) return false;
      rest.splice(at, 1);
      return true;
    });
    if (!ok || rest.length) continue;
    return { name, used: others };
  }
  return null;
}

function diagram(page, size, engine, pageIndex) {
  const cells = ['<mxCell id="0"/>'];
  const ids = new Set();
  // shapes the importer drew with their own outline (step 310): a group whose biggest member and inner lines match a known draw.io shape is written as that shape
  const recog = {}, dropped = new Set(), groupMain = {};
  const groups = new Map();
  for (const s of page.shapes) if (s.group && s.group.rootId) { if (!groups.has(s.group.rootId)) groups.set(s.group.rootId, []); groups.get(s.group.rootId).push(s); }
  const custom = (s) => s.type === 'custom' && Array.isArray(s.parts) && s.parts.length;
  for (const [root, members] of groups) {
    const ms = members.filter(custom).sort((a, b) => b.w * b.h - a.w * a.h);
    if (!ms.length) continue;
    const main = ms[0], cls = classify(main.parts, main.w, main.h);
    const found = recognize(main, cls, ms.slice(1));
    if (found) { recog[main.visioId] = found.name; groupMain[root] = main.visioId; for (const o of found.used) dropped.add(o.visioId); }
  }
  for (const s of page.shapes) {
    if (!s.group && custom(s)) { const cls = classify(s.parts, s.w, s.h); if (cls.kind === 'polyline') { const found = recognize(s, cls, []); if (found) recog[s.visioId] = found.name; } }
  }
  for (const s of page.shapes) {
    if (!custom(s) || dropped.has(s.visioId)) continue;
    const cls = classify(s.parts, s.w, s.h);
    const id = `s${s.visioId}`;
    if (recog[s.visioId]) {
      cells.push(`<mxCell id="${id}" value="${label(s)}" style="${esc(styleOf(s, `shape=${recog[s.visioId]}`))}" vertex="1" parent="1"><mxGeometry x="${num(s.x)}" y="${num(s.y)}" width="${num(s.w)}" height="${num(s.h)}" as="geometry"/></mxCell>`);
      ids.add(String(s.visioId));
      continue;
    }
    if (cls.kind === 'rectangle' || cls.kind === 'ellipse' || cls.kind === 'diamond') {
      const extra = cls.kind === 'ellipse' ? 'ellipse' : cls.kind === 'diamond' ? 'rhombus' : s.rounding > 0 ? 'rounded=1' : 'rounded=0';
      const textOnly = !colour(s.fill) && (s.strokeWidth === 0 || !colour(s.stroke)) && s.label;
      cells.push(`<mxCell id="${id}" value="${label(s)}" style="${esc(styleOf(s, textOnly ? 'text' : extra))}" vertex="1" parent="1"><mxGeometry x="${num(s.x)}" y="${num(s.y)}" width="${num(s.w)}" height="${num(s.h)}" as="geometry"/></mxCell>`);
      ids.add(String(s.visioId));
    } else {
      for (const [k, path] of cls.paths.entries()) {
        if (path.points.length < 2) continue;
        const pts = path.points.map(([px, py]) => [s.x + px, s.y + py]);
        const mid = pts.slice(1, -1).map((p) => `<mxPoint x="${num(p[0])}" y="${num(p[1])}"/>`).join('');
        const first = pts[0], last = pts[pts.length - 1];
        const own = !path.closed && arrowStyles(engine, pageIndex, s.visioId); // an open line may carry arrowheads
        cells.push(`<mxCell id="${id}-${k}" value="${k === 0 ? label(s) : ''}" style="${esc(styleOf(s, own && /Arrow=(?!none)/.test(own) ? own.replace(/Size=/g, 'Size=') : 'endArrow=none;endFill=0;startArrow=none'))}" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="${num(first[0])}" y="${num(first[1])}" as="sourcePoint"/><mxPoint x="${num(last[0])}" y="${num(last[1])}" as="targetPoint"/>${mid ? `<Array as="points">${mid}</Array>` : ''}</mxGeometry></mxCell>`);
      }
    }
  }
  // pictures (Insert > Picture, or drawn by the importer): shape=image with the picture as a data: URI in the style (draw.io leaves out ";base64" because ";" separates the entries)
  const pageSvg = (page.shapes || []).some((s) => s.type === 'image' && s.image && !s.group) ? engine.svg(pageIndex) : '';
  for (const s of page.shapes) {
    if (s.type !== 'image' || !s.image || s.group) continue;
    const at = pageSvg.indexOf(`<g data-vid="${s.visioId}"`);
    const m = at < 0 ? null : /href="data:image\/([a-z+.-]+);base64,([^"]+)"/.exec(pageSvg.slice(at, pageSvg.indexOf('</g>', at)));
    if (!m) continue;
    cells.push(`<mxCell id="s${s.visioId}" value="${label(s)}" style="${esc(`shape=image;verticalLabelPosition=bottom;labelBackgroundColor=default;verticalAlign=top;aspect=fixed;imageAspect=0;image=data:image/${m[1]},${m[2]};`)}" vertex="1" parent="1"><mxGeometry x="${num(s.x)}" y="${num(s.y)}" width="${num(s.w)}" height="${num(s.h)}" as="geometry"/></mxCell>`);
    ids.add(String(s.visioId));
  }
  for (const c of page.connectors || []) {
    const endOf = (e) => { const raw = String(e?.shapeId || '').replace('shape:visio:', ''); return groupMain[raw] ?? raw; };
    const from = endOf(c.from), to = endOf(c.to);
    if (!ids.has(from) || !ids.has(to)) continue;
    const pts = (c.waypoints || []).map((w) => `<mxPoint x="${num(w.x)}" y="${num(w.y)}"/>`).join('');
    const own = arrowStyles(engine, pageIndex, c.visioId);
    const st = `${own ?? `endArrow=${ARROW[c.arrowEnd] || 'none'};startArrow=${ARROW[c.arrowStart] || 'none'};`}html=1;strokeColor=${colour(c.stroke) || '#000000'};strokeWidth=${num(c.strokeWidth || 1)};${/pat[2-9]/.test(String(c.strokeStyle || '')) ? 'dashed=1;' : ''}`;
    cells.push(`<mxCell id="c${c.visioId}" value="${esc(c.label || '')}" style="${esc(st)}" edge="1" parent="1" source="s${from}" target="s${to}"><mxGeometry relative="1" as="geometry">${pts ? `<Array as="points">${pts}</Array>` : ''}</mxGeometry></mxCell>`);
  }
  // layers (step 327): the page's layers are layer cells (parent 0); a shape cell sits on the layer of its first membership, an edge on the layer of the shape it starts at; the default layer (id 1) is there when something sits on it
  const layers = page.layers || [];
  const layerCell = new Map();
  const layerOfShape = (vid) => { const s = (page.shapes || []).find((q) => String(q.visioId) === String(vid)); const k = s && s.layers && s.layers[0]; return k !== undefined && layers[k] ? `L${k + 1}` : null; };
  if (layers.length) {
    const defs = layers.map((l, k) => `<mxCell id="L${k + 1}" value="${esc(l.name || `Layer ${k + 1}`)}" parent="0"${l.visible === false ? ' visible="0"' : ''}${l.locked ? ' style="locked=1;"' : ''}/>`);
    for (let i = 1; i < cells.length; i++) {
      const m = /^<mxCell id="(?:s(\d+)(?:-\d+)?|c(\d+))"/.exec(cells[i]);
      if (!m) continue;
      let layer = null;
      if (m[1]) layer = layerOfShape(m[1]);
      else { const c = (page.connectors || []).find((q) => String(q.visioId) === m[2]); layer = c ? layerOfShape(String(c.from?.shapeId || '').replace('shape:visio:', '')) : null; }
      if (layer) cells[i] = cells[i].replace(' parent="1"', ` parent="${layer}"`);
    }
    cells.splice(1, 0, ...defs);
  }
  if (!layers.length || cells.some((c) => c.includes(' parent="1"'))) cells.splice(1 + layers.length, 0, '<mxCell id="1" parent="0"/>');
  return `<mxGraphModel dx="0" dy="0" grid="1" gridSize="10" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="1" pageScale="1" pageWidth="${Math.round(size.w * 96)}" pageHeight="${Math.round(size.h * 96)}" math="0" shadow="0"><root>${cells.join('')}</root></mxGraphModel>`;
}

/** The whole drawing as an uncompressed .drawio file (text). */
export function exportDrawio(engine) {
  const model = engine.model();
  const names = engine.pageNames();
  const body = model.pages.map((p, i) => `<diagram id="page${i + 1}" name="${esc(names[i] || p.name || `Page-${i + 1}`)}">${diagram(p, engine.pageSize(i), engine, i)}</diagram>`).join('');
  return `<mxfile host="LayoutCraft" version="1">${body}</mxfile>`;
}
