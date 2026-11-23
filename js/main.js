// Application entry point: owns the document, the view state and all actions.
import { History, newDoc, newPage, newShape, newConnector, page as getPageOf, calloutText, calloutTarget, DATA_COLOR_DEFAULTS } from './model.js';
import { SHAPES } from './shapes.js';
import { Renderer } from './render.js';
import { routeConnector } from './router.js';
import { initInteraction } from './interact.js';
import { buildPalette, buildRecent, buildPageTabs, updateInspector, updateToolbarState, updateStatus, wireToolbar, initContextMenu } from './ui.js';
import { saveLocal, loadLocal, exportJSON, importJSON, exportSVG, exportPNG, pageBounds } from './file.js';
import { importVisioFile } from './visio.js';
import { auditVSDX, exportVSDX, vsdxFileName } from './vsdx-writer.js';
import { persistVisioSource, loadVisioSource, deleteVisioSource } from './vsdx-source.js';
import { uid, clamp, fontString, measureText } from './util.js';
import { layoutUnits } from './layout.js';

const $ = (s) => document.querySelector(s);

// Built-in palettes for new/theme-switched documents. Imported documents keep
// the slots/variations parsed from their theme part.
function themePreset(name, accents, background = '#ffffff') {
  const slots = {
    dk1: '#000000', lt1: background, dk2: '#44546a', lt2: '#e7e6e6',
    accent1: accents[0], accent2: accents[1], accent3: accents[2],
    accent4: accents[3], accent5: accents[4], accent6: accents[5],
    bkgnd: background, hlink: '#0563c1', folhlink: '#954f72',
  };
  const variants = [0, 1, 2, 3].map((i) => {
    const colors = {};
    for (let n = 0; n < 6; n++) colors[`variant${n + 1}`] = accents[(n + i) % 6];
    return colors;
  });
  return { name, slots, variations: variants, activeVariation: 0, schemeEnum: null };
}

const THEME_PRESETS = {
  Office: themePreset('Office',
    ['#4472c4', '#ed7d31', '#a5a5a5', '#ffc000', '#5b9bd5', '#70ad47']),
  Linear: themePreset('Linear',
    ['#1ba1e2', '#339933', '#e51400', '#f09609', '#a200ff', '#00aba9']),
  Integral: themePreset('Integral',
    ['#1cade4', '#2683c6', '#27ced7', '#42ba97', '#3e8853', '#62a39f']),
  Colorful: themePreset('Colorful',
    ['#4285f4', '#ea4335', '#fbbc05', '#34a853', '#673ab7', '#00acc1']),
};

const DEFAULT_THEME = structuredClone(THEME_PRESETS.Office);

