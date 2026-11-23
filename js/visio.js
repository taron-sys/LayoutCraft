// Import Microsoft Visio files (.vsdx and .vdx) into a LayoutCraft document.
//
// A .vsdx is a ZIP of XML parts. Shapes carry a flat set of "cells"
// (PinX, PinY, Width, ...) measured in inches, and most of them inherit their
// geometry and styling from a *master* — often through a chain of nested
// groups, as in BPMN cross-functional flowcharts. This module resolves that
// inheritance and flattens the result into plain boxes.
//
// The legacy binary .vsd format is not supported.
import { newPage, newShape, newCustomShape, newConnector } from './model.js';
import { SCHEMA_VERSION } from './schema.js';

const VISIO_BUILTIN_COLORS = [
  '#000000', '#ffffff', '#ff0000', '#00ff00', '#0000ff', '#ffff00',
  '#ff00ff', '#00ffff', '#800000', '#008000', '#000080', '#808000',
  '#800080', '#008080', '#c0c0c0', '#e6e6e6', '#cdcdcd', '#b3b3b3',
  '#9a9a9a', '#808080', '#666666', '#4d4d4d', '#333333', '#1a1a1a',
  '#7f7f7f',
];

// Office fallback when a document carries no theme part (or a minimal one).
const DEFAULT_THEME_SLOTS = {
  dk1: '#000000', lt1: '#ffffff', dk2: '#44546a', lt2: '#e7e6e6',
  accent1: '#4472c4', accent2: '#ed7d31', accent3: '#a5a5a5',
  accent4: '#ffc000', accent5: '#5b9bd5', accent6: '#70ad47',
  hlink: '#0563c1', folhlink: '#954f72', bkgnd: '#ffffff',
};
const DEFAULT_THEME_VARIATIONS = [0, 1, 2, 3].map((shift) => {
  const out = {};
  const accents = ['#4472c4', '#ed7d31', '#a5a5a5', '#ffc000', '#5b9bd5', '#70ad47'];
  for (let i = 0; i < 6; i++) out[`variant${i + 1}`] = accents[(i + shift) % 6];
  return out;
});

const DPI = 96;                 // pixels per inch
const r2 = (n) => Math.round(n * 100) / 100;

// Package media that a browser <image> can display. EMF/WMF/TIFF pictures are
// kept so a patch save is lossless, but the editor shows a placeholder.
const MEDIA_MIME = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif',
  bmp: 'image/bmp', webp: 'image/webp', svg: 'image/svg+xml',
  tif: 'image/tiff', tiff: 'image/tiff', emf: 'image/x-emf', wmf: 'image/x-wmf',
};
const DISPLAYABLE_MEDIA = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);

/* ------------------------------------------------------------------ zip -- */
async function readZip(buffer) {
  const u8 = new Uint8Array(buffer);
  const dv = new DataView(buffer);
  let eocd = -1;
  for (let i = u8.length - 22; i >= 0 && i >= u8.length - 22 - 0xffff; i--) {
    if (dv.getUint32(i, true) === 0x06054b50) { eocd = i; break; }
  }
  if (eocd < 0) throw new Error('Not a ZIP archive — is this really a .vsdx file?');

  const count = dv.getUint16(eocd + 10, true);
  let off = dv.getUint32(eocd + 16, true);
  const entries = [];
  const dec = new TextDecoder();

  for (let i = 0; i < count && off + 46 <= u8.length; i++) {
    if (dv.getUint32(off, true) !== 0x02014b50) break;
    const method = dv.getUint16(off + 10, true);
    const compSize = dv.getUint32(off + 20, true);
    const nameLen = dv.getUint16(off + 28, true);
    const extraLen = dv.getUint16(off + 30, true);
    const commentLen = dv.getUint16(off + 32, true);
    const localOff = dv.getUint32(off + 42, true);
    entries.push({
      name: dec.decode(u8.subarray(off + 46, off + 46 + nameLen)),
      method, compSize, localOff,
    });
    off += 46 + nameLen + extraLen + commentLen;
  }

  const files = new Map();
  for (const e of entries) {
    const lNameLen = dv.getUint16(e.localOff + 26, true);
    const lExtraLen = dv.getUint16(e.localOff + 28, true);
    const start = e.localOff + 30 + lNameLen + lExtraLen;
    const raw = u8.subarray(start, start + e.compSize);
    if (e.method === 0) files.set(e.name, raw);
    else if (e.method === 8) files.set(e.name, await inflateRaw(raw));
    else throw new Error(`Unsupported ZIP compression method ${e.method}`);
  }
  return files;
}

async function inflateRaw(data) {
  const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  const buf = await new Response(stream).arrayBuffer();
  return new Uint8Array(buf);
}

const zipText = (zip, path) => {
  const u8 = zip.get(path);
  return u8 ? new TextDecoder().decode(u8) : null;
};

function joinPath(base, rel) {
  const out = [];
  for (const part of `${base}/${rel}`.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') out.pop();
    else out.push(part);
  }
  return out.join('/');
}

/* ------------------------------------------------------------------ xml -- */
function parseXml(text) {
  const doc = new DOMParser().parseFromString(text, 'application/xml');
  const bad = doc.querySelector('parsererror');
  if (bad) throw new Error(`Malformed XML: ${bad.textContent.slice(0, 120)}`);
  return doc;
}

/** All descendants with the given local name (namespace agnostic). */
function findAll(root, local) {
  const out = [];
  const walk = (node) => {
    for (const child of node.children) {
      if (child.localName === local) out.push(child);
      walk(child);
    }
  };
  walk(root);
  return out;
}

const kids = (el, local) => (el ? [...el.children].filter((c) => c.localName === local) : []);
const kid = (el, local) => kids(el, local)[0] || null;
const section = (el, name) => kids(el, 'Section').find((s) => s.getAttribute('N') === name) || null;

/* --------------------------------------------------------------- values -- */
/**
 * Cell value in internal units — inches for lengths, radians for angles.
 * The `U` attribute only records the unit Visio displays the cell in; the
 * cached `V` is always inches (0.1111 in is 8 pt, 0.25 in is 18 pt), so
 * applying `U` here would shrink every PT/MM cell by 72x or 25.4x.
 */
function inches(cell, fallback = 0) {
  if (!cell) return fallback;
  const raw = cell.getAttribute('V');
  if (raw === null || raw === '') return fallback;
  const n = parseFloat(raw);
  return Number.isFinite(n) ? n : fallback;
}

const cellValue = (cells, name, fallback = 0) => inches(cells.get(name), fallback);

const cellsOf = (el) => {
  const out = new Map();
  for (const c of kids(el, 'Cell')) {
    const n = c.getAttribute('N');
    if (n && !out.has(n)) out.set(n, c);
  }
  return out;
};

/** A User.* cell or a named row in a shape's User section. */
function userValue(el, name) {
  if (!el) return '';
  const direct = cellsOf(el).get(`User.${name}`);
  if (direct) return String(direct.getAttribute('V') ?? '').replace(/^"|"$/g, '');
  const sec = kids(el, 'Section').find((s) => s.getAttribute('N') === 'User');
  if (!sec) return '';
  for (const row of kids(sec, 'Row')) {
    if (row.getAttribute('N') !== name) continue;
    for (const c of kids(row, 'Cell')) {
      if (c.getAttribute('N') === 'Value') {
        return String(c.getAttribute('V') ?? '').replace(/^"|"$/g, '');
      }
    }
  }
  return '';
}

const NATIVE_DG_KINDS = {
  'text callout': 'text',
  heading: 'heading',
  'data bar': 'bar',
  'icon set': 'icon',
};

/** `Sheet.22!Prop.IPAddress` -> { id: '22', name: 'IPAddress' } (else null). */
export function parsePropertyReference(formula) {
  const m = /^\s*Sheet\.(\d+)!Prop\.([A-Za-z0-9_]+)/.exec(String(formula || ''));
  return m ? { id: m[1], name: m[2] } : null;
}

/**
 * Recognise a Visio-native data graphic instance (a master whose User section
 * has msvCalloutType). Metadata only: the shape keeps Visio's cached geometry
 * and is never regenerated from this. Mirrors the Rust importer.
 */
function nativeDataGraphicOf(r) {
  const type = userValue(r.el, 'msvCalloutType') || userValue(r.mshape, 'msvCalloutType');
  const kind = NATIVE_DG_KINDS[String(type).trim().toLowerCase()];
  if (!kind) return null;
  const prop = kids(r.el, 'Section').find((s) => s.getAttribute('N') === 'Property');
  const row = prop && kids(prop, 'Row').find((x) => x.getAttribute('N') === 'msvCalloutField');
  const cell = row && kids(row, 'Cell').find((c) => c.getAttribute('N') === 'Value');
  const ref = cell ? parsePropertyReference(cell.getAttribute('F')) : null;
  return {
    kind,
    master: r.masterName || '',
    targetVisioId: ref ? ref.id : null,
    field: ref ? ref.name : '',
    label: userValue(r.el, 'LabelText') || userValue(r.mshape, 'LabelText'),
    value: cell ? String(cell.getAttribute('V') ?? '').replace(/^"|"$/g, '') : '',
  };
}

/**
 * Visio serialises container/list membership in a Relationships cell:
 * SUM(DEPENDSON(type, Sheet.<id>!SheetRef(), ...)).
 * Types seen in real files: 1 container→members, 2 list→items,
 * 4 member→containers, 5 list-item→list, 11 list↔list.
 */
function parseRelationships(cells) {
  const c = cells.get('Relationships');
  if (!c) return [];
  const f = c.getAttribute('F') || '';
  if (!/DEPENDSON/i.test(f)) return [];
  const out = [];
  // DEPENDSON's arguments contain SheetRef() calls, so match the whole call
  // explicitly instead of stopping at the first ')'.
  const re = /DEPENDSON\s*\(\s*(\d+)\s*((?:,\s*Sheet\.[A-Za-z0-9_]+!SheetRef\s*\(\s*\))*)\s*\)/gi;
  let m;
  while ((m = re.exec(f))) {
    const type = Number(m[1]);
    const refs = [...m[2].matchAll(/Sheet\.([A-Za-z0-9_]+)!SheetRef/g)]
      .map((x) => String(x[1]));
    if (refs.length) out.push({ type, refs });
  }
  return out;
}

const canon = (s) => String(s || '').toLowerCase().replace(/[^a-z0-9]/g, '');

/** A cell holds a usable value rather than a placeholder like "Themed". */
const isConcrete = (cell) => {
  const v = cell.getAttribute('V');
  if (!v || v === 'Themed') return false;
  if (v.startsWith('#')) return true;
  return Number.isFinite(parseFloat(v));
};

/* --------------------------------------------------------------- colour -- */
const clamp255 = (v) => Math.max(0, Math.min(255, Math.round(v)));
const toHex = (r, g, b) => `#${[r, g, b].map((v) => clamp255(v).toString(16).padStart(2, '0')).join('')}`;

/** Mix a colour towards white (n > 0) or black (n < 0), as Visio's MSOTINT does. */
function tint(hex, n) {
  if (!hex || hex === 'none' || !hex.startsWith('#')) return hex;
  if (!Number.isFinite(n)) return hex;
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const t = Math.abs(n) / 100;
  const target = n >= 0 ? 255 : 0;
  return toHex(r + (target - r) * t, g + (target - g) * t, b + (target - b) * t);
}

/**
 * Visio's colour system: a palette of indexed colours plus a theme whose
 * slots (accent1, dk1, ...) are referenced by formulas like
 * THEMEGUARD(MSOTINT(THEMEVAL("accentcolor"),-50)).
 */
class Theme {
  constructor() {
    this.colors = new Map();     // index -> #rrggbb
    this.slots = new Map();      // name  -> #rrggbb
    this.variations = [];        // [{ variant1..7 }]
    this.activeVariation = 0;
    this.name = '';
    this.schemeEnum = null;
  }

  load(zip, docXml) {
    for (const c of findAll(docXml, 'ColorEntry')) {
      const rgb = c.getAttribute('RGB');
      if (rgb) this.colors.set(c.getAttribute('IX'), `#${rgb.replace('#', '').toLowerCase()}`);
    }
    // the Visio theme is the one carrying the vt: extensions; the other is
    // the plain Office theme and must not win
    for (const path of [...zip.keys()].filter((k) => /theme\/theme\d+\.xml$/i.test(k))) {
      const text = zipText(zip, path);
      if (!text) continue;
      const isVisio = /variationClrSchemeLst|<vt:bkgnd/.test(text);
      const slots = this.readScheme(text);
      if (slots.size && (isVisio || !this.slots.size)) {
        this.slots = slots;
        try {
          const doc = parseXml(text);
          const themeEl = findAll(doc, 'theme')[0];
          if (themeEl) this.name = themeEl.getAttribute('name') || this.name;
          const sid = findAll(doc, 'schemeID')[0];
          if (sid) this.schemeEnum = sid.getAttribute('schemeEnum') || null;
        } catch {}
        this.readExtras(text, zip);
      }
    }
    if (!this.slots.size) {
      this.slots = new Map(Object.entries(DEFAULT_THEME_SLOTS));
      this.variations = DEFAULT_THEME_VARIATIONS.map((v) => ({ ...v }));
      this.name = 'Office';
      this.activeVariation = 0;
      for (const [k, v] of Object.entries(this.variations[0])) this.slots.set(k, v);
    } else if (!this.variations.length) {
      this.variations = DEFAULT_THEME_VARIATIONS.map((v) => ({ ...v }));
      this.activeVariation = 0;
      for (const [k, v] of Object.entries(this.variations[0])) this.slots.set(k, v);
    }
  }

  toJSON() {
    return {
      name: this.name || 'Office',
      schemeEnum: this.schemeEnum,
      slots: Object.fromEntries(this.slots),
      variations: this.variations.map((v) => ({ ...v })),
      activeVariation: this.activeVariation,
    };
  }

  readScheme(text) {
    const slots = new Map();
    let doc;
    try { doc = parseXml(text); } catch { return slots; }
    for (const scheme of findAll(doc, 'clrScheme')) {
      for (const entry of scheme.children) {
        const hex = [...entry.children]
          .map((c) => {
            const v = c.getAttribute('val') || '';
            if (/^[0-9a-f]{6}$/i.test(v)) return v;
            const last = c.getAttribute('lastClr') || '';
            return /^[0-9a-f]{6}$/i.test(last) ? last : '';
          })
          .find(Boolean);
        if (hex) slots.set(canon(entry.localName), `#${hex.toLowerCase()}`);
      }
      break;
    }
    return slots;
  }

  /**
   * The vt: extensions of a Visio theme carry the variation palette and the
   * theme background; both are referenced by THEMEVAL("VariantColorN") and
   * THEMEVAL("BackgroundColor").
   */
  readExtras(text, zip = null) {
    let doc;
    try { doc = parseXml(text); } catch { return; }

    const bg = findAll(doc, 'bkgnd')[0];
    if (bg) {
      const hex = firstThemeColor(bg);
      if (hex) this.slots.set('bkgnd', hex);
    }

    const list = findAll(doc, 'variationClrSchemeLst')[0];
    const schemes = list ? kids(list, 'variationClrScheme') : [];
    if (!schemes.length) return;
    this.variations = schemes.map((scheme) => {
      const colors = {};
      for (const child of scheme.children) {
        const m = /^varColor(\d+)$/i.exec(child.localName || '');
        if (!m) continue;
        const hex = firstThemeColor(child);
        if (hex) colors[`variant${m[1]}`] = hex;
      }
      return colors;
    });
    // Pick the active variation palette. The document's theme index is not
    // always available, so score the palettes against the concrete fills the
    // file itself uses; without evidence, the first palette is the default.
    const explicit = explicitFillScores(zip);
    let chosen = 0;
    let best = 0;
    this.variations.forEach((colors, i) => {
      let score = 0;
      for (const hex of Object.values(colors)) {
        if (hex && explicit.has(hex)) score += explicit.get(hex);
      }
      if (score > best) { best = score; chosen = i; }
    });
    this.activeVariation = chosen;
    for (const [key, hex] of Object.entries(this.variations[chosen])) {
      this.slots.set(key, hex);
    }
  }

  baseColor() {
    return this.slots.get('accent1') || this.slots.get('dk1') || '#3b6ea5';
  }

  /**
   * Resolve a QuickStyle*Color cell. 0-8 index the base theme colours;
   * 100-106 index the active variation's seven colours.
   */
  quickColor(value) {
    if (value === null || value === undefined || value === '') return null;
    const n = Number(value);
    if (!Number.isFinite(n)) return null;
    if (n >= 100 && n <= 106) return this.slots.get(`variant${n - 99}`) || null;
    const keys = ['dk1', 'lt1', 'accent1', 'accent2', 'accent3',
      'accent4', 'accent5', 'accent6', 'bkgnd'];
    return this.slots.get(keys[n]) || null;
  }

  /**
   * A theme slot by the name used in Visio formulas. `fallback` is for
   * THEMEVAL/Theme(); a bare cell reference must return null instead of
   * inventing a colour.
   */
  slot(name, fallback = true) {
    const key = canon(name || '');
    const base = this.baseColor();
    if (key) {
      const direct = this.slots.get(key);
      if (direct) return direct;
      let m;
      if (key === 'accentcolor') return this.slots.get('accent1') || base;
      if ((m = key.match(/^accentcolor([1-6])$/))) {
        return this.slots.get(`accent${m[1]}`) || base;
      }
      if ((m = key.match(/^variantcolor(\d+)$/))) {
        const n = Number(m[1]);
        return this.slots.get(`variant${n}`)
          || this.slots.get(`accent${((n - 1) % 6) + 1}`)
          || base;
      }
      if (key === 'dark') return this.slots.get('dk1') || this.slots.get('dk2') || '#000000';
      if (key === 'light') return this.slots.get('lt1') || '#ffffff';
      if (key === 'backgroundcolor') {
        return this.slots.get('bkgnd') || this.slots.get('lt1') || '#ffffff';
      }
      if (key === 'textcolor') return this.slots.get('dk2') || tint(base, -50);
      if (key === 'fillcolor') return this.slots.get('accent1') || base;
      if (key === 'fillcolor2') {
        return this.slots.get('accent2') || this.slots.get('accent1') || base;
      }
      if (key === 'connectorcolor') return tint(this.slots.get('accent1') || base, -7);
    }
    return fallback ? base : null;
  }

  /** Resolve a cell to a colour, preferring the cached value. */
  colorOf(cell) {
    if (!cell) return null;
    const v = cell.getAttribute('V');
    if (v && v.startsWith('#')) return v.toLowerCase();
    const f = cell.getAttribute('F');
    if (f) {
      const c = this.evalFormula(f);
      if (c) return c;
    }
    if (v === 'Themed') return this.slot('accent1');
    if (v && /^\d+$/.test(v)) {
      const n = Number(v);
      return this.colors.get(v) || VISIO_BUILTIN_COLORS[n] || null;
    }
    return null;
  }

  /**
   * Role-aware Themed resolution. A bare THEMEVAL()/"Themed" cell means "the
   * theme's value for this cell's role"; without a quick-style matrix that
   * resolves to the document's light/dark defaults, not to accent1. Explicit
   * THEMEVAL("FillColor") etc. still go through the named-slot evaluator.
   */
  themedColor(cell, role) {
    if (!cell) return null;
    const v = cell.getAttribute('V');
    const f = cell.getAttribute('F') || '';
    if (v === 'Themed' && (!f || /^THEMEVAL\(\s*\)$/i.test(f) || f === 'Inh')) {
      if (role === 'fill') return this.slots.get('lt1') || '#ffffff';
      if (role === 'line') return this.slots.get('dk1') || '#000000';
      if (role === 'text') return this.slots.get('dk2') || this.slots.get('dk1') || '#000000';
    }
    return this.colorOf(cell);
  }

