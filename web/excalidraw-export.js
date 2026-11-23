// Excalidraw export (TODO H, second half, step 303): the page of the open drawing becomes an Excalidraw scene (.excalidraw JSON, the public format), built from the engine's
// document model (engine.model(): shapes with their outline paths, glued connectors). Public format only.
// Mapped: closed four-corner outlines to rectangles (rounded when the shape has rounding), ellipses (arc outlines), diamonds, every other outline to a line / polygon (curves flattened),
// shape text to the text bound inside rectangles / ellipses / diamonds or a free text element, glued connectors to arrows bound at both ends (the route's bends become arrow points),
// fill and line colours, line weight, dashes, transparency, rotation, font size. Not mapped: gradients, shadows, images, masters' groups (shapes arrive ungrouped), hyperlinks.

const FAMILY = (name) => (/consolas|courier|mono|cascadia/i.test(name || '') ? 3 : /print|hand|comic|segoe script/i.test(name || '') ? 1 : 2);
const ARROW_END = { none: null, filled: 'arrow', open: 'arrow', triangle: 'triangle', dot: 'dot', circle: 'dot', diamond: 'diamond', bar: 'bar' };

/** Flatten an SVG path (M L H V C Q A Z, absolute commands as the engine writes them) to [{points: [[x, y], ...], closed, arc}]. */
export function flattenPath(d) {
  const out = [];
  let cur = null, start = null, pts = [], hasArc = false, closed = false;
  const flush = () => { if (pts.length > 1) out.push({ points: pts, closed, arc: hasArc }); pts = []; closed = false; hasArc = false; };
  for (const m of String(d || '').matchAll(/([MLHVCQAZ])([^MLHVCQAZ]*)/g)) {
    const c = m[1], v = (m[2].match(/-?\d*\.?\d+(?:e-?\d+)?/gi) || []).map(Number);
    switch (c) {
      case 'M': flush(); cur = [v[0], v[1]]; start = cur; pts = [cur]; break;
      case 'L': for (let i = 0; i + 1 < v.length; i += 2) { cur = [v[i], v[i + 1]]; pts.push(cur); } break;
      case 'H': cur = [v[0], cur[1]]; pts.push(cur); break;
      case 'V': cur = [cur[0], v[0]]; pts.push(cur); break;
      case 'C': for (let i = 0; i + 5 < v.length; i += 6) { const p0 = cur; for (let k = 1; k <= 12; k++) { const t = k / 12, u = 1 - t; pts.push([u * u * u * p0[0] + 3 * u * u * t * v[i] + 3 * u * t * t * v[i + 2] + t * t * t * v[i + 4], u * u * u * p0[1] + 3 * u * u * t * v[i + 1] + 3 * u * t * t * v[i + 3] + t * t * t * v[i + 5]]); } cur = [v[i + 4], v[i + 5]]; } break;
      case 'Q': for (let i = 0; i + 3 < v.length; i += 4) { const p0 = cur; for (let k = 1; k <= 8; k++) { const t = k / 8, u = 1 - t; pts.push([u * u * p0[0] + 2 * u * t * v[i] + t * t * v[i + 2], u * u * p0[1] + 2 * u * t * v[i + 1] + t * t * v[i + 3]]); } cur = [v[i + 2], v[i + 3]]; } break;
      case 'A': hasArc = true; for (let i = 0; i + 6 < v.length; i += 7) { cur = [v[i + 5], v[i + 6]]; pts.push(cur); } break;
      case 'Z': closed = true; if (start) cur = start; flush(); pts = []; break;
      default: break;
    }
  }
  flush();
  return out;
}

const near = (a, b, tol) => Math.abs(a - b) <= tol;

/** Classify one outline: 'ellipse' | 'rectangle' | 'diamond' | 'polyline' (with its points, shape-local). */
export function classify(parts, w, h) {
  const paths = parts.flatMap((p) => flattenPath(p.d));
  if (paths.length !== 1) return { kind: 'polyline', paths };
  const { points, closed, arc } = paths[0];
  const tol = Math.max(0.6, Math.max(w, h) * 0.01);
  if (arc && closed !== undefined && points.length <= 6 && points.some((p) => near(p[0], 0, tol)) && points.some((p) => near(p[0], w, tol))) return { kind: 'ellipse', paths };
  const ring = points.length > 1 && near(points[0][0], points[points.length - 1][0], tol) && near(points[0][1], points[points.length - 1][1], tol) ? points.slice(0, -1) : points;
  if (ring.length === 4 && (closed || ring.length !== points.length)) {
    const corners = ring.every((p) => (near(p[0], 0, tol) || near(p[0], w, tol)) && (near(p[1], 0, tol) || near(p[1], h, tol)));
    if (corners) return { kind: 'rectangle', paths };
    const mids = ring.every((p) => (near(p[0], w / 2, tol) && (near(p[1], 0, tol) || near(p[1], h, tol))) || (near(p[1], h / 2, tol) && (near(p[0], 0, tol) || near(p[0], w, tol))));
    if (mids) return { kind: 'diamond', paths };
  }
  return { kind: 'polyline', paths };
}