function themeTint(hex, n) {
  if (!hex || !hex.startsWith('#')) return hex;
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const t = Math.abs(n) / 100;
  const target = n >= 0 ? 255 : 0;
  const mix = (v) => Math.max(0, Math.min(255, Math.round(v + (target - v) * t)));
  return `#${[mix(r), mix(g), mix(b)].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
}

function ensureTheme(doc) {
  if (!doc.theme || !doc.theme.slots) doc.theme = structuredClone(DEFAULT_THEME);
  doc.theme.slots = { ...DEFAULT_THEME.slots, ...(doc.theme.slots || {}) };
  if (!Array.isArray(doc.theme.variations) || !doc.theme.variations.length) {
    doc.theme.variations = structuredClone(DEFAULT_THEME.variations);
  }
  if (!Number.isFinite(doc.theme.activeVariation)) doc.theme.activeVariation = 0;
  return doc.theme;
}


const app = {
  doc: newDoc(),
  history: new History(),
  state: {
    tool: 'select',
    pendingType: 'rect',
    selection: new Set(),
    view: { x: 40, y: 30, zoom: 1 },
    grid: { on: true, size: 10, snap: true },
    marquee: null,
    connectSource: null,
    connectPreview: null,
    hoverShapeId: null,
    recent: [],
    themeScope: 'selection',
    quickStyle: '',
    dgField: '',
    autosaveWarning: false,
  },
  clipboard: null,
  // Original OPC package for a VSDX/VSTX opened in this session. Kept out of
  // the JSON document so autosave/undo never try to serialise binary parts.
  visioSource: null,
  svg: null,
  stage: null,
  editor: null,
  renderer: null,
};

// ------------------------------------------------------------------ core --
app.getPage = () => getPageOf(app.doc);

app.shapeMap = () => new Map(app.getPage().shapes.map((s) => [s.id, s]));

/** Geometry of a connector, as currently routed. */
app.routeOf = (conn) => routeConnector(conn, app.shapeMap());

app.render = () => {
  app.layoutAttachments();
  app.renderer.render();
  updateToolbarState(app);
  updateStatus(app);
  queueSave();
};

app.updateInspector = () => updateInspector(app);
app.updateStatus = () => updateStatus(app);
app.setStatus = (t) => { $('#statusHint').textContent = t; };

app.shapeDefaults = (type) => {
  const d = SHAPES[type] || SHAPES.rect;
  return { w: d.w, h: d.h };
};

app.setTool = (tool, type) => {
  app.state.tool = tool;
  if (type) {
    app.state.pendingType = type;
    app.state.recent = [type, ...app.state.recent.filter((t) => t !== type)].slice(0, 8);
    buildRecent(app);
    document.querySelectorAll('.shape-btn').forEach((b) => {
      b.dataset.active = b.dataset.shape === type && tool === 'shape' ? '1' : '';
    });
  }
  app.state.connectSource = null;
  app.state.connectPreview = null;
  app.svg.classList.remove('mode-pan', 'mode-connect', 'mode-shape', 'mode-text');
  if (tool === 'pan') app.svg.classList.add('mode-pan');
  if (tool === 'connector') app.svg.classList.add('mode-connect');
  if (tool === 'shape') app.svg.classList.add('mode-shape');
  app.render();
};

app.setSelection = (ids) => {
  app.state.selection = new Set(ids);
  app.updateInspector();
  app.updateStatus();
};

/** Expand a selection so touching one group member selects the whole group. */
app.expandGroups = (ids) => {
  const pg = app.getPage();
  const roots = new Set();
  for (const id of ids) {
    const o = pg.shapes.find((x) => x.id === id)
      || pg.connectors.find((x) => x.id === id);
    if (o?.group?.rootId) roots.add(o.group.rootId);
  }
  const out = new Set(ids);
  if (!roots.size) return [...out];
  for (const s of pg.shapes) if (s.group && roots.has(s.group.rootId)) out.add(s.id);
  for (const c of pg.connectors) if (c.group && roots.has(c.group.rootId)) out.add(c.id);
  return [...out];
};

/**
 * Selecting a container frame (or a list shape) brings its contents along, so
 * dragging the frame moves the members. Clicking a member does not select its
 * container. Membership follows the imported Relationships graph transitively
 * (pool → lists → lanes → tasks).
 */
app.expandContainers = (ids) => {
  const pg = app.getPage();
  const containers = pg.containers || [];
  if (!containers.length) return [...ids];
  const byId = new Map(containers.map((c) => [String(c.visioId || c.modelId), c]));
  const out = new Set(ids);
  const queue = [];
  const findObj = (visioId) => pg.shapes.find((x) => x.visio && String(x.visio.id) === String(visioId))
    || pg.connectors.find((x) => x.visio && String(x.visio.id) === String(visioId))
    || pg.shapes.find((x) => x.id === String(visioId))
    || pg.connectors.find((x) => x.id === String(visioId))
    // A container/lane frame is a group; its body/title shapes carry group.id
    // but the group itself has no model shape.
    || pg.shapes.find((x) => x.group && String(x.group.id) === String(visioId));
  const addObj = (obj) => {
    if (!obj || out.has(obj.id)) return;
    out.add(obj.id);
    if (obj.group?.kind === 'container') queue.push(String(obj.group.id));
    if (obj.containerKind === 'list' || obj.containerKind === 'container') {
      queue.push(String(obj.containerId));
    }
  };
  for (const id of ids) {
    const obj = pg.shapes.find((x) => x.id === id)
      || pg.connectors.find((x) => x.id === id);
    if (!obj) continue;
    if (obj.group?.kind === 'container') queue.push(String(obj.group.id));
    if (obj.containerKind === 'list' || obj.containerKind === 'container') {
      queue.push(String(obj.containerId));
    }
  }
  const seen = new Set();
  while (queue.length) {
    const cid = String(queue.pop());
    if (seen.has(cid)) continue;
    seen.add(cid);
    const c = byId.get(cid);
    if (!c) continue;
    for (const ref of [...(c.members || []), ...(c.items || []), ...(c.related || [])]) {
      addObj(findObj(ref));
    }
    // Authored containers keep membership only on the objects, not on an
    // imported record.
    for (const o of [...pg.shapes, ...pg.connectors]) {
      if ((o.containers || []).some((r) => String(r) === cid)
          || (o.listOf || []).some((r) => String(r) === cid)) {
        addObj(o);
      }
    }
  }
  return [...out];
};

/**
 * Add/remove regular container membership. Structured list items (type 5) are
 * left alone; this edits the member side (`shape.containers`) and the writer
 * updates both Relationships cells.
 */
app.setContainerMembership = (objectIds, containerId, add) => {
  const pg = app.getPage();
  const c = (pg.containers || []).find((x) => String(x.visioId) === String(containerId)
    || String(x.modelId) === String(containerId));
  if (!c) return;
  const ref = String(c.visioId || c.modelId);
  const objects = objectIds
    .map((id) => pg.shapes.find((s) => s.id === id)
      || pg.connectors.find((x) => x.id === id))
    .filter(Boolean)
    // Do not add a container's own frame to itself.
    .filter((o) => !(o.group && String(o.group.id) === ref))
    .filter((o) => !(o.visio && String(o.visio.id) === ref))
    .filter((o) => !(o.containerId && String(o.containerId) === ref));
  if (!objects.length) return;
  app.history.push(app.doc);
  for (const o of objects) {
    const ids = new Set((o.containers || []).map(String));
    if (add) ids.add(ref);
    else ids.delete(ref);
    if (ids.size) o.containers = [...ids];
    else delete o.containers;
  }
  app.render();
  const name = c.label || `${c.kind === 'list' ? 'list' : 'container'} ${c.visioId || c.modelId}`;
  app.setStatus(`${add ? 'Added' : 'Removed'} ${objects.length} object(s) ${add ? 'to' : 'from'} ${name}`);
};

/** Wrap the current selection in an inline container (no stencil master). */
app.createContainer = () => {
  const pg = app.getPage();
  const sel = app.state.selection;
  const selected = [...pg.shapes, ...pg.connectors].filter((o) => sel.has(o.id));
  let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
  for (const s of selected) {
    if (s.x === undefined) continue;
    x1 = Math.min(x1, s.x); y1 = Math.min(y1, s.y);
    x2 = Math.max(x2, s.x + s.w); y2 = Math.max(y2, s.y + s.h);
  }
  const pad = 32;
  let x, y, w, h;
  if (Number.isFinite(x1)) {
    x = x1 - pad; y = y1 - pad;
    w = Math.max(140, x2 - x1 + pad * 2);
    h = Math.max(100, y2 - y1 + pad * 2);
  } else {
    const r = app.svg.getBoundingClientRect();
    const v = app.state.view;
    w = 320; h = 240;
    x = (r.width / 2 - v.x) / v.zoom - w / 2;
    y = (r.height / 2 - v.y) / v.zoom - h / 2;
  }
  app.history.push(app.doc);
  const c = newShape('rect', x, y, { w, h, label: 'Container' });
  Object.assign(c, {
    fill: '#ffffff', stroke: '#d5d5d5', strokeWidth: 1,
    textColor: '#6b7785', fontSize: 12, z: -1000,
    containerKind: 'container', containerId: c.id,
    containerInfo: { kind: 'container', style: 1 },
  });
  pg.shapes.push(c);
  pg.containers = pg.containers || [];
  pg.containers.push({
    visioId: null, modelId: c.id, kind: 'container',
    containerType: 0, listDirection: null, label: c.label,
    members: [], items: [], related: [],
    authored: true, orig: { members: [], items: [], related: [] },
  });
  for (const o of selected) {
    if (o.id === c.id) continue;
    o.containers = [...new Set([...(o.containers || []).map(String), String(c.id)])];
  }
  app.setSelection([c.id, ...selected.map((o) => o.id)]);
  app.render();
  app.setStatus(`Created container with ${selected.length} member(s)`);
};

// ------------------------------------------------------- CFF / swimlanes --
// A swimlane/CFF is authored entirely from inline shapes: a pool container, a
// swimlane list, one Swimeye container per lane, an optional phase list and
// phase separators. The User rows and DEPENDSON graph are the same ones Visio
// writes for the stencil masters (verified with GetContainers/GetListMembers).

const CFF_LANE_CATEGORIES = 'CFF Container;CFF List;Swimlane;Phase;NonCFF';

function makeGuid() {
  const c = globalThis.crypto;
  if (c?.randomUUID) return `{${c.randomUUID().toUpperCase()}}`;
  const hex = '0123456789ABCDEF';
  let out = '';
  for (let i = 0; i < 32; i++) out += hex[Math.floor(Math.random() * 16)];
  return `{${out.slice(0, 8)}-${out.slice(8, 12)}-${out.slice(12, 16)}-${out.slice(16, 20)}-${out.slice(20)}}`;
}

const cffRef = (c) => String(c?.modelId ?? c?.visioId ?? '');
const cffRole = (o) => o?.containerInfo?.role || o?.role || null;

function cffRec(pg, ref) {
  const key = String(ref);
  return (pg.containers || []).find((c) => cffRef(c) === key
    || String(c.visioId ?? '') === key || String(c.modelId ?? '') === key);
}

function cffShapeFor(pg, rec) {
  if (!rec) return null;
  return pg.shapes.find((s) => (rec.modelId && s.id === rec.modelId)
    || (s.visio && rec.visioId && String(s.visio.id) === String(rec.visioId))) || null;
}

function cffGroupFor(pg, rec) {
  if (!rec?.visioId) return null;
  return (pg.groups || []).find((g) => String(g.visioId) === String(rec.visioId)) || null;
}

/** Lane/separator frame in world pixels, from a model shape or group frame. */
function cffBox(pg, rec) {
  const s = cffShapeFor(pg, rec);
  if (s && s.x !== undefined) return { x: s.x, y: s.y, w: s.w, h: s.h, shape: s };
  const g = cffGroupFor(pg, rec);
  if (g?.frame) {
    const pageH = pg.visio?.pageH || 0;
    return {
      x: g.frame.x * 96,
      y: (pageH - (g.frame.y + g.frame.h)) * 96,
      w: g.frame.w * 96,
      h: g.frame.h * 96,
      group: g,
    };
  }
  return null;
}

function cffHeading(pg, rec) {
  const s = cffShapeFor(pg, rec);
  const g = cffGroupFor(pg, rec);
  return rec?.heading || rec?.label || s?.label || g?.heading || g?.label || '';
}

function cffLanes(pg, listRec) {
  return (listRec?.items || [])
    .map((ref) => cffRec(pg, ref))
    .filter((r) => r && cffRole(r) === 'lane');
}

function cffContext(pg, rec) {
  if (!rec) return null;
  const role = cffRole(rec);
  let pool = null, list = null, phase = null, lane = null;
  if (role === 'pool') {
    pool = rec;
    list = cffRec(pg, rec.listRef);
    phase = cffRec(pg, rec.phaseRef);
  } else if (role === 'list') {
    list = rec;
    pool = cffRec(pg, rec.poolRef);
    phase = pool ? cffRec(pg, pool.phaseRef) : null;
  } else if (role === 'phase-list') {
    phase = rec;
    pool = cffRec(pg, rec.poolRef);
    list = pool ? cffRec(pg, pool.listRef) : null;
  } else if (role === 'lane') {
    lane = rec;
    list = cffRec(pg, rec.listRef);
    pool = list ? cffRec(pg, list.poolRef) : cffRec(pg, rec.poolRef);
    phase = pool ? cffRec(pg, pool.phaseRef) : null;
  } else if (role === 'separator') {
    phase = cffRec(pg, rec.listRef);
    pool = phase ? cffRec(pg, phase.poolRef) : null;
    list = pool ? cffRec(pg, pool.listRef) : null;
  } else {
    return null;
  }
  return { pool, list, phaseList: phase, lane, rec };
}

/** The CFF under the current selection, or the first CFF on the page. */
app.swimlaneContext = () => {
  const pg = app.getPage();
  const sel = [...app.state.selection];
  const objs = [...pg.shapes, ...pg.connectors].filter((o) => sel.includes(o.id));
  const fromRef = (ref) => cffContext(pg, cffRec(pg, ref));

  for (const o of objs) {
    if (o.containerInfo) {
      const ref = o.visio?.id ?? o.containerId ?? o.id;
      const ctx = fromRef(ref);
      if (ctx) return ctx;
    }
  }
  for (const o of objs) {
    const gid = o.group?.id;
    if (!gid) continue;
    const g = (pg.groups || []).find((x) => String(x.visioId) === String(gid)
      || String(x.id) === String(gid));
    if (g?.role) {
      const ctx = fromRef(g.visioId);
      if (ctx) return ctx;
    }
  }
  // Prefer a lane over a list/pool when a task is selected.
  const refs = objs.flatMap((o) => [...(o.containers || []), ...(o.listOf || [])]);
  const ranked = [...refs].sort((a, b) => {
    const ra = cffRole(cffRec(pg, a)), rb = cffRole(cffRec(pg, b));
    const rank = (r) => (r === 'lane' ? 0 : r === 'list' ? 1 : r === 'pool' ? 2 : 3);
    return rank(ra) - rank(rb);
  });
  for (const ref of ranked) {
    const ctx = fromRef(ref);
    if (ctx) return ctx;
  }
  // A single swimlane on the page is an obvious target for an unassigned
  // shape; with several CFFs, require selecting a lane/task first.
  const pools = (pg.containers || []).filter((c) => cffRole(c) === 'pool');
  return pools.length === 1 ? cffContext(pg, pools[0]) : null;
};

/** True when every lane is an inline shape the editor can insert/remove. */
app.canAuthorLanes = (ctx = app.swimlaneContext()) => {
  if (!ctx?.list) return false;
  const lanes = cffLanes(app.getPage(), ctx.list);
  return lanes.length > 0 && lanes.every((r) => !!cffShapeFor(app.getPage(), r));
};

/** True for a CFF pool/list/lane/separator frame (not a regular shape). */
app.isCffFrame = (o) => isCffFrame(app.getPage(), o);

/** Lanes of the current CFF, for inspector and context-menu pickers. */
app.swimlaneLanes = () => {
  const ctx = app.swimlaneContext();
  if (!ctx?.list) return [];
  const pg = app.getPage();
  return cffLanes(pg, ctx.list).map((r) => ({
    ref: cffRef(r), heading: cffHeading(pg, r) || 'Lane',
  }));
};

// ------------------------------------------------------------ themes ----
app.themePresets = THEME_PRESETS;
app.ensureTheme = () => ensureTheme(app.doc);

function themeTargets(scope) {
  const pg = app.getPage();
  const pick = (p) => [...p.shapes, ...p.connectors]
    .filter((o) => !isCffFrame(p, o));
  if (scope === 'document') return app.doc.pages.flatMap(pick);
  if (scope === 'page' || !app.state.selection.size) return pick(pg);
  return pick(pg).filter((o) => app.state.selection.has(o.id));
}

app.applyQuickStyle = (style = 'moderate', scope = app.state.themeScope || 'selection', live = false) => {
  const theme = ensureTheme(app.doc);
  const variation = theme.variations[theme.activeVariation] || {};
  const accent = variation.variant1 || theme.slots.accent1 || '#4472c4';
  const accent2 = variation.variant2 || theme.slots.accent2 || accent;
  const dk1 = theme.slots.dk1 || '#000000';
  const lt1 = theme.slots.lt1 || '#ffffff';
  const presets = {
    subtle: { fill: themeTint(accent, 85), stroke: accent, text: dk1 },
    moderate: { fill: themeTint(accent, 55), stroke: themeTint(accent, -25), text: dk1 },
    intense: { fill: accent, stroke: themeTint(accent, -35), text: lt1 },
    outline: { fill: lt1, stroke: accent, text: accent2 },
    plain: { fill: lt1, stroke: dk1, text: dk1 },
  };
  const st = presets[style] || presets.moderate;
  const targets = themeTargets(scope);
  if (!live) app.history.push(app.doc);
  for (const o of targets) {
    if (o.w !== undefined) {
      const parts = o.parts || [];
      if (parts.length > 1) {
        // Multi-part imported art (3-D blocks/icons) keeps its per-part fills;
        // recolouring it would need source sub-shape edits.
        o.textColor = st.text;
        continue;
      }
      o.fill = st.fill;
      o.stroke = st.stroke;
      o.textColor = st.text;
      if (parts.length === 1) {
        parts[0].fill = st.fill;
        parts[0].stroke = st.stroke;
        delete parts[0].fillGradient;
      }
    } else {
      o.stroke = accent;
    }
  }
  app.state.quickStyle = style;
  app.state.themeScope = scope;
  app.render();
  app.updateInspector();
  app.setStatus(`Applied ${style} quick style to ${targets.length} object(s)`);
};

app.setThemeVariation = (index, scope = app.state.themeScope || 'selection') => {
  const theme = ensureTheme(app.doc);
  const i = Math.max(0, Math.min(theme.variations.length - 1, Number(index) || 0));
  theme.activeVariation = i;
  for (const key of Object.keys(theme.variations[i] || {})) {
    theme.slots[key] = theme.variations[i][key];
  }
  app.applyQuickStyle(app.state.quickStyle || 'moderate', scope);
};

app.applyThemePreset = (name, scope = app.state.themeScope || 'selection') => {
  const preset = THEME_PRESETS[name];
  if (!preset) return;
  app.history.push(app.doc);
  app.doc.theme = structuredClone(preset);
  app.applyQuickStyle(app.state.quickStyle || 'moderate', scope, true);
  app.setStatus(`Theme: ${name}`);
};

// ---------------------------------------------------- rich text helpers ---
const richStyleOf = (s) => ({
  fontFamily: s.fontFamily,
  fontSize: s.fontSize || 12,
  bold: !!s.bold,
  italic: !!s.italic,
  underline: !!s.underline,
  textColor: s.textColor,
});

app.ensureRich = (shape) => {
  if (shape.rich?.paragraphs?.length) return shape.rich;
  shape.rich = {
    paragraphs: [{
      align: shape.align || 'center', bullet: false, bulletStr: '•', indent: 0,
      runs: shape.label ? [{ text: shape.label, style: richStyleOf(shape) }] : [],
    }],
  };
  return shape.rich;
};

app.richPlain = (rich) => (rich?.paragraphs || [])
  .map((p) => (p.runs || []).map((r) => (r.field ? (r.field.value ?? r.text ?? '') : (r.text || ''))).join(''))
  .join('\n');

app.syncRichLabel = (shape) => {
  if (!shape.rich) return;
  shape.label = app.richPlain(shape.rich);
};

app.fieldValueFor = (shape, formula) => {
  const f = String(formula || '');
  const prop = /(?:Sheet\.[A-Za-z0-9_]+!)?Prop\.([A-Za-z0-9_]+)/i.exec(f);
  if (prop) {
    const row = (shape.data || []).find((d) => String(d.name) === String(prop[1]));
    return row ? String(row.value ?? '') : '';
  }
  if (/PAGENUMBER\s*\(/i.test(f)) return String(app.doc.pageIndex + 1);
  if (/PAGECOUNT\s*\(/i.test(f)) return String(app.doc.pages.length);
  if (/FILENAME\s*\(/i.test(f)) return String(app.doc.name || '');
  if (/NOW\s*\(/i.test(f)) {
    const d = new Date();
    const pad = (n) => String(n).padStart(2, '0');
    const m = /FORMAT\s*\(\s*NOW\s*\(\s*\)\s*,\s*["']([^"']+)["']/i.exec(f);
    const pattern = m ? m[1] : 'yyyy-mm-dd';
    const hasTime = /HH|hh|ss/.test(pattern);
    return pattern.replace(/yyyy|MM|mm|dd|HH|hh|ss/g, (t) => ({
      yyyy: String(d.getFullYear()),
      MM: pad(d.getMonth() + 1),
      mm: pad(hasTime ? d.getMinutes() : d.getMonth() + 1),
      dd: pad(d.getDate()),
      HH: pad(d.getHours()), hh: pad(d.getHours()),
      ss: pad(d.getSeconds()),
    }[t] || t));
  }
  return '';
};

app.insertField = (shape, formula) => {
  const rich = app.ensureRich(shape);
  const para = rich.paragraphs[rich.paragraphs.length - 1];
  const value = app.fieldValueFor(shape, formula);
  para.runs.push({
    field: { formula, value, format: 'FIELDPICTURE(0)' },
    text: value,
    style: richStyleOf(shape),
  });
  app.syncRichLabel(shape);
};

function cffRecord(shape, info, pg) {
  const rec = {
    visioId: null, modelId: shape.id, kind: info.kind, role: info.role,
    containerType: info.kind === 'list' ? 1 : 0,
    listDirection: info.listDirection ?? null,
    direction: info.direction || 'horizontal',
    numLanes: info.numLanes ?? null,
    listGuid: info.listGuid || null,
    heading: info.heading ?? shape.label ?? '',
    label: info.heading ?? shape.label ?? '',
    listRef: info.listId || info.phaseId || null,
    poolRef: info.poolId || null,
    members: [], items: [], related: [],
    authored: true,
    orig: { members: [], items: [], related: [] },
  };
  pg.containers = pg.containers || [];
  pg.containers.push(rec);
  return rec;
}

function cffShape(pg, { role, kind, x, y, w, h, label = '', info = {}, z = 0, style = {} }) {
  const s = newShape('rect', x, y, { w, h, label });
  Object.assign(s, {
    z,
    fill: '#ffffff', stroke: '#b8c2cc', strokeWidth: 1, strokeStyle: 'solid',
    align: 'left', vAlign: 'top',
    containerKind: kind, containerId: null,
    containerInfo: {
      kind, role, heading: label, direction: info.direction,
      listDirection: info.listDirection, listGuid: info.listGuid,
      numLanes: info.numLanes, listId: info.listId, poolId: info.poolId,
      phaseId: info.phaseId,
    },
    ...style,
  });
  s.containerId = s.id;
  pg.shapes.push(s);
  const rec = cffRecord(s, {
    kind, role, heading: label, ...info,
  }, pg);
  return { shape: s, rec };
}

/** Keep the phase list and its separators spanning the current lane area. */
function cffSyncPhaseList(pg, ctx) {
  if (!ctx?.list || !ctx.phaseList) return;
  const listShape = cffShapeFor(pg, ctx.list);
  const phaseShape = cffShapeFor(pg, ctx.phaseList);
  if (!listShape || !phaseShape) return;
  const vertical = (ctx.list.direction || 'horizontal') === 'vertical';
  phaseShape.x = listShape.x;
  phaseShape.y = listShape.y;
  phaseShape.w = listShape.w;
  phaseShape.h = listShape.h;
  for (const ref of ctx.phaseList.items || []) {
    const rec = cffRec(pg, ref);
    const s = cffShapeFor(pg, rec);
    if (!s) continue;
    if (vertical) {
      s.x = listShape.x;
      s.w = listShape.w;
    } else {
      s.y = listShape.y;
      s.h = listShape.h;
    }
  }
}

/** Renumber lanes that still have their default "Lane N" heading. */
function cffRenumberLanes(pg, listRec) {
  const lanes = cffLanes(pg, listRec);
  lanes.forEach((rec, i) => {
    const heading = cffHeading(pg, rec) || '';
    if (!/^Lane \d+$/.test(heading)) return;
    const name = `Lane ${i + 1}`;
    if (name === heading) return;
    rec.heading = name;
    rec.label = name;
    const s = cffShapeFor(pg, rec);
    if (s) { s.label = name; s.containerInfo.heading = name; }
  });
}

/** Put moved tasks inside a lane without stacking them on existing tasks. */
function cffPlaceInLane(pg, objects, laneRec, listRec) {
  const box = cffBox(pg, laneRec);
  if (!box) return;
  const vertical = (listRec?.direction || 'horizontal') === 'vertical';
  const laneRef = cffRef(laneRec);
  const moving = new Set(objects.map((o) => o.id));
  const placed = [...pg.shapes].filter((s) => !moving.has(s.id)
    && (s.containers || []).some((r) => String(r) === laneRef));
  const overlap = (a, b) => a.x < b.x + b.w + 8 && a.x + a.w + 8 > b.x
    && a.y < b.y + b.h + 8 && a.y + a.h + 8 > b.y;

  for (const o of objects) {
    if (o.x === undefined) continue;
    if (vertical) {
      o.x = box.x + (box.w - o.w) / 2;
      let guard = 0;
      while (placed.some((p) => overlap(o, p)) && o.y + o.h + 12 < box.y + box.h && guard++ < 50) {
        o.y += o.h + 12;
      }
    } else {
      o.y = box.y + (box.h - o.h) / 2;
      let guard = 0;
      while (placed.some((p) => overlap(o, p)) && o.x + o.w + 12 < box.x + box.w && guard++ < 50) {
        o.x += o.w + 12;
      }
    }
    placed.push(o);
  }
}

/** Move a lane's member objects with the lane when lanes are inserted/removed. */
function shiftLaneContent(pg, laneRec, dx, dy) {
  const ref = cffRef(laneRec);
  for (const o of [...pg.shapes, ...pg.connectors]) {
    if (!(o.containers || []).some((r) => String(r) === ref)) continue;
    if (o.x !== undefined) { o.x += dx; o.y += dy; }
    if (o.from && !o.from.shapeId && o.from.point) {
      o.from.point.x += dx; o.from.point.y += dy;
    }
    if (o.to && !o.to.shapeId && o.to.point) {
      o.to.point.x += dx; o.to.point.y += dy;
    }
    if (o.waypoints) o.waypoints = o.waypoints.map((p) => ({ x: p.x + dx, y: p.y + dy }));
  }
}

/** Rebuild record member/item lists from the object membership.
 *  Group-only refs (imported lanes/separators) survive; model objects update. */
function cffRefresh(pg) {
  const objects = [...pg.shapes, ...pg.connectors];
  for (const rec of pg.containers || []) {
    const ref = cffRef(rec);
    const authored = !!rec.authored;
    const idOf = (o) => String(authored ? o.id : (o.visio?.id ?? o.id));
    const hasRef = (o, field) => (o[field] || []).some((r) => String(r) === ref);
    const existingMember = new Set((rec.members || []).map(String));
    const memberIds = objects.filter((o) => hasRef(o, 'containers')).map(idOf);
    const keepMembers = (rec.members || []).filter((r) => {
      const obj = objects.find((o) => idOf(o) === String(r));
      return obj ? hasRef(obj, 'containers') : true;
    });
    rec.members = [...new Set([
      ...keepMembers.map(String),
      ...memberIds.filter((r) => !existingMember.has(String(r))),
    ])];

    const itemIds = objects.filter((o) => hasRef(o, 'listOf')).map(idOf);
    const keepItems = (rec.items || []).filter((r) => {
      const obj = objects.find((o) => idOf(o) === String(r));
      return obj ? hasRef(obj, 'listOf') : true;
    });
    const added = itemIds.filter((r) => !keepItems.map(String).includes(String(r)));
    // New lanes/separators are appended in visual order.
    added.sort((a, b) => {
      const ba = cffBox(pg, cffRec(pg, a));
      const bb = cffBox(pg, cffRec(pg, b));
      if (!ba || !bb) return 0;
      return (ba.y - bb.y) || (ba.x - bb.x);
    });
    rec.items = [...keepItems.map(String), ...added];
  }
  // Keep the lane count and per-lane CFF settings in sync.
  for (const pool of (pg.containers || []).filter((c) => cffRole(c) === 'pool')) {
    const list = cffRec(pg, pool.listRef);
    if (!list) continue;
    const n = (list.items || []).length;
    pool.numLanes = n;
    const ps = cffShapeFor(pg, pool);
    if (ps) ps.containerInfo.numLanes = n;
    for (const lane of cffLanes(pg, list)) {
      lane.numLanes = n;
      const ls = cffShapeFor(pg, lane);
      if (ls) ls.containerInfo.numLanes = n;
    }
  }
}

function cffAddMembership(pg, objects, refs) {
  for (const o of objects) {
    const ids = new Set((o.containers || []).map(String));
    for (const r of refs) ids.add(String(r));
    o.containers = [...ids];
  }
}

function cffRemoveMembership(pg, objects, refs, listRef = null) {
  const drop = new Set(refs.map(String));
  for (const o of objects) {
    let ids = (o.containers || []).map(String);
    if (listRef != null) {
      // A task is in one lane per CFF: drop any lane in this list.
      for (const r of ids) {
        const rec = cffRec(pg, r);
        if (cffRole(rec) === 'lane' && String(rec.listRef) === String(listRef)) drop.add(r);
      }
    }
    ids = ids.filter((r) => !drop.has(r));
    if (ids.length) o.containers = ids;
    else delete o.containers;
  }
}

function selectedObjects(app) {
  const pg = app.getPage();
  const sel = app.state.selection;
  return [...pg.shapes, ...pg.connectors].filter((o) => sel.has(o.id));
}

/** A shape that is part of a CFF frame, not a user task. */
function isCffFrame(pg, o) {
  if (o.containerInfo) return true;
  if (!o.group) return false;
  const g = (pg.groups || []).find((x) => String(x.visioId) === String(o.group.id)
    || String(x.id) === String(o.group.id));
  return !!g?.role && g.role !== 'group';
}

app.createSwimlane = (opts = {}) => {
  const pg = app.getPage();
  const direction = opts.direction === 'vertical' ? 'vertical' : 'horizontal';
  const laneCount = Math.max(1, Math.min(12, Math.round(opts.lanes ?? 2)));
  const selected = selectedObjects(app).filter((o) => o.x !== undefined && !isCffFrame(pg, o));
  const vertical = direction === 'vertical';
  const titleH = 40;
  const laneH = 110;
  const laneW = 180;

  let x, y, w, h;
  if (selected.length) {
    let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
    for (const s of selected) {
      x1 = Math.min(x1, s.x); y1 = Math.min(y1, s.y);
      x2 = Math.max(x2, s.x + s.w); y2 = Math.max(y2, s.y + s.h);
    }
    const pad = 36;
    if (vertical) {
      const areaW = Math.max(laneCount * laneW, x2 - x1 + pad * 2);
      x = x1 - pad;
      y = y1 - pad - titleH;
      w = areaW;
      h = Math.max(240, y2 - y1 + pad * 2 + titleH);
    } else {
      const areaH = Math.max(laneCount * laneH, y2 - y1 + pad * 2);
      x = x1 - pad;
      y = y1 - pad - titleH;
      w = Math.max(360, x2 - x1 + pad * 2);
      h = areaH + titleH;
    }
  } else {
    const r = app.svg.getBoundingClientRect();
    const v = app.state.view;
    if (vertical) {
      w = laneCount * laneW;
      h = 440;
    } else {
      w = 620;
      h = titleH + laneCount * laneH;
    }
    x = (r.width / 2 - v.x) / v.zoom - w / 2;
    y = (r.height / 2 - v.y) / v.zoom - h / 2;
  }

  app.history.push(app.doc);
  const listGuid = makeGuid();
  const listDirection = vertical ? 0 : 2;
  const phaseDirection = vertical ? 2 : 0;
  const laneArea = vertical
    ? { x, y: y + titleH, w, h: h - titleH }
    : { x, y: y + titleH, w, h: h - titleH };

  const pool = cffShape(pg, {
    role: 'pool', kind: 'container', x, y, w, h,
    label: 'Cross-Functional Flowchart',
    z: -1000,
    style: {
      fill: '#ffffff', stroke: '#b8c2cc', strokeWidth: 1,
      textBox: { x: 10, y: 8, w: w - 20, h: 24 },
    },
    info: { direction, numLanes: laneCount },
  });
  const list = cffShape(pg, {
    role: 'list', kind: 'list',
    x: laneArea.x, y: laneArea.y, w: laneArea.w, h: laneArea.h,
    z: -960,
    style: { fill: 'none', stroke: 'none', strokeWidth: 0 },
    info: {
      direction, listDirection, numLanes: laneCount, listGuid,
      poolId: pool.shape.id,
    },
  });
  const phase = cffShape(pg, {
    role: 'phase-list', kind: 'list',
    x: laneArea.x, y: laneArea.y, w: laneArea.w, h: laneArea.h,
    z: -950,
    style: { fill: 'none', stroke: 'none', strokeWidth: 0 },
    info: {
      direction, listDirection: phaseDirection, numLanes: laneCount,
      listGuid, poolId: pool.shape.id,
    },
  });

  pool.rec.listRef = list.shape.id;
  pool.rec.phaseRef = phase.shape.id;
  pool.shape.containerInfo.listId = list.shape.id;
  pool.shape.containerInfo.phaseId = phase.shape.id;
  list.rec.poolRef = pool.shape.id;
  phase.rec.poolRef = pool.shape.id;
  list.rec.related = [phase.shape.id];
  phase.rec.related = [list.shape.id];
  list.rec.orig.related = [phase.shape.id];
  phase.rec.orig.related = [list.shape.id];
  cffAddMembership(pg, [list.shape, phase.shape], [pool.shape.id]);

  const lanes = [];
  for (let i = 0; i < laneCount; i++) {
    const lx = vertical ? laneArea.x + i * (laneArea.w / laneCount) : laneArea.x;
    const ly = vertical ? laneArea.y : laneArea.y + i * (laneArea.h / laneCount);
    const lw = vertical ? laneArea.w / laneCount : laneArea.w;
    const lh = vertical ? laneArea.h : laneArea.h / laneCount;
    const lane = cffShape(pg, {
      role: 'lane', kind: 'container',
      x: lx, y: ly, w: lw, h: lh,
      label: `Lane ${i + 1}`,
      z: -900 + i,
      style: {
        fill: i % 2 ? '#f4f7fa' : '#ffffff',
        stroke: '#c9d2dd', strokeWidth: 1,
        textBox: vertical
          ? { x: 8, y: 8, w: lw - 16, h: 24 }
          : { x: 10, y: 8, w: lw - 20, h: 24 },
        align: vertical ? 'center' : 'left',
      },
      info: {
        direction, listGuid, numLanes: laneCount,
        listId: list.shape.id, poolId: pool.shape.id,
      },
    });
    lane.shape.listOf = [list.shape.id];
    lanes.push(lane);
    list.rec.items.push(lane.shape.id);
  }

  // Drop the selection into the lane that contains its centre.
  for (const s of selected) {
    const cx = s.x + s.w / 2, cy = s.y + s.h / 2;
    let idx = 0;
    if (vertical) {
      idx = Math.max(0, Math.min(laneCount - 1, Math.floor((cx - laneArea.x) / (laneArea.w / laneCount))));
    } else {
      idx = Math.max(0, Math.min(laneCount - 1, Math.floor((cy - laneArea.y) / (laneArea.h / laneCount))));
    }
    cffAddMembership(pg, [s], [list.shape.id, lanes[idx].shape.id]);
  }

  cffRefresh(pg);
  app.setSelection([pool.shape.id]);
  app.render();
  app.setStatus(`Created ${vertical ? 'vertical' : 'horizontal'} swimlane with ${laneCount} lane(s)`);
};

app.addLane = (where = 'below') => {
  const ctx = app.swimlaneContext();
  if (!ctx?.list) { app.setStatus('Select a swimlane or lane first'); return; }
  if (!app.canAuthorLanes(ctx)) {
    app.setStatus('Adding lanes is supported on swimlanes created by LayoutCraft');
    return;
  }
  const pg = app.getPage();
  const list = ctx.list;
  const listShape = cffShapeFor(pg, list);
  const vertical = (list.direction || 'horizontal') === 'vertical';
  const lanes = cffLanes(pg, list);
  const laneBoxes = lanes.map((r) => cffBox(pg, r)).filter(Boolean);
  if (!listShape || !laneBoxes.length) { app.setStatus('This list has no lane geometry'); return; }
  const laneH = laneBoxes[0].h;
  const laneW = laneBoxes[0].w;
  const listGuid = list.listGuid
    || lanes.map((r) => r.listGuid).find(Boolean)
    || makeGuid();
  list.listGuid = listGuid;
  for (const r of lanes) r.listGuid = r.listGuid || listGuid;
  let index = ctx.lane
    ? lanes.findIndex((r) => cffRef(r) === cffRef(ctx.lane))
    : lanes.length - 1;
  if (index < 0) index = lanes.length - 1;
  const insertAt = ctx.lane && where === 'above' ? index : index + 1;

  app.history.push(app.doc);
  const laneCount = lanes.length + 1;
  let lx, ly, lw, lh;
  if (vertical) {
    lx = listShape.x + insertAt * laneW;
    ly = listShape.y;
    lw = laneW;
    lh = listShape.h;
    for (const r of lanes) {
      const s = cffShapeFor(pg, r);
      const b = cffBox(pg, r);
      if (s && b.x >= lx - 0.5) {
        s.x += laneW;
        shiftLaneContent(pg, r, laneW, 0);
      }
    }
    listShape.w += laneW;
    if (ctx.pool) { const ps = cffShapeFor(pg, ctx.pool); if (ps) ps.w += laneW; }
  } else {
    lx = listShape.x;
    ly = listShape.y + insertAt * laneH;
    lw = listShape.w;
    lh = laneH;
    for (const r of lanes) {
      const s = cffShapeFor(pg, r);
      const b = cffBox(pg, r);
      if (s && b.y >= ly - 0.5) {
        s.y += laneH;
        shiftLaneContent(pg, r, 0, laneH);
      }
    }
    listShape.h += laneH;
    if (ctx.pool) { const ps = cffShapeFor(pg, ctx.pool); if (ps) ps.h += laneH; }
  }

  cffSyncPhaseList(pg, ctx);
  const lane = cffShape(pg, {
    role: 'lane', kind: 'container', x: lx, y: ly, w: lw, h: lh,
    label: `Lane ${laneCount}`,
    z: -900 + insertAt,
    style: {
      fill: laneCount % 2 ? '#f4f7fa' : '#ffffff',
      stroke: '#c9d2dd', strokeWidth: 1,
      textBox: vertical
        ? { x: 8, y: 8, w: lw - 16, h: 24 }
        : { x: 10, y: 8, w: lw - 20, h: 24 },
      align: vertical ? 'center' : 'left',
    },
    info: {
      direction: vertical ? 'vertical' : 'horizontal',
      listGuid, numLanes: laneCount,
      listId: cffRef(list), poolId: ctx.pool ? cffRef(ctx.pool) : null,
    },
  });
  lane.shape.listOf = [cffRef(list)];
  list.items.splice(insertAt, 0, lane.shape.id);
  cffRefresh(pg);
  cffRenumberLanes(pg, list);
  app.setSelection([lane.shape.id]);
  app.render();
  app.setStatus(`Added lane ${laneCount}`);
};

app.removeLane = () => {
  const ctx = app.swimlaneContext();
  if (!ctx?.lane || !ctx.list) { app.setStatus('Select a lane first'); return; }
  if (!app.canAuthorLanes(ctx)) {
    app.setStatus('Removing imported lanes is not supported');
    return;
  }
  const pg = app.getPage();
  const lane = ctx.lane;
  const laneShape = cffShapeFor(pg, lane);
  const list = ctx.list;
  const lanes = cffLanes(pg, list);
  if (lanes.length <= 1) { app.setStatus('A swimlane needs at least one lane'); return; }
  const tasks = [...pg.shapes, ...pg.connectors]
    .filter((o) => (o.containers || []).some((r) => String(r) === cffRef(lane)));
  if (tasks.length && !window.confirm(`Move ${tasks.length} task(s) to the neighbouring lane and remove this lane?`)) return;
  app.history.push(app.doc);

  if (tasks.length) {
    const idx = lanes.findIndex((r) => cffRef(r) === cffRef(lane));
    const target = lanes[idx > 0 ? idx - 1 : idx + 1];
    for (const o of tasks) {
      cffRemoveMembership(pg, [o], [cffRef(lane)], cffRef(list));
      cffAddMembership(pg, [o], [cffRef(target), cffRef(list)]);
    }
    cffPlaceInLane(pg, tasks, target, list);
  }

  const box = cffBox(pg, lane);
  const vertical = (list.direction || 'horizontal') === 'vertical';
  for (const r of lanes) {
    if (cffRef(r) === cffRef(lane)) continue;
    const s = cffShapeFor(pg, r);
    const b = cffBox(pg, r);
    if (!s || !box) continue;
    if (vertical && b.x > box.x) {
      s.x -= box.w;
      shiftLaneContent(pg, r, -box.w, 0);
    }
    if (!vertical && b.y > box.y) {
      s.y -= box.h;
      shiftLaneContent(pg, r, 0, -box.h);
    }
  }
  const listShape = cffShapeFor(pg, list);
  if (listShape) {
    if (vertical) listShape.w = Math.max(40, listShape.w - box.w);
    else listShape.h = Math.max(40, listShape.h - box.h);
  }
  if (ctx.pool) {
    const ps = cffShapeFor(pg, ctx.pool);
    if (ps) { if (vertical) ps.w = Math.max(40, ps.w - box.w); else ps.h = Math.max(40, ps.h - box.h); }
  }
  cffSyncPhaseList(pg, ctx);
  for (const o of [...pg.shapes, ...pg.connectors]) {
    if ((o.containers || []).some((r) => String(r) === cffRef(lane))) {
      cffRemoveMembership(pg, [o], [cffRef(lane)]);
    }
  }
  pg.shapes = pg.shapes.filter((s) => s.id !== laneShape?.id);
  pg.containers = (pg.containers || []).filter((c) => c !== lane);
  list.items = (list.items || []).filter((r) => String(r) !== cffRef(lane));
  cffRefresh(pg);
  cffRenumberLanes(pg, list);
  app.setSelection([]);
  app.render();
  app.setStatus('Removed lane');
};

app.moveSelectionToLane = (laneRef) => {
  const pg = app.getPage();
  const lane = cffRec(pg, laneRef);
  const list = lane ? cffRec(pg, lane.listRef) : null;
  if (!lane || !list) { app.setStatus('Lane not found'); return; }
  const objects = selectedObjects(app).filter((o) => !isCffFrame(pg, o));
  if (!objects.length) { app.setStatus('Select tasks to move'); return; }
  app.history.push(app.doc);
  for (const o of objects) {
    cffRemoveMembership(pg, [o], [], cffRef(list));
    cffAddMembership(pg, [o], [cffRef(list), cffRef(lane)]);
  }
  cffPlaceInLane(pg, objects, lane, list);
  cffRefresh(pg);
  app.render();
  app.setStatus(`Moved ${objects.length} object(s) to ${cffHeading(pg, lane) || 'lane'}`);
};

app.setLaneHeading = (text, live = false) => {
  const ctx = app.swimlaneContext();
  if (!ctx?.lane) return;
  const pg = app.getPage();
  const lane = ctx.lane;
  const prev = lane.heading || lane.label || '';
  if (!live) app.history.push(app.doc);
  lane.heading = text;
  lane.label = text;
  const s = cffShapeFor(pg, lane);
  if (s) { s.label = text; s.containerInfo.heading = text; }
  const g = cffGroupFor(pg, lane);
  if (g) {
    g.heading = text;
    // Imported CFF lanes draw their heading from a child shape, not only the
    // User.visHeadingText row; keep that child in sync when it is the old
    // heading.
    for (const child of pg.shapes) {
      if (!child.group || String(child.group.id) !== String(g.visioId)) continue;
      if (prev && child.label === prev) child.label = text;
    }
  }
  app.render();
};

app.addPhase = () => {
  const ctx = app.swimlaneContext();
  const pg = app.getPage();
  if (!ctx?.list || !ctx.pool) { app.setStatus('Select a swimlane first'); return; }
  const listShape = cffShapeFor(pg, ctx.list);
  if (!listShape) { app.setStatus('This swimlane has no editable list frame'); return; }
  const vertical = (ctx.list.direction || 'horizontal') === 'vertical';
  app.history.push(app.doc);

  let phase = ctx.phaseList;
  if (!phase) {
    const made = cffShape(pg, {
      role: 'phase-list', kind: 'list',
      x: listShape.x, y: listShape.y, w: listShape.w, h: listShape.h,
      z: -955,
      style: { fill: 'none', stroke: 'none', strokeWidth: 0 },
      info: {
        direction: vertical ? 'vertical' : 'horizontal',
        listDirection: vertical ? 2 : 0,
        poolId: cffRef(ctx.pool), listGuid: ctx.list.listGuid,
      },
    });
    phase = made.rec;
    phase.related = [cffRef(ctx.list)];
    phase.orig.related = [cffRef(ctx.list)];
    cffAddMembership(pg, [made.shape], [cffRef(ctx.pool)]);
    ctx.pool.phaseRef = cffRef(phase);
    const ps = cffShapeFor(pg, ctx.pool);
    if (ps) ps.containerInfo.phaseId = cffRef(phase);
    if (!(ctx.list.related || []).includes(cffRef(phase))) {
      ctx.list.related = [...(ctx.list.related || []), cffRef(phase)];
      ctx.list.orig.related = [...(ctx.list.orig.related || []), cffRef(phase)];
    }
  }
  const existing = (phase.items || []).map((r) => cffRec(pg, r)).filter(Boolean);
  cffSyncPhaseList(pg, { ...ctx, phaseList: phase });
  const n = existing.length + 1;
  let sx, sy, sw, sh;
  if (vertical) {
    sx = listShape.x;
    sy = listShape.y + listShape.h * (n / (n + 1));
    sw = listShape.w;
    sh = 2;
  } else {
    sx = listShape.x + listShape.w * (n / (n + 1));
    sy = listShape.y;
    sw = 2;
    sh = listShape.h;
  }
  const sep = cffShape(pg, {
    role: 'separator', kind: 'container', x: sx, y: sy, w: sw, h: sh,
    label: `Phase ${n}`,
    z: -880 + n,
    style: {
      fill: 'none', stroke: '#7f7f7f', strokeWidth: 1, strokeStyle: 'dashed',
      textBox: vertical
        ? { x: 4, y: -26, w: 160, h: 22 }
        : { x: -70, y: 4, w: 150, h: 22 },
      align: vertical ? 'left' : 'center',
    },
    info: {
      direction: vertical ? 'vertical' : 'horizontal',
      listDirection: vertical ? 2 : 0,
      phaseId: cffRef(phase), poolId: cffRef(ctx.pool),
      listId: cffRef(phase),
    },
  });
  sep.shape.listOf = [cffRef(phase)];
  phase.items = [...(phase.items || []), sep.shape.id];
  cffRefresh(pg);
  app.setSelection([sep.shape.id]);
  app.render();
  app.setStatus(`Added phase ${n}`);
};

app.removePhase = () => {
  const ctx = app.swimlaneContext();
  const pg = app.getPage();
  let sep = null;
  if (ctx?.rec && cffRole(ctx.rec) === 'separator') sep = ctx.rec;
  else if (ctx?.lane) {
    const laneBox = cffBox(pg, ctx.lane);
    const phaseList = ctx.phaseList;
    if (phaseList) {
      const seps = (phaseList.items || []).map((r) => cffRec(pg, r)).filter(Boolean);
      sep = seps.find((r) => {
        const b = cffBox(pg, r);
        return b && laneBox && Math.abs((b.x + b.w / 2) - (laneBox.x + laneBox.w / 2)) < laneBox.w / 2;
      }) || null;
    }
  }
  if (!sep) { app.setStatus('Select a phase separator first'); return; }
  const shape = cffShapeFor(pg, sep);
  if (!shape) { app.setStatus('Removing imported phase separators is not supported'); return; }
  app.history.push(app.doc);
  for (const o of [...pg.shapes, ...pg.connectors]) {
    if ((o.containers || []).some((r) => String(r) === cffRef(sep))) {
      cffRemoveMembership(pg, [o], [cffRef(sep)]);
    }
  }
  const phase = cffRec(pg, sep.listRef);
  if (phase) phase.items = (phase.items || []).filter((r) => String(r) !== cffRef(sep));
  pg.shapes = pg.shapes.filter((s) => s.id !== shape.id);
  pg.containers = (pg.containers || []).filter((c) => c !== sep);
  cffRefresh(pg);
  app.setSelection([]);
  app.render();
  app.setStatus('Removed phase');
};

app.removeSwimlane = () => {
  const ctx = app.swimlaneContext();
  if (!ctx?.pool) { app.setStatus('Select a swimlane first'); return; }
  const pg = app.getPage();
  const phaseItems = (ctx.phaseList?.items || []).map((r) => cffRec(pg, r));
  const recs = [...new Set([
    ctx.pool, ctx.list, ctx.phaseList,
    ...cffLanes(pg, ctx.list),
    ...phaseItems,
  ].filter(Boolean))];
  if (!recs.length) return;
  if (!recs.every((r) => cffShapeFor(pg, r))) {
    app.setStatus('Removing imported swimlanes is not supported');
    return;
  }
  if (!window.confirm('Remove this swimlane and its lanes? Tasks and their shapes are kept.')) return;
  app.history.push(app.doc);
  const shapeIds = new Set(recs.map((r) => cffShapeFor(pg, r).id));
  const refs = new Set([...recs.map(cffRef), ...shapeIds]);
  for (const o of [...pg.shapes, ...pg.connectors]) {
    const c = (o.containers || []).filter((r) => !refs.has(String(r)));
    const l = (o.listOf || []).filter((r) => !refs.has(String(r)));
    if (c.length) o.containers = c; else delete o.containers;
    if (l.length) o.listOf = l; else delete o.listOf;
  }
  pg.shapes = pg.shapes.filter((s) => !shapeIds.has(s.id));
  pg.containers = (pg.containers || []).filter((c) => !refs.has(cffRef(c)));
  cffRefresh(pg);
  app.setSelection([]);
  app.render();
  app.setStatus('Removed swimlane');
};

app.groupSelection = () => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const members = [    ...pg.shapes.filter((s) => sel.has(s.id)),
    ...pg.connectors.filter((c) => sel.has(c.id)),
  ];
  if (members.length < 2) {
    app.setStatus('Select at least two objects to group');
    return;
  }
  app.history.push(app.doc);
  const gid = uid('g');
  for (const o of members) {
    o.group = {
      id: gid, rootId: gid, parentId: gid, depth: 1,
      rotated: false, kind: 'group', authored: true,
    };
  }
  app.render();
  const imported = members.some((o) => o.visio);
  app.setStatus(imported
    ? `Grouped ${members.length} objects (editor grouping; not written as a Visio group yet)`
    : `Grouped ${members.length} objects`);
};

app.ungroupSelection = () => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const all = [...pg.shapes, ...pg.connectors];
  const roots = new Set();
  for (const o of all) if (sel.has(o.id) && o.group) roots.add(o.group.rootId);
  if (!roots.size) {
    app.setStatus('Selection is not grouped');
    return;
  }
  const imported = all.some((o) => roots.has(o.group?.rootId) && o.visio);
  if (imported) {
    // Removing a source group needs its <Shape> subtree handled by the
    // writer; until then keep the provenance intact.
    app.setStatus('Ungrouping imported Visio groups is not written back yet');
    return;
  }
  app.history.push(app.doc);
  for (const o of all) if (o.group && roots.has(o.group.rootId)) o.group = null;
  app.render();
  app.setStatus('Ungrouped');
};

// ------------------------------------------------------------- re-layout --
/**
 * Group the page into layout units: one plain shape, or one whole group.
 * Container/list frames and their members are left alone so a re-layout does
 * not break swimlanes or container membership. Groups containing any such
 * member are skipped as a whole.
 */
function layoutUnitsForPage(pg) {
  const eligible = (s) => s.x !== undefined
    && !s.callout                    // follows its target; not a layout node
    && !isCffFrame(pg, s)
    && !s.containerKind
    && !(s.containers && s.containers.length)
    && !(s.listOf && s.listOf.length)
    && s.group?.kind !== 'container'
    && s.group?.kind !== 'list';
  const blockedGroups = new Set();
  for (const s of pg.shapes) {
    if (s.group?.rootId && !eligible(s)) blockedGroups.add(String(s.group.rootId));
  }
  const units = new Map();
  const unitByShape = new Map();
  for (const s of pg.shapes) {
    if (!eligible(s)) continue;
    if (s.group?.rootId && blockedGroups.has(String(s.group.rootId))) continue;
    const key = s.group?.rootId ? `g:${s.group.rootId}` : `s:${s.id}`;
    let unit = units.get(key);
    if (!unit) {
      unit = { id: key, shapes: [], x: Infinity, y: Infinity, r: -Infinity, b: -Infinity };
      units.set(key, unit);
    }
    unit.shapes.push(s);
    unit.x = Math.min(unit.x, s.x);
    unit.y = Math.min(unit.y, s.y);
    unit.r = Math.max(unit.r, s.x + s.w);
    unit.b = Math.max(unit.b, s.y + s.h);
    unitByShape.set(s.id, key);
  }
  for (const unit of units.values()) {
    unit.w = Math.max(1, unit.r - unit.x);
    unit.h = Math.max(1, unit.b - unit.y);
    // Stable initial order follows the page's reading order.
    unit.order = unit.x + unit.y * 1000;
    // Annotation text (no fill and hairline/no stroke) and 1-D line shapes are
    // not diagram nodes; keep them where they are.
    unit.static = unit.shapes.every((s) =>
      s.w <= 3 || s.h <= 3
      || (s.fill === 'none' && !(s.strokeWidth > 0)));
  }
  // A large shape that encloses several other shapes is a page backdrop, not
  // a flow node. Compare against the median unit area so ordinary boxes are
  // never caught.
  const areas = [...units.values()].map((u) => u.w * u.h).sort((a, b) => a - b);
  const median = areas.length ? areas[Math.floor(areas.length / 2)] : 0;
  for (const unit of units.values()) {
    if (!median || unit.w * unit.h < median * 4) continue;
    let contained = 0;
    for (const other of units.values()) {
      if (other === unit) continue;
      const cx = other.x + other.w / 2;
      const cy = other.y + other.h / 2;
      if (cx >= unit.x && cx <= unit.x + unit.w
        && cy >= unit.y && cy <= unit.y + unit.h) contained++;
    }
    if (contained >= 2) unit.backdrop = true;
  }
  return { units: [...units.values()], unitByShape };
}

/**
 * Lay out the current selection (two or more units) or the whole page.
 * Connectors between laid-out units guide a layered top-down/left-right
 * layout; other shapes stay where they are.
 */
app.relayout = (options = {}) => {
  const pg = app.getPage();
  const direction = options.direction === 'horizontal' ? 'horizontal' : 'vertical';
  const { units, unitByShape } = layoutUnitsForPage(pg);
  if (units.length < 2) {
    app.setStatus('Re-layout needs at least two plain shapes or groups');
    return;
  }
  const selected = app.state.selection;
  let chosen = units;
  let scope = 'page';
  if (selected.size) {
    const selectedUnits = new Set();
    const shapeIds = new Set(pg.shapes.map((s) => s.id));
    let selectedShapes = 0;
    for (const id of selected) {
      if (shapeIds.has(id)) selectedShapes++;
      const unitId = unitByShape.get(id);
      if (unitId) selectedUnits.add(unitId);
    }
    if (selectedUnits.size >= 2) {
      chosen = units.filter((u) => selectedUnits.has(u.id));
      scope = 'selection';
    } else if (selectedShapes > 0) {
      // One shape, or a shape we cannot move (container/lane member).
      app.setStatus('Select two or more shapes to re-layout, or clear the selection to lay out the page');
      return;
    }
    // A connector-only (or container-frame-only) selection lays out the page.
  }

  const allEdges = [];
  for (const c of pg.connectors) {
    const from = c.from?.shapeId ? unitByShape.get(c.from.shapeId) : null;
    const to = c.to?.shapeId ? unitByShape.get(c.to.shapeId) : null;
    if (from && to && from !== to) allEdges.push({ from, to });
  }

  if (scope === 'page') {
    const usable = units.filter((u) => !u.static && !u.backdrop);
    const touched = new Set();
    for (const edge of allEdges) { touched.add(edge.from); touched.add(edge.to); }
    // With a connected diagram, leave labels, line-only shapes, backdrops and
    // unconnected objects where they are. A page of loose shapes has no edges,
    // so grid-lay everything usable instead.
    chosen = allEdges.length
      ? usable.filter((u) => touched.has(u.id))
      : usable;
    if (chosen.length < 2) chosen = usable;
    if (chosen.length < 2) {
      app.setStatus('Re-layout found no movable diagram shapes');
      return;
    }
  }

  const chosenIds = new Set(chosen.map((u) => u.id));
  const edges = allEdges.filter((e) => chosenIds.has(e.from) && chosenIds.has(e.to));

  const originX = Math.min(...chosen.map((u) => u.x));
  const originY = Math.min(...chosen.map((u) => u.y));
  const positions = layoutUnits(chosen, edges, {
    direction,
    originX,
    originY,
  });
  if (!positions.size) {
    app.setStatus('Nothing to re-layout');
    return;
  }

  const deltas = new Map();
  const movedShapes = new Set();
  for (const unit of chosen) {
    const p = positions.get(unit.id);
    if (!p) continue;
    const dx = Math.round(p.x - unit.x);
    const dy = Math.round(p.y - unit.y);
    deltas.set(unit.id, { dx, dy });
    for (const s of unit.shapes) if (dx || dy) movedShapes.add(s.id);
  }
  if (!movedShapes.size) {
    app.setStatus('Already laid out');
    return;
  }

  app.history.push(app.doc);
  for (const unit of chosen) {
    const d = deltas.get(unit.id);
    if (!d || (!d.dx && !d.dy)) continue;
    for (const s of unit.shapes) {
      s.x += d.dx;
      s.y += d.dy;
    }
  }
  // A moved endpoint invalidates hand-placed waypoints; let the router pick
  // a fresh route for any connector touching a moved shape.
  for (const c of pg.connectors) {
    const a = c.from?.shapeId;
    const b = c.to?.shapeId;
    if ((a && movedShapes.has(a)) || (b && movedShapes.has(b))) c.waypoints = [];
  }
  app.render();
  app.updateInspector();
  app.setStatus(`Re-laid out ${movedShapes.size} object(s) ${scope === 'page' ? 'on the page' : 'in the selection'} (${direction === 'horizontal' ? 'left-to-right' : 'top-down'})`);
};


/** Copies keep only editor state; imported provenance and membership IDs go. */
function stripImportedIdentity(o) {
  delete o.visio;
  delete o.containers;
  delete o.listOf;
  delete o.containerKind;
  delete o.containerId;
  delete o.containerInfo;
}

/** A copied group needs a fresh root id, or the copy selects with the original. */
function remapCopiedGroups(shapes, connectors) {
  const roots = new Map();
  const remap = (o) => {
    if (!o.group?.rootId) return;
    const old = o.group.rootId;
    if (!roots.has(old)) roots.set(old, uid('g'));
    o.group = { ...o.group, rootId: roots.get(old) };
  };
  for (const s of shapes) remap(s);
  for (const c of connectors) remap(c);
}

app.selectAll = () => {
  const pg = app.getPage();
  app.setSelection([...pg.shapes.map((s) => s.id), ...pg.connectors.map((c) => c.id)]);
  app.render();
};

app.deleteSelection = () => {
  const sel = app.state.selection;
  if (!sel.size) return;
  app.history.push(app.doc);
  const pg = app.getPage();
  pg.shapes = pg.shapes.filter((s) => !sel.has(s.id));
  // A data callout cannot outlive its target.
  const alive = new Set(pg.shapes.map((s) => s.id));
  pg.shapes = pg.shapes.filter((s) => !s.callout || (s.attachedTo && alive.has(s.attachedTo)));
  pg.connectors = pg.connectors.filter((c) => {
    if (sel.has(c.id)) return false;
    if (c.from.shapeId && sel.has(c.from.shapeId)) return false;
    if (c.to.shapeId && sel.has(c.to.shapeId)) return false;
    return true;
  });
  // Remove authored CFF frames that were deleted outright and drop their
  // membership refs; imported group frames keep their shell (warned on save).
  const gone = new Set();
  for (const rec of pg.containers || []) {
    if (rec.modelId && !pg.shapes.some((s) => s.id === rec.modelId)) gone.add(cffRef(rec));
  }
  if (gone.size) {
    pg.containers = (pg.containers || []).filter((c) => !gone.has(cffRef(c)));
    for (const o of [...pg.shapes, ...pg.connectors]) {
      const c = (o.containers || []).filter((r) => !gone.has(String(r)));
      const l = (o.listOf || []).filter((r) => !gone.has(String(r)));
      if (c.length) o.containers = c; else delete o.containers;
      if (l.length) o.listOf = l; else delete o.listOf;
    }
  }
  cffRefresh(pg);
  app.setSelection([]);
  app.render();
};

app.duplicate = () => {
  const sel = app.state.selection;
  if (!sel.size) return;
  app.history.push(app.doc);
  const pg = app.getPage();
  const map = new Map();
  const created = [];
  const copyIds = new Set(sel);
  for (const s of pg.shapes) {
    if (s.callout && sel.has(s.attachedTo)) copyIds.add(s.id);
  }
  for (const s of pg.shapes) {
    if (!copyIds.has(s.id)) continue;
    const copy = structuredClone(s);
    copy.id = uid('s');
    copy.x += 24; copy.y += 24;
    stripImportedIdentity(copy);
    map.set(s.id, copy.id);
    if (copy.callout) {
      const mapped = map.get(copy.attachedTo);
      if (mapped) copy.attachedTo = mapped;
      else if (!pg.shapes.some((x) => x.id === copy.attachedTo)) {
        copy.attachedTo = null;
        copy.callout.position = 'free';
      }
    }
    pg.shapes.push(copy);
    created.push(copy.id);
  }
  for (const c of pg.connectors) {
    if (!sel.has(c.id)) continue;
    const copy = structuredClone(c);
    copy.id = uid('c');
    stripImportedIdentity(copy);
    copy.from.shapeId = map.get(copy.from.shapeId) || null;
    copy.to.shapeId = map.get(copy.to.shapeId) || null;
    copy.waypoints = (copy.waypoints || []).map((w) => ({ x: w.x + 24, y: w.y + 24 }));
    pg.connectors.push(copy);
    created.push(copy.id);
  }
  remapCopiedGroups(
    pg.shapes.filter((s) => created.includes(s.id)),
    pg.connectors.filter((c) => created.includes(c.id)),
  );
  app.setSelection(created);
  app.render();
};

app.copy = () => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const copyIds = new Set(sel);
  for (const s of pg.shapes) {
    if (s.callout && sel.has(s.attachedTo)) copyIds.add(s.id);
  }
  const shapes = pg.shapes.filter((s) => copyIds.has(s.id)).map((s) => structuredClone(s));
  // Carry the picture bytes too, so a paste into another document is complete.
  const media = {};
  for (const s of shapes) {
    const id = s.image && s.image.mediaId;
    if (id && app.doc.media && app.doc.media[id]) {
      media[id] = structuredClone(app.doc.media[id]);
    }
  }
  app.clipboard = {
    shapes,
    connectors: pg.connectors.filter((c) => sel.has(c.id)).map((c) => structuredClone(c)),
    media,
  };
  app.setStatus(`${app.clipboard.shapes.length + app.clipboard.connectors.length} object(s) copied`);
};

app.cut = () => {
  app.copy();
  app.deleteSelection();
};

app.setRoute = (route) => {
  const sel = app.state.selection;
  const conns = app.getPage().connectors.filter((c) => sel.has(c.id));
  if (!conns.length) return;
  app.history.push(app.doc);
  conns.forEach((c) => { c.route = route; c.waypoints = []; });
  app.render();
  app.updateInspector();
};

app.paste = () => {
  if (!app.clipboard || !app.clipboard.shapes.length) return;
  app.history.push(app.doc);
  const pg = app.getPage();
  const media = app.mediaMap();
  for (const s of app.clipboard.shapes) {
    const id = s.image && s.image.mediaId;
    if (id && !media[id] && app.clipboard.media && app.clipboard.media[id]) {
      media[id] = structuredClone(app.clipboard.media[id]);
    }
  }
  const map = new Map();
  const created = [];
  for (const s of app.clipboard.shapes) {
    const copy = structuredClone(s);
    copy.id = uid('s');
    copy.x += 20; copy.y += 20;
    stripImportedIdentity(copy);
    map.set(s.id, copy.id);
    if (copy.callout) {
      const mapped = map.get(copy.attachedTo);
      if (mapped) copy.attachedTo = mapped;
      else if (!pg.shapes.some((x) => x.id === copy.attachedTo)) {
        copy.attachedTo = null;
        copy.callout.position = 'free';
      }
    }
    pg.shapes.push(copy);
    created.push(copy.id);
  }
  for (const c of app.clipboard.connectors) {
    const copy = structuredClone(c);
    copy.id = uid('c');
    stripImportedIdentity(copy);
    copy.from.shapeId = map.get(copy.from.shapeId) || null;
    copy.to.shapeId = map.get(copy.to.shapeId) || null;
    copy.waypoints = (copy.waypoints || []).map((w) => ({ x: w.x + 20, y: w.y + 20 }));
    if (copy.from.point) copy.from.point = { x: copy.from.point.x + 20, y: copy.from.point.y + 20 };
    if (copy.to.point) copy.to.point = { x: copy.to.point.x + 20, y: copy.to.point.y + 20 };
    pg.connectors.push(copy);
    created.push(copy.id);
  }
  remapCopiedGroups(
    pg.shapes.filter((s) => created.includes(s.id)),
    pg.connectors.filter((c) => created.includes(c.id)),
  );
  app.setSelection(created);
  app.render();
};

app.nudge = (dx, dy) => {
  const sel = app.state.selection;
  if (!sel.size) return;
  app.history.push(app.doc);
  const pg = app.getPage();
  for (const s of pg.shapes) {
    if (!sel.has(s.id)) continue;
    s.x += dx; s.y += dy;
    if (s.callout) s.callout.position = 'free';
  }
  for (const c of pg.connectors) {
    if (!sel.has(c.id)) continue;
    c.waypoints = (c.waypoints || []).map((w) => ({ x: w.x + dx, y: w.y + dy }));
  }
  app.render();
};

app.alignShapes = (kind) => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const shapes = pg.shapes.filter((s) => sel.has(s.id));
  if (shapes.length < 2) return;
  app.history.push(app.doc);
  const b = pageBounds({ shapes, connectors: [] });
  for (const s of shapes) {
    if (s.callout) s.callout.position = 'free';
    switch (kind) {
      case 'left': s.x = b.x; break;
      case 'hcenter': s.x = b.x + (b.w - s.w) / 2; break;
      case 'right': s.x = b.x + b.w - s.w; break;
      case 'top': s.y = b.y; break;
      case 'vcenter': s.y = b.y + (b.h - s.h) / 2; break;
      case 'bottom': s.y = b.y + b.h - s.h; break;
      default: break;
    }
  }
  app.render();
};

app.reorder = (dir) => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const picked = pg.shapes.filter((s) => sel.has(s.id));
  if (!picked.length) return;
  app.history.push(app.doc);
  const rest = pg.shapes.filter((s) => !sel.has(s.id));
  pg.shapes = dir > 0 ? [...rest, ...picked] : [...picked, ...rest];
  // keep the stacking order in step with the new array order
  const base = Math.min(0, ...pg.shapes.map((s) => s.z ?? 0));
  pg.shapes.forEach((s, i) => { s.z = base + i; });
  app.render();
};

app.toggleStyle = (prop) => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const shapes = pg.shapes.filter((s) => sel.has(s.id));
  if (!shapes.length) return;
  app.history.push(app.doc);
  const target = !shapes[0][prop];
  shapes.forEach((s) => { s[prop] = target; });
  app.render();
  app.updateInspector();
};

app.applyStyleToSelection = (patch) => {
  const sel = app.state.selection;
  const pg = app.getPage();
  const shapes = pg.shapes.filter((s) => sel.has(s.id));
  const conns = pg.connectors.filter((c) => sel.has(c.id));
  if (!shapes.length && !conns.length) return;
  app.history.push(app.doc);
  const NUMERIC = new Set(['strokeWidth', 'fontSize', 'fillOpacity', 'rot']);
  for (const s of shapes) {
    for (const k in patch) s[k] = NUMERIC.has(k) ? (parseFloat(patch[k]) || 0) : patch[k];
  }
  for (const c of conns) if ('stroke' in patch) c.stroke = patch.stroke;
  app.render();
};

app.addShape = (type, x, y) => {
  app.history.push(app.doc);
  const d = app.shapeDefaults(type);
  const s = newShape(type, x - d.w / 2, y - d.h / 2);
  s.w = d.w; s.h = d.h;
  app.getPage().shapes.push(s);
  app.state.recent = [type, ...app.state.recent.filter((t) => t !== type)].slice(0, 8);
  buildRecent(app);
  app.setSelection([s.id]);
  app.render();
};

// ------------------------------------------------------- data callouts --
/** Callouts whose target is one of `ids`. */
app.calloutsFor = (ids) => {
  const pg = app.getPage();
  const set = ids instanceof Set ? ids : new Set(ids);
  return pg.shapes.filter((s) => s.callout && set.has(s.attachedTo));
};

/**
 * Keep data callouts attached to their target. Non-free callouts are placed
 * above/below/left/right of the live target box and auto-sized to their text;
 * a free callout keeps its own position. Run before every render.
 */
app.layoutAttachments = () => {
  const pg = app.getPage();
  for (const s of pg.shapes) {
    if (!s.callout) continue;
    const t = calloutTarget(s, pg);
    if (!t) continue;
    if (s.callout.autoSize !== false) {
      if (s.callout.kind === 'bar') {
        const pos0 = s.callout.position || 'top';
        s.h = Math.max(16, Math.round((s.fontSize || 11) * 1.35 + 4));
        s.w = (pos0 === 'top' || pos0 === 'bottom')
          ? Math.max(60, t.w) : Math.max(100, 120);
      } else if (s.callout.kind === 'icon') {
        s.h = 16;
        s.w = 88;
      } else {
        const text = calloutText(s, pg);
        s.w = Math.max(48, Math.ceil(measureText(text, fontString(s)) + 26));
        s.h = Math.max(24, Math.ceil((s.fontSize || 12) * 1.35 + 9));
      }
    }
    const pos = s.callout.position || 'top';
    if (pos === 'free') continue;
    const gap = s.callout.gap ?? 8;
    const cx = t.x + t.w / 2;
    const cy = t.y + t.h / 2;
    if (pos === 'top') { s.x = cx - s.w / 2; s.y = t.y - gap - s.h; }
    else if (pos === 'bottom') { s.x = cx - s.w / 2; s.y = t.y + t.h + gap; }
    else if (pos === 'left') { s.x = t.x - gap - s.w; s.y = cy - s.h / 2; }
    else { s.x = t.x + t.w + gap; s.y = cy - s.h / 2; }
  }
};

app.addDataCallout = (targetId, fieldName, position = 'top', label = '') => {
  const pg = app.getPage();
  const target = pg.shapes.find((s) => s.id === targetId && !s.callout);
  if (!target) return;
  const rows = target.data || [];
  const field = rows.some((d) => String(d.name) === String(fieldName))
    ? fieldName : rows[0]?.name;
  if (!field) {
    alert('Add Shape Data to this shape first; a data callout is bound to a property.');
    return;
  }
  const pos = ['top', 'bottom', 'left', 'right', 'free'].includes(position) ? position : 'top';
  app.history.push(app.doc);
  const s = newShape('dataCallout', target.x, target.y, {
    w: 150, h: 30,
    fill: '#fff9e6', stroke: '#c9b458', strokeWidth: 1,
    textColor: '#4a3b00', fontSize: 12, align: 'center', vAlign: 'middle',
  });
  s.attachedTo = target.id;
  s.callout = {
    kind: 'text',
    field: String(field), label: String(label || ''),
    position: pos, gap: 8, autoSize: true,
    min: 0, max: 100, showValue: true,
  };
  pg.shapes.push(s);
  app.layoutAttachments();
  app.setSelection([s.id]);
  app.render();
  app.setStatus(`Added data callout for ${field}`);
};

app.removeDataCallout = (id) => {
  const pg = app.getPage();
  const index = pg.shapes.findIndex((s) => s.id === id && s.callout);
  if (index < 0) return;
  app.history.push(app.doc);
  const [removed] = pg.shapes.splice(index, 1);
  app.setSelection(removed.attachedTo ? [removed.attachedTo] : []);
  app.render();
  app.setStatus('Removed data graphic');
};

/** A round upper bound for a new bar's scale (1200 → 2000, 42 → 50). */
function niceMax(v) {
  if (!Number.isFinite(v) || v <= 0) return 100;
  const exp = Math.floor(Math.log10(v));
  const base = Math.pow(10, exp);
  for (const m of [1, 2, 5, 10]) {
    if (m * base >= v * 1.05) return m * base;
  }
  return 10 * base;
}

/** Add a numeric data bar using a Shape Data row's value. */
app.addDataBar = (targetId, fieldName, position = 'top', options = {}) => {
  const pg = app.getPage();
  const target = pg.shapes.find((s) => s.id === targetId && !s.callout);
  if (!target) return;
  const rows = target.data || [];
  const field = rows.some((d) => String(d.name) === String(fieldName))
    ? fieldName : rows[0]?.name;
  if (!field) {
    alert('Add Shape Data to this shape first; a data bar is bound to a numeric property.');
    return;
  }
  const raw = String((rows.find((d) => String(d.name) === String(field)) || {}).value ?? '')
    .replace(/[, ]/g, '').trim();
  const value = parseFloat(raw);
  const pos = ['top', 'bottom', 'left', 'right', 'free'].includes(position) ? position : 'top';
  app.history.push(app.doc);
  const s = newShape('dataBar', target.x, target.y, {
    w: Math.max(60, target.w), h: 18,
    fill: '#5b9bd5', stroke: '#9aa7b4', strokeWidth: 1,
    textColor: '#17212b', fontSize: 11, align: 'center', vAlign: 'middle',
  });
  s.attachedTo = target.id;
  s.callout = {
    kind: 'bar',
    field: String(field), label: String(options.label || ''),
    position: pos, gap: 8, autoSize: true,
    min: Number.isFinite(Number(options.min)) ? Number(options.min) : 0,
    max: Number.isFinite(Number(options.max))
      ? Number(options.max) : niceMax(Number.isFinite(value) ? value : 0),
    showValue: options.showValue !== false,
  };
  pg.shapes.push(s);
  app.layoutAttachments();
  app.setSelection([s.id]);
  app.render();
  app.setStatus(`Added data bar for ${field}`);
};

/** Add a five-level icon set using a Shape Data row's numeric value. */
app.addDataIcon = (targetId, fieldName, position = 'left', options = {}) => {
  const pg = app.getPage();
  const target = pg.shapes.find((s) => s.id === targetId && !s.callout);
  if (!target) return;
  const rows = target.data || [];
  const field = rows.some((d) => String(d.name) === String(fieldName))
    ? fieldName : rows[0]?.name;
  if (!field) {
    alert('Add Shape Data to this shape first; a data icon is bound to a numeric property.');
    return;
  }
  const raw = String((rows.find((d) => String(d.name) === String(field)) || {}).value ?? '')
    .replace(/[, ]/g, '').trim();
  const value = parseFloat(raw);
  const pos = ['top', 'bottom', 'left', 'right', 'free'].includes(position) ? position : 'left';
  app.history.push(app.doc);
  const s = newShape('dataIcon', target.x, target.y, {
    w: 88, h: 16,
    fill: '#2ca0f1', stroke: '#7a8798', strokeWidth: 1,
    fontSize: 11, align: 'center', vAlign: 'middle',
  });
  s.attachedTo = target.id;
  s.callout = {
    kind: 'icon',
    iconSet: ['boxes', 'diamonds', 'circles'].includes(options.iconSet)
      ? options.iconSet : 'boxes',
    field: String(field), label: '',
    position: pos, gap: 8, autoSize: false,
    min: Number.isFinite(Number(options.min)) ? Number(options.min) : 0,
    max: Number.isFinite(Number(options.max))
      ? Number(options.max) : niceMax(Number.isFinite(value) ? value : 0),
    showValue: false,
  };
  pg.shapes.push(s);
  app.layoutAttachments();
  app.setSelection([s.id]);
  app.render();
  app.setStatus(`Added data icon for ${field}`);
};

/** Add a filled heading callout that shows a Shape Data value. */
app.addDataHeading = (targetId, fieldName, position = 'top', label = '') => {
  const pg = app.getPage();
  const target = pg.shapes.find((s) => s.id === targetId && !s.callout);
  if (!target) return;
  const rows = target.data || [];
  const field = rows.some((d) => String(d.name) === String(fieldName))
    ? fieldName : rows[0]?.name;
  if (!field) {
    alert('Add Shape Data to this shape first; a data heading is bound to a property.');
    return;
  }
  const pos = ['top', 'bottom', 'left', 'right', 'free'].includes(position) ? position : 'top';
  app.history.push(app.doc);
  const s = newShape('dataCallout', target.x, target.y, {
    w: 150, h: 34,
    fill: '#4472c4', stroke: '#2f5597', strokeWidth: 1,
    textColor: '#ffffff', fontSize: 13, bold: true,
    align: 'center', vAlign: 'middle',
  });
  s.attachedTo = target.id;
  s.callout = {
    kind: 'heading',
    field: String(field), label: String(label || ''),
    position: pos, gap: 8, autoSize: true,
    min: 0, max: 100, showValue: true,
  };
  pg.shapes.push(s);
  app.layoutAttachments();
  app.setSelection([s.id]);
  app.render();
  app.setStatus(`Added data heading for ${field}`);
};

/** Add or replace a three-band colour-by-value rule on the target shape. */
app.addDataColor = (targetId, fieldName, options = {}) => {
  const pg = app.getPage();
  const target = pg.shapes.find((s) => s.id === targetId && !s.callout);
  if (!target) return;
  const rows = target.data || [];
  const requested = rows.some((d) => String(d.name) === String(fieldName))
    ? fieldName : rows[0]?.name;
  const numeric = (d) => Number.isFinite(parseFloat(
    String(d.value ?? '').replace(/[, ]/g, '').trim()));
  const field = rows.some((d) => String(d.name) === String(requested) && numeric(d))
    ? requested : (rows.find(numeric)?.name || requested);
  if (!field) {
    alert('Add Shape Data to this shape first; colour by value is bound to a numeric property.');
    return;
  }
  const raw = String((rows.find((d) => String(d.name) === String(field)) || {}).value ?? '')
    .replace(/[, ]/g, '').trim();
  const value = parseFloat(raw);
  app.history.push(app.doc);
  target.dataColor = {
    field: String(field),
    min: Number.isFinite(Number(options.min)) ? Number(options.min) : 0,
    max: Number.isFinite(Number(options.max))
      ? Number(options.max) : niceMax(Number.isFinite(value) ? value : 0),
    low: options.low || DATA_COLOR_DEFAULTS.low,
    mid: options.mid || DATA_COLOR_DEFAULTS.mid,
    high: options.high || DATA_COLOR_DEFAULTS.high,
    baseFill: target.fill ?? '#dbe9f6',
  };
  app.setSelection([target.id]);
  app.render();
  app.setStatus(`Colored ${target.label || target.type} by ${field}`);
};

app.removeDataColor = (targetId) => {
  const target = app.getPage().shapes.find((s) => s.id === targetId && s.dataColor);
  if (!target) return;
  app.history.push(app.doc);
  if (target.dataColor.baseFill) target.fill = target.dataColor.baseFill;
  delete target.dataColor;
  app.render();
  app.setStatus('Removed colour rule');
};

// -------------------------------------------------------------- pictures --
/** Media parts (data URLs) shared by every shape that shows the same image. */
app.mediaMap = () => {
  if (!app.doc.media) app.doc.media = {};
  return app.doc.media;
};

function readImageFile(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error || new Error('Could not read the image'));
    reader.readAsDataURL(file);
  });
}

function imageNaturalSize(src) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error('This browser cannot display that image format'));
    img.src = src;
  });
}

const VISIO_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/bmp']);

/** Formats Visio does not embed directly (WebP, SVG, ...) become PNG. */
function imageToPng(img, w, h) {
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, w);
  canvas.height = Math.max(1, h);
  canvas.getContext('2d').drawImage(img, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL('image/png');
}

function mediaExtFor(file, type) {
  const fromName = (/\.([a-z0-9]+)$/i.exec(file.name || '') || [null, ''])[1].toLowerCase();
  if (fromName) return fromName === 'jpeg' ? 'jpeg' : fromName;
  const sub = String(type || '').split('/')[1] || 'png';
  return sub === 'svg+xml' ? 'svg' : sub === 'jpeg' ? 'jpeg' : sub;
}

/** Visio skips brightness/contrast/gamma on palette-indexed pictures. */
function mediaAdjustable(type, src) {
  if (type === 'image/gif') return false;
  if (type === 'image/png') {
    try {
      const start = src.indexOf(',') + 1;
      const head = atob(src.slice(start, start + 64));
      return head.charCodeAt(25) !== 3;       // IHDR colour type
    } catch { return true; }
  }
  return true;
}

/** Normalised crop of the source picture: fractions of the original image. */
function imageCropOf(img) {
  const sx = Math.max(1e-6, img.scaleX ?? 1);
  const sy = Math.max(1e-6, img.scaleY ?? 1);
  // An image drawn smaller than its frame is not cropped.
  if (sx < 1 - 1e-6 || sy < 1 - 1e-6) return { left: 0, top: 0, right: 0, bottom: 0 };
  return {
    left: Math.max(0, -(img.offsetX || 0) / sx),
    top: Math.max(0, -(img.offsetY || 0) / sy),
    right: Math.max(0, ((img.offsetX || 0) + sx - 1) / sx),
    bottom: Math.max(0, ((img.offsetY || 0) + sy - 1) / sy),
  };
}

function setImageCrop(img, side, value) {
  let c = imageCropOf(img);
  c = { ...c, [side]: clamp(value, 0, 0.94) };
  const fit = (a, b) => {
    const total = a + b;
    if (total <= 0.96) return [a, b];
    const k = 0.96 / total;
    return [a * k, b * k];
  };
  [c.left, c.right] = fit(c.left, c.right);
  [c.top, c.bottom] = fit(c.top, c.bottom);
  img.scaleX = 1 / Math.max(1e-3, 1 - c.left - c.right);
  img.offsetX = -c.left * img.scaleX;
  img.scaleY = 1 / Math.max(1e-3, 1 - c.top - c.bottom);
  img.offsetY = -c.top * img.scaleY;
}

app.imageCropOf = imageCropOf;
app.setImageCrop = setImageCrop;

/**
 * Insert a picture at the viewport centre (or at a drop point), or replace the
 * picture of the shape picked with "Replace image". Raster bytes live in
 * `doc.media` as data URLs; shapes reference them by media id.
 */
app.insertImageFile = async (file, at = null) => {
  if (!file || !/^image\//.test(file.type || '')) {
    alert('Choose a picture file (PNG, JPEG, GIF, BMP or WebP).');
    return;
  }
  let src, loaded;
  try {
    src = await readImageFile(file);
    loaded = await imageNaturalSize(src);
  } catch (err) {
    alert(err.message || 'Could not read the image');
    return;
  }
  let type = file.type || 'image/png';
  let ext = mediaExtFor(file, type);
  const size = {
    w: loaded.naturalWidth || 320,
    h: loaded.naturalHeight || 240,
  };
  if (!VISIO_IMAGE_TYPES.has(type)) {
    try {
      src = imageToPng(loaded, size.w, size.h);
      type = 'image/png';
      ext = 'png';
    } catch {
      alert('This picture could not be converted to PNG.');
      return;
    }
  }
  const media = app.mediaMap();
  const mediaId = uid('m');
  media[mediaId] = {
    id: mediaId, src, type, ext,
    name: file.name || 'picture', width: size.w, height: size.h,
    unsupported: false,
    adjust: mediaAdjustable(type, src),
  };

  const targetId = app.state.imageTarget;
  app.state.imageTarget = null;
  const target = targetId
    ? app.getPage().shapes.find((s) => s.id === targetId && s.image) : null;
  if (target) {
    app.history.push(app.doc);
    target.image.mediaId = mediaId;
    target.image.name = media[mediaId].name;
    target.image.mediaType = type;
    target.image.unsupported = false;
    app.render();
    app.updateInspector();
    app.setStatus(`Replaced picture with ${media[mediaId].name}`);
    return;
  }

  const k = Math.min(1, 480 / Math.max(size.w, size.h));
  const w = Math.max(24, Math.round(size.w * k));
  const h = Math.max(24, Math.round(size.h * k));
  const rect = app.svg.getBoundingClientRect();
  const v = app.state.view;
  const cx = at ? at.x : (rect.width / 2 - v.x) / v.zoom;
  const cy = at ? at.y : (rect.height / 2 - v.y) / v.zoom;
  app.history.push(app.doc);
  const shape = newShape('image', Math.round(cx - w / 2), Math.round(cy - h / 2), {
    w, h, fill: 'none', stroke: '#000000', strokeWidth: 0,
  });
  shape.image = {
    mediaId, name: media[mediaId].name, mediaType: type,
    offsetX: 0, offsetY: 0, scaleX: 1, scaleY: 1,
    brightness: 0, contrast: 0, gamma: 1, transparency: 0,
    foreignType: 'Bitmap', unsupported: false,
  };
  app.getPage().shapes.push(shape);
  app.setSelection([shape.id]);
  app.render();
  app.setStatus(`Inserted ${media[mediaId].name} (${size.w}×${size.h})`);
};

app.imagePick = (targetId = null) => {
  app.state.imageTarget = targetId;
  $('#imageInput').click();
};

/** Reset the frame to the picture's natural size, preserving its centre. */
app.fitImageNatural = (shape) => {
  const media = app.doc.media && app.doc.media[shape.image?.mediaId];
  if (!shape.image || !media || !media.width || !media.height) return;
  const k = Math.min(1, 960 / Math.max(media.width, media.height));
  const w = Math.max(16, Math.round(media.width * k));
  const h = Math.max(16, Math.round(media.height * k));
  const cx = shape.x + shape.w / 2;
  const cy = shape.y + shape.h / 2;
  app.history.push(app.doc);
  shape.w = w; shape.h = h;
  shape.x = Math.round(cx - w / 2);
  shape.y = Math.round(cy - h / 2);
  app.render();
  app.updateInspector();
};

/** Full-bleed the picture in its frame (clears crop and offset). */
app.fitImageFrame = (shape) => {
  if (!shape.image) return;
  app.history.push(app.doc);
  shape.image.offsetX = 0;
  shape.image.offsetY = 0;
  shape.image.scaleX = 1;
  shape.image.scaleY = 1;
  app.render();
  app.updateInspector();
};

app.resetImageAdjustments = (shape) => {
  if (!shape.image) return;
  app.history.push(app.doc);
  shape.image.brightness = 0;
  shape.image.contrast = 0;
  shape.image.gamma = 1;
  shape.image.transparency = 0;
  app.render();
  app.updateInspector();
};


// ------------------------------------------------------------- viewport --
app.zoomTo = (z, cx, cy) => {
  const v = app.state.view;
  const nz = clamp(z, 0.05, 8);
  const wx = (cx - v.x) / v.zoom, wy = (cy - v.y) / v.zoom;
  v.zoom = nz;
  v.x = cx - wx * nz;
  v.y = cy - wy * nz;
  app.renderer.updateView();
  app.updateStatus();
};

app.zoomStep = (f) => {
  const r = app.svg.getBoundingClientRect();
  app.zoomTo(app.state.view.zoom * f, r.width / 2, r.height / 2);
};

app.fitView = () => {
  const pg = app.getPage();
  const b = pageBounds(pg);
  const r = app.svg.getBoundingClientRect();
  const z = clamp(Math.min((r.width - 60) / b.w, (r.height - 60) / b.h), 0.1, 2);
  app.state.view.zoom = z;
  app.state.view.x = (r.width - b.w * z) / 2 - b.x * z;
  app.state.view.y = (r.height - b.h * z) / 2 - b.y * z;
  app.renderer.updateView();
  app.updateStatus();
};

app.toggleGrid = () => { app.state.grid.on = !app.state.grid.on; app.render(); };
app.toggleSnap = () => { app.state.grid.snap = !app.state.grid.snap; app.render(); };

// --------------------------------------------------------------- history --
app.undo = () => {
  const d = app.history.undo(app.doc);
  if (!d) return;
  app.doc = d;
  app.state.selection.clear();
  app.render();
  app.updateInspector();
  buildPageTabs(app);
};

app.redo = () => {
  const d = app.history.redo(app.doc);
  if (!d) return;
  app.doc = d;
  app.state.selection.clear();
  app.render();
  app.updateInspector();
  buildPageTabs(app);
};

// ---------------------------------------------------------------- pages --
app.setPage = (i) => {
  if (i === app.doc.pageIndex) return;
  app.doc.pageIndex = i;
  app.state.selection.clear();
  app.render();
  app.updateInspector();
  buildPageTabs(app);
};

app.addPage = () => {
  app.history.push(app.doc);
  const p = newPage(`Page-${app.doc.pages.length + 1}`);
  app.doc.pages.push(p);
  app.doc.pageIndex = app.doc.pages.length - 1;
  app.state.selection.clear();
  app.render();
  app.updateInspector();
  buildPageTabs(app);
};

app.deletePage = (id) => {
  if (app.doc.pages.length <= 1) return;
  app.history.push(app.doc);
  const i = app.doc.pages.findIndex((p) => p.id === id);
  app.doc.pages.splice(i, 1);
  app.doc.pageIndex = Math.max(0, Math.min(app.doc.pageIndex, app.doc.pages.length - 1));
  app.state.selection.clear();
  app.render();
  app.updateInspector();
  buildPageTabs(app);
};

// ----------------------------------------------------------------- file --
app.newDoc = () => {
  if (!confirm('Start a new diagram? Unsaved changes will be lost.')) return;
  if (app.doc.visioKey) deleteVisioSource(app.doc.visioKey);
  app.doc = newDoc();
  ensureTheme(app.doc);
  app.history = new History();
  app.visioSource = null;
  app.state.selection.clear();
  app.state.view = { x: 40, y: 30, zoom: 1 };
  app.render();
  app.updateInspector();
  buildPageTabs(app);
};

app.saveLocal = () => {
  if (saveLocal(app.doc)) app.setStatus('Saved to browser storage');
  else app.setStatus('Could not save');
};

app.openFile = () => $('#fileInput').click();

/** True when the current document still descends from the retained package. */
function docMatchesSource(doc, source) {
  if (!source) return false;
  const parts = source.pageParts || [];
  return doc.pages.some((p) => {
    if (!p.visio?.part || !parts.includes(p.visio.part)) return false;
    // The package key separates two files that both happen to use
    // visio/pages/page1.xml, which is the normal case.
    if (source.key && p.visio.docKey && p.visio.docKey !== source.key) return false;
    return true;
  });
}

app.saveVSDX = async () => {
  let source = app.visioSource;
  if (!source || !docMatchesSource(app.doc, source)) {
    // Undo can walk back to a different imported document. Its package may
    // still be in the side cache under the document's own key.
    if (app.doc.visioKey) {
      const restored = await loadVisioSource(app.doc.visioKey);
      if (restored && docMatchesSource(app.doc, restored)) source = restored;
    }
  }
  if (!source || !docMatchesSource(app.doc, source)) source = null;
  let fromScratch = !source;
  let audit = auditVSDX(app.doc, source);
  let rebuilt = false;

  if (!fromScratch && audit.removedPages) {
    const scratch = auditVSDX(app.doc, null);
    const importedBits = [];
    if (scratch.importedPages) importedBits.push(`${scratch.importedPages} imported page(s)`);
    if (scratch.importedObjects) importedBits.push(`${scratch.importedObjects} object(s)`);
    const note = importedBits.length
      ? ` ${importedBits.join(' and ')} will be rebuilt from the editable model, so masters and unknown parts may be flattened.`
      : '';
    const parts = scratch.newPartStyleLoss
      ? ` ${scratch.newPartStyleLoss} per-part shape(s) will be flattened to one fill/line.`
      : '';
    if (!confirm(
      'Page deletion cannot be applied while patching the original package.\n\n'
      + `Build a new VSDX from the current document instead?${note}${parts}`)) return;
    source = null;
    fromScratch = true;
    audit = scratch;
    rebuilt = true;
  }

  if (!fromScratch) {
    if (!audit.ok) {
      alert('The original Visio package is no longer available.');
      return;
    }
    if (audit.missingParts.length) {
      alert(`The original package is missing ${audit.missingParts.length} page part(s); refusing to write an incomplete file.`);
      return;
    }
    if (audit.duplicateIds.length) {
      alert('Two objects point at the same original Visio shape. Reopen the file before saving.');
      return;
    }
    if (audit.removedPages) {
      alert('Page deletion is not supported while patching an existing package. Undo the page deletion, or reopen the file and use Save after exporting a fresh copy.');
      return;
    }
    if (audit.addedPages && !audit.canAddPages) {
      alert('This package stores its pages inline; adding pages is not supported for it yet.');
      return;
    }
  }

  const omitted = [];
  if (audit.missingImageData) {
    alert(`${audit.missingImageData} picture(s) have no image data in this document. `
      + 'Reinsert the picture, or reopen the source file before saving.');
    return;
  }
  if (audit.fontChanges) omitted.push(`${audit.fontChanges} font-family change(s)`);
  if (audit.customStyleChanges) omitted.push(`${audit.customStyleChanges} fill/line change(s) on imported per-part shapes`);
  if (audit.newPartStyleLoss) omitted.push(`${audit.newPartStyleLoss} per-part shape(s) flattened to one fill/line`);
  if (audit.zOrderChanges) omitted.push(`${audit.zOrderChanges} z-order change(s)`);
  if (audit.typeChanges) omitted.push(`${audit.typeChanges} shape-type change(s)`);
  if (audit.authoredGroups) omitted.push(`${audit.authoredGroups} group(s) written as separate shapes`);
  if (audit.rotatedGroupChanges) omitted.push(`${audit.rotatedGroupChanges} rotated group change(s) other than a whole-group move`);
  if (audit.containerGroupChanges) omitted.push(`${audit.containerGroupChanges} container frame change(s)`);
  if (audit.ungroupChanges) omitted.push(`${audit.ungroupChanges} imported group ungroup/regroup change(s)`);
  if (audit.emptyGroups) omitted.push(`${audit.emptyGroups} emptied group shell(s)`);
  if (audit.containerMembershipLoss) omitted.push(`${audit.containerMembershipLoss} container/list membership graph(s)`);

  let intro = '';
  if (fromScratch) {
    intro = 'VSDX save will build a new Visio package from this document.';
    const importedBits = [];
    if (audit.importedPages) importedBits.push(`${audit.importedPages} imported page(s)`);
    if (audit.importedObjects) importedBits.push(`${audit.importedObjects} object(s)`);
    if (importedBits.length) {
      intro += `\n\n${importedBits.join(' and ')} will be rebuilt from the editable model;`
        + ' masters, unknown parts and some layout/style details may be flattened.';
    }
  } else {
    intro = 'VSDX save patches imported content and writes objects added in LayoutCraft as inline shapes.';
  }
  const needsAsk = !rebuilt && ((fromScratch && (audit.importedPages || audit.importedObjects)) || omitted.length);
  if (needsAsk && !confirm(
    `${intro}\n\n`
    + (omitted.length ? `Not written exactly: ${omitted.join('; ')}.\n\n` : '')
    + 'Save anyway?')) return;

  try {
    const name = vsdxFileName(app.doc, source);
    const result = await exportVSDX(app.doc, source, name);
    if (fromScratch) {
      // The new file is now the document's canonical Visio content. Drop the
      // old package lineage so later saves rebuild from the model instead of
      // repeatedly warning about a package that no longer matches.
      app.visioSource = null;
      delete app.doc.visioKey;
      for (const pg of app.doc.pages) {
        delete pg.visio;
        for (const s of pg.shapes) delete s.visio;
        for (const c of pg.connectors) delete c.visio;
      }
      saveLocal(app.doc);
    }
    const bits = [];
    if (result.patchedPages) bits.push(`${result.patchedPages} page(s) updated`);
    if (result.newPages) bits.push(`${result.newPages} page(s) added`);
    if (result.newObjects) bits.push(`${result.newObjects} object(s)`);
    else if (audit.addedShapes) bits.push(`${audit.addedShapes} new object(s)`);
    app.setStatus(`Saved ${name}${bits.length ? ` — ${bits.join(', ')}` : ''}`);
  } catch (err) {
    alert(`Could not save VSDX: ${err.message}`);
  }
};


app.runCommand = (cmd) => {
  switch (cmd) {
    case 'new': app.newDoc(); break;
    case 'open': app.openFile(); break;
    case 'save': app.saveLocal(); break;
    case 'export-json': exportJSON(app.doc, app.doc.name); break;
    case 'export-svg': exportSVG(app); break;
    case 'export-vsdx': app.saveVSDX(); break;
    case 'insert-image': app.imagePick(); break;
    case 'feature-check': {
      const dialog = document.getElementById('featureDialog');
      if (dialog) {
        if (typeof dialog.showModal === 'function') dialog.showModal();
        else dialog.setAttribute('open', '');
      }
      break;
    }
    case 'export-png': exportPNG(app); break;
    case 'undo': app.undo(); break;
    case 'redo': app.redo(); break;
    case 'grid': app.toggleGrid(); break;
    case 'snap': app.toggleSnap(); break;
    case 'duplicate': app.duplicate(); break;
    case 'group': app.groupSelection(); break;
    case 'ungroup': app.ungroupSelection(); break;
    case 'relayout': app.relayout({ direction: 'vertical' }); break;
    case 'relayout-horizontal': app.relayout({ direction: 'horizontal' }); break;
    case 'delete': app.deleteSelection(); break;
    case 'fill-none': app.applyStyleToSelection({ fill: 'none' }); break;
    case 'bold': app.toggleStyle('bold'); break;
    case 'italic': app.toggleStyle('italic'); break;
    case 'underline': app.toggleStyle('underline'); break;
    case 'zoom-in': app.zoomStep(1.25); break;
    case 'zoom-out': app.zoomStep(0.8); break;
    case 'zoom-fit': app.fitView(); break;
    default: break;
  }
};

// ----------------------------------------------------------------- misc --
const featureDialog = document.getElementById('featureDialog');
if (featureDialog) {
  featureDialog.addEventListener('click', (event) => {
    const action = event.target.closest('[data-feature]')?.dataset.feature;
    if (!action) return;
    featureDialog.close();
    if (action === 'connector') {
      document.querySelector('[data-tool="connector"]')?.click();
    } else if (action === 'picture') {
      app.imagePick();
    } else if (action === 'new') {
      app.newDoc();
    } else if (action === 'export') {
      app.saveVSDX();
    }
  });
}

app.cancelTextEdit = () => {
  const ed = app.editor;
  if (ed && ed.classList.contains('editing')) ed.blur();
};

let saveTimer = null;
function queueSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    if (saveLocal(app.doc)) {
      if (app.state.autosaveWarning) {
        app.state.autosaveWarning = false;
        app.updateStatus();
      }
      return;
    }
    // Pictures are stored as data URLs, so a large document can exceed the
    // browser's storage quota. Do not overwrite the last good autosave.
    if (!app.state.autosaveWarning) {
      app.state.autosaveWarning = true;
      app.updateStatus();
      console.warn('LayoutCraft autosave exceeded browser storage; the previous autosave was kept.');
    }
  }, 700);
}

// ------------------------------------------------------------------ boot --
function boot() {
  app.svg = $('#canvas');
  app.stage = $('#stage');
  app.editor = $('#textEditor');
  app.renderer = new Renderer(app.svg, app);

  const saved = loadLocal();
  if (saved) {
    app.doc = saved;
  } else {
    app.doc = sampleDoc();
  }
  ensureTheme(app.doc);

  // Restore the original Visio package that autosave deliberately leaves out.
  if (saved && saved.visioKey) {
    loadVisioSource(saved.visioKey).then((src) => {
      if (src && app.doc.visioKey === saved.visioKey) app.visioSource = src;
    });
  }

  buildPalette(app);
  buildRecent(app);
  buildPageTabs(app);
  wireToolbar(app);
  initContextMenu(app);
  initInteraction(app);
  updateInspector(app);
  app.render();

  // palette drag & drop
  app.svg.addEventListener('dragover', (e) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  });
  app.svg.addEventListener('drop', (e) => {
    e.preventDefault();
    const file = e.dataTransfer.files && e.dataTransfer.files[0];
    if (file && /^image\//.test(file.type || '')) {
      app.state.imageTarget = null;      // dropping always inserts
      const r = app.svg.getBoundingClientRect();
      const v = app.state.view;
      app.insertImageFile(file, {
        x: (e.clientX - r.left - v.x) / v.zoom,
        y: (e.clientY - r.top - v.y) / v.zoom,
      });
      return;
    }
    const type = e.dataTransfer.getData('application/x-layoutcraft-shape');
    if (!type || !SHAPES[type]) return;
    const r = app.svg.getBoundingClientRect();
    const v = app.state.view;
    app.addShape(type, (e.clientX - r.left - v.x) / v.zoom, (e.clientY - r.top - v.y) / v.zoom);
  });

  // file import
  $('#fileInput').addEventListener('change', async (e) => {
    const f = e.target.files[0];
    if (!f) return;
    try {
      const lower = f.name.toLowerCase();
      const isVisio = /\.(vsdx|vstx|vsdm|vstm|vdx)$/.test(lower);
      const doc = isVisio
        ? await importVisioFile(f)
        : importJSON(await f.text());
      app.history.push(app.doc);
      app.doc = doc;
      ensureTheme(app.doc);
      app.doc.name = f.name.replace(/\.[^.]+$/, '');
      app.visioSource = isVisio && doc._visioPackage
        ? {
          package: doc._visioPackage,
          name: f.name,
          pageParts: doc.pages.map((p) => p.visio?.part).filter(Boolean),
        }
        : null;
      if (app.visioSource) {
        doc.visioKey = uid('v');
        app.visioSource.key = doc.visioKey;
        for (const p of doc.pages) if (p.visio) p.visio.docKey = doc.visioKey;
        await persistVisioSource(doc.visioKey, app.visioSource);
        saveLocal(app.doc);
      }
      app.state.selection.clear();
      app.render();
      app.updateInspector();
      buildPageTabs(app);
      app.fitView();
      const shapes = app.doc.pages.reduce((n, p) => n + p.shapes.length, 0);
      const conns = app.doc.pages.reduce((n, p) => n + p.connectors.length, 0);
      app.setStatus(`Imported ${app.doc.pages.length} page(s): ${shapes} shapes, ${conns} connectors`);
    } catch (err) {
      alert(`Could not open file: ${err.message}`);
    }
    e.target.value = '';
  });

  // picture insertion / replacement
  $('#imageInput').addEventListener('change', async (e) => {
    const f = e.target.files[0];
    if (f) await app.insertImageFile(f);
    e.target.value = '';
  });

  // wheel: ctrl/scroll to zoom, otherwise pan
  app.svg.addEventListener('wheel', (e) => {
    e.preventDefault();
    const r = app.svg.getBoundingClientRect();
    const cx = e.clientX - r.left, cy = e.clientY - r.top;
    if (e.ctrlKey || e.metaKey) {
      const f = Math.pow(1.0015, -e.deltaY);
      app.zoomTo(app.state.view.zoom * f, cx, cy);
    } else if (e.shiftKey) {
      app.state.view.x -= e.deltaY;
      app.renderer.updateView();
    } else {
      app.state.view.x -= e.deltaX;
      app.state.view.y -= e.deltaY;
      app.renderer.updateView();
    }
  }, { passive: false });

  window.addEventListener('beforeunload', () => saveLocal(app.doc));
  app.importFile = importVisioFile;
  window.layoutcraft = app;
}

/** A small starter diagram so the canvas is never empty. */
function sampleDoc() {
  const doc = newDoc('Network Overview');
  const pg = doc.pages[0];
  pg.name = 'Architecture';
  const mk = (type, x, y, w, h, label, over = {}) => {
    const s = newShape(type, x, y, { ...over, w, h, label });
    pg.shapes.push(s);
    return s;
  };
  const client = mk('roundRect', 40, 220, 120, 60, 'Client');
  const router = mk('router', 240, 210, 90, 90, 'Router');
  const firewall = mk('firewall', 410, 200, 100, 100, 'Firewall');
  const server = mk('server', 600, 200, 110, 110, 'Web Server');
  const db = mk('database', 790, 205, 100, 120, 'Database');
  const cloud = mk('cloud', 250, 30, 150, 90, 'Internet', { fill: '#e7e2f7', stroke: '#7b6bb0' });

  const link = (a, b, over = {}) => {
    pg.connectors.push(newConnector(
      { shapeId: a.id, side: 'e' },
      { shapeId: b.id, side: 'w' },
      { stroke: a.stroke, ...over },
    ));
  };
  link(client, router);
  link(router, firewall);
  link(firewall, server);
  link(server, db);
  pg.connectors.push(newConnector(
    { shapeId: router.id, side: 'n' },
    { shapeId: cloud.id, side: 's' },
    { stroke: router.stroke },
  ));
  return doc;
}

boot();