  /** Evaluate the colour subset of Visio's formula language. */
  evalFormula(f) {
    const v = new FormulaParser(f, this).parse();
    if (typeof v === 'string') {
      if (/^#[0-9a-f]{6}$/i.test(v)) return v.toLowerCase();
      return this.slot(v, false);
    }
    return null;
  }
}

/** First hex colour inside a theme element (srgbClr / sysClr lastClr). */
function firstThemeColor(el) {
  if (!el) return null;
  for (const c of findAll(el, 'srgbClr')) {
    const v = c.getAttribute('val') || '';
    if (/^[0-9a-f]{6}$/i.test(v)) return `#${v.toLowerCase()}`;
  }
  for (const c of findAll(el, 'sysClr')) {
    const v = c.getAttribute('lastClr') || c.getAttribute('val') || '';
    if (/^[0-9a-f]{6}$/i.test(v)) return `#${v.toLowerCase()}`;
  }
  return null;
}

/** Counts explicit FillForegnd colours across page/master parts. */
function explicitFillScores(zip) {
  const counts = new Map();
  if (!zip) return counts;
  const neutral = new Set(['#ffffff', '#000000', '#f2f2f2', '#fefefe', '#d0d0d0', '#e7e6e6', '#d9d9d9']);
  for (const path of zip.keys()) {
    if (!/^visio\/(pages|masters)\/[^/]+\.xml$/i.test(path)) continue;
    const text = zipText(zip, path);
    if (!text) continue;
    for (const cell of text.match(/<Cell\b[^>]*\/>/g) || []) {
      if (!/\bN='FillForegnd'/.test(cell)) continue;
      const m = /\bV='(#[0-9a-fA-F]{6})'/.exec(cell);
      if (!m) continue;
      const hex = m[1].toLowerCase();
      if (neutral.has(hex)) continue;
      counts.set(hex, (counts.get(hex) || 0) + 1);
    }
  }
  return counts;
}

/** Relative luminance, 0..100, used by Visio's LUMDIFF(). */
function hexLuminance(hex) {
  if (!hex || !/^#[0-9a-f]{6}$/i.test(hex)) return null;
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return (0.299 * r + 0.587 * g + 0.114 * b) / 2.55;
}

const FORMULA_UNITS = /^(IN_F|IN|DEGREE|DEG|PT|CM|MM|PC|PX|FT|%)/i;

/** Visio formula literals are converted to internal units (inches/radians). */
const FORMULA_UNIT_SCALE = {
  IN: 1, IN_F: 1, PT: 1 / 72, CM: 1 / 2.54, MM: 1 / 25.4,
  PC: 1 / 6, PX: 1 / 96, FT: 12,
  DEG: Math.PI / 180, DEGREE: Math.PI / 180, '%': 1 / 100,
};

function formulaNumber(v) {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string') {
    if (/^#[0-9a-f]{6}$/i.test(v)) {
      const lum = hexLuminance(v);
      return lum === null ? 0 : lum;
    }
    const n = parseFloat(v);
    if (Number.isFinite(n)) return n;
  }
  return 0;
}

function formulaTruthy(v) {
  if (typeof v === 'number') return v !== 0;
  if (typeof v === 'string') {
    return v !== '' && v.toLowerCase() !== 'false' && v !== '0';
  }
  return false;
}

function formulaBinary(op, a, b) {
  const aHex = typeof a === 'string' && /^#[0-9a-f]{6}$/i.test(a);
  const bHex = typeof b === 'string' && /^#[0-9a-f]{6}$/i.test(b);
  if (op === '+') {
    if (aHex || bHex) {
      if (aHex && bHex) return null;
      return (aHex ? a : b).toLowerCase();
    }
    if (typeof a === 'string' || typeof b === 'string') {
      return String(a ?? '') + String(b ?? '');
    }
    return formulaNumber(a) + formulaNumber(b);
  }
  const x = formulaNumber(a), y = formulaNumber(b);
  if (op === '-') return x - y;
  if (op === '*') return x * y;
  if (op === '/') return y === 0 ? null : x / y;
  return null;
}

function formulaCompare(op, a, b) {
  const same = (typeof a === 'string' && typeof b === 'string')
    ? a.toLowerCase() === b.toLowerCase()
    : formulaNumber(a) === formulaNumber(b);
  const x = formulaNumber(a), y = formulaNumber(b);
  switch (op) {
    case '=': return same ? 1 : 0;
    case '<>': return same ? 0 : 1;
    case '<': return x < y ? 1 : 0;
    case '>': return x > y ? 1 : 0;
    case '<=': return x <= y ? 1 : 0;
    case '>=': return x >= y ? 1 : 0;
    default: return null;
  }
}

/**
 * A small evaluator for the parts of Visio's formula language the importer
 * needs: colour/theme formulas and the numeric formulas of master geometry.
 * It is deliberately a small evaluator; `resolve` supplies cell references.
 */
class FormulaParser {
  constructor(text, theme, options = {}) {
    this.text = String(text ?? '');
    this.i = 0;
    this.theme = theme;
    this.resolve = options.resolve || null;
    this.failed = false;
  }

  parse() {
    const v = this.expr();
    this.ws();
    if (this.failed || this.i < this.text.length) return null;
    return v;
  }

  ws() {
    while (this.i < this.text.length && /\s/.test(this.text[this.i])) this.i++;
  }

  peek() {
    this.ws();
    return this.text[this.i] || '';
  }

  take(ch) {
    this.ws();
    if (this.text[this.i] === ch) { this.i++; return true; }
    return false;
  }

  expr() { return this.compare(); }

  compare() {
    let a = this.additive();
    for (;;) {
      this.ws();
      const two = this.text.slice(this.i, this.i + 2);
      let op = null;
      if (two === '<>' || two === '<=' || two === '>=') {
        op = two;
        this.i += 2;
      } else if (this.text[this.i] === '=' || this.text[this.i] === '<'
                 || this.text[this.i] === '>') {
        op = this.text[this.i];
        this.i++;
      }
      if (!op) break;
      a = formulaCompare(op, a, this.additive());
    }
    return a;
  }

  additive() {
    let a = this.multiplicative();
    for (;;) {
      const op = this.peek();
      if (op !== '+' && op !== '-') break;
      this.i++;
      a = formulaBinary(op, a, this.multiplicative());
    }
    return a;
  }

  multiplicative() {
    let a = this.unary();
    for (;;) {
      const op = this.peek();
      if (op !== '*' && op !== '/') break;
      this.i++;
      a = formulaBinary(op, a, this.unary());
    }
    return a;
  }

  unary() {
    const op = this.peek();
    if (op === '+' || op === '-') {
      this.i++;
      const a = this.unary();
      return op === '-' ? -formulaNumber(a) : a;
    }
    return this.primary();
  }

  primary() {
    this.ws();
    const ch = this.text[this.i];
    if (ch === '(') {
      this.i++;
      const v = this.expr();
      this.take(')');
      return v;
    }
    if (ch === '"') return this.string();
    const n = this.number();
    if (n !== null) return n;
    const name = this.name();
    if (name) {
      this.ws();
      if (this.text[this.i] === '(') {
        this.i++;
        const args = [];
        if (!this.take(')')) {
          for (;;) {
            args.push(this.expr());
            if (!this.take(',')) break;
          }
          this.take(')');
        }
        return this.call(name, args);
      }
      return this.ident(name);
    }
    this.failed = true;
    return null;
  }

  number() {
    this.ws();
    const m = /^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/.exec(
      this.text.slice(this.i));
    if (!m) return null;
    this.i += m[0].length;
    const value = Number(m[0]);
    const unit = FORMULA_UNITS.exec(this.text.slice(this.i));
    if (!unit) return value;
    this.i += unit[0].length;
    return value * (FORMULA_UNIT_SCALE[unit[0].toUpperCase()] ?? 1);
  }

  string() {
    this.i++;
    let out = '';
    while (this.i < this.text.length) {
      const ch = this.text[this.i++];
      if (ch === '"') {
        if (this.text[this.i] === '"') { out += '"'; this.i++; continue; }
        return out;
      }
      out += ch;
    }
    this.failed = true;
    return null;
  }

  name() {
    this.ws();
    const m = /^[A-Za-z_$][A-Za-z0-9_$.!]*/.exec(this.text.slice(this.i));
    if (!m) return null;
    this.i += m[0].length;
    return m[0];
  }

  ident(name) {
    const n = name.toLowerCase();
    if (n === 'true') return 1;
    if (n === 'false') return 0;
    if (n === 'pi') return Math.PI;
    if (this.resolve) {
      const v = this.resolve(name);
      if (v !== undefined && v !== null) return v;
      // Geometry must fall back to the cached value rather than silently
      // treating an unresolved cell reference as zero.
      this.failed = true;
      return null;
    }
    return name;
  }

  color(v) {
    if (typeof v === 'string') {
      if (/^#[0-9a-f]{6}$/i.test(v)) return v.toLowerCase();
      return this.theme.slot(v, false);
    }
    return null;
  }

  call(name, args) {
    const n = name.toLowerCase();
    const color = (v) => this.color(v);
    const num = (v) => formulaNumber(v);
    switch (n) {
      case 'themeguard':
      case 'guard':
      case 'setatrefexpr':
      case 'setatref':
      case 'setf':
          return args.length ? args[args.length - 1] : null;
      case 'themeval':
      case 'theme': {
        const slot = args.length ? String(args[0]) : 'accent1';
        return this.theme.slot(slot);
      }
      case 'msotint':
      case 'tint':
        return tint(color(args[0]), num(args[1]));
      case 'shade':
        return tint(color(args[0]), -num(args[1]));
      case 'lumdiff': {
        const a = hexLuminance(color(args[0]));
        const b = hexLuminance(color(args[1]));
        return a === null || b === null ? 0 : Math.abs(a - b);
      }
      case 'rgb':
        return toHex(num(args[0]), num(args[1]), num(args[2]));
      case 'if':
        return formulaTruthy(args[0]) ? (args[1] ?? null) : (args[2] ?? null);
      case 'min': return Math.min(...args.map(num));
      case 'max': return Math.max(...args.map(num));
      case 'abs': return Math.abs(num(args[0]));
      case 'round': return Math.round(num(args[0]));
      case 'floor': return Math.floor(num(args[0]));
      case 'ceiling': return Math.ceil(num(args[0]));
      case 'mod': return num(args[0]) % num(args[1]);
      case 'sqrt': return Math.sqrt(num(args[0]));
      case 'power': return Math.pow(num(args[0]), num(args[1]));
      case 'int': return Math.trunc(num(args[0]));
      case 'sign': return Math.sign(num(args[0]));
      case 'sin': return Math.sin(num(args[0]));
      case 'cos': return Math.cos(num(args[0]));
      case 'tan': return Math.tan(num(args[0]));
      case 'asin': return Math.asin(num(args[0]));
      case 'acos': return Math.acos(num(args[0]));
      case 'atan': return Math.atan(num(args[0]));
      case 'atan2': return Math.atan2(num(args[0]), num(args[1]));
      case 'deg': return num(args[0]) * 180 / Math.PI;
      case 'rad': return num(args[0]) * Math.PI / 180;
      case 'log': return Math.log(num(args[0]));
      case 'exp': return Math.exp(num(args[0]));
      case 'not': return formulaTruthy(args[0]) ? 0 : 1;
      case 'and': return args.every(formulaTruthy) ? 1 : 0;
      case 'or': return args.some(formulaTruthy) ? 1 : 0;
      case 'strsame':
        return String(args[0] ?? '').toLowerCase()
          === String(args[1] ?? '').toLowerCase() ? 1 : 0;
      case 'index': {
        const i = Math.round(num(args[0]));
        const arr = String(args[1] ?? '').split(';');
        return arr[i] ?? null;
      }
      case 'cellisthemed':
      case 'isthemed':
        return 1;
      default:
        return null;
    }
  }
}

/* -------------------------------------------------------------- masters -- */
const CONNECTOR_MASTERS = new Set([
  'dynamicconnector', 'sequenceflow', 'messageflow', 'connector', 'straightconnector',
  // UML and database relationship masters are groups with their own shape
  // tree, but they behave as connectors: BeginX/EndX, glue and arrow cells.
  'relationship', 'umlrelationship', 'association', 'dependency', 'generalization',
  'inheritance', 'realization', 'aggregation', 'composition',
]);
const CONTAINER_MASTERS = new Set([
  'cffcontainer', 'poollane', 'swimlanelist', 'phaselist', 'separator',
]);

const NAME_TO_TYPE = {
  rectangle: 'rect', roundedrectangle: 'roundRect', box: 'rect', square: 'rect',
  ellipse: 'ellipse', oval: 'ellipse', circle: 'circle',
  diamond: 'diamond', triangle: 'triangle', pentagon: 'pentagon', hexagon: 'hexagon',
  parallelogram: 'parallelogram', trapezoid: 'trapezoid', star: 'star5', cross: 'cross',
  blockarrow: 'blockArrow', chevron: 'chevron', subprocess: 'flowProcess',
  process: 'flowProcess', task: 'rect', activity: 'rect',
  decision: 'flowDecision', gateway: 'diamond', exclusivegateway: 'diamond',
  terminator: 'flowTerminator', startevent: 'ellipse', endevent: 'ellipse',
  intermediaterevent: 'ellipse', event: 'ellipse',
  document: 'flowDocument', data: 'flowData', datastore: 'flowData',
  manualinput: 'flowManualInput', delay: 'flowDelay', display: 'flowDisplay',
  preparation: 'flowPreparation', card: 'flowCard', storeddata: 'flowStoredData',
  directaccessstorage: 'flowDirectData', note: 'note', callout: 'callout',
  textcallout: 'callout', or: 'orShape', and: 'andShape',
  database: 'database', server: 'server', tape: 'tape', cloud: 'cloud', cube: 'cube',
  router: 'router', switch: 'switch', firewall: 'firewall', web: 'web', vpn: 'vpn',
  collapsedsubprocess: 'rect', datasink: 'flowData', datastoreobject: 'flowData',
};

/** Master name -> the shape we draw for it. */
function typeFor(name, geometry) {
  const c = canon(name);
  if (!c) return geometry === 'ellipse' ? 'ellipse' : 'rect';
  if (NAME_TO_TYPE[c]) return NAME_TO_TYPE[c];
  if (c.includes('gateway')) return 'diamond';
  if (c.includes('event')) return 'ellipse';
  if (c.includes('task') || c.includes('activity') || c.includes('process')) return 'rect';
  if (c.includes('callout')) return 'callout';
  if (c.includes('document')) return 'flowDocument';
  if (c.includes('data')) return 'flowData';
  if (c.includes('database')) return 'database';
  return geometry === 'ellipse' ? 'ellipse' : 'rect';
}

/* --------------------------------------------------------------- import -- */
export async function importVisioFile(file) {
  const lower = file.name.toLowerCase();
  if (lower.endsWith('.vdx')) return importVDX(await file.text());
  // .vstx/.vsdm/.vstm are the same Open Packaging ZIP + Visio XML format.
  if (/\.(vsdx|vstx|vsdm|vstm)$/.test(lower)) {
    const zip = await readZip(await file.arrayBuffer());
    const doc = importVSDX(zip);
    // Keep the original package out of JSON/undo snapshots but reachable by
    // the writer, so a save can patch the file instead of regenerating it.
    Object.defineProperty(doc, '_visioPackage', {
      value: zip, writable: true, configurable: true, enumerable: false,
    });
    Object.defineProperty(doc, '_visioFileName', {
      value: file.name, writable: true, configurable: true, enumerable: false,
    });
    return doc;
  }
  throw new Error(`${file.name} is not a Visio file (.vsdx, .vstx, .vsdm, .vstm or .vdx expected)`);
}

/** Relationships of a package part: rId -> target path. */
function relationships(zip, part) {
  if (!part) return {};
  const cut = part.lastIndexOf('/');
  const relsPath = cut >= 0
    ? `${part.slice(0, cut)}/_rels/${part.slice(cut + 1)}.rels`
    : `_rels/${part}.rels`;
  const text = zipText(zip, relsPath);
  if (!text) return {};
  const out = {};
  for (const r of findAll(parseXml(text), 'Relationship')) {
    out[r.getAttribute('Id')] = r.getAttribute('Target');
  }
  return out;
}

const relId = (el) => {
  for (const r of kids(el, 'Rel')) {
    const id = r.getAttribute('r:id') || r.getAttribute('Id');
    if (id) return id;
  }
  return null;
};

/* ---------------------------------------------------------------- media -- */
function bytesToBase64(bytes) {
  let out = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    out += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }
  return btoa(out);
}

/** Natural pixel size from a raster header, for authored-image sizing. */
function imagePixelSize(bytes, ext) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (ext === 'png' && bytes.length >= 26) {
    // IHDR: bit depth at 24, colour type at 25 (3 = palette/indexed).
    return { w: view.getUint32(16), h: view.getUint32(20), colorType: bytes[25] };
  }
  if (ext === 'gif' && bytes.length >= 10) {
    return { w: view.getUint16(6, true), h: view.getUint16(8, true) };
  }
  if (ext === 'bmp' && bytes.length >= 26) {
    return { w: Math.abs(view.getInt32(18, true)), h: Math.abs(view.getInt32(22, true)) };
  }
  if ((ext === 'jpg' || ext === 'jpeg') && bytes.length > 4) {
    let off = 2;
    while (off + 9 < bytes.length) {
      if (bytes[off] !== 0xff) { off += 1; continue; }
      const marker = bytes[off + 1];
      if (marker === 0xd8 || marker === 0xd9) { off += 2; continue; }
      const len = view.getUint16(off + 2);
      if (marker >= 0xc0 && marker <= 0xcf
          && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
        return { w: view.getUint16(off + 7), h: view.getUint16(off + 5) };
      }
      off += 2 + len;
    }
  }
  return null;
}

/** Resolve a shape's ForeignData rel to a package media part and a data URL. */
function readForeignMedia(ctx, part, rels, foreign) {
  const rid = relId(foreign);
  if (!rid) return null;
  const target = rels && rels[rid];
  if (!target) return null;
  const cut = part ? part.lastIndexOf('/') : -1;
  const base = cut >= 0 ? part.slice(0, cut) : '';
  const path = joinPath(base, target);
  const cached = ctx.mediaIds.get(path);
  if (cached) return cached;
  const bytes = ctx.zip.get(path);
  if (!bytes) return null;
  const ext = (/\.([a-z0-9]+)$/i.exec(path) || [null, 'bin'])[1].toLowerCase();
  const type = MEDIA_MIME[ext] || 'application/octet-stream';
  const media = {
    id: `m${++ctx.mediaSeq}`,
    src: `data:${type};base64,${bytesToBase64(bytes)}`,
    type,
    ext,
    name: path.split('/').pop(),
    part: path,
    unsupported: !DISPLAYABLE_MEDIA.has(ext),
  };
  const size = imagePixelSize(bytes, ext);
  if (size && size.w > 0 && size.h > 0) {
    media.width = size.w;
    media.height = size.h;
  }
  // Visio does not apply Brightness/Contrast/Gamma to palette (indexed)
  // pictures — measured with truecolour and PNG-8 copies of the same image.
  media.adjust = !(ext === 'gif'
    || (ext === 'png' && size && size.colorType === 3));
  ctx.mediaIds.set(path, media);
  return media;
}

class VisioFile {
  constructor(zip) {
    this.zip = zip;
    this.masters = new Map();
    this.styles = new Map();
    this.theme = new Theme();
    // Media parts referenced by ForeignData, keyed by package path. One record
    // is shared by every shape that uses the same picture.
    this.mediaIds = new Map();
    this.mediaSeq = 0;
  }

  /** StyleSheets carry the text look a shape inherits (font, size, colour). */
  loadStyles(docXml) {
    for (const ss of findAll(docXml, 'StyleSheet')) {
      const id = ss.getAttribute('ID');
      if (id !== null) this.styles.set(id, ss);
    }
    const settings = findAll(docXml, 'DocumentSettings')[0];
    if (settings) {
      this.defaultFillStyle = settings.getAttribute('DefaultFillStyle');
      this.defaultLineStyle = settings.getAttribute('DefaultLineStyle');
      this.defaultTextStyle = settings.getAttribute('DefaultTextStyle');
    }
  }

  /**
   * The Character rows of a shape's TextStyle chain, most specific first.
   * A style with no Character section of its own hands off to its TextStyle.
   */
  textStyleRows(shapeEl, masterEl = null) {
    const rows = [];
    const seen = new Set();
    let id = (shapeEl && shapeEl.getAttribute('TextStyle'))
      || (masterEl && masterEl.getAttribute('TextStyle'))
      || this.defaultTextStyle;
    while (id !== null && id !== undefined && !seen.has(id)) {
      seen.add(id);
      const ss = this.styles.get(id);
      if (!ss) break;
      const sec = section(ss, 'Character');
      const row = sec ? kids(sec, 'Row')[0] : null;
      if (row) rows.push(cellsOf(row));
      id = ss.getAttribute('TextStyle');
    }
    return rows;
  }

