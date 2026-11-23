// draw.io shape fidelity (step 310): the outlines of the draw.io built-in shapes that the engine has no native shape for. Every outline is written from the plain geometry of
// the shape (a parallelogram is four corners, a cylinder two ellipse halves and two sides, ...), with the proportions draw.io's defaults give them, not from draw.io's stencil files.
// shapeGeometry(name, style, w, h) returns { outline: [[x, y], ...] (closed, px, y down, inside 0..w x 0..h), details: [{ pts, closed }] (inner lines of the shape: a cylinder's lid, a
// document's fold ...), invisibleMain: true when the shape is only its details (a stick figure) } or null when the name is not known.

const arc = (cx, cy, rx, ry, a0, a1, n = 24) => Array.from({ length: n + 1 }, (_, k) => { const t = a0 + ((a1 - a0) * k) / n; return [cx + rx * Math.cos(t), cy + ry * Math.sin(t)]; });
const PI = Math.PI;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, d) => { const n = parseFloat(v); return Number.isFinite(n) ? n : d; };

/** Scale a point list to fill exactly 0..w x 0..h. */
function fit(points, w, h) {
  const xs = points.map((p) => p[0]), ys = points.map((p) => p[1]);
  const x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.min(...ys), y1 = Math.max(...ys);
  const sx = x1 > x0 ? w / (x1 - x0) : 1, sy = y1 > y0 ? h / (y1 - y0) : 1;
  return points.map(([x, y]) => [(x - x0) * sx, (y - y0) * sy]);
}

/** The flowchart / basic stencil names of draw.io mapped to the base shapes below. */
const ALIASES = {
  'mxgraph.flowchart.decision': 'rhombus', 'mxgraph.flowchart.terminator': 'terminator', 'mxgraph.flowchart.process': 'rect', 'mxgraph.flowchart.alternate_process': 'rounded',
  'mxgraph.flowchart.document': 'document', 'mxgraph.flowchart.multi-document': 'document', 'mxgraph.flowchart.data': 'parallelogram', 'mxgraph.flowchart.predefined_process': 'process',
  'mxgraph.flowchart.database': 'cylinder3', 'mxgraph.flowchart.stored_data': 'dataStorage', 'mxgraph.flowchart.manual_input': 'manualInput', 'mxgraph.flowchart.preparation': 'hexagon',
  'mxgraph.flowchart.start_1': 'terminator', 'mxgraph.flowchart.start_2': 'ellipse', 'mxgraph.flowchart.on-page_reference': 'ellipse', 'mxgraph.flowchart.off-page_reference': 'offPageConnector',
  'mxgraph.flowchart.delay': 'delay', 'mxgraph.flowchart.display': 'display', 'mxgraph.flowchart.summing_function': 'sumEllipse', 'mxgraph.flowchart.or': 'orEllipse',
  'mxgraph.flowchart.internal_storage': 'internalStorage', 'mxgraph.flowchart.loop_limit': 'loopLimit', 'mxgraph.flowchart.paper_tape': 'tape', 'mxgraph.flowchart.manual_operation': 'trapezoidDown',
  'mxgraph.basic.star': 'star', 'mxgraph.basic.pentagon': 'pentagon', 'mxgraph.basic.octagon2': 'octagon', 'mxgraph.basic.cross2': 'cross', 'mxgraph.basic.cube': 'cube', 'mxgraph.basic.document': 'document',
  'mxgraph.bpmn.gateway2': 'bpmnGateway', 'mxgraph.bpmn.event': 'bpmnEvent', 'mxgraph.bpmn.task': 'rounded', 'mxgraph.bpmn.activity': 'rounded', 'lifeline': 'umlLifeline',
  'mxgraph.basic.cloud_callout': 'cloud', 'umlActor': 'umlActor', 'actor': 'actor', 'cylinder': 'cylinder3', 'datastore': 'cylinder3', 'plus': 'cross', 'hexagon': 'hexagon',
};