/** The scene of page `pageIndex` of the engine's drawing. */
export function exportExcalidraw(engine, pageIndex = 0) {
  const model = engine.model();
  const page = model.pages[pageIndex];
  if (!page) throw new Error('There is no such page to export');
  let seed = 1000;
  const base = (type, id, x, y, w, h, more = {}) => ({ id, type, x, y, width: w, height: h, angle: 0, strokeColor: '#1e1e1e', backgroundColor: 'transparent', fillStyle: 'solid', strokeWidth: 2, strokeStyle: 'solid', roughness: 0, opacity: 100, groupIds: [], frameId: null, roundness: null, seed: ++seed, version: 1, versionNonce: ++seed, isDeleted: false, boundElements: null, updated: 1, link: null, locked: false, ...more });
  const elements = [], byShape = {};
  const colour = (c) => (typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c) ? c : null);
  const paint = (s) => {
    const dash = String(s.strokeStyle || '');
    const out = { strokeColor: colour(s.stroke) || '#000000', backgroundColor: colour(s.fill) || 'transparent', strokeWidth: Math.max(1, Math.round(s.strokeWidth || 1)), strokeStyle: /pat2|pat4|pat5|pat6|pat7|pat8/.test(dash) ? 'dashed' : /pat3|pat9|pat1[0-9]/.test(dash) ? 'dotted' : 'solid' };
    if (!colour(s.stroke) || s.strokeWidth === 0) out.strokeColor = s.strokeWidth === 0 ? 'transparent' : out.strokeColor;
    if (typeof s.fillOpacity === 'number' && s.fillOpacity < 1) out.opacity = Math.round(s.fillOpacity * 100);
    return out;
  };
  const textFor = (s, containerId, x, y, w, h) => {
    const t = base('text', `${s.visioId}-t`, x, y, w, h, { strokeColor: colour(s.textColor) || '#000000', strokeWidth: 1, text: s.label, originalText: s.label, fontSize: Math.round((s.fontSize || 13.33) * 10) / 10, fontFamily: FAMILY(s.fontFamily), textAlign: s.align === 'left' ? 'left' : s.align === 'right' ? 'right' : 'center', verticalAlign: containerId ? 'middle' : 'top', containerId: containerId || null, lineHeight: 1.25, autoResize: true, baseline: 0 });
    return t;
  };
  const estimate = (s) => { const lines = String(s.label).split('\n'); const fs = s.fontSize || 13.33; return [Math.max(...lines.map((l) => l.length)) * fs * 0.55, lines.length * fs * 1.25]; };

  for (const s of page.shapes) {
    if (s.type !== 'custom' || !Array.isArray(s.parts) || !s.parts.length) continue;
    const id = `s${s.visioId}`;
    const cls = classify(s.parts, s.w, s.h);
    const angle = ((s.rot || 0) * Math.PI) / 180;
    const style = paint(s);
    const hasLook = style.backgroundColor !== 'transparent' || (style.strokeColor !== 'transparent' && s.strokeWidth !== 0);
    if (cls.kind === 'rectangle' || cls.kind === 'ellipse' || cls.kind === 'diamond') {
      if (!hasLook && s.label) { // text only
        const [tw, th] = estimate(s);
        const t = textFor(s, null, s.x + (s.w - tw) / 2, s.y + (s.h - th) / 2, tw, th); t.angle = angle; elements.push(t); byShape[s.visioId] = { id: t.id, el: t, box: s }; continue;
      }
      const el = base(cls.kind, id, s.x, s.y, s.w, s.h, { ...style, angle, roundness: cls.kind === 'rectangle' && s.rounding > 0 ? { type: 3 } : null });
      elements.push(el); byShape[s.visioId] = { id, el, box: s };
      if (s.label) { const [tw, th] = estimate(s); const t = textFor(s, id, s.x + (s.w - tw) / 2, s.y + (s.h - th) / 2, tw, th); elements.push(t); el.boundElements = [{ id: t.id, type: 'text' }]; }
    } else {
      for (const [k, path] of cls.paths.entries()) {
        const pts = path.points.map(([px, py]) => [px, py]);
        if (pts.length < 2) continue;
        const x0 = Math.min(...pts.map((p) => p[0])), y0 = Math.min(...pts.map((p) => p[1])), x1 = Math.max(...pts.map((p) => p[0])), y1 = Math.max(...pts.map((p) => p[1]));
        const closed = path.closed && style.backgroundColor !== 'transparent';
        const line = base('line', `${id}-${k}`, s.x + pts[0][0], s.y + pts[0][1], x1 - x0, y1 - y0, { ...style, backgroundColor: path.closed ? style.backgroundColor : 'transparent', angle, points: pts.map((p) => [p[0] - pts[0][0], p[1] - pts[0][1]]), lastCommittedPoint: null, startBinding: null, endBinding: null, startArrowhead: null, endArrowhead: null });
        if (path.closed && pts.length > 2 && (pts[0][0] !== pts[pts.length - 1][0] || pts[0][1] !== pts[pts.length - 1][1])) line.points.push([0, 0]);
        if (closed) line.polygon = true;
        elements.push(line);
        if (k === 0) byShape[s.visioId] = { id: line.id, el: line, box: s };
      }
      if (s.label) { const [tw, th] = estimate(s); elements.push(textFor(s, null, s.x + (s.w - tw) / 2, s.y + (s.h - th) / 2, tw, th)); }
    }
  }
  // pictures (step 326): an `image` element and an entry of the scene's `files` with the picture as a data: URL (taken from the page SVG)
  const files = {};
  const imageShapes = page.shapes.filter((s) => s.type === 'image' && s.image && !s.group);
  const pageSvg = imageShapes.length ? engine.svg(pageIndex) : '';
  for (const s of imageShapes) {
    const at = pageSvg.indexOf(`<g data-vid="${s.visioId}"`);
    const m = at < 0 ? null : /href="data:image\/(png|jpe?g|gif|bmp);base64,([^"]+)"/.exec(pageSvg.slice(at, pageSvg.indexOf('</g>', at)));
    if (!m) continue;
    const fileId = `file${s.visioId}`, type = m[1] === 'jpg' ? 'jpeg' : m[1];
    files[fileId] = { mimeType: `image/${type}`, id: fileId, dataURL: `data:image/${type};base64,${m[2]}`, created: 1 };
    const el = base('image', `s${s.visioId}`, s.x, s.y, s.w, s.h, { angle: ((s.rot || 0) * Math.PI) / 180, fileId, status: 'saved', scale: [1, 1] });
    elements.push(el); byShape[s.visioId] = { id: el.id, el, box: s };
  }
  // glued connectors become arrows bound at both ends; the route's bends are the arrow's inner points
  for (const c of page.connectors || []) {
    const from = byShape[String(c.from?.shapeId || '').replace('shape:visio:', '')], to = byShape[String(c.to?.shapeId || '').replace('shape:visio:', '')];
    const edge = (end, ref) => { const b = ref.box; return [b.x + (end.local?.x ?? 0) * b.w, b.y + b.h * (1 - (end.local?.y ?? 0))]; };
    if (!from || !to) continue;
    const p0 = edge(c.from, from), p1 = edge(c.to, to);
    const all = [p0, ...(c.waypoints || []).map((w) => [w.x, w.y]), p1];
    const x0 = Math.min(...all.map((p) => p[0])), y0 = Math.min(...all.map((p) => p[1])), x1 = Math.max(...all.map((p) => p[0])), y1 = Math.max(...all.map((p) => p[1]));
    const arrow = base('arrow', `c${c.visioId}`, p0[0], p0[1], x1 - x0, y1 - y0, { ...paint({ stroke: c.stroke, strokeWidth: c.strokeWidth, strokeStyle: c.strokeStyle, fill: 'none' }), points: all.map((p) => [p[0] - p0[0], p[1] - p0[1]]), lastCommittedPoint: null, startBinding: { elementId: from.id, focus: 0, gap: 1 }, endBinding: { elementId: to.id, focus: 0, gap: 1 }, startArrowhead: ARROW_END[c.arrowStart] ?? null, endArrowhead: ARROW_END[c.arrowEnd] ?? null, roundness: all.length > 2 ? null : { type: 2 } });
    elements.push(arrow);
    for (const ref of [from, to]) ref.el.boundElements = [...(ref.el.boundElements || []), { id: arrow.id, type: 'arrow' }];
  }
  return { type: 'excalidraw', version: 2, source: 'LayoutCraft', elements, appState: { gridSize: null, viewBackgroundColor: '#ffffff' }, files };
}