  /** Direct cells and section rows of a shape's FillStyle/LineStyle chain. */
  styleChainRows(shapeEl, masterEl, prop) {
    const rows = [];
    const seen = new Set();
    let id = (shapeEl && shapeEl.getAttribute(prop))
      || (masterEl && masterEl.getAttribute(prop))
      || (prop === 'FillStyle' ? this.defaultFillStyle
        : prop === 'LineStyle' ? this.defaultLineStyle : this.defaultTextStyle);
    const secName = prop === 'FillStyle' ? 'Fill'
      : prop === 'LineStyle' ? 'Line' : 'Character';
    while (id !== null && id !== undefined && id !== '' && !seen.has(id)) {
      seen.add(id);
      const ss = this.styles.get(id);
      if (!ss) break;
      rows.push(cellsOf(ss));
      const sec = section(ss, secName);
      if (sec) for (const row of kids(sec, 'Row')) rows.push(cellsOf(row));
      id = ss.getAttribute(prop);
    }
    return rows;
  }

  /** Fill/line style rows for one shape or connector. */
  styleContext(shapeEl, masterEl) {
    return {
      fillStyles: this.styleChainRows(shapeEl, masterEl, 'FillStyle'),
      lineStyles: this.styleChainRows(shapeEl, masterEl, 'LineStyle'),
    };
  }

  loadMasters(docXml) {
    this.theme.load(this.zip, docXml);
    this.loadStyles(docXml);
    const text = zipText(this.zip, 'visio/masters/masters.xml');
    if (!text) return;
    const rels = relationships(this.zip, 'visio/masters/masters.xml');
    for (const m of findAll(parseXml(text), 'Master')) {
      const id = m.getAttribute('ID');
      const rid = relId(m);
      const target = rid ? rels[rid] : null;
      const part = target ? joinPath('visio/masters', target) : null;
      const entry = {
        nameU: m.getAttribute('NameU') || m.getAttribute('Name'),
        part, rels: null, shapes: new Map(), top: null,
      };
      if (part && this.zip.has(part)) {
        entry.rels = relationships(this.zip, part);
        const root = parseXml(zipText(this.zip, part));
        indexShapes(root, entry.shapes);
        for (const sh of findAll(root, 'Shapes')) {
          if (sh.children.length) { entry.top = sh.children[0]; break; }
        }
      }
      this.masters.set(id, entry);
    }
  }

  /** The master shape a page shape inherits from. */
  masterShape(masterId, shapeId) {
    const m = this.masters.get(masterId);
    if (!m) return null;
    if (shapeId) return m.shapes.get(shapeId) || null;
    return m.top;
  }
}

/** Index every shape in a part by ID, recursing into groups. */
function indexShapes(root, map) {
  const walk = (node) => {
    for (const s of kids(node, 'Shapes')) {
      for (const c of s.children) {
        if (c.localName !== 'Shape') continue;
        const id = c.getAttribute('ID');
        if (id && !map.has(id)) map.set(id, c);
        walk(c);
      }
    }
  };
  walk(root);
  for (const s of findAll(root, 'Shape')) {
    const id = s.getAttribute('ID');
    if (id && !map.has(id)) map.set(id, s);
  }
}

/* ------------------------------------------------------------- page walk -- */
function importVSDX(zip) {
  const docText = zipText(zip, 'visio/document.xml');
  if (!docText) throw new Error('visio/document.xml is missing — this does not look like a .vsdx file');

  const doc = parseXml(docText);
  const ctx = new VisioFile(zip);
  ctx.loadMasters(doc);

  // pages live either inline in document.xml or in a separate pages part
  let pageEls = findAll(doc, 'Page').filter((p) => p.getAttribute('ID') !== null);
  let pageRels = {};

  if (!pageEls.length) {
    const docRels = relationships(zip, 'visio/document.xml');
    const target = Object.values(docRels).find((t) => t.endsWith('pages.xml'));
    if (target) {
      const part = joinPath('visio', target);
      const text = zipText(zip, part);
      if (text) {
        const pagesDoc = parseXml(text);
        pageEls = findAll(pagesDoc, 'Page');
        pageRels = relationships(zip, part);
      }
    }
  }

  const fallback = [...zip.keys()]
    .filter((k) => /^visio\/pages\/page\d+\.xml$/i.test(k))
    .sort();

  const pages = [];
  pageEls.forEach((pe, i) => {
    let partText = null;
    let partPath = null;
    const rid = relId(pe);
    if (rid && pageRels[rid]) {
      const target = pageRels[rid];
      for (const cand of [joinPath('visio/pages', target), target, joinPath('visio', target)]) {
        const t = zipText(zip, cand);
        if (t) { partText = t; partPath = cand; break; }
      }
    }
    if (!partText && fallback[i]) {
      partText = zipText(zip, fallback[i]);
      partPath = fallback[i];
    }
    if (!partText) return;

    pages.push(buildPage(ctx, parseXml(partText), {
      name: pe.getAttribute('NameU') || pe.getAttribute('Name') || `Page-${i + 1}`,
      height: pageHeightOf(pe),
      layers: pageLayersOf(pe),
      part: partPath,
      pageId: pe.getAttribute('ID'),
      pageRels: relationships(zip, partPath),
    }));
  });

  if (!pages.length) throw new Error('No pages found in this Visio file');
  const media = {};
  for (const rec of ctx.mediaIds.values()) media[rec.id] = rec;
  return {
    name: 'Imported Diagram', version: 1, schemaVersion: SCHEMA_VERSION, pages, pageIndex: 0,
    theme: ctx.theme.toJSON(),
    ...(Object.keys(media).length ? { media } : {}),
  };
}

function importVDX(text) {
  const doc = parseXml(text);
  const pageEls = findAll(doc, 'Page');
  const ctx = new VisioFile(new Map());
  ctx.loadStyles(doc);
  const pages = [];
  pageEls.forEach((pe, i) => {
    pages.push(buildPage(ctx, doc, {
      name: pe.getAttribute('NameU') || pe.getAttribute('Name') || `Page-${i + 1}`,
      height: pageHeightOf(pe),
      layers: pageLayersOf(pe),
      scope: pe,
    }));
  });
  if (!pages.length) throw new Error('No pages found in this Visio file');
  return {
    name: 'Imported Diagram', version: 1, schemaVersion: SCHEMA_VERSION, pages, pageIndex: 0,
    theme: ctx.theme.toJSON(),
  };
}

/** PageSheet `Layer` section: one row per layer (IX = layer index). */
export function pageLayersOf(pageEl) {
  const sheet = kid(pageEl, 'PageSheet');
  const section = sheet && kids(sheet, 'Section').find((s) => s.getAttribute('N') === 'Layer');
  if (!section) return [];
  return kids(section, 'Row').map((row, position) => {
    const cell = (name) => {
      const c = kids(row, 'Cell').find((x) => x.getAttribute('N') === name);
      return c ? c.getAttribute('V') : null;
    };
    const flag = (name, fallback) => {
      const v = cell(name);
      return v == null ? fallback : String(v).trim() !== '0';
    };
    const ix = Number(row.getAttribute('IX'));
    return {
      index: Number.isFinite(ix) ? ix : position,
      name: cell('Name') || '',
      visible: flag('Visible', true),
      print: flag('Print', true),
      locked: flag('Lock', false),
    };
  });
}

/** `LayerMember` ("0;2") -> [0, 2]. */
export function layerMembersOf(cells) {
  const c = cells && cells.get && cells.get('LayerMember');
  if (!c) return [];
  return String(c.getAttribute('V') ?? '').split(';')
    .map((x) => x.trim()).filter((x) => /^\d+$/.test(x)).map(Number);
}

/**
 * A shape is drawn when it has no layers or at least one of its layers is
 * visible (checked against Visio's Page.Export). Unknown layers count visible.
 */
export function layersHide(members, layers) {
  return members.length > 0 && members.every((ix) => {
    const layer = layers.find((l) => l.index === ix);
    return !!layer && !layer.visible;
  });
}

function pageHeightOf(pageEl) {
  const sheet = kid(pageEl, 'PageSheet');
  if (!sheet) return 0;
  for (const name of ['PageHeight', 'Height']) {
    const c = kids(sheet, 'Cell').find((x) => x.getAttribute('N') === name);
    const h = inches(c, 0);
    if (h > 0) return h;
  }
  return 0;
}

/**
 * Affine transforms for Visio page coordinates (inches, y up).
 *
 * A shape's local frame maps to its parent's space as
 *   p = Pin + R(angle) * (u - LocPin)
 * and a group passes that frame on to its children. Composing those matrices
 * down the group tree is what makes rotated groups land in the right place;
 * the old translation-only `origin` was correct only for angle 0.
 *
 * Matrices are `{ a, b, c, d, e, f }` with x' = a*x + c*y + e and
 * y' = b*x + d*y + f.
 */
const IDENTITY_T = Object.freeze({ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 });

function applyTransform(t, x, y) {
  return { x: t.a * x + t.c * y + t.e, y: t.b * x + t.d * y + t.f };
}

/** Parent transform `p` applied after local transform `l`: p ∘ l. */
function composeTransform(p, l) {
  return {
    a: p.a * l.a + p.c * l.b,
    b: p.b * l.a + p.d * l.b,
    c: p.a * l.c + p.c * l.d,
    d: p.b * l.c + p.d * l.d,
    e: p.a * l.e + p.c * l.f + p.e,
    f: p.b * l.e + p.d * l.f + p.f,
  };
}

function shapeTransform(pinX, pinY, locX, locY, angle) {
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return {
    a: cos, b: sin, c: -sin, d: cos,
    e: pinX - (cos * locX - sin * locY),
    f: pinY - (sin * locX + cos * locY),
  };
}

/** Rotation of a rotation+translation matrix, in radians. */
const transformAngle = (t) => Math.atan2(t.b, t.a);

/** CFF role from the inherited/instance container cells. */
function cffRoleOf({ structure, categories, required, listRequired, name }) {
  const cats = String(categories || '').toLowerCase()
    .split(/[;,]/).map((x) => x.trim()).filter(Boolean);
  const has = (t) => cats.includes(t);
  const req = String(required || '').toLowerCase();
  const listReq = String(listRequired || '').toLowerCase();
  if (listReq.includes('phase') || (String(structure).toLowerCase() === 'list' && req.includes('phase'))) {
    return 'phase-list';
  }
  if (has('cff container') || /cff container/i.test(name || '')) return 'pool';
  if (String(structure).toLowerCase() === 'list') return 'list';
  if (has('swimlane') || /swimlane/i.test(name || '')) return 'lane';
  if (has('phase') || /separator|phase/i.test(name || '')) return 'separator';
  return null;
}

/**
 * Turn one page of Visio shapes into a LayoutCraft page.
 */