/** Names this module draws (for the report of what is still approximated). */
export const KNOWN_SHAPES = new Set(['parallelogram', 'trapezoid', 'trapezoidDown', 'hexagon', 'step', 'process', 'cylinder3', 'cloud', 'document', 'note', 'card', 'folder', 'tape', 'actor', 'umlActor', 'delay', 'display', 'cube', 'cross', 'star', 'pentagon', 'octagon', 'singleArrow', 'doubleArrow', 'callout', 'terminator', 'manualInput', 'internalStorage', 'offPageConnector', 'loopLimit', 'dataStorage', 'sumEllipse', 'orEllipse', 'swimlane', 'component', 'umlLifeline', 'bpmnGateway', 'bpmnEvent']);

export function canonicalName(name) { return ALIASES[name] || name; }

export function shapeGeometry(rawName, style, w, h) {
  const name = canonicalName(rawName);
  if (!KNOWN_SHAPES.has(name) || !(w > 0) || !(h > 0)) return null;
  const rect = [[0, 0], [w, 0], [w, h], [0, h]];
  const frac = (key, d, max = 0.5) => clamp(num(style[key], d), 0, max); // a fraction of the width
  const px = (key, d, max) => clamp(num(style[key], d), 0, max); // a length in px
  switch (name) {
    case 'parallelogram': { const s = style.fixedSize === '1' ? px('size', 20, w * 0.9) : frac('size', 0.25) * w; return { outline: [[s, 0], [w, 0], [w - s, h], [0, h]], details: [] }; }
    case 'trapezoid': { const s = style.fixedSize === '1' ? px('size', 20, w / 2) : frac('size', 0.25) * w; return { outline: [[s, 0], [w - s, 0], [w, h], [0, h]], details: [] }; }
    case 'trapezoidDown': { const s = frac('size', 0.25) * w; return { outline: [[0, 0], [w, 0], [w - s, h], [s, h]], details: [] }; }
    case 'hexagon': { const s = style.fixedSize === '1' ? px('size', 20, w / 2) : frac('size', 0.25) * w; return { outline: [[s, 0], [w - s, 0], [w, h / 2], [w - s, h], [s, h], [0, h / 2]], details: [] }; }
    case 'step': { const s = style.fixedSize === '1' ? px('size', 20, w * 0.9) : frac('size', 0.25) * w; return { outline: [[0, 0], [w - s, 0], [w, h / 2], [w - s, h], [0, h], [s, h / 2]], details: [] }; }
    case 'process': { const s = frac('size', 0.125) * w; return { outline: rect, details: [{ pts: [[s, 0], [s, h]], closed: false }, { pts: [[w - s, 0], [w - s, h]], closed: false }] }; }
    case 'internalStorage': { const dx = frac('dx', 0.15, 0.5) * w, dy = frac('dy', 0.15, 0.5) * h; return { outline: rect, details: [{ pts: [[dx, 0], [dx, h]], closed: false }, { pts: [[0, dy], [w, dy]], closed: false }] }; }
    case 'cylinder3': {
      const e = clamp(px('size', 15, h / 2), 1, h / 2); // the height of the elliptical lid
      const top = arc(w / 2, e, w / 2, e, PI, 2 * PI, 24); // left to right over the top
      const bottom = arc(w / 2, h - e, w / 2, e, 0, PI, 24); // right to left under the bottom
      return { outline: [...top, ...bottom], details: [{ pts: arc(w / 2, e, w / 2, e, 0, PI, 24), closed: false }] };
    }
    case 'cloud': { // eight half-circle bumps around an ellipse
      const n = 8, pts = [];
      const base = Array.from({ length: n }, (_, k) => [Math.cos((2 * PI * k) / n) * 0.8, Math.sin((2 * PI * k) / n) * 0.62]);
      for (let k = 0; k < n; k++) {
        const a = base[k], b = base[(k + 1) % n];
        const mx = (a[0] + b[0]) / 2, my = (a[1] + b[1]) / 2, r = Math.hypot(b[0] - a[0], b[1] - a[1]) / 2;
        const a0 = Math.atan2(a[1] - my, a[0] - mx);
        let sweep = Math.atan2(b[1] - my, b[0] - mx) - a0;
        while (sweep <= 0) sweep += 2 * PI;
        const mid = a0 + sweep / 2;
        if (Math.cos(mid) * mx + Math.sin(mid) * my < 0) sweep -= 2 * PI; // take the half circle that bulges away from the centre
        for (const p of arc(mx, my, r, r, a0, a0 + sweep, 10).slice(0, -1)) pts.push(p);
      }
      return { outline: fit(pts, w, h), details: [] };
    }
    case 'document': {
      const f = h * clamp(num(style.size, 0.15), 0, 0.3), wave = [];
      for (let k = 0; k <= 24; k++) { const t = k / 24; wave.push([w * (1 - t), h - f + Math.sin(t * 2 * PI) * -f * 0.9 + f * 0.1]); }
      return { outline: [[0, 0], [w, 0], ...wave], details: [] };
    }
    case 'tape': {
      const f = h * clamp(num(style.size, 0.4), 0, 0.5) / 2, pts = [];
      for (let k = 0; k <= 24; k++) { const t = k / 24; pts.push([w * t, f + Math.sin(t * 2 * PI) * f * 0.9]); }
      for (let k = 24; k >= 0; k--) { const t = k / 24; pts.push([w * t, h - f + Math.sin(t * 2 * PI) * f * 0.9]); }
      return { outline: fit(pts, w, h), details: [] };
    }
    case 'note': { const s = px('size', 30, Math.min(w, h)); return { outline: [[0, 0], [w - s, 0], [w, s], [w, h], [0, h]], details: [{ pts: [[w - s, 0], [w - s, s], [w, s]], closed: false }] }; }
    case 'card': { const s = px('size', 30, Math.min(w, h)); return { outline: [[s, 0], [w, 0], [w, h], [0, h], [0, s]], details: [] }; }
    case 'folder': { const tw = px('tabWidth', 60, w * 0.9), th = px('tabHeight', 20, h * 0.5); return { outline: [[0, 0], [tw, 0], [tw + th * 0.5, th], [w, th], [w, h], [0, h]], details: [] }; }
    case 'cube': { const s = px('size', 20, Math.min(w, h) * 0.9); return { outline: [[s, 0], [w, 0], [w, h - s], [w - s, h], [0, h], [0, s]], details: [{ pts: [[0, s], [w - s, s], [w, 0]], closed: false }, { pts: [[w - s, s], [w - s, h]], closed: false }] }; }
    case 'delay': return { outline: [[0, 0], ...arc(w - h / 2, h / 2, Math.min(h / 2, w), h / 2, -PI / 2, PI / 2, 24).map(([x, y]) => [Math.min(x, w), y]), [0, h]], details: [] };
    case 'display': { const s = Math.min(h / 2, w / 3); return { outline: [[0, h / 2], [s, 0], ...arc(w - s, h / 2, s, h / 2, -PI / 2, PI / 2, 18), [s, h]], details: [] }; }
    case 'terminator': { const r = Math.min(h / 2, w / 2); return { outline: [...arc(w - r, h / 2, r, h / 2, -PI / 2, PI / 2, 18), ...arc(r, h / 2, r, h / 2, PI / 2, (3 * PI) / 2, 18)], details: [] }; }
    case 'manualInput': { const s = h * 0.2; return { outline: [[0, s], [w, 0], [w, h], [0, h]], details: [] }; }
    case 'offPageConnector': return { outline: [[0, 0], [w, 0], [w, h * 0.75], [w / 2, h], [0, h * 0.75]], details: [] };
    case 'loopLimit': { const s = px('size', 20, Math.min(w, h) / 2); return { outline: [[s, 0], [w - s, 0], [w, s], [w, h], [0, h], [0, s]], details: [] }; }
    case 'dataStorage': { // stored data: a concave right side and a convex left side
      const sd = frac('size', 0.12) * w, pts = [[sd, 0], [w, 0]];
      for (let k = 1; k <= 12; k++) { const t = k / 12; pts.push([w - sd * Math.sin(PI * t), h * t]); }
      pts.push([sd, h]);
      for (let k = 11; k >= 1; k--) { const t = k / 12; pts.push([sd - sd * Math.sin(PI * t), h * t]); }
      return { outline: pts, details: [] };
    }
    case 'cross': { const t = clamp(num(style.size, 0.33), 0.1, 0.9), a = (w * (1 - t)) / 2, b = (h * (1 - t)) / 2; return { outline: [[a, 0], [w - a, 0], [w - a, b], [w, b], [w, h - b], [w - a, h - b], [w - a, h], [a, h], [a, h - b], [0, h - b], [0, b], [a, b]], details: [] }; }
    case 'star': { const pts = []; for (let k = 0; k < 10; k++) { const r = k % 2 === 0 ? 1 : clamp(num(style.size, 0.38), 0.2, 0.8); const t = -PI / 2 + (PI * k) / 5; pts.push([Math.cos(t) * r, Math.sin(t) * r]); } return { outline: fit(pts, w, h), details: [] }; }
    case 'pentagon': return { outline: fit(Array.from({ length: 5 }, (_, k) => [Math.cos(-PI / 2 + (2 * PI * k) / 5), Math.sin(-PI / 2 + (2 * PI * k) / 5)]), w, h), details: [] };
    case 'octagon': { const s = 0.29; return { outline: [[w * s, 0], [w * (1 - s), 0], [w, h * s], [w, h * (1 - s)], [w * (1 - s), h], [w * s, h], [0, h * (1 - s)], [0, h * s]], details: [] }; }
    case 'singleArrow': case 'doubleArrow': {
      const aw = clamp(num(style.arrowWidth, 0.5), 0.1, 1) * h, hl = Math.min(clamp(num(style.arrowSize, 0.4), 0.05, 1) * h, name === 'doubleArrow' ? w / 2 : w), m = (h - aw) / 2;
      if (name === 'singleArrow') return { outline: [[0, m], [w - hl, m], [w - hl, 0], [w, h / 2], [w - hl, h], [w - hl, h - m], [0, h - m]], details: [] };
      return { outline: [[0, h / 2], [hl, 0], [hl, m], [w - hl, m], [w - hl, 0], [w, h / 2], [w - hl, h], [w - hl, h - m], [hl, h - m], [hl, h]], details: [] };
    }
    case 'callout': { const t = Math.min(h * 0.25, 25), tx = clamp(num(style.position, 0.3), 0.1, 0.9) * w; return { outline: [[0, 0], [w, 0], [w, h - t], [tx + t, h - t], [tx - t * 0.3, h], [tx, h - t], [0, h - t]], details: [] }; }
    case 'actor': { // a person: a head and shoulders silhouette
      const hr = Math.min(w, h) * 0.22;
      return { outline: fit([[0.0, 1.0], [0.0, 0.62], [0.2, 0.5], [0.34, 0.46], [0.3, 0.36], [0.28, 0.2], [0.35, 0.05], [0.5, 0.0], [0.65, 0.05], [0.72, 0.2], [0.7, 0.36], [0.66, 0.46], [0.8, 0.5], [1.0, 0.62], [1.0, 1.0]].map(([x, y]) => [x * w, y * h]), w, h), details: [], hr };
    }
    case 'umlActor': { // a stick figure: the head, the body, arms and legs (the shape itself is invisible)
      const cx = w / 2, hr = Math.min(w * 0.25, h * 0.12);
      return { outline: rect, invisibleMain: true, details: [{ pts: arc(cx, hr, hr, hr, 0, 2 * PI, 24), closed: true }, { pts: [[cx, hr * 2], [cx, h * 0.62]], closed: false }, { pts: [[0, h * 0.34], [w, h * 0.34]], closed: false }, { pts: [[0, h], [cx, h * 0.62], [w, h]], closed: false }] };
    }
    case 'sumEllipse': case 'orEllipse': {
      const el = arc(w / 2, h / 2, w / 2, h / 2, 0, 2 * PI, 48).slice(0, -1);
      const k = 0.7071, cx = w / 2, cy = h / 2;
      const lines = name === 'orEllipse' ? [[[cx, 0], [cx, h]], [[0, cy], [w, cy]]] : [[[cx - (w / 2) * k, cy - (h / 2) * k], [cx + (w / 2) * k, cy + (h / 2) * k]], [[cx - (w / 2) * k, cy + (h / 2) * k], [cx + (w / 2) * k, cy - (h / 2) * k]]];
      return { outline: el, details: lines.map((pts) => ({ pts, closed: false })) };
    }
    case 'component': { // a box with two small tabs on its left edge (UML component)
      const tw = Math.min(20, w / 4), th = Math.min(10, h / 5), tab = (cy) => ({ pts: [[0, cy - th / 2], [tw, cy - th / 2], [tw, cy + th / 2], [0, cy + th / 2]], closed: true });
      return { outline: rect, details: [tab(h * 0.3), tab(h * 0.7)] };
    }
    case 'umlLifeline': { // UML sequence diagram: a header box and a vertical line below it (the outline runs down the line and back, so the shape keeps the whole height)
      const hs = clamp(num(style.size, 40), 8, h), cx = w / 2;
      return { outline: [[0, 0], [w, 0], [w, hs], [cx, hs], [cx, h], [cx, hs], [0, hs]], details: [] };
    }
    case 'bpmnGateway': { // BPMN gateway: a diamond with the marker of its kind (symbol=exclusiveGw / parallelGw / inclusiveGw / eventGw / complexGw)
      const cx = w / 2, cy = h / 2, kx = w * 0.17, ky = h * 0.17, sym = style.symbol || '';
      const ring = (r) => ({ pts: arc(cx, cy, w * r, h * r, 0, 2 * PI, 32), closed: true });
      const x = [{ pts: [[cx - kx, cy - ky], [cx + kx, cy + ky]], closed: false }, { pts: [[cx - kx, cy + ky], [cx + kx, cy - ky]], closed: false }];
      const plus = [{ pts: [[cx - kx * 1.4, cy], [cx + kx * 1.4, cy]], closed: false }, { pts: [[cx, cy - ky * 1.4], [cx, cy + ky * 1.4]], closed: false }];
      let details = x;
      if (sym === 'parallelGw') details = plus;
      else if (sym === 'inclusiveGw') details = [ring(0.22)];
      else if (sym === 'eventGw') details = [ring(0.24), ring(0.18)];
      else if (sym === 'complexGw') details = [...x, ...plus];
      return { outline: [[cx, 0], [w, cy], [cx, h], [0, cy]], details };
    }
    case 'bpmnEvent': { // BPMN event: a circle; catching / throwing events have an inner ring, an end event a second, outer ring
      const kind = style.outline || 'standard', cx = w / 2, cy = h / 2, ring = (k) => ({ pts: arc(cx, cy, (w / 2) * k, (h / 2) * k, 0, 2 * PI, 32), closed: true });
      const details = [];
      if (['catching', 'throwing', 'boundInt', 'boundNonint', 'eventNonint'].includes(kind)) details.push(ring(0.82));
      if (kind === 'end') details.push(ring(0.9), ring(0.8));
      return { outline: arc(cx, cy, w / 2, h / 2, 0, 2 * PI, 48).slice(0, -1), details };
    }
    case 'swimlane': { const hs = clamp(num(style.startSize, 23), 8, h * 0.9); return { outline: rect, details: [{ pts: [[0, hs], [w, hs]], closed: false }] }; }
    default: return null;
  }
}
