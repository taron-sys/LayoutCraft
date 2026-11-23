// Document model: pages, shapes, connectors, plus undo/redo history.
import { uid, FONT_STACK } from './util.js';
import { SCHEMA_VERSION } from './schema.js';

export const DEFAULTS = {
  fill: '#dbe9f6',
  fillOpacity: 1,
  stroke: '#3b6ea5',
  strokeWidth: 1,
  strokeStyle: 'solid',
  fontSize: 12,
  fontFamily: FONT_STACK,
  textColor: '#1b2733',
  bold: false,
  italic: false,
  underline: false,
  align: 'center',
  vAlign: 'middle',
};

export const DATA_COLOR_DEFAULTS = {
  low: '#f8696b',
  mid: '#ffeb84',
  high: '#63be7b',
};

export function newPage(name = 'Page-1') {
  return { id: uid('p'), name, shapes: [], connectors: [], groups: [], containers: [] };
}

export function newDoc(name = 'Untitled Diagram') {
  return {
    name,
    version: 1,
    schemaVersion: SCHEMA_VERSION,
    pages: [newPage()],
    pageIndex: 0,
  };
}

export function newShape(type, x = 0, y = 0, over = {}) {
  return {
    id: uid('s'),
    type,
    x, y,
    w: 120, h: 70,
    rot: 0,
    flipX: false,
    flipY: false,
    label: '',
    group: null,
    ...structuredClone(DEFAULTS),
    ...over,
  };
}

/**
 * A shape drawn from explicit geometry rather than the shape catalogue.
 * `parts` holds one or more sub-paths, each with its own fill and stroke —
 * that is how Visio draws things like 3-D blocks, where a bevelled rim and
 * a flat front face are separate shapes.
 */
export function newCustomShape(x = 0, y = 0, w = 120, h = 70, over = {}) {
  const s = newShape('custom', x, y, { w, h, ...over });
  s.parts = [];
  return s;
}

export function newConnector(from, to, over = {}) {
  return {
    id: uid('c'),
    from: { shapeId: null, side: null, point: null, ...from },
    to: { shapeId: null, side: null, point: null, ...to },
    route: 'straight',        // straight | elbow | curved
    waypoints: [],
    label: '',
    group: null,
    stroke: '#3b6ea5',
    strokeWidth: 1,
    strokeStyle: 'solid',
    arrowStart: 'none',
    arrowEnd: 'arrow',
    ...over,
  };
}

export const page = (doc) => doc.pages[doc.pageIndex] || doc.pages[0];

export function findShape(doc, id) {
  for (const p of doc.pages) {
    const s = p.shapes.find((x) => x.id === id);
    if (s) return s;
  }
  return null;
}

export function findConnector(doc, id) {
  for (const p of doc.pages) {
    const c = p.connectors.find((x) => x.id === id);
    if (c) return c;
  }
  return null;
}

export function shapeById(pg, id) {
  return pg.shapes.find((s) => s.id === id) || null;
}

/** Snapshot / restore for undo-redo. */
export class History {
  constructor(limit = 200) {
    this.limit = limit;
    this.undoStack = [];
    this.redoStack = [];
  }

  /**
   * Picture bytes are large and immutable, and a document can reuse the same
   * media many times. Snapshots therefore keep the `media` map by reference
   * instead of serialising every data URL into every undo step; records are
   * only ever added, never mutated or removed, so a shared map is safe. The
   * rest of the document is a JSON string.
   */
  snapshot(doc) {
    const { media, ...rest } = doc;
    return { json: JSON.stringify(rest), media };
  }

  pushSnapshot(snap) {
    this.undoStack.push(snap);
    if (this.undoStack.length > this.limit) this.undoStack.shift();
    this.redoStack.length = 0;
  }

  /** Call *before* mutating the document. */
  push(doc) {
    this.pushSnapshot(this.snapshot(doc));
  }

  undo(doc) {
    if (!this.undoStack.length) return null;
    const cur = this.snapshot(doc);
    const prev = this.undoStack.pop();
    this.redoStack.push(cur);
    const restored = JSON.parse(prev.json);
    if (prev.media) restored.media = prev.media;
    return restored;
  }

  redo(doc) {
    if (!this.redoStack.length) return null;
    const cur = this.snapshot(doc);
    const next = this.redoStack.pop();
    this.undoStack.push(cur);
    const restored = JSON.parse(next.json);
    if (next.media) restored.media = next.media;
    return restored;
  }