function buildPage(ctx, root, { name, height, scope, layers = [], part = null, pageId = null, pageRels = null }) {
  const pg = newPage(name);
  if (layers.length) pg.layers = layers;
  const guides = [];
  const hiddenGroups = new Set();
  const doc = scope || root.documentElement || root;
  const shapesRoot = kid(doc, 'Shapes');
  const connectsRoot = kid(doc, 'Connects');
  if (!shapesRoot) {
    pg.visio = { part, pageId, pageH: height || 1, ids: [] };
    return pg;
  }

  const theme = ctx.theme;
  const importedIds = new Set();
  const rotatedGroups = [];
  const rawRelationships = new Map();
  const containerMeta = new Map();

  /* --- resolve the shape tree ------------------------------------------- */
  const resolved = [];
  const connectors = [];

  let zCounter = 0;

  const visit = (el, parentMaster, T, groupCtx) => {
    const own = cellsOf(el);
    // Ruler guides are page furniture, not drawable shapes.
    if (el.getAttribute('Type') === 'Guide') {
      guides.push({
        visioId: String(el.getAttribute('ID') || ''),
        xIn: cellValue(own, 'PinX'),
        yIn: cellValue(own, 'PinY'),
        angle: cellValue(own, 'Angle'),
      });
      return;
    }
    const masterId = el.getAttribute('Master') || parentMaster;
    const m = ctx.masters.get(masterId) || null;
    const mshape = m ? ctx.masterShape(masterId, el.getAttribute('MasterShape')) : null;

    // A picture is a Type="Foreign" shape whose ForeignData names a media part
    // through a relationship. The ForeignData may come from the master, in
    // which case its relationship belongs to the master part.
    const ownForeign = kid(el, 'ForeignData');
    const masterForeign = mshape ? kid(mshape, 'ForeignData') : null;
    const foreign = ownForeign || masterForeign;
    const foreignType = foreign ? foreign.getAttribute('ForeignType') : null;
    const foreignMedia = foreign
      ? readForeignMedia(ctx,
        ownForeign ? part : (m && m.part) || part,
        ownForeign ? (pageRels || {}) : (m && m.rels) || pageRels || {},
        foreign)
      : null;

    // inherited cells first, then the shape's own (unless they only say "inherit")
    const cells = new Map(mshape ? cellsOf(mshape) : []);
    for (const [k, v] of own) {
      // "inherit" cells still win when they carry a concrete value: that is
      // the value this particular instance resolved to
      if (cells.has(k) && v.getAttribute('F') === 'Inh' && !isConcrete(v)) continue;
      cells.set(k, v);
    }

    const pinX = cellValue(cells, 'PinX');
    const pinY = cellValue(cells, 'PinY');
    const w = cellValue(cells, 'Width');
    const h = cellValue(cells, 'Height');
    const locX = cellValue(cells, 'LocPinX', w / 2);
    const locY = cellValue(cells, 'LocPinY', h / 2);
    const angle = cellValue(cells, 'Angle');

    // T maps the parent's local space to the page; this shape's local frame
    // composes its own rotation on top. An unrotated tree reduces to the old
    // translation: Pin - LocPin plus the parent origin.
    const selfT = composeTransform(T, shapeTransform(pinX, pinY, locX, locY, angle));
    // Lower-left of the unrotated box in page inches.
    const ll = applyTransform(T, pinX - locX, pinY - locY);
    // Exact page-space centre of the box. Visio rotates about LocPin, while
    // the editor rotates about the box centre, so compute the mapped centre
    // instead of assuming LocPin is the middle.
    const cdx = w / 2 - locX;
    const cdy = h / 2 - locY;
    const worldCenter = applyTransform(T,
      pinX + cdx * Math.cos(angle) - cdy * Math.sin(angle),
      pinY + cdx * Math.sin(angle) + cdy * Math.cos(angle));
    const parentAngle = transformAngle(T);
    const worldAngle = parentAngle + angle;

    const masterName = (m && m.nameU) || el.getAttribute('NameU') || el.getAttribute('Name') || '';
    const groupEl = kid(el, 'Shapes');
    const isGroup = el.getAttribute('Type') === 'Group' && groupEl && groupEl.children.length;

    const sid = String(el.getAttribute('ID') || '');
    const rels = parseRelationships(cells);
    if (sid && rels.length) rawRelationships.set(sid, rels);
    const listDirection = userValue(el, 'msvSDListDirection')
      || userValue(mshape, 'msvSDListDirection');
    const isListItem = !!(userValue(el, 'msvSDListItemMaster')
      || userValue(mshape, 'msvSDListItemMaster'));
    const categories = userValue(el, 'msvShapeCategories') || userValue(mshape, 'msvShapeCategories');
    const structure = userValue(el, 'msvStructureType') || userValue(mshape, 'msvStructureType');
    const required = userValue(el, 'msvSDContainerRequiredCategories')
      || userValue(mshape, 'msvSDContainerRequiredCategories');
    const listRequired = userValue(el, 'msvSDListRequiredCategories')
      || userValue(mshape, 'msvSDListRequiredCategories');
    const heading = userValue(el, 'visHeadingText') || userValue(mshape, 'visHeadingText');
    const vertical = userValue(el, 'CFFVertical') || userValue(mshape, 'CFFVertical');
    const numLanes = userValue(el, 'numLanes') || userValue(mshape, 'numLanes');
    const listGuid = userValue(el, 'SwimlaneListGUID') || userValue(mshape, 'SwimlaneListGUID');
    const role = cffRoleOf({ structure, categories, required, listRequired, name: masterName });
    const hasContainerMeta = ['msvSDContainerStyle', 'msvSDContainerLocked', 'msvSDListItemMaster']
      .some((n) => userValue(el, n) || userValue(mshape, n));
    const containerLike = String(structure).toLowerCase() === 'container' && (
      /(^|;)\s*(container|containerstyledefaults|cffcontainer|pool|swimlane|lane|phase)\s*(;|$)/i.test(categories)
      || hasContainerMeta || role === 'pool' || role === 'lane' || role === 'separator');
    const listLike = String(structure).toLowerCase() === 'list'
      || listDirection !== '' || isListItem || role === 'list' || role === 'phase-list';
    if (sid && (containerLike || listLike)) {
      containerMeta.set(sid, {
        kind: listLike ? 'list' : 'container',
        role,
        structure,
        listDirection: Number.isFinite(Number(listDirection)) ? Number(listDirection) : null,
        heading,
        direction: String(vertical) === '1' ? 'vertical' : 'horizontal',
        numLanes: Number.isFinite(Number(numLanes)) ? Number(numLanes) : null,
        listGuid: listGuid ? String(listGuid).replace(/^"|"$/g, '') : null,
      });
    }

    // Visio's ObjType 2 marks a 1-D shape. UML relationship masters are
    // groups with BeginX/EndX and their own body tree, so they land here too.
    if (CONNECTOR_MASTERS.has(canon(masterName)) ||
        (cells.has('BeginX') && cells.has('EndX') && cellValue(cells, 'ObjType') === 2)) {
      connectors.push({
        el, cells, ll, parentT: T, selfT, worldAngle,
        origin: { x: T.e, y: T.f },
        masterName, mshape, group: groupCtx, z: zCounter++,
      });
      return;
    }

    // Page-level groups are a nested <Shapes> tree. Recursing keeps every
    // child; the flat editor model records membership and absolute positions.
    // Container masters stay on their existing traversal path and are tagged
    // as containers so their body parts are not mistaken for user members.
    // Container/list metadata is often inherited from the master and stored
    // as a User section row, not a direct instance cell. A bare
    // msvStructureType="Container" is not enough: Azure/network stencils use
    // it on ordinary icon groups that should keep their parts.
    const container = isGroup && (CONTAINER_MASTERS.has(canon(masterName))
      || /(^|;)\s*(container|containerstyledefaults|cffcontainer|pool|swimlane|lane|phase)\s*(;|$)/i.test(categories)
      || hasContainerMeta);
    if (isGroup && (!mshape || container)) {
      const gid = String(el.getAttribute('ID') || '');
      if (gid) {
        const parentId = groupCtx ? groupCtx.id : null;
        const rootId = groupCtx ? groupCtx.rootId : gid;
        const depth = (groupCtx ? groupCtx.depth : 0) + 1;
        const rotated = Math.abs(worldAngle) > 1e-6;
        const info = { id: gid, rootId, parentId, depth, rotated, kind: container ? 'container' : 'group' };
        if (gid && !containerMeta.has(gid)) {
          containerMeta.set(gid, {
            kind: 'container',
            role,
            structure,
            listDirection: Number.isFinite(Number(listDirection)) ? Number(listDirection) : null,
            heading,
            direction: String(vertical) === '1' ? 'vertical' : 'horizontal',
            numLanes: Number.isFinite(Number(numLanes)) ? Number(numLanes) : null,
            listGuid: listGuid ? String(listGuid).replace(/^"|"$/g, '') : null,
          });
        }
        const label = textOf(el);
        // The group's text block is in its local frame. Store it as a fraction
        // so the editor can move/scale the label with the live member frame.
        const tw = Math.max(0.01, cellValue(cells, 'TxtWidth', w || 0.01));
        // No explicit TxtHeight on a plain group: use a one-line box at the
        // group's pin. Using the whole frame height centres the text in a tall
        // box and can put it behind a member.
        const th = Math.max(0.01, cellValue(cells, 'TxtHeight', 0.25));
        const tpx = cellValue(cells, 'TxtPinX', (w || 0.01) / 2);
        const tpy = cellValue(cells, 'TxtPinY', (h || 0.01) / 2);
        const tlx = cellValue(cells, 'TxtLocPinX', tw / 2);
        const tly = cellValue(cells, 'TxtLocPinY', th / 2);
        const labelRel = w > 0 && h > 0 ? {
          x: (tpx - tlx) / w,
          y: (tpy - tly) / h,
          w: tw / w,
          h: th / h,
        } : null;
        // A rotated group's label is positioned in the group's local frame;
        // keep the mapped centre and size so the renderer can rotate it.
        const labelCenter = applyTransform(selfT,
          tpx - tlx + tw / 2, tpy - tly + th / 2);
        const textStyle = characterStyle(el, mshape, cells, theme,
          ctx.textStyleRows(el, mshape));
        const labelStyle = {
          fontSize: textStyle.fontSize || 12,
          textColor: textStyle.textColor || '#1b2733',
          bold: !!textStyle.bold, italic: !!textStyle.italic,
          underline: !!textStyle.underline,
          fontFamily: textStyle.fontFamily || 'Calibri',
          align: paragraphAlign(el, mshape) || 'center',
          vAlign: verticalAlign(cells) || 'middle',
        };
        pg.groups.push({
          id: `g_${gid}`, visioId: gid, rootId, parentId, depth,
          name: masterName || '', label,
          kind: info.kind, rotated,
          role: role || (container ? 'container' : null),
          heading,
          origHeading: heading,
          direction: String(vertical) === '1' ? 'vertical' : 'horizontal',
          listDirection: Number.isFinite(Number(listDirection)) ? Number(listDirection) : null,
          listGuid: listGuid ? String(listGuid).replace(/^"|"$/g, '') : null,
          // lower-left frame in page inches; the writer uses it to tell a
          // pure move from a resize/partial move.
          frame: { x: ll.x, y: ll.y, w, h },
          // Rotated groups are moved by patching their Pin cells; the
          // cumulative parent angle converts a world delta into their
          // parent's local space.
          ownAngle: angle,
          parentAngle,
          angle: worldAngle,
          labelRel,
          labelStyle,
          labelCenter: { x: labelCenter.x, y: labelCenter.y },
          labelSize: { w: tw, h: th },
        });
        if (rotated) rotatedGroups.push(gid);
        if (layersHide(layerMembersOf(cells), layers)
          || (groupCtx && hiddenGroups.has(groupCtx.id))) hiddenGroups.add(gid);
        for (const child of groupEl.children) {
          if (child.localName === 'Shape') visit(child, masterId, selfT, info);
        }
        return;
      }
    }

    resolved.push({
      el, cells, ll, parentT: T, selfT, worldCenter, worldAngle,
      origin: { x: T.e, y: T.f },
      masterName, mshape, masterId, w, h,
      media: foreignMedia, foreignType,
      group: groupCtx, z: zCounter++,
    });
    // A master-composited group draws its own body, so its page children are
    // normally skipped. Native data graphics (callouts, bars, icon sets) are
    // separate instances nested there; keep them, as the Rust importer does.
    if (isGroup && mshape && !container) {
      for (const child of groupEl.children) {
        if (child.localName !== 'Shape') continue;
        const childMaster = child.getAttribute('Master');
        const childShape = childMaster && ctx.masters.get(childMaster)
          ? ctx.masterShape(childMaster, child.getAttribute('MasterShape')) : null;
        if (childShape && userValue(childShape, 'msvCalloutType')) {
          visit(child, masterId, selfT, null);
        }
      }
    }
  };

  for (const s of shapesRoot.children) {
    if (s.localName === 'Shape') visit(s, null, IDENTITY_T, null);
  }
  if (guides.length) pg.guides = guides;

  /* --- page size and the y flip ---------------------------------------- */
  let pageH = height || 1;
  for (const r of resolved) {
    // Rotated boxes extend past their unrotated frame; use the mapped centre
    // plus the rotated half-extents so a rotated group is not clipped.
    const halfW = r.w / 2;
    const halfH = r.h / 2;
    const reach = Math.abs(Math.sin(r.worldAngle)) * halfW
      + Math.abs(Math.cos(r.worldAngle)) * halfH;
    pageH = Math.max(pageH, r.worldCenter.y + reach);
  }
  for (const c of connectors) pageH = Math.max(pageH, c.ll.y);
  const toPx = (x, y) => ({ x: x * DPI, y: (pageH - y) * DPI });

  /* --- shapes ----------------------------------------------------------- */
  const idMap = new Map();          // visio shape id -> my shape
  for (const r of resolved) {
    const box = toPx(r.worldCenter.x - r.w / 2, r.worldCenter.y + r.h / 2);
    r.styleCtx = ctx.styleContext(r.el, r.mshape);
    r.styleCtx.large = (r.w || 0) * DPI * (r.h || 0) * DPI > 60000;
    const parts = partsOf(r, theme);
    const styleRows = ctx.textStyleRows(r.el, r.mshape);
    const text = characterStyle(r.el, r.mshape, r.cells, theme, styleRows);
    const block = textBlock(r);
    const valign = verticalAlign(r.cells);
    const halign = paragraphAlign(r.el, r.mshape);
    const rich = richTextOf(r, theme, styleRows, halign);
    const data = propertyDataOf(r.el, r.mshape);
    const links = hyperlinkDataOf(r.el, r.mshape);

    const kind = typeFor(r.masterName, geometryKind(r.el));

    let shape;
    if (r.media) {
      const style = styleOf([r.cells], theme, r.styleCtx);
      shape = newShape('image', box.x, box.y, {
        w: Math.max(2, r.w * DPI),
        h: Math.max(2, r.h * DPI),
        rot: -r.worldAngle * (180 / Math.PI),
        flipX: cellValue(r.cells, 'FlipX') !== 0,
        flipY: cellValue(r.cells, 'FlipY') !== 0,
        label: labelOf(r),
        z: r.z,
      });
      // ImgOffset/ImgWidth are in the shape's local inches. Normalising them
      // by the box keeps a crop tied to the shape when it is resized; Visio's
      // own formulas are usually Width*N, so the same invariant holds there.
      const wIn = Math.max(1e-6, r.w);
      const hIn = Math.max(1e-6, r.h);
      const imgW = cellValue(r.cells, 'ImgWidth', wIn);
      const imgH = cellValue(r.cells, 'ImgHeight', hIn);
      shape.image = {
        mediaId: r.media.id,
        name: r.media.name,
        mediaType: r.media.type,
        offsetX: cellValue(r.cells, 'ImgOffsetX', 0) / wIn,
        offsetY: cellValue(r.cells, 'ImgOffsetY', 0) / hIn,
        scaleX: imgW > 0 ? imgW / wIn : 1,
        scaleY: imgH > 0 ? imgH / hIn : 1,
        // Visio stores Brightness and Contrast as 0..1 with 0.5 neutral;
        // normalise them to -1..1 so the editor and SVG filter see 0 = none.
        brightness: cellValue(r.cells, 'Brightness', 0.5) * 2 - 1,
        contrast: cellValue(r.cells, 'Contrast', 0.5) * 2 - 1,
        gamma: cellValue(r.cells, 'Gamma', 1),
        transparency: cellValue(r.cells, 'Transparency', 0),
        foreignType: r.foreignType || 'Bitmap',
        unsupported: !!r.media.unsupported,
      };
      if (block) {
        shape.textBox = block;
        // Visio's picture text block is relative to the frame (TxtWidth=Width*1,
        // TxtPinY=0). Keep fractions so the editor caption follows a resize
        // while the writer leaves Visio's formulas alone.
        const wPx = Math.max(1, r.w * DPI);
        const hPx = Math.max(1, r.h * DPI);
        shape.textBoxRel = {
          x: block.x / wPx, y: block.y / hPx,
          w: block.w / wPx, h: block.h / hPx,
        };
      }
      if (valign) shape.vAlign = valign;
      if (halign) shape.align = halign;
      shape.fill = style.fill;
      shape.stroke = style.stroke;
      shape.strokeWidth = style.strokeWidth;
      if (style.dash) shape.strokeStyle = style.dash;
    } else if (parts.length) {
      shape = newCustomShape(box.x, box.y, Math.max(2, r.w * DPI), Math.max(2, r.h * DPI), {
        rot: -r.worldAngle * (180 / Math.PI),
        flipX: cellValue(r.cells, 'FlipX') !== 0,
        flipY: cellValue(r.cells, 'FlipY') !== 0,
        label: labelOf(r),
        z: r.z,
      });
      shape.parts = parts;
      if (block) shape.textBox = block;
      if (valign) shape.vAlign = valign;
      if (halign) shape.align = halign;
      const front = parts[parts.length - 1];
      shape.fill = front.fill;
      shape.stroke = front.stroke;
      shape.strokeWidth = front.strokeWidth;
      if (front.dash) shape.strokeStyle = front.dash;
    } else {
      const style = styleOf([r.cells], theme, r.styleCtx);
      shape = newShape(kind, box.x, box.y, {
        w: Math.max(2, r.w * DPI),
        h: Math.max(2, r.h * DPI),
        rot: -r.worldAngle * (180 / Math.PI),
        flipX: cellValue(r.cells, 'FlipX') !== 0,
        flipY: cellValue(r.cells, 'FlipY') !== 0,
        label: labelOf(r),
        z: r.z,
      });
      if (block) shape.textBox = block;
      if (valign) shape.vAlign = valign;
      if (halign) shape.align = halign;
      shape.fill = style.fill;
      shape.stroke = style.stroke;
      shape.strokeWidth = style.strokeWidth;
      if (style.dash) shape.strokeStyle = style.dash;
    }
    if (text.fontSize) shape.fontSize = text.fontSize;
    if (text.textColor) shape.textColor = text.textColor;
    if (text.bold !== undefined) shape.bold = text.bold;
    if (text.italic !== undefined) shape.italic = text.italic;
    if (text.underline !== undefined) shape.underline = text.underline;
    if (text.fontFamily) shape.fontFamily = text.fontFamily;
    if (rich) shape.rich = rich;
    if (data.length) shape.data = data;
    if (links.length) shape.hyperlinks = links;
    // A LayoutCraft-authored colour-by-value rule lives on the target shape.
    if (userValue(r.el, 'layoutcraftDGColor') !== '') {
      const field = userValue(r.el, 'layoutcraftDGColorField');
      shape.dataColor = {
        field,
        min: Number(userValue(r.el, 'layoutcraftDGColorMin') || 0),
        max: Number(userValue(r.el, 'layoutcraftDGColorMax') || 100),
        low: userValue(r.el, 'layoutcraftDGColorLow') || '#f8696b',
        mid: userValue(r.el, 'layoutcraftDGColorMid') || '#ffeb84',
        high: userValue(r.el, 'layoutcraftDGColorHigh') || '#63be7b',
        baseFill: userValue(r.el, 'layoutcraftDGColorBase') || null,
      };
    }
    const twCell = r.cells.get('TxtWidth');
    const thCell = r.cells.get('TxtHeight');
    const fitW = !!(twCell && /TEXTWIDTH\s*\(/i.test(twCell.getAttribute('F') || ''));
    const fitH = !!(thCell && /TEXTHEIGHT\s*\(/i.test(thCell.getAttribute('F') || ''));
    if (fitW || fitH) shape.textFit = { width: fitW, height: fitH };
    // LayoutCraft-authored data callout: restore the link to its target. The
    // rendered text is derived from the target's Shape Data; the imported
    // field run is kept only for Visio's live display.
    const dgTarget = userValue(r.el, 'layoutcraftDGTarget');
    if (dgTarget) {
      const icon = userValue(r.el, 'layoutcraftDGKind') === 'icon'
        || userValue(r.el, 'layoutcraftDGIcon') !== '';
      const bar = !icon && (userValue(r.el, 'layoutcraftDGBar') !== ''
        || userValue(r.el, 'layoutcraftDGKind') === 'bar');
      const heading = !icon && !bar && userValue(r.el, 'layoutcraftDGKind') === 'heading';
      if ((bar || icon) && shape.parts && shape.parts.length) {
        if (bar) {
          // Geometry 0 is the filled bar, the last section the track outline.
          const body = shape.parts[0];
          const track = shape.parts[shape.parts.length - 1];
          if (body && body.fill !== 'none') shape.fill = body.fill;
          if (track && track.stroke && track.stroke !== 'none') {
            shape.stroke = track.stroke;
            shape.strokeWidth = track.strokeWidth || shape.strokeWidth;
          }
        } else {
          // The outline geometry carries the line, the filled sections the fill.
          const body = shape.parts.find((p) => p.fill && p.fill !== 'none');
          const outline = shape.parts.find((p) => p.stroke && p.stroke !== 'none');
          if (body) shape.fill = body.fill;
          if (outline) {
            shape.stroke = outline.stroke;
            shape.strokeWidth = outline.strokeWidth || shape.strokeWidth;
          }
        }
      }
      if (shape.parts) delete shape.parts;
      shape.type = icon ? 'dataIcon' : bar ? 'dataBar' : 'dataCallout';
      shape.callout = {
        kind: icon ? 'icon' : bar ? 'bar' : heading ? 'heading' : 'text',
        field: userValue(r.el, 'layoutcraftDGField'),
        label: userValue(r.el, 'layoutcraftDGLabel'),
        position: userValue(r.el, 'layoutcraftDGPosition') || (icon ? 'left' : 'top'),
        autoSize: userValue(r.el, 'layoutcraftDGAuto') !== '0',
        min: Number(userValue(r.el, 'layoutcraftDGMin') || 0),
        max: Number(userValue(r.el, 'layoutcraftDGMax') || 100),
        showValue: icon ? false : userValue(r.el, 'layoutcraftDGShowValue') !== '0',
        targetVisioId: dgTarget,
      };
      if (icon) {
        const set = userValue(r.el, 'layoutcraftDGIconSet');
        shape.callout.iconSet = ['boxes', 'diamonds', 'circles'].includes(set) ? set : 'boxes';
      }
      shape.attachedTo = null;
      // msvCalloutField is the callout's own transport row for the target's
      // property, not user Shape Data.
      if (shape.data) {
        shape.data = shape.data.filter((d) => String(d.name) !== 'msvCalloutField');
        if (!shape.data.length) delete shape.data;
      }
    }
    if (!shape.callout) {
      const native = nativeDataGraphicOf(r);
      if (native) shape.nativeDataGraphic = native;
    }
    const layerIx = layerMembersOf(r.cells);
    if (layerIx.length) shape.layers = layerIx;
    if (layersHide(layerIx, layers) || (r.group && hiddenGroups.has(r.group.id))) {
      shape.layerHidden = true;
    }
    pg.shapes.push(shape);
    if (r.group) {
      // id/parentId are the immediate group for a member; rootId is the
      // outermost group and drives unit selection.
      shape.group = {
        id: r.group.id, rootId: r.group.rootId, parentId: r.group.id,
        depth: r.group.depth, rotated: r.group.rotated, kind: r.group.kind,
      };
    }
    const sourceId = r.el.getAttribute('ID');
    if (sourceId) {
      shape.visio = {
        id: sourceId,
        origin: { x: r.origin.x, y: r.origin.y },
        // Immutable import-time membership, so the writer can detect an
        // editor ungroup/regroup that it cannot represent in the package.
        group: shape.group ? { ...shape.group } : null,
        orig: shapeSnapshot(shape),
        origData: shape.data ? structuredClone(shape.data) : null,
        origLinks: shape.hyperlinks ? structuredClone(shape.hyperlinks) : null,
        origTextFit: shape.textFit ? { ...shape.textFit } : null,
      };
      idMap.set(sourceId, shape);
      importedIds.add(sourceId);
      const meta = containerMeta.get(String(sourceId));
      if (meta) {
        shape.containerKind = meta.kind;
        shape.containerId = String(sourceId);
        shape.containerInfo = {
          kind: meta.kind,
          role: meta.role || 'container',
          heading: meta.heading || '',
          listDirection: meta.listDirection,
          direction: meta.direction || 'horizontal',
          numLanes: meta.numLanes,
          listGuid: meta.listGuid,
        };
        shape.visio.origContainer = { ...shape.containerInfo };
      }
    }
  }

  // Resolve data-callout targets after every shape has a source ID.
  for (const s of pg.shapes) {
    if (!s.callout || !s.callout.targetVisioId) continue;
    const want = String(s.callout.targetVisioId);
    const target = pg.shapes.find((x) => x.visio && String(x.visio.id) === want)
      || pg.shapes.find((x) => String(x.id) === want) || null;
    s.attachedTo = target ? target.id : null;
    if (s.visio) s.visio.orig = shapeSnapshot(s);
  }

  /* --- connectors ------------------------------------------------------- */
  const glue = new Map();
  if (connectsRoot) {
    for (const c of connectsRoot.children) {
      if (c.localName !== 'Connect') continue;
      const fromSheet = c.getAttribute('FromSheet');
      const fromCell = c.getAttribute('FromCell') || '';
      const toSheet = c.getAttribute('ToSheet');
      if (!fromSheet || !toSheet) continue;
      const rec = glue.get(fromSheet) || {};
      if (fromCell.startsWith('Begin')) rec.from = toSheet;
      else if (fromCell.startsWith('End')) rec.to = toSheet;
      glue.set(fromSheet, rec);
    }
  }

  for (const c of connectors) {
    const style = styleOf([c.cells], theme, ctx.styleContext(c.el, c.mshape));
    // BeginX/EndX are in the connector's parent space, like PinX. A connector
    // inside a group must go through that group's transform too.
    const beginWorld = c.cells.get('BeginX')
      ? applyTransform(c.parentT, cellValue(c.cells, 'BeginX'), cellValue(c.cells, 'BeginY'))
      : null;
    const endWorld = c.cells.get('EndX')
      ? applyTransform(c.parentT, cellValue(c.cells, 'EndX'), cellValue(c.cells, 'EndY'))
      : null;
    const beginPt = beginWorld ? toPx(beginWorld.x, beginWorld.y) : null;
    const endPt = endWorld ? toPx(endWorld.x, endWorld.y) : null;

    const g = glue.get(c.el.getAttribute('ID')) || {};
    const a = idMap.get(g.from) || null;
    const b = idMap.get(g.to) || null;

    const conn = newConnector(
      a ? { shapeId: a.id, side: sideTowards(a, beginPt), local: toShapeLocal(a, beginPt) }
        : { shapeId: null, side: null, point: beginPt },
      b ? { shapeId: b.id, side: sideTowards(b, endPt), local: toShapeLocal(b, endPt) }
        : { shapeId: null, side: null, point: endPt },
      {
        stroke: style.stroke,
        strokeWidth: Math.max(1, style.strokeWidth),
        strokeStyle: style.dash || 'solid',
        label: textOf(c.el),
        z: c.z,
      },
    );
    // Visio's default is no arrow at the start and a filled arrow at the end.
    // A Themed/inherited EndArrow cell also resolves to that default (the
    // style sheets and themes, not the page shape, carry the concrete value).
    const arrowValue = (cell) => {
      const v = cell ? cell.getAttribute('V') : null;
      if (v === null || v === '' || v === 'Themed') return null;
      const n = parseFloat(v);
      return Number.isFinite(n) ? n : null;
    };
    const beginArrow = c.cells.get('LineBeginArrow') || c.cells.get('BeginArrow');
    const endArrow = c.cells.get('LineEndArrow') || c.cells.get('EndArrow');
    const begin = arrowValue(beginArrow);
    const end = arrowValue(endArrow);
    conn.arrowStart = begin === null ? 'none' : arrowKind(begin);
    conn.arrowEnd = end === null ? 'filled' : arrowKind(end);

    // Dynamic-connector routing lives in config cells and master formulas, not
    // in the page Geometry. Map the visible route style so the editor draws a
    // Visio-like route instead of a straight line through every bend.
    const cfg = connectorConfigOf(c.cells);
    for (const [k, v] of Object.entries(cfg)) if (v !== undefined) conn[k] = v;
    const routeStyle = cfg.routeStyle;
    if (/curve|arc|wavy/i.test(c.masterName)) conn.route = 'curved';
    else if (routeStyle === 2 || routeStyle === 7 || routeStyle === 8 || cfg.lineRouteExt === 2) conn.route = 'straight';
    else if (routeStyle !== undefined) conn.route = 'elbow';
    else if (/dynamic connector|connector/i.test(c.masterName) && !/^line|1-D/i.test(c.masterName)) conn.route = 'elbow';

    // the connector's own geometry is its route: keep the bends as waypoints.
    // Relationship groups use the first row as the begin point and the last
    // as the end, exactly like a dynamic connector's routing geometry.
    const geom = section(c.el, 'Geometry');
    if (geom) {
      const pts = [];
      for (const row of kids(geom, 'Row')) {
        if (row.getAttribute('Del') === '1') continue;
        const rc = cellsOf(row);
        if (!rc.has('X') && !rc.has('Y')) continue;
        const p = applyTransform(c.selfT, cellValue(rc, 'X'), cellValue(rc, 'Y'));
        pts.push(toPx(p.x, p.y));
      }
      const near = (p, q) => q && Math.hypot(p.x - q.x, p.y - q.y) <= 0.01;
      const bends = [];
      for (const p of pts) {
        if (near(p, beginPt) || near(p, endPt)) continue;
        if (!bends.length || !near(p, bends[bends.length - 1])) bends.push(p);
      }
      if (bends.length) conn.waypoints = bends;
    }
    const connId = c.el.getAttribute('ID');
    if (connId) {
      conn.visio = {
        id: connId,
        group: conn.group ? { ...conn.group } : null,
        orig: connectorSnapshot(conn),
      };
      importedIds.add(connId);
    }
    pg.connectors.push(conn);
    if (c.group) {
      conn.group = {
        id: c.group.id, rootId: c.group.rootId, parentId: c.group.id,
        depth: c.group.depth, rotated: c.group.rotated, kind: c.group.kind,
      };
    }
  }

  /* --- containers and lists -------------------------------------------- */
  const modelByVisio = new Map();
  for (const s of pg.shapes) if (s.visio?.id) modelByVisio.set(String(s.visio.id), s);
  for (const c of pg.connectors) if (c.visio?.id) modelByVisio.set(String(c.visio.id), c);

  const containers = new Map();
  const ensureContainer = (id, init = {}) => {
    const key = String(id);
    let rec = containers.get(key);
    if (!rec) {
      const meta = containerMeta.get(key) || {};
      const kind = meta.kind || init.kind || 'container';
      rec = {
        visioId: key,
        kind,
        role: meta.role || init.role || (kind === 'list' ? 'list' : 'container'),
        containerType: kind === 'list' ? 1 : 0,
        listDirection: meta.listDirection ?? init.listDirection ?? null,
        direction: meta.direction || init.direction || 'horizontal',
        numLanes: meta.numLanes ?? init.numLanes ?? null,
        listGuid: meta.listGuid || init.listGuid || null,
        heading: meta.heading || init.heading || '',
        label: init.label || meta.heading || '',
        members: [], items: [], related: [],
      };
      containers.set(key, rec);
    }
    if (init.label && !rec.label) rec.label = init.label;
    return rec;
  };
  const addUnique = (arr, id) => {
    const key = String(id);
    if (!arr.includes(key)) arr.push(key);
  };

  for (const g of pg.groups) {
    if (g.kind === 'container') {
      ensureContainer(g.visioId, {
        kind: 'container', role: g.role || 'container', label: g.label,
        heading: g.heading, direction: g.direction, listDirection: g.listDirection,
        listGuid: g.listGuid,
      });
    }
  }
  // Inline containers/lists authored by LayoutCraft (or any producer that writes
  // container cells on a plain Shape) need a record even with no Relationships.
  for (const s of pg.shapes) {
    if (!s.visio?.id || !s.containerInfo) continue;
    ensureContainer(s.visio.id, {
      kind: s.containerInfo.kind,
      role: s.containerInfo.role,
      heading: s.containerInfo.heading,
      direction: s.containerInfo.direction,
      listDirection: s.containerInfo.listDirection,
      numLanes: s.containerInfo.numLanes,
      listGuid: s.containerInfo.listGuid,
      label: s.label,
    });
  }

  for (const [sid2, rels] of rawRelationships) {
    for (const { type, refs } of rels) {
      if (type === 1) {
        const c = ensureContainer(sid2);
        refs.forEach((r) => addUnique(c.members, r));
      } else if (type === 2) {
        const c = ensureContainer(sid2);
        refs.forEach((r) => addUnique(c.items, r));
      } else if (type === 11) {
        const c = ensureContainer(sid2);
        refs.forEach((r) => addUnique(c.related, r));
      } else if (type === 4) {
        const obj = modelByVisio.get(sid2);
        if (obj) {
          obj.containers = obj.containers || [];
          refs.forEach((r) => addUnique(obj.containers, r));
        }
        for (const ref of refs) addUnique(ensureContainer(ref).members, sid2);
      } else if (type === 5) {
        const obj = modelByVisio.get(sid2);
        if (obj) {
          obj.listOf = obj.listOf || [];
          refs.forEach((r) => addUnique(obj.listOf, r));
        }
        for (const ref of refs) addUnique(ensureContainer(ref).items, sid2);
      }
    }
  }

  pg.containers = [...containers.values()];
  // Link the CFF graph for the editor: every list knows its pool, and every
  // lane/separator knows its list and pool. The Relationships cells remain the
  // source of truth for the writer; these links only drive selection and UI.
  const contByRef = new Map(pg.containers.map((c) => [String(c.visioId), c]));
  for (const c of pg.containers) {
    if (!c.role) c.role = c.kind === 'list' ? 'list' : 'container';
    if (c.kind !== 'list') continue;
    const pool = pg.containers.find((p) => p !== c
      && (p.members || []).map(String).includes(String(c.visioId))
      && (p.role === 'pool' || p.kind === 'container'));
    if (pool) {
      c.poolRef = String(pool.visioId);
      if (c.role === 'phase-list') pool.phaseRef = String(c.visioId);
      else pool.listRef = String(c.visioId);
    }
  }
  for (const c of pg.containers) {
    if (c.kind === 'list') continue;
    const list = pg.containers.find((l) => l.kind === 'list'
      && (l.items || []).map(String).includes(String(c.visioId)));
    if (list) {
      c.listRef = String(list.visioId);
      c.poolRef = list.poolRef;
    }
  }
  for (const g of pg.groups) {
    const c = contByRef.get(String(g.visioId));
    if (!c) continue;
    g.role = c.role;
    g.listRef = c.listRef;
    g.poolRef = c.poolRef;
  }
  // Import-time membership snapshots: the writer only rewrites a
  // Relationships cell when the editor actually changed it.
  for (const c of pg.containers) {
    c.orig = {
      members: [...c.members],
      items: [...c.items],
      related: [...c.related],
    };
  }
  for (const obj of [...pg.shapes, ...pg.connectors]) {
    if (!obj.visio) continue;
    if (obj.containers?.length) obj.visio.containers = [...obj.containers];
    if (obj.listOf?.length) obj.visio.listOf = [...obj.listOf];
  }
  // Mirror the membership onto the group records so group selection can use it.
  for (const g of pg.groups) {
    const c = containers.get(String(g.visioId));
    if (!c) continue;
    g.containerType = c.containerType;
    g.listDirection = c.listDirection;
    g.members = c.members;
    g.items = c.items;
    g.related = c.related;
  }

  // The page's source part and the IDs it represented. The writer uses these
  // to patch the original XML in place and to notice deleted objects.
  pg.visio = { part, pageId, pageH, ids: [...importedIds], rotatedGroups };
  return pg;
}

/* ---------------------------------------------------------------- pieces -- */
/** The editor-visible state of an imported shape, for save-time diffing. */
function shapeSnapshot(shape) {
  return {
    x: shape.x, y: shape.y, w: shape.w, h: shape.h,
    rot: shape.rot || 0, flipX: !!shape.flipX, flipY: !!shape.flipY,
    label: shape.label || '',
    fill: shape.fill ?? 'none',
    stroke: shape.stroke ?? 'none',
    strokeWidth: shape.strokeWidth ?? 0,
    strokeStyle: shape.strokeStyle || 'solid',
    type: shape.type,
    fontSize: shape.fontSize || 12,
    fontFamily: shape.fontFamily || '',
    textColor: shape.textColor || '#1b2733',
    bold: !!shape.bold, italic: !!shape.italic, underline: !!shape.underline,
    align: shape.align || 'center', vAlign: shape.vAlign || 'middle',
    custom: !!(shape.parts && (shape.parts.length > 1
      || shape.parts.some((p) => p.fillGradient || p.bottomEdge))),
    z: shape.z ?? 0,
    image: shape.image ? { ...shape.image } : null,
    attachedTo: shape.attachedTo || null,
    callout: shape.callout ? { ...shape.callout } : null,
    dataColor: shape.dataColor ? { ...shape.dataColor } : null,
  };
}

function connectorConfigOf(cells) {
  const num = (name) => {
    const c = cells.get(name);
    if (!c) return undefined;
    const v = c.getAttribute('V');
    if (v === null || v === '' || v === 'Themed') return undefined;
    const n = parseFloat(v);
    return Number.isFinite(n) ? n : undefined;
  };
  return {
    routeStyle: num('ShapeRouteStyle'),
    conFixedCode: num('ConFixedCode'),
    lineRouteExt: num('ConLineRouteExt'),
    walkPreference: num('WalkPreference'),
    plowCode: num('ShapePlowCode'),
    glueType: num('GlueType'),
    dynFeedback: num('DynFeedback'),
  };
}

/** The editor-visible state of an imported connector, for save-time diffing. */
function connectorSnapshot(conn) {
  return {
    label: conn.label || '',
    stroke: conn.stroke,
    strokeWidth: conn.strokeWidth,
    strokeStyle: conn.strokeStyle || 'solid',
    arrowStart: conn.arrowStart || 'none',
    arrowEnd: conn.arrowEnd || 'none',
    route: conn.route || 'straight',
    routeStyle: conn.routeStyle,
    conFixedCode: conn.conFixedCode,
    lineRouteExt: conn.lineRouteExt,
    walkPreference: conn.walkPreference,
    plowCode: conn.plowCode,
    glueType: conn.glueType,
    dynFeedback: conn.dynFeedback,
    waypoints: (conn.waypoints || []).map((p) => ({ x: p.x, y: p.y })),
    from: endSnapshot(conn.from),
    to: endSnapshot(conn.to),
  };
}

function endSnapshot(end = {}) {
  return {
    shapeId: end.shapeId || null,
    side: end.side || null,
    point: end.point ? { x: end.point.x, y: end.point.y } : null,
    local: end.local ? { x: end.local.x, y: end.local.y } : null,
  };
}

function textOf(el) {
  if (!el) return '';
  const t = kid(el, 'Text');
  if (!t) return '';
  return t.textContent
    .replace(/\r/g, '')
    .replace(/[\u2028\u2029]/g, '\n')   // Visio's line separator
    .replace(/[\u00a0\u2007\u202f]/g, ' ')
    .replace(/[ \t]+/g, ' ')
    .trim();
}

/** The instance's own text wins even when it is empty; masters hold placeholders. */
function labelOf(r) {
  if (kid(r.el, 'Text')) return textOf(r.el);
  return textOf(r.mshape);
}

/**
 * The geometry of a shape, as one or more SVG paths.
 * A shape that draws itself (a plain rectangle) uses its own geometry; a
 * shape built from a master uses the master's body shapes — Visio's 3-D
 * blocks, for instance, are a bevelled rim and a flat front face.
 */
const geometrySections = (el) =>
  (el ? kids(el, 'Section').filter((s) => s.getAttribute('N') === 'Geometry') : []);

/** A geometry section's rows, keyed by row index. */
function geomRows(geom) {
  const rows = new Map();
  if (!geom) return rows;
  let auto = 0;
  for (const row of kids(geom, 'Row')) {
    const ix = row.getAttribute('IX') || String(++auto);
    const entry = rows.get(ix) || { t: '', cells: new Map() };
    if (row.getAttribute('T')) entry.t = row.getAttribute('T');
    for (const [k, v] of cellsOf(row)) entry.cells.set(k, v);
    rows.set(ix, entry);
  }
  return rows;
}

/**
 * Visio stores only the changed cells on a page shape; the rest of each row
 * still comes from the master. Merge the two row by row so the master's Y
 * values survive where the page only overrides X (and vice versa).
 */
function mergeRows(master, page) {
  const rows = geomRows(master);
  for (const [ix, row] of geomRows(page)) {
    const entry = rows.get(ix);
    if (!entry) { rows.set(ix, row); continue; }
    if (row.t) entry.t = row.t;
    for (const [k, v] of row.cells) entry.cells.set(k, v);
  }
  return [...rows.entries()].sort((a, b) => Number(a[0]) - Number(b[0]));
}

/** A boolean cell directly on one geometry section. */
function sectionFlag(sec, name) {
  if (!sec) return false;
  const c = kids(sec, 'Cell').find((x) => x.getAttribute('N') === name);
  if (!c) return false;
  const v = c.getAttribute('V');
  if (v === null || v === '' || v === 'Themed') return false;
  const n = parseFloat(v);
  return Number.isFinite(n) && n !== 0;
}

/** A boolean cell on a Section (NoShow / NoFill / NoLine). */
function geomFlag(master, page, name) {
  for (const sec of [page, master]) {
    if (!sec) continue;
    const c = kids(sec, 'Cell').find((x) => x.getAttribute('N') === name);
    if (!c) continue;
    const v = c.getAttribute('V');
    if (v !== null && v !== '' && v !== 'Themed') {
      const n = parseFloat(v);
      if (Number.isFinite(n)) return n !== 0;
    }
  }
  return false;
}

/**
 * Lazily evaluates one shape's cells for a particular instance. Master
 * geometry formulas refer to Width/Height, Scratch.A1, User.AntiScale,
 * Sheet.5!Width, Geometry1.X4 and so on; this scope resolves those and falls
 * back to the cached V value when a reference cannot be evaluated.
 */
class GeometryScope {
  constructor({ id = null, cells, el = null, masterEl = null, parent = null, scopes = null }) {
    this.id = id;
    this.cells = cells || new Map();
    this.el = el;
    this.masterEl = masterEl;
    this.parent = parent;
    this.scopes = scopes;
    this.cache = new Map();
    this.active = new Set();
    this.geometry = new Map();   // 1-based section -> row IX -> cell -> value
  }

  get(name) {
    const key = String(name ?? '').trim();
    if (!key) return null;
    if (this.cache.has(key)) return this.cache.get(key);
    if (this.active.has(key)) return null;
    this.active.add(key);
    let value = null;
    const cell = this.cells.get(key);
    if (cell) value = this.evalCell(cell);
    else if (this.parent) value = this.parent.get(key);
    this.active.delete(key);
    this.cache.set(key, value);
    return value;
  }

  evalCell(cell) {
    if (!cell) return null;
    const f = cell.getAttribute('F');
    if (f && f !== 'Inh' && f !== 'No Formula') {
      const v = new FormulaParser(f, null, { resolve: (n) => this.resolve(n) }).parse();
      if (typeof v === 'number' && Number.isFinite(v)) return v;
    }
    const n = parseFloat(cell.getAttribute('V'));
    return Number.isFinite(n) ? n : null;
  }

  resolve(name) {
    const raw = String(name ?? '').trim();
    if (!raw) return undefined;
    let m;
    if ((m = /^Sheet\.?(\d+)!\s*(.+)$/i.exec(raw))) {
      const scope = this.scopes?.get(m[1]);
      return scope ? scope.get(m[2].trim()) : undefined;
    }
    if ((m = /^Scratch\.([A-Za-z])(\d*)$/i.exec(raw))) {
      // Formula rows are 1-based (Scratch.A1); XML Row IX is 0-based.
      const row = m[2] ? String(Number(m[2]) - 1) : '0';
      return this.sectionCell('Scratch', row, m[1].toUpperCase());
    }
    if ((m = /^(User|Property|Prop|Controls?|Control)\.(.+)$/i.exec(raw))) {
      const kind = /^prop/i.test(m[1]) ? 'Property'
        : /^control/i.test(m[1]) ? 'Control' : 'User';
      return this.sectionCell(kind, m[2], kind === 'Control' ? 'X' : 'Value');
    }
    if ((m = /^Geometry(\d+)\.([A-Za-z]+)(\d+)$/i.exec(raw))) {
      return this.geometryValue(Number(m[1]), m[3], m[2].toUpperCase());
    }
    if (this.cells.has(raw)) return this.get(raw);
    return this.parent ? this.parent.resolve(raw) : undefined;
  }

  sectionCell(kind, rowName, cellName) {
    let cell = findSectionCell(this.el, kind, rowName, cellName);
    if (!cell) cell = findSectionCell(this.masterEl, kind, rowName, cellName);
    if (cell) {
      const v = this.evalCell(cell);
      if (v !== null) return v;
    }
    return this.parent ? this.parent.sectionCell(kind, rowName, cellName) : undefined;
  }

  geometryValue(sectionNo, rowIX, cellName) {
    const section = this.geometry.get(sectionNo);
    if (!section) return undefined;
    const row = section.get(String(rowIX));
    if (!row || !row.has(cellName)) return undefined;
    return row.get(cellName);
  }
}

function findSectionCell(el, kind, rowName, cellName) {
  if (!el) return null;
  for (const sec of kids(el, 'Section')) {
    if (sec.getAttribute('N') !== kind) continue;
    for (const row of kids(sec, 'Row')) {
      const key = kind === 'Scratch'
        ? (row.getAttribute('IX') || '0')
        : (row.getAttribute('N') || row.getAttribute('IX') || '');
      if (key.toLowerCase() !== String(rowName).toLowerCase()) continue;
      const cell = kids(row, 'Cell').find(
        (c) => c.getAttribute('N').toLowerCase() === String(cellName).toLowerCase());
      if (cell) return cell;
    }
  }
  return null;
}

/** Build a scope map for a master group and all of its nested body shapes. */
function buildScopes(r, mshape, nested) {
  const group = new GeometryScope({
    id: mshape?.getAttribute('ID') || null,
    cells: r.cells,
    el: r.el,
    masterEl: mshape,
    parent: null,
  });
  const scopes = new Map();
  if (group.id) scopes.set(group.id, group);
  group.scopes = scopes;
  for (const sub of nested) {
    const scope = new GeometryScope({
      id: sub.getAttribute('ID') || null,
      cells: cellsOf(sub),
      el: sub,
      parent: group,
      scopes,
    });
    if (scope.id) scopes.set(scope.id, scope);
  }
  return { group, scopes };
}

/** Clamped uniform knot vector for a B-spline of n control points. */
function clampedKnots(n, degree) {
  const p = Math.max(1, Math.min(degree, n - 1));
  const count = n + p + 1;
  const knots = [];
  for (let i = 0; i < count; i++) {
    if (i <= p) knots.push(0);
    else if (i >= n) knots.push(1);
    else knots.push((i - p) / (n - p));
  }
  return knots;
}

/**
 * De Boor evaluation of a (rational) B-spline at parameter t.
 * `controls` entries may carry a `w` weight; `knots` is the full knot vector.
 */
function nurbsPoint(controls, degree, knots, t) {
  const n = controls.length;
  const p = Math.max(1, Math.min(degree, n - 1));
  const kv = knots && knots.length === n + p + 1 ? knots : clampedKnots(n, p);
  let k = p;
  if (t >= kv[n]) {
    k = n - 1;
  } else {
    while (k < n - 1 && t >= kv[k + 1]) k++;
  }
  const d = [];
  for (let j = 0; j <= p; j++) {
    const c = controls[j + k - p];
    const w = typeof c.w === 'number' && c.w !== 0 ? c.w : 1;
    d.push({ x: c.x * w, y: c.y * w, w });
  }
  for (let r = 1; r <= p; r++) {
    for (let j = p; j >= r; j--) {
      const i = j + k - p;
      const lo = kv[i];
      const hi = kv[i + p - r + 1];
      const den = hi - lo;
      const a = den > 0 ? (t - lo) / den : 0;
      d[j] = {
        x: (1 - a) * d[j - 1].x + a * d[j].x,
        y: (1 - a) * d[j - 1].y + a * d[j].y,
        w: (1 - a) * d[j - 1].w + a * d[j].w,
      };
    }
  }
  const r = d[p];
  const w = r.w || 1;
  return { x: r.x / w, y: r.y / w };
}

/**
 * Sample a B-spline. Without a knot vector the spline is clamped-uniform;
 * with one, each knot span is sampled separately so long spans stay smooth.
 */
function sampleSpline(controls, degree, knots = null, perSpan = 12) {
  if (!controls.length) return [];
  if (controls.length === 1) return [controls[0]];
  const n = controls.length;
  const p = Math.max(1, Math.min(degree, n - 1));
  const kv = knots && knots.length === n + p + 1 ? knots : clampedKnots(n, p);
  const out = [];
  for (let s = 0; s < n - p; s++) {
    const a = kv[p + s];
    const b = kv[p + s + 1];
    if (!(b > a)) continue;
    for (let i = s === 0 ? 0 : 1; i <= perSpan; i++) {
      out.push(nurbsPoint(controls, p, kv, a + ((b - a) * i) / perSpan));
    }
  }
  if (!out.length) out.push(nurbsPoint(controls, p, kv, kv[n]));
  return out;
}

/** Parse a Visio NURBS(knotLast, degree, xType, yType, x1,y1,knot1,weight1, …) string. */
function parseNurbsFormula(text) {
  if (!text) return null;
  const m = /NURBS\s*\(([\s\S]*)\)/i.exec(String(text));
  if (!m) return null;
  const args = m[1].split(',').map((s) => s.trim()).filter((s) => s !== '');
  if (args.length < 8) return null;
  const nums = args.map((s) => {
    const n = parseFloat(s);
    return Number.isFinite(n) ? n : 0;
  });
  const [knotLast, degree, xType, yType] = nums;
  const points = [];
  for (let i = 4; i + 3 < nums.length; i += 4) {
    points.push({ x: nums[i], y: nums[i + 1], knot: nums[i + 2], weight: nums[i + 3] });
  }
  return { knotLast, degree, xType: xType ? 1 : 0, yType: yType ? 1 : 0, points };
}

/**
 * Parse a Visio POLYLINE(xType, yType, x1, y1, …) formula. The row's X/Y is
 * the final point; the formula lists the interior vertices.
 */
function parsePolylineFormula(text) {
  if (!text) return null;
  const m = /POLYLINE\s*\(([\s\S]*)\)/i.exec(String(text));
  if (!m) return null;
  const nums = m[1].split(',').map((s) => {
    const n = parseFloat(s.trim());
    return Number.isFinite(n) ? n : 0;
  });
  if (nums.length < 6) return null;
  const [xType, yType] = nums;
  const points = [];
  for (let i = 2; i + 1 < nums.length; i += 2) {
    points.push({ x: nums[i], y: nums[i + 1] });
  }
  return { xType: xType ? 1 : 0, yType: yType ? 1 : 0, points };
}

/**
 * Circular ArcTo row: A is the bow (sagitta) from the chord's midpoint.
 * A positive bow bulges to the right of the start→end direction (Visio's
 * clockwise sweep), which is what the cached ArcTo rows and SVG export show.
 */
function circularArcPoints(start, end, bow, steps = 32) {
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const chord = Math.hypot(dx, dy);
  if (!(chord > 1e-9) || !Number.isFinite(bow)) return null;
  if (Math.abs(bow) < 1e-9) return [start, end];
  const mx = (start.x + end.x) / 2;
  const my = (start.y + end.y) / 2;
  const nx = dy / chord;
  const ny = -dx / chord;
  const mid = { x: mx + nx * bow, y: my + ny * bow };
  const h = (bow * bow - (chord * chord) / 4) / (2 * bow);
  const cx = mx + nx * h;
  const cy = my + ny * h;
  const radius = Math.hypot(chord / 2, h);
  if (!(radius > 1e-9)) return null;
  const ps = Math.atan2(start.y - cy, start.x - cx);
  const pe = Math.atan2(end.y - cy, end.x - cx);
  const pm = Math.atan2(mid.y - cy, mid.x - cx);
  const TAU = Math.PI * 2;
  const norm = (x) => ((x % TAU) + TAU) % TAU;
  const ccwSpan = norm(pe - ps);
  const ccwM = norm(pm - ps);
  const cwSpan = norm(ps - pe);
  const cwM = norm(ps - pm);
  let ccw;
  if (ccwM <= ccwSpan + 1e-6 && cwM > cwSpan + 1e-6) ccw = true;
  else if (cwM <= cwSpan + 1e-6 && ccwM > ccwSpan + 1e-6) ccw = false;
  else ccw = ccwSpan <= cwSpan;
  const span = ccw ? ccwSpan : -cwSpan;
  const count = Math.max(8, Math.min(180,
    steps || Math.ceil(Math.abs(span) / (Math.PI / 16))));
  const out = [start];
  for (let i = 1; i < count; i++) {
    const phi = ps + (span * i) / count;
    out.push({ x: cx + radius * Math.cos(phi), y: cy + radius * Math.sin(phi) });
  }
  out.push(end);
  return out;
}

/**
 * Reconstruct a Visio EllipticalArcTo row in local coordinates.
 *
 * A/B is a point on the arc and C/D give the ellipse's major-axis angle and
 * axis ratio. Solving for the ellipse centre keeps the curve passing through
 * all three points, including the "long way round" arcs Visio draws (a
 * quadratic through the middle point does not).
 */
function ellipticalArcPoints(start, end, mid, theta, ratio, steps = 32) {
  const d = Number.isFinite(ratio) && ratio > 0 && ratio <= 1000 ? ratio : 1;
  const k = d * d;
  const cos = Math.cos(theta);
  const sin = Math.sin(theta);
  const U = (p) => p.x * cos + p.y * sin;
  const V = (p) => -p.x * sin + p.y * cos;
  const us = U(start), vs = V(start);
  const ue = U(end), ve = V(end);
  const um = U(mid), vm = V(mid);
  // 2cu(ui-uj) + 2k cv(vi-vj) = (ui²-uj²) + k(vi²-vj²)
  const a11 = 2 * (us - ue);
  const a12 = 2 * k * (vs - ve);
  const b1 = us * us - ue * ue + k * (vs * vs - ve * ve);
  const a21 = 2 * (ue - um);
  const a22 = 2 * k * (ve - vm);
  const b2 = ue * ue - um * um + k * (ve * ve - vm * vm);
  const det = a11 * a22 - a12 * a21;
  if (!Number.isFinite(det) || Math.abs(det) < 1e-12) return null;
  const cu = (b1 * a22 - a12 * b2) / det;
  const cv = (a11 * b2 - b1 * a21) / det;
  const a2 = (us - cu) * (us - cu) + k * (vs - cv) * (vs - cv);
  if (!(a2 > 1e-12)) return null;
  const a = Math.sqrt(a2);
  const b = a / d;
  const phiOf = (u, v) => Math.atan2((v - cv) / b, (u - cu) / a);
  const ps = phiOf(us, vs);
  const pe = phiOf(ue, ve);
  const pm = phiOf(um, vm);
  const TAU = Math.PI * 2;
  const norm = (x) => ((x % TAU) + TAU) % TAU;
  const ccwSpan = norm(pe - ps);
  const ccwM = norm(pm - ps);
  const cwSpan = norm(ps - pe);
  const cwM = norm(ps - pm);
  let ccw;
  if (ccwM <= ccwSpan + 1e-6 && cwM > cwSpan + 1e-6) ccw = true;
  else if (cwM <= cwSpan + 1e-6 && ccwM > ccwSpan + 1e-6) ccw = false;
  else ccw = ccwSpan <= cwSpan;
  const span = ccw ? ccwSpan : -cwSpan;
  if (Math.abs(span) < 1e-9) return null;
  const count = Math.max(8, Math.min(240,
    steps || Math.ceil((Math.abs(span) / (Math.PI / 16)))));
  const out = [start];
  for (let i = 1; i < count; i++) {
    const phi = ps + (span * i) / count;
    const u = cu + a * Math.cos(phi);
    const v = cv + b * Math.sin(phi);
    out.push({ x: u * cos - v * sin, y: u * sin + v * cos });
  }
  out.push(end);
  return out;
}

/**
 * Assemble a NURBS curve from a NURBSTo row.
 *
 * The row's X/Y is the last control point and the E formula carries the
 * interior control points. The first and last control points (MoveTo and
 * X/Y), their weights (D/B) and the first/second-to-last knots (C/A) live in
 * the row cells. Visio writes a knot for every interior point, but the
 * leading ones repeat the clamped start knot; the meaningful interior knots
 * are the last `n - degree - 1` of that list plus A.
 */
function nurbsCurve(parsed, start, end, cells, size) {
  const weight = (v) => (typeof v === 'number' && v !== 0 ? v : 1);
  const controls = [{ x: start.x, y: start.y, w: weight(cells.firstWeight) }];
  if (parsed) {
    for (const p of parsed.points) {
      controls.push({
        x: parsed.xType ? p.x : p.x * size.w,
        y: parsed.yType ? p.y : p.y * size.h,
        w: weight(p.weight),
      });
    }
  }
  controls.push({ x: end.x, y: end.y, w: weight(cells.lastWeight) });
  const n = controls.length;
  const degree = Math.max(1, Math.min(Math.round(parsed?.degree) || 3, n - 1));
  if (controls.length <= 2 || !parsed || !Number.isFinite(parsed.knotLast)) {
    return { controls, degree, knots: null };
  }
  const interiorCount = Math.max(0, n - degree - 1);
  const seq = parsed.points.map((p) => (Number.isFinite(p.knot) ? p.knot : 0));
  const secondLast = Number.isFinite(cells.secondLastKnot)
    ? cells.secondLastKnot : parsed.knotLast;
  seq.push(secondLast);
  const first = Number.isFinite(cells.firstKnot) ? cells.firstKnot : 0;
  const interior = seq.slice(Math.max(0, seq.length - interiorCount));
  let knots = [
    ...Array(degree + 1).fill(first),
    ...interior,
    ...Array(degree + 1).fill(parsed.knotLast),
  ];
  for (let i = 1; i < knots.length; i++) {
    if (knots[i] < knots[i - 1]) return { controls, degree, knots: null };
  }
  if (!(knots[knots.length - 1] > knots[0])) return { controls, degree, knots: null };
  return { controls, degree, knots };
}

/**
 * Build a path from geometry rows using the scope's evaluated coordinates.
 * Values are stored by row IX so later rows can refer to Geometry1.X4.
 */
function evaluatedPath(scope, rows, sectionNo, map, relMap, close = true, localCtx = null) {
  const table = new Map();
  scope.geometry.set(sectionNo, table);
  let d = '';
  let started = false;
  let cur = null;
  let curLocal = null;
  let spline = null;
  let hasInfinite = false;
  const size = localCtx?.size || { w: 1, h: 1 };
  const localOf = (x, y, relative = false) => {
    if (relative && localCtx?.rel) return localCtx.rel(x, y);
    return [x, y];
  };
  const point = (cmd, mx, my) => {
    const c = started ? cmd : 'M';
    started = true;
    cur = { x: mx, y: my };
    return `${c}${r2(mx)} ${r2(my)}`;
  };
  // Spline/NURBS control points are in the shape's local space; the curve is
  // sampled there and every sample mapped like an ordinary vertex.
  const appendCurve = (controls, degree, knots = null) => {
    const perSpan = Math.max(8, Math.min(24,
      Math.round(96 / Math.max(1, controls.length - 1))));
    const parts = sampleSpline(controls, degree, knots, perSpan);
    for (const p of parts.slice(1)) {
      const [mx, my] = map(p.x, p.y);
      d += point('L', mx, my);
    }
    const last = parts[parts.length - 1];
    if (last) curLocal = { x: last.x, y: last.y };
  };
  const appendPeriodic = (controls, degree, base, step) => {
    const n = controls.length;
    const p = Math.max(1, Math.min(degree, n - 1));
    if (n <= p || !(step > 0)) {
      appendCurve(controls, degree);
      return;
    }
    // A uniform knot sequence with one knot per control point is Visio's
    // closed (periodic) spline. Repeat the controls so de Boor can cross the
    // seam, and evaluate the middle copy.
    const ext = [...controls, ...controls, ...controls];
    const full = [];
    for (let i = 0; i < ext.length + p + 1; i++) full.push(base + i * step);
    const per = Math.max(8, Math.min(24, Math.round(240 / Math.max(1, n))));
    const pts = [];
    for (let s = 0; s < n; s++) {
      const a = base + (n + s) * step;
      for (let i = s === 0 ? 0 : 1; i <= per; i++) {
        pts.push(nurbsPoint(ext, p, full, a + (step * i) / per));
      }
    }
    if (!pts.length) return;
    const [mx0, my0] = map(pts[0].x, pts[0].y);
    d += point('M', mx0, my0);
    for (let i = 1; i < pts.length; i++) {
      const [mx, my] = map(pts[i].x, pts[i].y);
      d += point('L', mx, my);
    }
    const last = pts[pts.length - 1];
    curLocal = { x: last.x, y: last.y };
  };
  const flushSpline = () => {
    if (!spline) return;
    const sp = spline;
    spline = null;
    if (sp.controls.length < 2) return;
    const k = sp.knots;
    const uniform = k.length === sp.controls.length + 1
      && k.every((v, i) => i === 0 || Math.abs(v - k[i - 1] - (k[1] - k[0])) < 1e-9);
    if (uniform) appendPeriodic(sp.controls, sp.degree, k[0], k[1] - k[0]);
    else appendCurve(sp.controls, sp.degree);
  };
  const cellText = (row, name) => {
    for (const [key, cell] of row.cells) {
      if (String(key).toUpperCase() !== name) continue;
      const f = cell.getAttribute('F');
      if (f && f !== 'Inh' && f !== 'No Formula') return f;
      return cell.getAttribute('V');
    }
    return null;
  };
  const valueOf = (row, cellName) => {
    const cell = row.cells.get(cellName);
    if (!cell) return null;
    const f = cell.getAttribute('F');
    if (f && f !== 'Inh' && f !== 'No Formula') {
      const v = new FormulaParser(f, null, { resolve: (n) => scope.resolve(n) }).parse();
      if (typeof v === 'number' && Number.isFinite(v)) return v;
    }
    const n = parseFloat(cell.getAttribute('V'));
    return Number.isFinite(n) ? n : null;
  };

  for (const [ix, row] of rows) {
    const values = new Map();
    for (const name of row.cells.keys()) {
      values.set(name.toUpperCase(), valueOf(row, name));
    }
    table.set(String(ix), values);
    const val = (name) => {
      const v = values.get(name);
      return typeof v === 'number' && Number.isFinite(v) ? v : 0;
    };
    const t = row.t;
    if (spline && t !== 'SplineKnot' && t !== 'SplineEnd') flushSpline();
    const gx = val('X');
    const gy = val('Y');
    if (t === 'RelMoveTo' || t === 'RelLineTo') {
      const [lx, ly] = localOf(gx, gy, true);
      const [mx, my] = relMap(gx, gy);
      curLocal = { x: lx, y: ly };
      d += point(t === 'RelMoveTo' ? 'M' : 'L', mx, my);
      continue;
    }
    if (t === 'MoveTo' || t === 'LineTo') {
      const [mx, my] = map(gx, gy);
      curLocal = { x: gx, y: gy };
      d += point(t === 'MoveTo' ? 'M' : 'L', mx, my);
    } else if (t === 'ArcTo') {
      // A is the bow (sagitta); B/C/D are not part of an ArcTo row.
      const end = { x: gx, y: gy };
      const startLocal = curLocal ? { x: curLocal.x, y: curLocal.y } : { x: gx, y: gy };
      const pts = circularArcPoints(startLocal, end, val('A'));
      if (pts) {
        for (let i = 1; i < pts.length; i++) {
          const [mx, my] = map(pts[i].x, pts[i].y);
          d += point('L', mx, my);
        }
      } else {
        const [mx, my] = map(gx, gy);
        d += point('L', mx, my);
      }
      curLocal = end;
    } else if (t === 'Ellipse') {
      // X,Y is the centre; A,B is the rightmost point and C,D the topmost
      // point (Visio stores edge points, not radii).
      const aX = values.get('A');
      const bY = values.get('B');
      const cX = values.get('C');
      const dY = values.get('D');
      const [cx, cy] = map(gx, gy);
      const [ex] = map(typeof aX === 'number' ? aX : gx,
        typeof bY === 'number' ? bY : gy);
      const [, ey] = map(typeof cX === 'number' ? cX : gx,
        typeof dY === 'number' ? dY : gy);
      const rx = Math.abs(ex - cx);
      const ry = Math.abs(ey - cy);
      curLocal = { x: gx, y: gy };
      d += point('M', cx - rx, cy)
        + `A${r2(rx)} ${r2(ry)} 0 1 0 ${r2(cx + rx)} ${r2(cy)}`
        + `A${r2(rx)} ${r2(ry)} 0 1 0 ${r2(cx - rx)} ${r2(cy)}Z`;
    } else if (t === 'EllipticalArcTo') {
      const end = { x: gx, y: gy };
      const startLocal = curLocal ? { x: curLocal.x, y: curLocal.y } : { x: 0, y: 0 };
      const ax = values.get('A');
      const by = values.get('B');
      const mid = {
        x: typeof ax === 'number' ? ax : startLocal.x,
        y: typeof by === 'number' ? by : startLocal.y,
      };
      const theta = typeof values.get('C') === 'number' ? values.get('C') : 0;
      const ratio = typeof values.get('D') === 'number' ? values.get('D') : 1;
      const pts = ellipticalArcPoints(startLocal, end, mid, theta, ratio);
      if (pts) {
        for (let i = 1; i < pts.length; i++) {
          const [mx, my] = map(pts[i].x, pts[i].y);
          d += point('L', mx, my);
        }
      } else {
        const [ex, ey] = map(gx, gy);
        if (cur && typeof ax === 'number' && typeof by === 'number') {
          // Fallback: a quadratic through the control point.
          const [mx, my] = map(ax, by);
          const qx = 2 * mx - 0.5 * (cur.x + ex);
          const qy = 2 * my - 0.5 * (cur.y + ey);
          d += `Q${r2(qx)} ${r2(qy)} ${r2(ex)} ${r2(ey)}`;
          cur = { x: ex, y: ey };
        } else {
          d += point('L', ex, ey);
        }
      }
      curLocal = end;
    } else if (t === 'NURBSTo') {
      // X/Y is the final control point; the E cell carries the interior
      // controls as NURBS(knotLast, degree, xType, yType, x,y,knot,weight, …).
      const end = { x: gx, y: gy };
      const parsed = parseNurbsFormula(cellText(row, 'E'));
      const start = curLocal || { x: 0, y: 0 };
      const curve = nurbsCurve(parsed, start, end, {
        firstKnot: values.get('C'),
        secondLastKnot: values.get('A'),
        firstWeight: values.get('D'),
        lastWeight: values.get('B'),
      }, size);
      if (curve.controls.length > 2) {
        appendCurve(curve.controls, curve.degree, curve.knots);
        curLocal = end;
      } else {
        const [mx, my] = map(gx, gy);
        curLocal = end;
        d += point('L', mx, my);
      }
    } else if (t === 'SplineStart') {
      const start = curLocal ? { x: curLocal.x, y: curLocal.y } : { x: gx, y: gy };
      spline = {
        // the row before SplineStart is the first control point
        controls: [start, { x: gx, y: gy }],
        degree: Math.max(1, Math.round(val('D')) || 3),
        knots: [val('B'), val('A')],
        lastKnot: val('C'),
      };
      curLocal = { x: gx, y: gy };
    } else if (t === 'SplineKnot') {
      if (spline) {
        spline.controls.push({ x: gx, y: gy });
        spline.knots.push(val('A'));
        curLocal = { x: gx, y: gy };
      } else {
        const [mx, my] = map(gx, gy);
        curLocal = { x: gx, y: gy };
        d += point('L', mx, my);
      }
    } else if (t === 'SplineEnd') {
      if (spline) {
        spline.knots.push(spline.lastKnot);
        flushSpline();
      }
    } else if (t === 'InfiniteLine') {
      // X/Y and A/B are two points on an unbounded line. Draw a long segment
      // through them; a section with an infinite line is never closed.
      const ax = values.get('A');
      const by = values.get('B');
      const p1 = { x: gx, y: gy };
      const p2 = { x: typeof ax === 'number' ? ax : gx + 1, y: typeof by === 'number' ? by : gy };
      const dx = p2.x - p1.x;
      const dy = p2.y - p1.y;
      const len = Math.hypot(dx, dy);
      if (len > 1e-9) {
        const ext = 240;
        const [x1, y1] = map(p1.x - (dx / len) * ext, p1.y - (dy / len) * ext);
        const [x2, y2] = map(p2.x + (dx / len) * ext, p2.y + (dy / len) * ext);
        d += point('M', x1, y1) + point('L', x2, y2);
      }
      curLocal = p2;
      hasInfinite = true;
    } else if (t === 'PolylineTo') {
      const end = { x: gx, y: gy };
      const poly = parsePolylineFormula(cellText(row, 'A'));
      let last = curLocal;
      if (poly) {
        for (const p of poly.points) {
          const lx = poly.xType ? p.x : p.x * size.w;
          const ly = poly.yType ? p.y : p.y * size.h;
          if (last && Math.abs(lx - last.x) < 1e-9 && Math.abs(ly - last.y) < 1e-9) {
            last = { x: lx, y: ly };
            continue;
          }
          const [mx, my] = map(lx, ly);
          d += point('L', mx, my);
          last = { x: lx, y: ly };
        }
      }
      if (!last || Math.abs(end.x - last.x) > 1e-9 || Math.abs(end.y - last.y) > 1e-9) {
        const [mx, my] = map(gx, gy);
        d += point('L', mx, my);
      }
      curLocal = end;
    }
  }
  flushSpline();
  if (hasInfinite) return d;
  return d ? (close && !d.endsWith('Z') ? `${d}Z` : d) : '';
}

/** Bounding box of evaluated geometry coordinates, in inches. */
function evaluatedBox(table) {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const row of table.values()) {
    const x = row.get('X');
    const y = row.get('Y');
    if (typeof x !== 'number' || typeof y !== 'number') continue;
    minX = Math.min(minX, x); maxX = Math.max(maxX, x);
    minY = Math.min(minY, y); maxY = Math.max(maxY, y);
  }
  if (!Number.isFinite(minX)) return null;
  return { x: minX, y: minY, w: Math.max(0.01, maxX - minX), h: Math.max(0.01, maxY - minY) };
}

/**
 * The geometry of a shape, as one or more SVG paths.
 *
 * A shape that draws itself uses its own geometry; a shape built from a
 * master merges the master's rows with the page's partial overrides. A
 * master that is a *group* of body shapes (Visio's 3-D blocks are a bevelled
 * rim plus a flat front face) contributes one path per body part.
 */
function partsOf(r, theme) {
  const parts = [];
  const pageGeoms = geometrySections(r.el);
  const mshape = r.mshape;
  const masterGeoms = geometrySections(mshape);
  const nested = mshape ? findAll(mshape, 'Shape') : [];

  /* --- a master assembled from several body shapes ---------------------- */
  if (nested.length) {
    const mCells = cellsOf(mshape);
    // 3-D masters: Visio projects the top face at a shallow angle, so its
    // depth is much less than the side's. Keep that foreshortening.
    const bevelMaster = ['BevelTopType', 'BevelBottomType', 'BevelDepthSize']
      .some((n) => mCells.has(n));
    const projectY = (gy) => (bevelMaster && gy < 0 ? gy * 0.5 : gy);
    let beveled = false;
    let frontFace = null;
    const { group, scopes } = buildScopes(r, mshape, nested);

    for (const sub of nested) {
      const geoms = geometrySections(sub);
      if (!geoms.length) continue;
      const subCells = cellsOf(sub);
      const scope = scopes.get(sub.getAttribute('ID')) || new GeometryScope({
        id: sub.getAttribute('ID') || null,
        cells: subCells,
        el: sub,
        parent: group,
        scopes,
      });
      const width = scope.get('Width');
      const height = scope.get('Height');
      const subW = Number.isFinite(width) && width !== 0 ? width
        : (cellValue(subCells, 'Width', 1) || 1);
      const subH = Number.isFinite(height) && height !== 0 ? height
        : (cellValue(subCells, 'Height', 1) || 1);
      const pinX = scope.get('PinX');
      const pinY = scope.get('PinY');
      const locX = scope.get('LocPinX');
      const locY = scope.get('LocPinY');
      const ox = (Number.isFinite(pinX) ? pinX : cellValue(subCells, 'PinX'))
        - (Number.isFinite(locX) ? locX : subW / 2);
      const oy = (Number.isFinite(pinY) ? pinY : cellValue(subCells, 'PinY'))
        - (Number.isFinite(locY) ? locY : subH / 2);

      // a master's own coordinate space runs top-down, unlike the page's
      const map = (gx, gy) => [
        (ox + gx) * DPI,
        (oy + projectY(gy)) * DPI,
      ];
      const relMap = (gx, gy) => map(gx * subW, gy * subH);
      const style = styleOf([r.cells, subCells, mCells], theme, r.styleCtx);

      // A master body shape's later geometry sections are the alternatives
      // that NoShow formulas pick between; without evaluating those, keep
      // the primary section (the pre-existing behaviour for BPMN icons).
      for (const geom of geoms.slice(0, 1)) {
        // Cached NoShow is reliable for ordinary master bodies (the BPMN
        // task hides its collapsed-subprocess icons this way).
        if (sectionFlag(geom, 'NoShow')) continue;
        const rows = mergeRows(geom, null);
        const sectionNo = geoms.indexOf(geom) + 1;
        const part = { ...partStyle(style, null, geom) };
        const d = evaluatedPath(scope, rows, sectionNo, map, relMap,
          part.fill !== 'none', {
            size: { w: subW, h: subH },
            rel: (x, y) => [x * subW, y * subH],
          });
        if (!d) continue;
        part.d = d;
        // A part that pokes out of the box is the bevelled rim. Visio shades
        // it with a bevel; a diagonal tint from the face colour reads the
        // same at screen size, and keeps the rim from turning white.
        const box = evaluatedBox(scope.geometry.get(sectionNo));
        if (box) {
          const pts = [[box.x, box.y], [box.x + box.w, box.y + box.h]]
            .map((p) => map(p[0], p[1]));
          const outside = pts.some(([px, py]) => px < -1 || py < -1
            || px > r.w * DPI + 1 || py > r.h * DPI + 1);
          if (outside && part.fill && part.fill !== 'none') {
            beveled = true;
            part.fillGradient = [tint(part.fill, 26), tint(part.fill, 0)];
          } else if (!outside && part.fill && part.fill !== 'none') {
            const [x1, y] = map(box.x, box.y + box.h);
            const [x2] = map(box.x + box.w, box.y + box.h);
            frontFace = { part, edge: { x1, y1: y, x2, y2: y } };
          }
        }
        parts.push(part);
      }
    }

    // the bevelled block is outlined in a light tint of its own colour
    if (beveled) {
      for (const p of parts) {
        if (!p.fill || p.fill === 'none') continue;
        p.stroke = tint(p.fill, 80);
        p.strokeWidth = Math.max(0.9, p.strokeWidth || 0);
      }
      // the shaded bottom lip of the front face gives the block its weight
      if (frontFace) {
        frontFace.part.bottomEdge = {
          ...frontFace.edge,
          stroke: tint(frontFace.part.fill, -52),
          strokeWidth: Math.max(1, frontFace.part.strokeWidth || 0),
        };
      }
    }
    return parts;
  }

  /* --- one body shape, with page overrides merged in -------------------- */
  if (!pageGeoms.length && !masterGeoms.length) return parts;

  const mCells = mshape ? cellsOf(mshape) : new Map();
  const style = styleOf(mshape ? [r.cells, mCells] : [r.cells], theme, r.styleCtx);
  const bevelMaster = ['BevelTopType', 'BevelBottomType', 'BevelDepthSize']
    .some((n) => mCells.has(n));
  const projectY = (gy) => (bevelMaster && gy < 0 ? gy * 0.5 : gy);

  const pageMap = (gx, gy) => [gx * DPI, (r.h - gy) * DPI];
  const pageRel = (gx, gy) => pageMap(gx * r.w, gy * r.h);

  const usePage = pageGeoms.length > 0;
  const ownScope = new GeometryScope({
    id: mshape?.getAttribute('ID') || null,
    cells: r.cells,
    el: r.el,
    masterEl: mshape,
    parent: null,
  });
  const scopes = new Map();
  if (ownScope.id) scopes.set(ownScope.id, ownScope);
  ownScope.scopes = scopes;
  const subW = Number.isFinite(ownScope.get('Width'))
    ? ownScope.get('Width') : (r.w || 1);
  const subH = Number.isFinite(ownScope.get('Height'))
    ? ownScope.get('Height') : (r.h || 1);
  const masterMap = (gx, gy) => [gx * DPI, projectY(gy) * DPI];
  const masterRel = (gx, gy) => masterMap(gx * subW, gy * subH);
  const map = usePage ? pageMap : masterMap;
  const relMap = usePage ? pageRel : masterRel;

  if (usePage) {
    const byIx = new Map();
    for (const g of masterGeoms) {
      byIx.set(g.getAttribute('IX') || '0', { master: g, page: null });
    }
    for (const g of pageGeoms) {
      const ix = g.getAttribute('IX') || '0';
      const entry = byIx.get(ix) || { master: null, page: null };
      entry.page = g;
      byIx.set(ix, entry);
    }
    for (const { master, page } of byIx.values()) {
      if (sectionFlag(page, 'NoShow')) continue;
      const part = { ...partStyle(style, master, page) };
      const d = pathFromRows(mergeRows(master, page), map, relMap,
        part.fill !== 'none', {
          size: { w: r.w || 1, h: r.h || 1 },
          rel: pageRel,
        });
      if (!d) continue;
      parts.push({ d, ...part });
    }
    return parts;
  }

  // No page geometry: evaluate the master's formulas against this instance.
  masterGeoms.forEach((master, i) => {
    const part = { ...partStyle(style, master, null) };
    const d = evaluatedPath(ownScope, mergeRows(master, null), i + 1,
      map, relMap, part.fill !== 'none', {
        size: { w: subW, h: subH },
        rel: masterRel,
      });
    if (d) parts.push({ d, ...part });
  });
  return parts;
}

/** Fill/line switches that belong to one geometry section. */
function partStyle(base, master, page) {
  const out = { ...base };
  if (geomFlag(master, page, 'NoFill')) out.fill = 'none';
  if (geomFlag(master, page, 'NoLine')) { out.stroke = 'none'; out.strokeWidth = 0; }
  return out;
}

/** Convert merged geometry rows into an SVG path. Only fills are closed. */
function pathFromRows(rows, map, relMap, close = true, localCtx = null) {
  let d = '';
  let started = false;
  let cur = null;
  let curLocal = null;
  let spline = null;
  let hasInfinite = false;
  const size = localCtx?.size || { w: 1, h: 1 };
  const point = (cmd, mx, my) => {
    const c = started ? cmd : 'M';
    started = true;
    cur = { x: mx, y: my };
    return `${c}${r2(mx)} ${r2(my)}`;
  };
  const appendCurve = (controls, degree, knots = null) => {
    const perSpan = Math.max(8, Math.min(24,
      Math.round(96 / Math.max(1, controls.length - 1))));
    const parts = sampleSpline(controls, degree, knots, perSpan);
    for (const p of parts.slice(1)) {
      const [mx, my] = map(p.x, p.y);
      d += point('L', mx, my);
    }
    const last = parts[parts.length - 1];
    if (last) curLocal = { x: last.x, y: last.y };
  };
  const appendPeriodic = (controls, degree, base, step) => {
    const n = controls.length;
    const p = Math.max(1, Math.min(degree, n - 1));
    if (n <= p || !(step > 0)) {
      appendCurve(controls, degree);
      return;
    }
    // A uniform knot sequence with one knot per control point is Visio's
    // closed (periodic) spline. Repeat the controls so de Boor can cross the
    // seam, and evaluate the middle copy.
    const ext = [...controls, ...controls, ...controls];
    const full = [];
    for (let i = 0; i < ext.length + p + 1; i++) full.push(base + i * step);
    const per = Math.max(8, Math.min(24, Math.round(240 / Math.max(1, n))));
    const pts = [];
    for (let s = 0; s < n; s++) {
      const a = base + (n + s) * step;
      for (let i = s === 0 ? 0 : 1; i <= per; i++) {
        pts.push(nurbsPoint(ext, p, full, a + (step * i) / per));
      }
    }
    if (!pts.length) return;
    const [mx0, my0] = map(pts[0].x, pts[0].y);
    d += point('M', mx0, my0);
    for (let i = 1; i < pts.length; i++) {
      const [mx, my] = map(pts[i].x, pts[i].y);
      d += point('L', mx, my);
    }
    const last = pts[pts.length - 1];
    curLocal = { x: last.x, y: last.y };
  };
  const flushSpline = () => {
    if (!spline) return;
    const sp = spline;
    spline = null;
    if (sp.controls.length < 2) return;
    const k = sp.knots;
    const uniform = k.length === sp.controls.length + 1
      && k.every((v, i) => i === 0 || Math.abs(v - k[i - 1] - (k[1] - k[0])) < 1e-9);
    if (uniform) appendPeriodic(sp.controls, sp.degree, k[0], k[1] - k[0]);
    else appendCurve(sp.controls, sp.degree);
  };
  const cellText = (c, name) => {
    let cell = c.get(name);
    if (!cell) {
      for (const [k, v] of c) {
        if (String(k).toUpperCase() === name) { cell = v; break; }
      }
    }
    if (!cell) return null;
    const f = cell.getAttribute('F');
    if (f && f !== 'Inh' && f !== 'No Formula') return f;
    return cell.getAttribute('V');
  };

  for (const [, row] of rows) {
    const t = row.t;
    const c = row.cells;
    if (spline && t !== 'SplineKnot' && t !== 'SplineEnd') flushSpline();
    const gx = cellValue(c, 'X');
    const gy = cellValue(c, 'Y');
    if (t === 'RelMoveTo' || t === 'RelLineTo') {
      const [mx, my] = relMap(gx, gy);
      const [lx, ly] = localCtx?.rel ? localCtx.rel(gx, gy) : [gx, gy];
      curLocal = { x: lx, y: ly };
      d += point(t === 'RelMoveTo' ? 'M' : 'L', mx, my);
      continue;
    }
    if (t === 'MoveTo' || t === 'LineTo') {
      const [mx, my] = map(gx, gy);
      curLocal = { x: gx, y: gy };
      d += point(t === 'MoveTo' ? 'M' : 'L', mx, my);
    } else if (t === 'ArcTo') {
      const end = { x: gx, y: gy };
      const startLocal = curLocal ? { x: curLocal.x, y: curLocal.y } : { x: gx, y: gy };
      const pts = circularArcPoints(startLocal, end, cellValue(c, 'A', 0));
      if (pts) {
        for (let i = 1; i < pts.length; i++) {
          const [mx, my] = map(pts[i].x, pts[i].y);
          d += point('L', mx, my);
        }
      } else {
        const [mx, my] = map(gx, gy);
        d += point('L', mx, my);
      }
      curLocal = end;
    } else if (t === 'Ellipse') {
      const [cx, cy] = map(gx, gy);
      const ex = cellValue(c, 'A', gx);
      const ey = cellValue(c, 'D', gy);
      const rx = Math.abs(map(ex, cellValue(c, 'B', gy))[0] - cx);
      const ry = Math.abs(map(cellValue(c, 'C', gx), ey)[1] - cy);
      curLocal = { x: gx, y: gy };
      d += point('M', cx - rx, cy)
        + `A${r2(rx)} ${r2(ry)} 0 1 0 ${r2(cx + rx)} ${r2(cy)}`
        + `A${r2(rx)} ${r2(ry)} 0 1 0 ${r2(cx - rx)} ${r2(cy)}Z`;
    } else if (t === 'EllipticalArcTo') {
      const end = { x: gx, y: gy };
      const startLocal = curLocal ? { x: curLocal.x, y: curLocal.y } : { x: 0, y: 0 };
      const mid = {
        x: cellValue(c, 'A', startLocal.x),
        y: cellValue(c, 'B', startLocal.y),
      };
      const pts = ellipticalArcPoints(startLocal, end, mid,
        cellValue(c, 'C', 0), cellValue(c, 'D', 1));
      if (pts) {
        for (let i = 1; i < pts.length; i++) {
          const [mx, my] = map(pts[i].x, pts[i].y);
          d += point('L', mx, my);
        }
      } else {
        const [ex, ey] = map(gx, gy);
        if (cur) {
          const [mx, my] = map(mid.x, mid.y);
          const qx = 2 * mx - 0.5 * (cur.x + ex);
          const qy = 2 * my - 0.5 * (cur.y + ey);
          d += `Q${r2(qx)} ${r2(qy)} ${r2(ex)} ${r2(ey)}`;
          cur = { x: ex, y: ey };
        } else {
          d += point('L', ex, ey);
        }
      }
      curLocal = end;
    } else if (t === 'NURBSTo') {
      // X/Y is the final control point; the E cell carries the interior
      // controls as NURBS(knotLast, degree, xType, yType, x,y,knot,weight, …).
      const end = { x: gx, y: gy };
      const parsed = parseNurbsFormula(cellText(c, 'E'));
      const start = curLocal || { x: 0, y: 0 };
      const curve = nurbsCurve(parsed, start, end, {
        firstKnot: cellValue(c, 'C', 0),
        secondLastKnot: cellValue(c, 'A', 0),
        firstWeight: cellValue(c, 'D', 1),
        lastWeight: cellValue(c, 'B', 1),
      }, size);
      if (curve.controls.length > 2) {
        appendCurve(curve.controls, curve.degree, curve.knots);
        curLocal = end;
      } else {
        const [mx, my] = map(gx, gy);
        curLocal = end;
        d += point('L', mx, my);
      }
    } else if (t === 'SplineStart') {
      const start = curLocal ? { x: curLocal.x, y: curLocal.y } : { x: gx, y: gy };
      spline = {
        controls: [start, { x: gx, y: gy }],
        degree: Math.max(1, Math.round(cellValue(c, 'D', 3)) || 3),
        knots: [cellValue(c, 'B', 0), cellValue(c, 'A', 0)],
        lastKnot: cellValue(c, 'C', 0),
      };
      curLocal = { x: gx, y: gy };
    } else if (t === 'SplineKnot') {
      if (spline) {
        spline.controls.push({ x: gx, y: gy });
        spline.knots.push(cellValue(c, 'A', 0));
        curLocal = { x: gx, y: gy };
      } else {
        const [mx, my] = map(gx, gy);
        curLocal = { x: gx, y: gy };
        d += point('L', mx, my);
      }
    } else if (t === 'SplineEnd') {
      if (spline) {
        spline.knots.push(spline.lastKnot);
        flushSpline();
      }
    } else if (t === 'InfiniteLine') {
      const p1 = { x: gx, y: gy };
      const p2 = { x: cellValue(c, 'A', gx + 1), y: cellValue(c, 'B', gy) };
      const dx = p2.x - p1.x;
      const dy = p2.y - p1.y;
      const len = Math.hypot(dx, dy);
      if (len > 1e-9) {
        const ext = 240;
        const [x1, y1] = map(p1.x - (dx / len) * ext, p1.y - (dy / len) * ext);
        const [x2, y2] = map(p2.x + (dx / len) * ext, p2.y + (dy / len) * ext);
        d += point('M', x1, y1) + point('L', x2, y2);
      }
      curLocal = p2;
      hasInfinite = true;
    } else if (t === 'PolylineTo') {
      const end = { x: gx, y: gy };
      const poly = parsePolylineFormula(cellText(c, 'A'));
      let last = curLocal;
      if (poly) {
        for (const p of poly.points) {
          const lx = poly.xType ? p.x : p.x * size.w;
          const ly = poly.yType ? p.y : p.y * size.h;
          if (last && Math.abs(lx - last.x) < 1e-9 && Math.abs(ly - last.y) < 1e-9) {
            last = { x: lx, y: ly };
            continue;
          }
          const [mx, my] = map(lx, ly);
          d += point('L', mx, my);
          last = { x: lx, y: ly };
        }
      }
      if (!last || Math.abs(end.x - last.x) > 1e-9 || Math.abs(end.y - last.y) > 1e-9) {
        const [mx, my] = map(gx, gy);
        d += point('L', mx, my);
      }
      curLocal = end;
    }
  }
  flushSpline();
  if (hasInfinite) return d;
  return d ? (close && !d.endsWith('Z') ? `${d}Z` : d) : '';
}

function geometryKind(el) {
  const geom = section(el, 'Geometry');
  if (!geom) return null;
  for (const row of kids(geom, 'Row')) {
    const t = row.getAttribute('T');
    if (t === 'Ellipse' || t === 'EllipticalArcTo') return 'ellipse';
  }
  return null;
}

/**
 * Fill, line and text styling.
 * `chain` is a list of cell maps in priority order — the shape's own cells
 * first, then the master's, then whatever it inherited.
 */
function styleOf(chain, theme, styleCtx = null) {
  const fillStyles = styleCtx?.fillStyles || [];
  const lineStyles = styleCtx?.lineStyles || [];
  const findChain = (name) => {
    for (const set of chain) {
      const c = set.get(name);
      if (!c) continue;
      // "Inh" usually means "look further up", but Visio also caches the
      // resolved value on the instance; a concrete cached value wins.
      if (c.getAttribute('F') === 'Inh' && !isConcrete(c)) continue;
      return c;
    }
    for (const set of chain) {
      const c = set.get(name);
      if (c) return c;
    }
    return null;
  };
  const findExtra = (name, extra) => {
    for (const set of extra) {
      const c = set.get(name);
      if (c && c.getAttribute('F') !== 'Inh') return c;
    }
    for (const set of extra) {
      const c = set.get(name);
      if (c) return c;
    }
    return null;
  };
  const pickWith = (name, extra) => findChain(name) || findExtra(name, extra);
  const numeric = (cell) => {
    const v = cell ? cell.getAttribute('V') : null;
    if (v === null || v === '' || v === 'Themed') return null;
    const n = parseFloat(v);
    return Number.isFinite(n) ? n : null;
  };

  const ownFill = findChain('FillForegnd');
  const fillCell = ownFill || findExtra('FillForegnd', fillStyles);
  const bkgndCell = pickWith('FillBkgnd', fillStyles);
  const pattern = pickWith('FillPattern', fillStyles);
  const lineColor = pickWith('LineColor', lineStyles);
  const lineWeight = pickWith('LineWeight', lineStyles);
  const linePattern = pickWith('LinePattern', lineStyles);
  const qsfc = numeric(pickWith('QuickStyleFillColor',
    [...fillStyles, ...lineStyles]));
  const qslc = numeric(pickWith('QuickStyleLineColor',
    [...lineStyles, ...fillStyles]));

  // An explicit cached fill wins; otherwise a real quick-style colour supplies
  // the fill, then the themed role value (white for a bare THEMEVAL()).
  let fill = null;
  if (fillCell && isConcrete(fillCell)) fill = theme.colorOf(fillCell);
  let quickFill = qsfc;
  if (quickFill === 100) quickFill = null;      // 100 is the style default
  if (!fill && quickFill !== null) {
    let quick = theme.quickColor(quickFill);
    // Quick-style matrices 1-3 are light tints. Large frames/containers use
    // them; painting the raw variant makes a swimlane a solid colour block.
    if (quick && styleCtx?.large) quick = tint(quick, 85);
    fill = quick;
  }
  if (!fill) fill = theme.themedColor(fillCell, 'fill');
  if (!fill) fill = theme.themedColor(bkgndCell, 'fill');
  if (!fill) fill = '#ffffff';
  const noFill = pattern ? inches(pattern, 1) === 0 : false;

  let stroke = null;
  if (lineColor && isConcrete(lineColor)) stroke = theme.colorOf(lineColor);
  let quickLine = qslc;
  if (quickLine === 100) quickLine = null;
  if (!stroke && quickLine !== null) stroke = theme.quickColor(quickLine);
  if (!stroke) stroke = theme.themedColor(lineColor, 'line');
  if (!stroke) stroke = '#000000';

  const weight = lineWeight ? inches(lineWeight, 0.01) : 0.01;
  const out = {
    fill: noFill ? 'none' : fill,
    stroke,
    strokeWidth: Math.max(0.5, Math.round(weight * DPI * 100) / 100),
  };
  if (linePattern) {
    const p = inches(linePattern, 1);
    if (p === 0) out.strokeWidth = 0;
    else if (p === 2) out.dash = 'dashed';
    else if (p === 3) out.dash = 'dotted';
    else if (p === 23) out.dash = 'dashFine';
  }
  return out;
}

/**
 * The shape's text block: where the label sits inside the shape.
 * Visio keeps this separate from the shape's own box.
 */
function textBlock(r) {
  const c = r.cells;
  if (!c.has('TxtWidth') || !c.has('TxtHeight')) {
    // no separate text block: the label sits in the shape, inside its margins
    // No margins in the shape or its style means zero, not Visio's default:
    // an imported text-only shape uses its whole box for the label.
    const mL = cellValue(c, 'LeftMargin', 0);
    const mR = cellValue(c, 'RightMargin', 0);
    const mT = cellValue(c, 'TopMargin', 0);
    const mB = cellValue(c, 'BottomMargin', 0);
    return {
      x: mL * DPI,
      y: mT * DPI,
      w: Math.max(4, (r.w - mL - mR) * DPI),
      h: Math.max(4, (r.h - mT - mB) * DPI),
    };
  }
  const w = cellValue(c, 'TxtWidth');
  const h = cellValue(c, 'TxtHeight');
  const pinX = cellValue(c, 'TxtPinX', w / 2);
  const pinY = cellValue(c, 'TxtPinY', h / 2);
  const locX = cellValue(c, 'TxtLocPinX', w / 2);
  const locY = cellValue(c, 'TxtLocPinY', h / 2);
  const x = pinX - locX;
  const y = pinY - locY;                       // lower-left, y up
  return {
    x: x * DPI,
    y: (r.h - (y + h)) * DPI,
    w: w * DPI,
    h: h * DPI,
  };
}

/** Visio's VerticalAlign: 0 top, 1 middle, 2 bottom. */
function verticalAlign(cells) {
  if (!cells.has('VerticalAlign')) return null;
  const v = Math.round(cellValue(cells, 'VerticalAlign', 1));
  return v === 0 ? 'top' : v === 2 ? 'bottom' : 'middle';
}

/** Paragraph HorzAlign: 0 left, 1 centre, 2 right. */
function paragraphAlign(el, mshape) {
  for (const src of [el, mshape]) {
    const sec = src ? section(src, 'Paragraph') : null;
    const row = sec ? kids(sec, 'Row')[0] : null;
    if (!row) continue;
    const c = kids(row, 'Cell').find((x) => x.getAttribute('N') === 'HorzAlign');
    if (!c) continue;
    const v = Math.round(inches(c, 1));
    return v === 0 ? 'left' : v === 2 ? 'right' : 'center';
  }
  return null;
}

/** Shape hyperlinks (`Section N='Hyperlink'`). */
function hyperlinkDataOf(el, mshape) {
  const rows = new Map();
  for (const src of [mshape, el]) {
    if (!src) continue;
    const sec = section(src, 'Hyperlink');
    if (!sec) continue;
    for (const row of kids(sec, 'Row')) {
      const name = String(row.getAttribute('N') || '');
      if (!name) continue;
      const rc = cellsOf(row);
      const val = (k, fallback = '') => {
        const c = rc.get(k);
        if (!c) return fallback;
        return String(c.getAttribute('V') ?? fallback).replace(/^"|"$/g, '');
      };
      rows.set(name, {
        name,
        address: val('Address'),
        subAddress: val('SubAddress'),
        description: val('Description'),
        extraInfo: val('ExtraInfo'),
        frame: val('Frame'),
        newWindow: Number(val('NewWindow', '0')) || 0,
        default: Number(val('Default', '0')) || 0,
        invisible: Number(val('Invisible', '0')) || 0,
        sortKey: val('SortKey'),
      });
    }
  }
  return [...rows.values()];
}

/** Shape Data rows (`Section N='Property'`), instance over master by name. */
function propertyDataOf(el, mshape) {
  const rows = new Map();
  for (const src of [mshape, el]) {
    if (!src) continue;
    const sec = section(src, 'Property') || section(src, 'Prop');
    if (!sec) continue;
    for (const row of kids(sec, 'Row')) {
      const name = String(row.getAttribute('N') || '');
      if (!name) continue;
      const rc = cellsOf(row);
      const val = (k, fallback = '') => {
        const c = rc.get(k);
        if (!c) return fallback;
        return String(c.getAttribute('V') ?? fallback).replace(/^"|"$/g, '');
      };
      rows.set(name, {
        name,
        label: val('Label', name),
        value: val('Value', ''),
        prompt: val('Prompt', ''),
        format: val('Format', ''),
        type: Number(val('Type', '0')) || 0,
        invisible: Number(val('Invisible', '0')) || 0,
      });
    }
  }
  return [...rows.values()];
}

/**
 * Text styling for one Character row, falling back to row 0, the master and
 * the TextStyle chain. Runs switch rows with <cp IX="n"/>.
 */
function characterRowStyle(el, mshape, cells, theme, styleRows, rowIX) {
  const rows = [];
  const add = (src, ix) => {
    const sec = src ? section(src, 'Character') : null;
    if (!sec) return;
    const row = kids(sec, 'Row').find((r) => String(r.getAttribute('IX') || '0') === String(ix));
    if (row) rows.push(cellsOf(row));
  };
  add(el, rowIX);
  add(mshape, rowIX);
  if (String(rowIX) !== '0') {
    add(el, 0);
    add(mshape, 0);
  }
  const ownRows = rows.length;
  rows.push(...styleRows);

  const pickEntry = (name) => {
    for (let i = 0; i < rows.length; i++) {
      const c = rows[i].get(name);
      if (c && c.getAttribute('F') !== 'Inh') return { c, i };
    }
    for (let i = 0; i < rows.length; i++) {
      const c = rows[i].get(name);
      if (c) return { c, i };
    }
    return null;
  };
  const pick = (name) => pickEntry(name)?.c || null;

  const out = {};
  const size = pick('Size') || cells.get('Char.Size');
  if (size) {
    const v = inches(size, 0);
    if (v > 0) out.fontSize = Math.max(5, Math.round(v * DPI));
  }
  const colorPick = pickEntry('Color');
  let colour = theme.themedColor(colorPick?.c || cells.get('Char.Color'), 'text');
  if (colour && colorPick && colorPick.i >= ownRows
      && colorPick.c.getAttribute('V') === 'Themed') {
    colour = tint(colour, -12);
  }
  if (colour) out.textColor = colour;
  const style = pick('Style') || cells.get('Char.Style');
  if (style) {
    const s = Math.round(inches(style, 0));
    out.bold = (s & 1) !== 0;
    out.italic = (s & 2) !== 0;
    out.underline = (s & 4) !== 0;
  }
  const font = pick('Font') || cells.get('Char.Font');
  if (font) {
    const f = font.getAttribute('V');
    if (f && f !== 'Themed') out.fontFamily = f;
  }
  return out;
}

/** Paragraph row properties, keyed by IX where present. */
function paragraphRowStyle(el, mshape, ix, fallbackAlign) {
  const rows = [];
  const add = (src, wanted) => {
    const sec = src ? section(src, 'Paragraph') : null;
    const row = sec ? kids(sec, 'Row').find(
      (r) => String(r.getAttribute('IX') || '0') === String(wanted)) : null;
    if (row) rows.push(cellsOf(row));
  };
  add(el, ix);
  add(mshape, ix);
  if (String(ix) !== '0') {
    add(el, 0);
    add(mshape, 0);
  }
  const pick = (name) => {
    for (const row of rows) if (row.has(name)) return row.get(name);
    return null;
  };
  const alignCell = pick('HorzAlign');
  let align = fallbackAlign || 'center';
  if (alignCell) {
    const v = Math.round(inches(alignCell, 1));
    align = v === 0 ? 'left' : v === 2 ? 'right' : 'center';
  }
  const bulletCell = pick('Bullet');
  const bullet = bulletCell ? Math.round(inches(bulletCell, 0)) !== 0 : false;
  const strCell = pick('BulletStr');
  let bulletStr = strCell ? String(strCell.getAttribute('V') ?? '') : '';
  bulletStr = bulletStr.replace(/^"|"$/g, '') || '•';
  const leftCell = pick('IndLeft');
  const indent = leftCell ? Math.max(0, inches(leftCell, 0)) * DPI : 0;
  return { align, bullet, bulletStr, indent };
}

/**
 * Parse a Visio Text element into paragraphs of styled runs and field nodes.
 * Returns null for the ordinary single-run case so plain labels keep their
 * current fast path.
 */
function richTextOf(r, theme, styleRows, fallbackAlign) {
  const textEl = kid(r.el, 'Text') || kid(r.mshape, 'Text');
  if (!textEl) return null;
  const fieldRows = new Map();
  const paraIXs = new Set([0]);
  const charIXs = new Set([0]);
  for (const src of [r.el, r.mshape]) {
    if (!src) continue;
    const fsec = section(src, 'Field');
    if (fsec) {
      for (const row of kids(fsec, 'Row')) {
        const ix = String(row.getAttribute('IX') || '0');
        const rc = cellsOf(row);
        const value = rc.get('Value');
        fieldRows.set(ix, {
          formula: value ? (value.getAttribute('F') || '') : '',
          value: value ? String(value.getAttribute('V') ?? '') : '',
          format: rc.get('Format') ? String(rc.get('Format').getAttribute('F') || '') : '',
        });
      }
    }
    const psec = section(src, 'Paragraph');
    if (psec) for (const row of kids(psec, 'Row')) paraIXs.add(String(row.getAttribute('IX') || '0'));
    const csec = section(src, 'Character');
    if (csec) for (const row of kids(csec, 'Row')) charIXs.add(String(row.getAttribute('IX') || '0'));
  }

  const charCache = new Map();
  const charStyle = (ix) => {
    const key = String(ix);
    if (!charCache.has(key)) {
      charCache.set(key, characterRowStyle(r.el, r.mshape, r.cells, theme, styleRows, key));
    }
    return charCache.get(key);
  };

  const paragraphs = [];
  let cur = { pp: '0', runs: [] };
  let cp = '0';
  let hasField = false;
  let cpSwitches = 0;
  let ppSwitches = 0;

  const startParagraph = (pp) => {
    if (cur.runs.some((run) => (run.text || '').length || run.field)) {
      paragraphs.push(cur);
      cur = { pp: String(pp), runs: [] };
    } else {
      cur.pp = String(pp);
    }
  };
  const pushText = (text) => {
    if (!text) return;
    cur.runs.push({ text, cp, style: charStyle(cp) });
  };

  for (const node of textEl.childNodes) {
    if (node.nodeType === 1) {
      const local = node.localName;
      if (local === 'cp') { cp = String(node.getAttribute('IX') || '0'); cpSwitches++; }
      else if (local === 'pp') { ppSwitches++; startParagraph(node.getAttribute('IX') || '0'); }
      else if (local === 'fld') {
        hasField = true;
        const ix = String(node.getAttribute('IX') || '0');
        const field = fieldRows.get(ix) || { formula: '', value: '', format: '' };
        cur.runs.push({
          field, cp, style: charStyle(cp), text: node.textContent || field.value || '',
        });
      }
      continue;
    }
    if (node.nodeType !== 3) continue;
    const raw = String(node.nodeValue ?? '')
      .split(String.fromCharCode(0x2028)).join('\n')
      .split(String.fromCharCode(0x2029)).join('\n');
    const parts = raw.split(/\r\n|\r|\n/);
    parts.forEach((part, i) => {
      if (i > 0) startParagraph(cur.pp);
      pushText(part);
    });
  }
  if (cur.runs.some((run) => (run.text || '').length || run.field)) paragraphs.push(cur);
  if (!paragraphs.length) return null;

  const paraRows = [...paraIXs].filter((ix) => ix !== '0');
  const charRows = [...charIXs].filter((ix) => ix !== '0');
  const rich = hasField || cpSwitches > 1 || ppSwitches > 1
    || paraRows.length > 0 || charRows.length > 0;
  if (!rich) return null;

  for (const p of paragraphs) {
    p.style = paragraphRowStyle(r.el, r.mshape, p.pp, fallbackAlign);
    // Fill in properties a sparse paragraph row did not write.
    p.align = p.style.align || fallbackAlign || 'center';
    p.bullet = !!p.style.bullet;
    p.bulletStr = p.style.bulletStr || '•';
    p.indent = p.style.indent || 0;
  }
  return { paragraphs };
}

/**
 * Text styling: the shape's Character row first, then its master's, then the
 * StyleSheets its TextStyle points at (where Visio keeps the theme font and
 * colour). Each property falls back independently — a row may set only Font.
 */
function characterStyle(el, mshape, cells, theme, styleRows = []) {
  const out = {};
  const rows = [];
  for (const src of [el, mshape]) {
    const sec = src ? section(src, 'Character') : null;
    const row = sec ? kids(sec, 'Row')[0] : null;
    if (row) rows.push(cellsOf(row));
  }
  const ownRows = rows.length;
  rows.push(...styleRows);

  const pickEntry = (name) => {
    for (let i = 0; i < rows.length; i++) {
      const c = rows[i].get(name);
      if (c && c.getAttribute('F') !== 'Inh') return { c, i };
    }
    for (let i = 0; i < rows.length; i++) {
      const c = rows[i].get(name);
      if (c) return { c, i };
    }
    return null;
  };
  const pick = (name) => pickEntry(name)?.c || null;

  const size = pick('Size') || cells.get('Char.Size');
  if (size) {
    const v = inches(size, 0);
    if (v > 0) out.fontSize = Math.max(5, Math.round(v * DPI));
  }
  const colorPick = pickEntry('Color');
  let colour = theme.themedColor(colorPick?.c || cells.get('Char.Color'), 'text');
  // A theme colour inherited only through a style is Visio's font-scheme
  // tint, a little darker than the raw accent the theme map returns.
  if (colour && colorPick && colorPick.i >= ownRows
      && colorPick.c.getAttribute('V') === 'Themed') {
    colour = tint(colour, -12);
  }
  if (colour) out.textColor = colour;
  const style = pick('Style') || cells.get('Char.Style');
  if (style) {
    const s = Math.round(inches(style, 0));
    out.bold = (s & 1) !== 0;
    out.italic = (s & 2) !== 0;
    out.underline = (s & 4) !== 0;
  }
  const font = pick('Font') || cells.get('Char.Font');
  if (font) {
    const f = font.getAttribute('V');
    if (f && f !== 'Themed') out.fontFamily = f;
  }
  return out;
}

const arrowKind = (v) => {
  if (v === 0) return 'none';
  // UML relation ends, as assigned by the Relationship master's actions.
  if (v === 12) return 'arrow';           // dependency
  if (v === 14) return 'hollowTriangle';  // inheritance / generalization
  if (v === 22) return 'hollowDiamond';   // aggregation
  if (v === 38) return 'filledDiamond';   // composition
  // 1 and 2 are the thin open heads; every heavier Visio head reads as a
  // filled triangle at screen size (4 is by far the common default).
  if (v === 1 || v === 2) return 'arrow';
  return 'filled';
};

/**
 * A world point expressed in a shape's local box, normalised to 0..1 so it
 * stays glued to the same spot when the shape is resized.
 */
function toShapeLocal(shape, point) {
  if (!shape || !point) return null;
  const cx = shape.x + shape.w / 2, cy = shape.y + shape.h / 2;
  const dx = point.x - cx, dy = point.y - cy;
  const a = -((shape.rot || 0) * Math.PI) / 180;
  const rx = dx * Math.cos(a) - dy * Math.sin(a);
  const ry = dx * Math.sin(a) + dy * Math.cos(a);
  const fx = shape.flipX ? -1 : 1, fy = shape.flipY ? -1 : 1;
  return {
    x: (rx / fx + shape.w / 2) / (shape.w || 1),
    y: (ry / fy + shape.h / 2) / (shape.h || 1),
  };
}

/** Which side of `shape` a point is approaching from. */
function sideTowards(shape, point) {
  if (!point) return 'e';
  const cx = shape.x + shape.w / 2, cy = shape.y + shape.h / 2;
  const dx = point.x - cx, dy = point.y - cy;
  if (Math.abs(dx) * (shape.h / 2) > Math.abs(dy) * (shape.w / 2)) return dx > 0 ? 'e' : 'w';
  return dy > 0 ? 's' : 'n';
}