  get canUndo() { return this.undoStack.length > 0; }
  get canRedo() { return this.redoStack.length > 0; }
}

export function shapeRect(s) {
  return { x: s.x, y: s.y, w: s.w, h: s.h };
}

/** A Shape Data row's value by row name (Row_1, ...), or ''. */
export function dataRowValue(shape, name) {
  const row = (shape?.data || []).find((d) => String(d.name) === String(name));
  return row ? String(row.value ?? '') : '';
}

/** The target of a data callout, or null. */
export function calloutTarget(shape, pg) {
  if (!shape?.callout || !shape.attachedTo) return null;
  return pg.shapes.find((s) => s.id === shape.attachedTo) || null;
}

/**
 * The visible text of a data callout: an optional prefix plus the live value
 * of the target's Shape Data row. It is derived, never stored as shape.label.
 */
export function calloutText(shape, pg) {
  if (!shape?.callout) return shape?.label || '';
  // An icon set is a picture glyph; it carries a value but shows no text.
  if (shape.callout.kind === 'icon') return '';
  const target = calloutTarget(shape, pg);
  if (!target) return '(missing target)';
  const value = dataRowValue(target, shape.callout.field);
  if (shape.callout.kind === 'bar' && shape.callout.showValue === false) {
    return shape.callout.label || '';
  }
  const prefix = shape.callout.label ? `${shape.callout.label} ` : '';
  const text = `${prefix}${value}`.trim();
  return text || dataRowValue(target, shape.callout.field) || '…';
}

/** 0..1 fill of a numeric data bar, relative to its min/max. */
export function barRatio(shape, pg) {
  if (!shape?.callout || shape.callout.kind !== 'bar') return 0;
  const target = calloutTarget(shape, pg);
  if (!target) return 0;
  const raw = dataRowValue(target, shape.callout.field).replace(/[^0-9.+-]/g, '').trim();
  const value = parseFloat(raw);
  if (!Number.isFinite(value)) return 0;
  let min = Number(shape.callout.min);
  let max = Number(shape.callout.max);
  if (!Number.isFinite(min)) min = 0;
  if (!Number.isFinite(max)) max = 100;
  if (max === min) return value > max ? 1 : 0;
  const t = (value - min) / (max - min);
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

/**
 * Resolve a Shape Data colour rule to the colour that should paint the target.
 * Three equal bands run from min to max; non-numeric values leave the shape's
 * own fill alone.
 */
export function dataColorFill(shape) {
  if (!shape?.dataColor) return null;
  const raw = dataRowValue(shape, shape.dataColor.field)
    .replace(/[^0-9.+-]/g, '').trim();
  const value = parseFloat(raw);
  if (!Number.isFinite(value)) return null;
  let min = Number(shape.dataColor.min);
  let max = Number(shape.dataColor.max);
  if (!Number.isFinite(min)) min = 0;
  if (!Number.isFinite(max)) max = 100;
  const low = shape.dataColor.low || DATA_COLOR_DEFAULTS.low;
  const mid = shape.dataColor.mid || DATA_COLOR_DEFAULTS.mid;
  const high = shape.dataColor.high || DATA_COLOR_DEFAULTS.high;
  if (max === min) return value >= max ? high : low;
  const t = (value - min) / (max - min);
  return t < 1 / 3 ? low : t < 2 / 3 ? mid : high;
}

/**
 * Which of five icon levels a numeric value selects. The first band starts at
 * min; a value at max or above selects the fifth level.
 */
export function iconLevel(shape, pg) {
  if (!shape?.callout || shape.callout.kind !== 'icon') return 0;
  const target = calloutTarget(shape, pg);
  if (!target) return 1;
  const raw = dataRowValue(target, shape.callout.field).replace(/[^0-9.+-]/g, '').trim();
  const value = parseFloat(raw);
  if (!Number.isFinite(value)) return 1;
  let min = Number(shape.callout.min);
  let max = Number(shape.callout.max);
  if (!Number.isFinite(min)) min = 0;
  if (!Number.isFinite(max)) max = 100;
  if (max === min) return value > max ? 5 : 1;
  const t = (value - min) / (max - min);
  const clamped = t < 0 ? 0 : t > 1 ? 1 : t;
  return Math.max(1, Math.min(5, Math.floor(clamped * 5) + 1));
}

export function connectorsFor(pg, shapeId) {
  return pg.connectors.filter((c) => c.from.shapeId === shapeId || c.to.shapeId === shapeId);
}
