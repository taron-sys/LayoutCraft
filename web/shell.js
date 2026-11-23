import { loadEngine } from './engine.js';
import { buildPdf } from './pdf.js';
import { svgToPdfPage, Unsupported } from './svgpdf.js';
import { svgToDxf } from './svgdxf.js';
import { importExcalidraw } from './excalidraw.js';
import { exportExcalidraw } from './excalidraw-export.js';
import { exportDrawio } from './drawio-export.js';
import { buildWebPage } from './webpage.js';
import { importDrawio } from './drawio.js';
import { familiesInSvg, loadPdfFonts } from './pdffonts.js';
import { buildZip } from './zip.js';
import { applyBorder, removeBorder, BORDER_STYLES } from './borders.js';
import { checkDiagram, applyFix, issuesToCsv } from './checkdiagram.js';
import { compareModels, diffsToCsv } from './compare.js';
import { inventoryData as inventoryDataOf, inventoryCsv as inventoryCsvOf } from './inventory.js';
import { loadSets, saveSets, setFromRows, applySet } from './datasets.js';
import { mcpConfig } from './mcpinfo.js';

const $ = (id) => document.getElementById(id);
const engine = await loadEngine('./layoutcraft.wasm');
// the application palette (palette.js is loaded in <head> and has already set the class): fill the picker and keep it in step with the saved / system choice
{
  const sel = $('paletteSel'), theme = window.layoutcraftTheme;
  if (sel && theme) {
    for (const p of theme.palettes) { const o = document.createElement('option'); o.value = p.id; o.textContent = p.name; sel.appendChild(o); }
    sel.value = theme.mode;
    sel.addEventListener('change', () => theme.set(sel.value));
    document.addEventListener('layoutcraft-theme', () => { sel.value = theme.mode; });
  }
}
window.layoutcraft = { engine, askFit: true, render: () => render(), get selection() { return selection; }, setSelection: (ids) => setSelection(ids), worldPoint: (id, u, v) => worldPoint(id, u, v), fromClient: (x, y) => fromClient(x, y), toStage: (x, y) => toStage(x, y), toClient: (x, y) => { const [sx, sy] = toStage(x, y); const r = $('stage').getBoundingClientRect(); return { x: sx + r.x - $('stage').scrollLeft, y: sy + r.y - $('stage').scrollTop }; }, connectorAt: (x, y) => connectorAt({ clientX: x, clientY: y }), get connectors() { return connectorMap; } }; // for tests and debugging
let page = 0;
let frames = new Map();
let selected = null; // the single selected shape (handles, size panel); null for 0 or several
let selection = []; // every selected shape id
let armedMaster = null; // the master picked in the Shapes panel for AutoConnect: {master, stencil}
let acShape = null; // AutoConnect: the shape whose arrows are shown
let acTimer = null;
let connectorMap = new Map(); // page-level connectors by id (route and glue), see engine.connectors
const isConnector = (id) => connectorMap.has(id);
let zoom = 1;
let pageH = 11; // page height in inches (the SVG maps page y to (pageH - y) * 96)
let baseWidth = 0; // SVG width in CSS px at 100%
const PX_PER_INCH = 96;

const say = (text = '') => { $('msg').textContent = text; };
const run = (fn) => { try { say(); fn(); } catch (error) { say(error.message); } };
const svgEl = () => $('stage').querySelector('svg');
// A shape's group carries data-vid, a connector's group data-cid (a free line then moves with a live preview).
// The shape a plain click selects: the outermost group around `vid` that is not selected yet, or, when a group around it is selected, the member just below it.
function groupFirst(vid) {
  const chain = [vid];
  for (let up = frames.get(vid)?.parentId, guard = 0; up && frames.has(up) && guard < 64; up = frames.get(up)?.parentId, guard++) chain.push(up);
  if (chain.length === 1) return vid;
  const selected = chain.findIndex((id) => selection.includes(id));
  if (selected === 0) return vid;
  if (selected > 0) return chain[selected - 1];
  return chain[chain.length - 1];
}
// The svg elements that show `ids` while they are dragged: the shapes' own groups plus parts that the svg draws beside them (written-out stencil pictures), none twice.
function previewNodes(ids) {
  const under = (id, root) => { for (let up = frames.get(id)?.parentId, guard = 0; up && guard < 64; up = frames.get(up)?.parentId, guard++) if (up === root) return true; return false; };
  const all = new Set(ids.map(nodeOf).filter(Boolean));
  for (const id of frames.keys()) if (!ids.includes(id) && ids.some((root) => under(id, root))) { const node = nodeOf(id); if (node) all.add(node); }
  return [...all].filter((node) => ![...all].some((other) => other !== node && other.contains(node)));
}
const nodeOf = (id) => id && (document.querySelector(`[data-vid="${CSS.escape(id)}"]`) || document.querySelector(`[data-cid="${CSS.escape(id)}"]`));

// ---- the Pan & Zoom pane: a thumbnail of the whole page with the framed part that the stage shows ----
let pzKey = '';
function renderPanZoom(redraw = true) {
  const pane = $('panZoomPane');
  const on = $('panePanZoom').checked;
  pane.hidden = !on;
  if (!on) { pzKey = ''; return; }
  const svg = svgEl();
  if (!svg) return;
  if (redraw) {
    // the page without the selection handles, as a picture (re-made when the drawing was drawn again)
    const clone = svg.cloneNode(true);
    clone.removeAttribute('style');
    const key = `${page}:${engine.undoDepth()}:${engine.redoDepth()}`;
    if (key !== pzKey || !$('pzImg').src) { pzKey = key; $('pzImg').src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(new XMLSerializer().serializeToString(clone)); }
  }
  const stage = $('stage');
  const sr = svg.getBoundingClientRect(), vr = stage.getBoundingClientRect();
  const w = sr.width || 1, h = sr.height || 1;
  const box = $('pzBox');
  const bw = box.clientWidth || 1;
  const scale = bw / w;
  box.style.height = `${Math.max(10, Math.round(h * scale))}px`;
  const x0 = (vr.left - sr.left) / w, y0 = (vr.top - sr.top) / h;
  const x1 = (vr.right - sr.left) / w, y1 = (vr.bottom - sr.top) / h;
  const clip = (v) => Math.min(1, Math.max(0, v));
  const view = $('pzView');
  view.style.left = `${clip(x0) * 100}%`; view.style.top = `${clip(y0) * 100}%`;
  view.style.width = `${(clip(x1) - clip(x0)) * 100}%`; view.style.height = `${(clip(y1) - clip(y0)) * 100}%`;
}
// centre the stage on the page point under the pointer in the thumbnail; the pointer then drags the frame
function panFromThumb(event) {
  const svg = svgEl();
  if (!svg) return;
  const r = $('pzBox').getBoundingClientRect();
  const fx = Math.min(1, Math.max(0, (event.clientX - r.left) / r.width)), fy = Math.min(1, Math.max(0, (event.clientY - r.top) / r.height));
  const stage = $('stage');
  const sr = svg.getBoundingClientRect(), vr = stage.getBoundingClientRect();
  stage.scrollLeft += sr.left + fx * sr.width - (vr.left + vr.width / 2);
  stage.scrollTop += sr.top + fy * sr.height - (vr.top + vr.height / 2);
}
let pzDrag = false;
$('pzBox').addEventListener('pointerdown', (event) => { pzDrag = true; $('pzBox').setPointerCapture?.(event.pointerId); panFromThumb(event); event.preventDefault(); });
$('pzBox').addEventListener('pointermove', (event) => { if (pzDrag) panFromThumb(event); });
$('pzBox').addEventListener('pointerup', () => { pzDrag = false; });
$('pzBox').addEventListener('wheel', (event) => { event.preventDefault(); setZoom(zoom * (event.deltaY < 0 ? 1.15 : 1 / 1.15)); panFromThumb(event); }, { passive: false });
// zoom by resizing the frame: a handle on each corner of the frame (Visio's Pan & Zoom window works like this); the corner opposite the
// dragged one stays where it is on the page, the stage then shows the framed rectangle
let pzResize = null;
for (const [cx, cy] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
  const handle = document.createElement('div');
  handle.className = 'pzh';
  handle.dataset.corner = `${cx}${cy}`;
  handle.style.cssText = `position:absolute;width:9px;height:9px;background:#d33;border:1px solid #fff;box-sizing:border-box;pointer-events:auto;touch-action:none;left:${cx * 100}%;top:${cy * 100}%;margin:-5px 0 0 -5px;cursor:${cx === cy ? 'nwse-resize' : 'nesw-resize'}`;
  handle.addEventListener('pointerdown', (event) => {
    event.stopPropagation(); event.preventDefault();
    const v = $('pzView').getBoundingClientRect(), b = $('pzBox').getBoundingClientRect();
    // the fixed corner = the opposite one, as a fraction of the thumbnail
    pzResize = { fx: (v.left + (cx ? 0 : v.width) - b.left) / b.width, fy: (v.top + (cy ? 0 : v.height) - b.top) / b.height };
    handle.setPointerCapture?.(event.pointerId);
  });
  handle.addEventListener('pointermove', (event) => { if (pzResize) zoomToThumbRect(pzResize, event); });
  handle.addEventListener('pointerup', () => { pzResize = null; });
  handle.addEventListener('pointercancel', () => { pzResize = null; });
  $('pzView').append(handle);
}
function zoomToThumbRect(fixed, event) {
  const svg = svgEl();
  if (!svg) return;
  const b = $('pzBox').getBoundingClientRect();
  const px = Math.min(1, Math.max(0, (event.clientX - b.left) / b.width)), py = Math.min(1, Math.max(0, (event.clientY - b.top) / b.height));
  const [x0, x1] = [Math.min(fixed.fx, px), Math.max(fixed.fx, px)], [y0, y1] = [Math.min(fixed.fy, py), Math.max(fixed.fy, py)];
  const fw = Math.max(0.02, x1 - x0), fh = Math.max(0.02, y1 - y0);
  const stage = $('stage');
  const sr = svg.getBoundingClientRect(), vr = stage.getBoundingClientRect();
  setZoom(zoom * Math.min(vr.width / (fw * sr.width), vr.height / (fh * sr.height)));
  const after = svg.getBoundingClientRect();
  stage.scrollLeft += after.left + ((x0 + x1) / 2) * after.width - (vr.left + vr.width / 2);
  stage.scrollTop += after.top + ((y0 + y1) / 2) * after.height - (vr.top + vr.height / 2);
}
$('panePanZoom').addEventListener('change', () => renderPanZoom());
$('stage').addEventListener('scroll', () => { if (!$('panZoomPane').hidden) renderPanZoom(false); });

function applyZoom() {
  const svg = svgEl();
  if (!svg) return;
  if (!baseWidth) baseWidth = svg.viewBox.baseVal.width * (96 / PX_PER_INCH);
  svg.style.width = `${baseWidth * zoom}px`;
  svg.style.height = 'auto';
  $('zl').textContent = `${Math.round(zoom * 100)}%`;
  drawOverlay();
  renderPanZoom(false);
}
function setZoom(z) { zoom = Math.min(8, Math.max(0.1, z)); applyZoom(); }
/**
 * Zoom by `factor` keeping the page point under (clientX, clientY) where it is on screen: the stage
 * scrolls by however much the content under that point moved.
 */
function zoomAt(factor, clientX, clientY) {
  const svg = svgEl();
  if (!svg) return;
  const stage = $('stage');
  const before = svg.getBoundingClientRect();
  const fx = (clientX - before.x) / before.width, fy = (clientY - before.y) / before.height;
  setZoom(zoom * factor);
  const after = svg.getBoundingClientRect();
  stage.scrollLeft += after.x + fx * after.width - clientX;
  stage.scrollTop += after.y + fy * after.height - clientY;
  drawOverlay();
}
const stageCentre = () => { const r = $('stage').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; };
$('stage').addEventListener('wheel', (event) => {
  if (!svgEl()) return;
  const zooms = event.ctrlKey || event.metaKey || $('wheelZoom').checked;
  if (!zooms) return; // a plain wheel scrolls the stage as usual
  event.preventDefault();
  const lines = event.deltaMode === 1 ? 33 : event.deltaMode === 2 ? 400 : 1; // lines / pages -> pixels
  zoomAt(Math.exp(-event.deltaY * lines * 0.0015), event.clientX, event.clientY);
}, { passive: false });

// Touch: two fingers pinch to zoom around their midpoint and pan with it (one finger edits like the mouse). The
// second finger cancels whatever the first one had started (a drag, a marquee) without changing the drawing.
const touches = new Map();
let pinch = null;
let selectionBeforeTouch = [];
function pinchState() {
  const [a, b] = [...touches.values()];
  return { dist: Math.hypot(a.x - b.x, a.y - b.y) || 1, cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2 };
}
function cancelDrag() {
  if (!drag) return;
  for (const node of drag.nodes || []) node.style.transform = '';
  clearConnectorPreview(drag);
  $('ov').style.transform = '';
  $('marquee')?.remove();
  drawGuides();
  drag = null;
  drawOverlay();
}
/**
 * Live preview of a move: the connectors glued to the dragged shapes are drawn (blue, dashed) on the route they will have once the shapes are
 * dropped - the engine routes them against the shapes as if moved (`engine.previewMove`, nothing is changed) - and the connector as it is now is
 * hidden meanwhile. Skipped for the rest of the drag when it proves slow (a huge page).
 */
function showConnectorPreview(d, dx, dy) {
  const layer = $('cprev');
  if (!layer || d.noPreview) return;
  const started = performance.now();
  let routes;
  try { routes = engine.previewMove(page, d.ids, dx, dy); } catch { d.noPreview = true; clearConnectorPreview(d); return; }
  if (performance.now() - started > 90) d.noPreview = true;
  d.hidden ??= new Set();
  const keep = new Set(routes.map((r) => r.id));
  layer.innerHTML = routes.map(({ id, route }) => `<polyline class="cpreview" data-for="${String(id).replace(/[^0-9A-Za-z_.-]/g, '')}" points="${route.map(([x, y]) => toStage(x, y).map((v) => v.toFixed(1)).join(',')).join(' ')}"/>`).join('');
  for (const id of keep) { const node = nodeOf(id); if (node) { node.style.visibility = 'hidden'; d.hidden.add(id); } }
  for (const id of [...d.hidden]) if (!keep.has(id)) { const node = nodeOf(id); if (node) node.style.visibility = ''; d.hidden.delete(id); }
}
function clearConnectorPreview(d) {
  const layer = $('cprev');
  if (layer) layer.innerHTML = '';
  for (const id of d?.hidden || []) { const node = nodeOf(id); if (node) node.style.visibility = ''; }
  if (d?.hidden) d.hidden.clear();
}
$('stage').addEventListener('pointerdown', (event) => {
  if (event.pointerType !== 'touch') return;
  if (touches.size === 0) selectionBeforeTouch = [...selection];
  touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
  if (touches.size === 2) { cancelDrag(); setSelection(selectionBeforeTouch.filter((id) => frames.has(id))); pinch = pinchState(); event.stopImmediatePropagation(); event.preventDefault(); }
  else if (touches.size > 2) event.stopImmediatePropagation();
}, true);
window.addEventListener('pointermove', (event) => {
  if (event.pointerType !== 'touch' || !touches.has(event.pointerId)) return;
  touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
  if (!pinch || touches.size < 2) return;
  event.stopImmediatePropagation();
  const now = pinchState();
  zoomAt(now.dist / pinch.dist, now.cx, now.cy);
  const stage = $('stage');
  stage.scrollLeft -= now.cx - pinch.cx;
  stage.scrollTop -= now.cy - pinch.cy;
  pinch = now;
  drawOverlay();
}, true);
const touchEnd = (event) => {
  if (event.pointerType !== 'touch') return;
  const wasPinching = !!pinch;
  touches.delete(event.pointerId);
  if (touches.size < 2) pinch = null;
  if (wasPinching) event.stopImmediatePropagation(); // lifting a finger after a pinch must not act as a click
};
window.addEventListener('pointerup', touchEnd, true);
window.addEventListener('pointercancel', touchEnd, true);
window.addEventListener('pointercancel', (event) => { if (event.pointerType !== 'touch' && drag) cancelDrag(); }); // the browser took the pointer away: no stuck drag

// Pan: drag with the middle button, or with the left button while Space is held.
let spaceDown = false;
let handMode = false; // the Hand tool: a left-button drag pans
const setSpace = (on) => { spaceDown = on; $('stage').style.cursor = on || handMode ? 'grab' : (connectMode ? 'crosshair' : ''); };
function setHandMode(on) {
  handMode = on;
  $('handTool').setAttribute('aria-pressed', String(on)); $('handTool').classList.toggle('active', on);
  $('stage').style.cursor = on ? 'grab' : (connectMode ? 'crosshair' : '');
}
function startPan(event) {
  const stage = $('stage');
  drag = { kind: 'pan', x: event.clientX, y: event.clientY, left: stage.scrollLeft, top: stage.scrollTop };
  stage.style.cursor = 'grabbing';
  event.preventDefault();
}
function movePan(event) {
  const stage = $('stage');
  stage.scrollLeft = drag.left - (event.clientX - drag.x);
  stage.scrollTop = drag.top - (event.clientY - drag.y);
}
$('stage').addEventListener('mousedown', (event) => { if (event.button === 1) event.preventDefault(); }); // no browser auto-scroll
window.addEventListener('keyup', (event) => { if (event.code === 'Space') setSpace(false); });
window.addEventListener('blur', () => setSpace(false));
// Free panning: the page sits inside a margin of about the window size (CSS variables --panx / --pany on the stage, see shell.html), so the
// scroll range allows the page to be anywhere in the window at every zoom, also when it is smaller than the window.
const PAN_KEEP = 90; // px of the page that can still be seen when it is dragged as far as it goes
function updatePanRoom() {
  const stage = $('stage');
  stage.style.setProperty('--panx', `${Math.max(0, stage.clientWidth - PAN_KEEP)}px`);
  stage.style.setProperty('--pany', `${Math.max(0, stage.clientHeight - PAN_KEEP)}px`);
}
/** Put the page in view: centred along an axis where it is smaller than the window, else its start at the window's edge. */
function placePage() {
  const svg = svgEl();
  if (!svg) return;
  const stage = $('stage');
  const sr = svg.getBoundingClientRect(), vr = stage.getBoundingClientRect();
  const inset = 15;
  stage.scrollLeft += sr.width <= stage.clientWidth - 2 * inset ? sr.left + sr.width / 2 - (vr.left + stage.clientWidth / 2) : sr.left - (vr.left + inset);
  stage.scrollTop += sr.height <= stage.clientHeight - 2 * inset ? sr.top + sr.height / 2 - (vr.top + stage.clientHeight / 2) : sr.top - (vr.top + inset);
  drawOverlay();
  renderPanZoom(false);
}
updatePanRoom();
new ResizeObserver(() => updatePanRoom()).observe($('stage'));
function fit() {
  const svg = svgEl();
  if (!svg) return;
  const vb = svg.viewBox.baseVal;
  const room = $('stage');
  setZoom(Math.min((room.clientWidth - 30) / vb.width, (room.clientHeight - 30) / vb.height));
  placePage();
}

/**
 * "Fixed page" view: the SVG's viewBox is the whole page rectangle (0, 0, width, height) instead of the box around
 * the shapes, so the page keeps its place when every shape moves (select-all moves are visible) and the page
 * border is drawn.
 */
function fixPageView() {
  const svg = svgEl();
  // A drawing with no shapes has no content box to fit: its page is shown (and a new drawing starts in this view).
  if (!svg || !($('fixedPage').checked || engine.groups(page).length === 0)) return;
  const { w, h, metric } = engine.pageSize(page);
  syncGridUnit(metric);
  const width = w * PX_PER_INCH, height = h * PX_PER_INCH;
  svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
  svg.setAttribute('width', String(width));
  svg.setAttribute('height', String(height));
  svg.style.overflow = 'visible';
  const sheet = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
  sheet.setAttribute('x', '0'); sheet.setAttribute('y', '0'); sheet.setAttribute('width', String(width)); sheet.setAttribute('height', String(height));
  sheet.setAttribute('fill', $('paperColor').value); sheet.setAttribute('stroke', '#9aa5b1'); sheet.setAttribute('stroke-width', '1'); sheet.setAttribute('id', 'pagesheet');
  const first = [...svg.children].find((c) => c.tagName === 'rect') ?? null;
  svg.insertBefore(sheet, first);
  const ns = 'http://www.w3.org/2000/svg';
  const step = Number($('gridStep').value) * PX_PER_INCH;
  if ($('showGrid').checked && step >= 2) {
    // Grid lines from the page's lower left corner (Visio's grid origin), drawn above the paper, below the shapes.
    const g = document.createElementNS(ns, 'path');
    let d = '';
    for (let x = 0; x <= width + 0.01; x += step) d += `M${x.toFixed(2)} 0V${height}`;
    for (let y = height; y >= -0.01; y -= step) d += `M0 ${y.toFixed(2)}H${width}`;
    g.setAttribute('d', d); g.setAttribute('id', 'pagegrid'); g.setAttribute('stroke', '#c9d3dd'); g.setAttribute('stroke-width', '0.5'); g.setAttribute('fill', 'none');
    g.setAttribute('pointer-events', 'none');
    svg.insertBefore(g, first);
  }
  if ($('showRulers').checked) {
    // Rulers in the page unit along the top and left edge, outside the page (the svg overflows); ticks at the grid step.
    const r = document.createElementNS(ns, 'g');
    r.setAttribute('id', 'pagerulers'); r.setAttribute('pointer-events', 'none'); r.setAttribute('font-size', '8'); r.setAttribute('fill', '#52606d'); r.setAttribute('stroke', '#9aa5b1'); r.setAttribute('stroke-width', '0.5');
    const unitPx = metric ? PX_PER_INCH / 25.4 * 10 : PX_PER_INCH; // one label per cm, or per inch
    const label = (n) => (metric ? `${n}` : `${n}`);
    let out = '';
    for (let i = 0, x = 0; x <= width + 0.01; i++, x = i * unitPx) out += `<line x1="${x.toFixed(2)}" y1="-12" x2="${x.toFixed(2)}" y2="0"/><text stroke="none" x="${(x + 2).toFixed(2)}" y="-3">${label(i)}</text>`;
    for (let i = 0, y = height; y >= -0.01; i++, y = height - i * unitPx) out += `<line x1="-12" y1="${y.toFixed(2)}" x2="0" y2="${y.toFixed(2)}"/><text stroke="none" x="-24" y="${(y - 2).toFixed(2)}">${label(i)}</text>`;
    r.innerHTML = out;
    svg.append(r);
  }
}
const GRID_STEPS = { in: [['0.125', '1/8 in'], ['0.25', '1/4 in'], ['0.5', '1/2 in'], ['1', '1 in']], mm: [['0.1968503937', '5 mm'], ['0.3937007874', '10 mm'], ['0.7874015748', '20 mm'], ['1.1811023622', '30 mm']] };
let gridUnit = 'in';
/** The grid step list follows the page unit (inches, or millimetres for a metric drawing); the nearest step is kept. */
function syncGridUnit(metric) {
  const unit = metric ? 'mm' : 'in';
  if (unit === gridUnit) return;
  gridUnit = unit;
  const select = $('gridStep');
  select.innerHTML = '';
  for (const [value, text] of GRID_STEPS[unit]) select.append(new Option(text, value));
  select.value = GRID_STEPS[unit][1][0];
}
for (const id of ['showGrid', 'showRulers', 'paperColor', 'gridStep']) $(id).addEventListener('change', () => { if (svgEl()) { render(); } });
$('fixedPage').onchange = () => { if (svgEl()) { render(); fit(); } };

/** Screen-reader names for the shapes in the drawing itself: a shape without alt text is named after its text, else "Shape <id>". */
function nameShapes() {
  const nodes = [...$('stage').querySelectorAll('svg g[data-vid]:not([aria-label])')];
  if (!nodes.length) return;
  const labels = new Map();
  try { for (const s of engine.model().pages[page]?.shapes ?? []) if (s.visioId && s.label && s.label.trim()) labels.set(s.visioId, s.label.trim().replace(/\s+/g, ' ').slice(0, 80)); } catch { /* names fall back to the id */ }
  for (const g of nodes) {
    const id = g.getAttribute('data-vid');
    g.setAttribute('role', 'img');
    g.setAttribute('aria-label', labels.get(id) || `Shape ${id}`);
  }
}
let commentCache = null; // the comments of the drawing; emptied by every render()
const linkCache = new Map(); // `${page}:${shape id}` -> hyperlinks; emptied by every render()
function render() {
  buildThemeGallery();
  linkCache.clear();
  commentCache = null;
  const keepZoom = zoom;
  // Undo or redo can remove the page that is shown (undoing an added page): show the last one that is left.
  if (page >= engine.pageCount()) { page = Math.max(0, engine.pageCount() - 1); selection = []; }
  $('stage').innerHTML = engine.svg(page, { print: $('print').checked }) + '<div id="guides"></div><svg id="cprev" aria-hidden="true"></svg><div id="ov"></div><div id="acLayer"></div>';
  acShape = null; // the layer was rebuilt: the arrows are gone
  fixPageView();
  baseWidth = 0;
  nameShapes();
  frames = new Map(engine.groups(page).map((f) => [f.id, f]));
  try { connectorMap = new Map(engine.connectors(page).map((c) => [c.id, c])); } catch { connectorMap = new Map(); }
  pageH = engine.pageSize(page).h;
  worldSiteCache.clear();
  modelCache = null;
  guideCache = null;
  if (selectedGuide && !guidesOf().some((g) => g.id === selectedGuide)) selectedGuide = null;
  syncUndoButtons();
  renderPanZoom();
  $('save').disabled = false;
  syncPageTools();
  zoom = keepZoom;
  applyZoom();
  setSelection(selection.filter((id) => frames.has(id)));
  const tabs = $('tabs');
  tabs.innerHTML = '';
  let pageNames = [];
  try { pageNames = engine.pageNames(); } catch { /* numbered tabs */ }
  for (let i = 0; i < engine.pageCount(); i++) {
    const b = document.createElement('button');
    b.textContent = pageNames[i] || `Page ${i + 1}`;
    b.disabled = i === page;
    b.onclick = () => { page = i; selection = []; selectedGuide = null; render(); fit(); };
    tabs.append(b);
  }
  drawGuides();
  renderPreview();
}

// ---- the second window (View > New Window): the same document in a pane beside the drawing with its own page and zoom. It is read only
// (no selection, no handles) and is drawn again after every render of the main window, so it follows every edit, undo and redo.
let pvPage = 0;
let pvZoom = 1;
const PV_BASE = () => { const svg = $('pvStage').querySelector('svg'); return svg ? svg.viewBox.baseVal.width : 1; };
function pvApplyZoom() {
  const svg = $('pvStage').querySelector('svg');
  if (svg) { svg.style.width = `${PV_BASE() * pvZoom}px`; svg.style.height = 'auto'; }
  $('pvZoomLbl').textContent = `${Math.round(pvZoom * 100)}%`;
}
function pvFit() {
  const svg = $('pvStage').querySelector('svg');
  if (!svg) return;
  const room = $('pvStage'), vb = svg.viewBox.baseVal;
  pvZoom = Math.max(0.05, Math.min(8, Math.min((room.clientWidth - 30) / vb.width, (room.clientHeight - 30) / vb.height)));
  pvApplyZoom();
}
function renderPreview(refit = false) {
  const pane = $('previewPane');
  if (pane.hidden) return;
  const count = engine.pageCount();
  if (pvPage >= count) pvPage = Math.max(0, count - 1);
  let names = [];
  try { names = engine.pageNames(); } catch { /* numbered */ }
  const select = $('pvPage');
  if (select.options.length !== count || [...select.options].some((o, i) => o.textContent !== (names[i] || `Page ${i + 1}`))) {
    select.innerHTML = '';
    for (let i = 0; i < count; i++) { const o = document.createElement('option'); o.value = String(i); o.textContent = names[i] || `Page ${i + 1}`; select.append(o); }
  }
  select.value = String(pvPage);
  const stage = $('pvStage');
  const keep = [stage.scrollLeft, stage.scrollTop];
  // the shape ids are left out: the second window must never be taken for the drawing by the code that looks shapes up by id
  stage.innerHTML = engine.svg(pvPage, { print: false }).replace(/ data-(vid|cid)="[^"]*"/g, '');
  const svg = stage.querySelector('svg');
  if (!svg) return;
  const { w, h } = engine.pageSize(pvPage);
  svg.setAttribute('viewBox', `0 0 ${w * PX_PER_INCH} ${h * PX_PER_INCH}`); // the whole page, as the main window's fixed page view
  svg.removeAttribute('width'); svg.removeAttribute('height');
  if (refit) pvFit(); else pvApplyZoom();
  stage.scrollLeft = keep[0]; stage.scrollTop = keep[1];
}
function setPreview(on) {
  $('previewPane').hidden = !on;
  $('newWindow').setAttribute('aria-pressed', String(on));
  $('newWindow').classList.toggle('active', on);
  if (on) { pvPage = page; renderPreview(true); }
  else $('pvStage').innerHTML = '';
  drawOverlay();
}
$('newWindow').onclick = () => setPreview($('previewPane').hidden);
$('pvClose').onclick = () => setPreview(false);
$('pvPage').onchange = () => { pvPage = Number($('pvPage').value); renderPreview(true); };
$('pvIn').onclick = () => { pvZoom = Math.min(8, pvZoom * 1.25); pvApplyZoom(); };
$('pvOut').onclick = () => { pvZoom = Math.max(0.05, pvZoom / 1.25); pvApplyZoom(); };
$('pvFit').onclick = pvFit;

/** Selection box with eight resize handles, in stage coordinates. */
const HANDLES = [[0, 0], [.5, 0], [1, 0], [0, .5], [1, .5], [0, 1], [.5, 1], [1, 1]];
function boxOf(id) {
  const node = nodeOf(id);
  if (!node) return null;
  const r = node.getBoundingClientRect();
  const s = $('stage').getBoundingClientRect();
  return { x: r.x - s.x + $('stage').scrollLeft, y: r.y - s.y + $('stage').scrollTop, w: r.width, h: r.height };
}
/** Page inches (y up) -> stage pixels, from the SVG's viewBox and the page height. */
function toStage(wx, wy) {
  const svg = svgEl();
  const vb = svg.viewBox.baseVal;
  const r = svg.getBoundingClientRect();
  const s = $('stage').getBoundingClientRect();
  const k = r.width / vb.width;
  return [r.x - s.x + $('stage').scrollLeft + (wx * PX_PER_INCH - vb.x) * k, r.y - s.y + $('stage').scrollTop + ((pageH - wy) * PX_PER_INCH - vb.y) * k];
}
const pxPerInch = () => { const svg = svgEl(); return PX_PER_INCH * svg.getBoundingClientRect().width / svg.viewBox.baseVal.width; };
/** A point (u, v) of shape `id`'s own box (inches, y up) in page inches; `turn` adds an angle to that shape. */
function worldPoint(id, u, v, turn = 0) {
  let x = u, y = v;
  // A mirrored ancestor (a flipped group) turns the other way round: a world turn of the shape is a turn of its angle cell by -turn.
  const sign = turn ? parentsMirror(id) : 1;
  for (let cur = id; cur; cur = frames.get(cur)?.parentId) {
    const f = frames.get(cur);
    if (!f) break;
    const a = f.angle + (cur === id ? turn * sign : 0);
    const sn = Math.sin(a), cs = Math.cos(a);
    // FlipX / FlipY mirror about the shape's pin (loc pin) before it is turned (measured on Visio).
    let dx = x - f.loc[0], dy = y - f.loc[1];
    if (f.flipX) dx = -dx;
    if (f.flipY) dy = -dy;
    x = f.pin[0] + cs * dx - sn * dy;
    y = f.pin[1] + sn * dx + cs * dy;
  }
  return [x, y];
}
/** -1 when the ancestors of a shape mirror it (an odd number of flips between it and the page), else 1. */
function parentsMirror(id) {
  let sign = 1;
  for (let cur = frames.get(id)?.parentId; cur; cur = frames.get(cur)?.parentId) {
    const f = frames.get(cur);
    if (f?.flipX) sign = -sign;
    if (f?.flipY) sign = -sign;
  }
  return sign;
}
/**
 * The page angle of a shape (radians counter-clockwise): the direction its own x axis, turned by its angle cell, takes
 * through the turns and mirrors of its ancestors (a shape's own flip does not count: the box is the same either way).
 */
function worldAngle(id) {
  const f0 = frames.get(id);
  let x = Math.cos(f0?.angle || 0), y = Math.sin(f0?.angle || 0);
  for (let cur = f0?.parentId; cur; cur = frames.get(cur)?.parentId) {
    const f = frames.get(cur);
    if (!f) break;
    if (f.flipX) x = -x;
    if (f.flipY) y = -y;
    const sn = Math.sin(f.angle), cs = Math.cos(f.angle);
    [x, y] = [cs * x - sn * y, sn * x + cs * y];
  }
  return Math.atan2(y, x);
}
/** The page directions of shape `id`'s own x and y axes (unit vectors; the pair is mirrored when the shape or a group above it is flipped). */
function worldAxes(id, turn = 0) {
  const o = worldPoint(id, 0, 0, turn), ex = worldPoint(id, 1, 0, turn), ey = worldPoint(id, 0, 1, turn);
  const norm = (p) => { const dx = p[0] - o[0], dy = p[1] - o[1], n = Math.hypot(dx, dy) || 1; return [dx / n, dy / n]; };
  return { ex: norm(ex), ey: norm(ey) };
}

/**
 * Selection of one shape: an oriented box (it turns with the shape), 8 resize handles on the
 * shape's own edges and a rotation handle above the top edge. `rect` = {u0,v0,u1,v1} previews a
 * resize in the shape's own box coordinates, `turn` previews a rotation about the pin.
 */
function drawOverlay(preview = {}) {
  const ov = $('ov');
  if (!ov) return;
  ov.innerHTML = '';
  addCommentMarkers(ov);
  const add = (box) => {
    const el = document.createElement('div');
    el.className = 'box';
    Object.assign(el.style, { left: `${box.x}px`, top: `${box.y}px`, width: `${box.w}px`, height: `${box.h}px` });
    ov.append(el);
  };
  if (selection.length > 1) { // several shapes: a box around each, no handles
    for (const id of selection) { if (isConnector(id)) drawConnector(ov, id, false); else { const box = boxOf(id); if (box) add(box); } }
    return;
  }
  const id = selected;
  if (id && isConnector(id)) {
    if (editPointsMode && editPoints.id === id) return; // Edit Points: the vertex handles replace the two end handles
    drawConnector(ov, id, true);
    // a one-dimensional GROUP (a stencil rack server) also shows a box around what it draws, next to its two end handles
    if (frames.has(id) && [...frames.values()].some((f) => f.parentId === id)) { const box = boxOf(id); if (box) add(box); }
    return;
  }
  const frame = id && frames.get(id);
  if (!frame) return;
  if (Math.min(frame.size[0], frame.size[1]) < 0.02 && frame.isGroup) { // a flat group (height 0): the box of what it draws, no handles that would squash it
    const box = boxOf(id);
    if (box) add(box);
    addLinkBadge(ov, id);
    if (frame.ends && frame.editable) { // a flat 1-D group that turns by its ends (once turned it is no longer a connector): the two end handles
      ['begin', 'end'].forEach((which, i) => {
        const [x, y] = toStage(frame.ends[i][0], frame.ends[i][1]);
        const hd = document.createElement('div');
        hd.className = 'ch te';
        Object.assign(hd.style, { left: `${x}px`, top: `${y}px` });
        hd.dataset.end = i;
        hd.tabIndex = 0; hd.setAttribute('role', 'button');
        hd.setAttribute('aria-label', `${which === 'begin' ? 'Begin' : 'End'} point handle. Drag, or use the arrow keys (Shift for larger steps), to stretch and turn the shape.`);
        ov.append(hd);
      });
    }
    return;
  }
  const turn = preview.turn || 0;
  const [w, h] = frame.size;
  const r = preview.rect || { u0: 0, v0: 0, u1: w, v1: h };
  const pt = (u, v) => toStage(...worldPoint(id, u, v, turn));
  const axes = worldAxes(id, turn);
  // the box is drawn along the image of the shape's own x axis (with the turn being previewed)
  const theta = Math.atan2(axes.ex[1], axes.ex[0]);
  const [cx, cy] = pt((r.u0 + r.u1) / 2, (r.v0 + r.v1) / 2);
  const ppi = pxPerInch();
  const el = document.createElement('div');
  el.className = 'box';
  Object.assign(el.style, {
    left: `${cx - ((r.u1 - r.u0) * ppi) / 2}px`, top: `${cy - ((r.v1 - r.v0) * ppi) / 2}px`,
    width: `${(r.u1 - r.u0) * ppi}px`, height: `${(r.v1 - r.v0) * ppi}px`, transform: `rotate(${-theta}rad)`,
  });
  ov.append(el);
  addLinkBadge(ov, id);
  if (!frame.editable) return;
  const handle = (cls, x, y, extra = {}) => {
    const hd = document.createElement('div');
    hd.className = cls;
    Object.assign(hd.style, { left: `${x}px`, top: `${y}px`, ...extra });
    ov.append(hd);
    return hd;
  };
  if (frame.ends) { // a TALL 1-D group (a PDU): two end handles; dragging one stretches and turns it about the middle of its Begin-End line (Visio)
    ['begin', 'end'].forEach((which, i) => {
      const [x, y] = toStage(frame.ends[i][0], frame.ends[i][1]);
      const hd = handle('ch te', x, y);
      hd.dataset.end = i;
      hd.tabIndex = 0; hd.setAttribute('role', 'button');
      hd.setAttribute('aria-label', `${which === 'begin' ? 'Begin' : 'End'} point handle. Drag, or use the arrow keys (Shift for larger steps), to stretch and turn the shape.`);
    });
    return;
  }
  for (const [fx, fy] of HANDLES) { // fx 0..1 left to right, fy 0..1 top to bottom
    const [x, y] = pt(r.u0 + (r.u1 - r.u0) * fx, r.v0 + (r.v1 - r.v0) * (1 - fy));
    const hd = handle('h', x, y);
    hd.dataset.fx = fx; hd.dataset.fy = fy;
    hd.tabIndex = 0; hd.setAttribute('role', 'button');
    hd.setAttribute('aria-label', `Resize handle, ${fy === 0 ? 'top' : fy === 1 ? 'bottom' : 'middle'} ${fx === 0 ? 'left' : fx === 1 ? 'right' : 'centre'}. Arrow keys resize, Shift for larger steps.`);
    const diagonal = fx !== 0.5 && fy !== 0.5;
    // Pick the resize cursor that is closest to the handle's direction on screen.
    const dir = Math.atan2(-((fy === 0.5 ? 0 : 0.5 - fy)), fx === 0.5 ? 0 : fx - 0.5) + theta;
    const octant = ((Math.round(dir / (Math.PI / 4)) % 4) + 4) % 4;
    hd.style.cursor = ['ew', 'nesw', 'ns', 'nwse'][octant] + '-resize';
    if (diagonal) hd.dataset.corner = '1';
  }
  const [tx, ty] = pt((r.u0 + r.u1) / 2, r.v1);
  const up = [axes.ey[0], -axes.ey[1]]; // the shape's "up" (its own +y) on screen
  const rotHandle = handle('rot', tx + up[0] * 24, ty + up[1] * 24);
  rotHandle.tabIndex = 0; rotHandle.setAttribute('role', 'button');
  rotHandle.setAttribute('aria-label', 'Rotate handle. Left and right arrow keys turn the shape by 5 degrees, Shift for 15.');
  const stem = document.createElement('div');
  stem.className = 'stem';
  Object.assign(stem.style, { left: `${tx}px`, top: `${ty}px`, height: '24px', transform: `rotate(${Math.PI - theta}rad)` });
  ov.append(stem);
}

/** The route of connector `id` in stage pixels. */
const connectorPath = (id) => connectorMap.get(id).route.map(([x, y]) => toStage(x, y));
/** A selected connector: its path highlighted, and (alone) a handle on each end (red = glued, green = loose). */
function drawConnector(ov, id, handles) {
  const path = connectorPath(id);
  for (let i = 1; i < path.length; i++) {
    const [x0, y0] = path[i - 1], [x1, y1] = path[i];
    const seg = document.createElement('div');
    seg.className = 'cseg';
    Object.assign(seg.style, { left: `${x0}px`, top: `${y0}px`, width: `${Math.hypot(x1 - x0, y1 - y0)}px`, transform: `rotate(${Math.atan2(y1 - y0, x1 - x0)}rad)` });
    ov.append(seg);
  }
  if (!handles) return;
  const c = connectorMap.get(id);
  [['begin', path[0]], ['end', path[path.length - 1]]].forEach(([which, [x, y]], end) => {
    const hd = document.createElement('div');
    hd.className = c[which].glue ? 'ch glued' : 'ch';
    hd.dataset.end = end;
    hd.tabIndex = 0;
    hd.setAttribute('aria-label', `${end ? 'End' : 'Begin'} point of the line (arrow keys move a loose end)`);
    Object.assign(hd.style, { left: `${x}px`, top: `${y}px` });
    ov.append(hd);
  });
}
/** The connector within 6 px of the pointer (the nearest one), or null. */
function connectorAt(event, tolerance = 6) {
  const at = stagePoint(event);
  let best = null;
  for (const id of connectorMap.keys()) {
    const path = connectorPath(id);
    for (let i = 1; i < path.length; i++) {
      const [x0, y0] = path[i - 1], [x1, y1] = path[i];
      const len2 = (x1 - x0) ** 2 + (y1 - y0) ** 2;
      const t = len2 ? Math.max(0, Math.min(1, ((at.x - x0) * (x1 - x0) + (at.y - y0) * (y1 - y0)) / len2)) : 0;
      const d = Math.hypot(at.x - (x0 + t * (x1 - x0)), at.y - (y0 + t * (y1 - y0)));
      if (d <= tolerance && (!best || d < best.d)) best = { id, d };
    }
  }
  return best && best.id;
}

/** The selection without shapes whose ancestor is selected too (they move/delete with it). */
function topmost(ids) {
  const set = new Set(ids);
  const covered = (id) => { for (let p = frames.get(id)?.parentId; p; p = frames.get(p)?.parentId) if (set.has(p)) return true; return false; };
  return ids.filter((id) => (frames.get(id)?.editable || isConnector(id)) && !covered(id));
}
/** A line (a 1-D shape: a drawn line, an arrow line of a stencil) with BOTH ends loose: it moves like any shape. */
const isFreeLine = (id) => { const c = connectorMap.get(id); return !!c && !c.begin.glue && !c.end.glue; };
/** What can be moved or nudged: not connectors with a glued end (that end follows its shape), but free lines. */
const movable = () => topmost(selection).filter((id) => (!isConnector(id) || isFreeLine(id)) && !frames.get(id)?.locked); // a locked shape does not move (Visio protection)

/** Shapes on a locked layer cannot be selected (Visio: the layer's Lock flag); read from the model of the shown page. */
function lockedLayerSet() { return new Set(layerList().filter((l) => l.locked).map((l) => l.index)); }
function isLocked(id) {
  const locked = lockedLayerSet();
  if (!locked.size) return false;
  const shape = (engine.model().pages[page]?.shapes || []).find((s) => s.visioId === id);
  return !!shape && (shape.layers || []).some((n) => locked.has(n));
}
// Format painter: the look of the one selected shape (read from the format panel), applied to the next shape clicked.
let painter = null;
function setPainter(next) { painter = next; $('painter').classList.toggle('active', !!next); document.body.style.cursor = next ? 'copy' : ''; }
// Undo / redo buttons show how many steps they can still take (the history depth).
function syncUndoButtons() {
  const [u, r] = [engine.undoDepth(), engine.redoDepth()];
  $('undo').disabled = u === 0; $('redo').disabled = r === 0;
  // the label goes into the hidden text span next to the icon (ribbon.js), so the icon stays; the tooltip shows the depth too
  const label = (button, text) => { (button.querySelector(':scope > .rhide, :scope > .rtext') || button).textContent = text; button.title = text; button.setAttribute('aria-label', text); };
  label($('undo'), u ? `Undo (${u})` : 'Undo'); label($('redo'), r ? `Redo (${r})` : 'Redo');
}
function setSelection(ids) {
  if (ids.length && lockedLayerSet().size) ids = ids.filter((id) => !isLocked(id));
  if (painter && ids.length && !ids.includes(painter.source)) { const style = painter.style; setPainter(null); const target = ids.slice(); queueMicrotask(() => { try { run(() => { engine.styleShapes(page, topmost(target), style); render(); }); } catch (error) { say(error.message); } }); }
  selection = ids;
  queueMicrotask(() => { renderCommentsPane(); renderSheetPane(); syncClipboardButtons(); updateFormatPanel(); updateSizePos(); $('insertHyperlink').disabled = selection.length !== 1 || !frames.has(selection[0]) && !isConnector(selection[0]); });
  selected = ids.length === 1 ? ids[0] : null;
  const frame = selected && frames.get(selected);
  if (!ids.length) { $('del').disabled = true; $('sel').textContent = 'Nothing selected'; $('panel').hidden = true; drawOverlay(null); return; }
  if (ids.length > 1) {
    $('sel').textContent = `${ids.length} shapes selected`;
    $('panel').hidden = true;
    $('del').disabled = topmost(ids).length === 0;
    drawOverlay();
    return;
  }
  if (isConnector(selected)) {
    const c = connectorMap.get(selected);
    const end = (e) => (e.glue ? `glued to ${e.glue.sheet}${e.glue.n ? ` point ${e.glue.n}` : ''}` : 'loose');
    $('sel').textContent = `Connector ${selected}: begin ${end(c.begin)}, end ${end(c.end)}`;
    $('panel').hidden = true;
    $('del').disabled = false;
    drawOverlay();
    return;
  }
  $('sel').textContent = `Shape ${selected}${frame.isGroup ? ' (group)' : ''}${frame.parentId ? ` in ${frame.parentId}` : ''}${frame.editable ? '' : ' (read-only: flipped)'}`;
  $('panel').hidden = !(frame.isGroup && frame.editable);
  $('del').disabled = !frame.editable;
  if (frame.isGroup) { $('w').value = frame.size[0].toFixed(3); $('h').value = frame.size[1].toFixed(3); }
  drawOverlay();
}
const select = (id) => setSelection(id ? [id] : []);

// ---- Size & Position: X, Y, width, height and angle of the selected shape, and which point of it X and Y mean ---------------------
// The numbers are in the page unit (inches, or millimetres on a metric page). Width / height keep the chosen point where it is,
// X / Y move the shape so that the chosen point lands there, the angle turns the shape about its pin (Visio's rule).
const SP_FIELDS = ['spX', 'spY', 'spW', 'spH', 'spA'];
let spUnit = { k: 1, label: 'in', decimals: 3 };
const spPoint = () => $('spPin').value.split(',').map(Number); // fractions of the shape's own box, y up
function updateSizePos() {
  const id = selection.length === 1 && !isConnector(selection[0]) ? selection[0] : null;
  const frame = id && frames.get(id);
  $('sizePos').hidden = !frame || !$('paneSizePos').checked;
  if (!frame) return;
  const metric = engine.pageSize(page).metric;
  spUnit = metric ? { k: 25.4, label: 'mm', decimals: 2 } : { k: 1, label: 'in', decimals: 4 };
  for (const el of document.querySelectorAll('#sizePos .spUnit')) el.textContent = spUnit.label;
  const [u, v] = spPoint(), [w, h] = frame.size;
  const [x, y] = worldPoint(id, u * w, v * h);
  const fmt = (n) => String(Math.round(n * spUnit.k * 10 ** spUnit.decimals) / 10 ** spUnit.decimals);
  $('spX').value = fmt(x); $('spY').value = fmt(y); $('spW').value = fmt(w); $('spH').value = fmt(h);
  let deg = (worldAngle(id) * 180) / Math.PI;
  deg = Math.round((((deg % 360) + 360) % 360) * 1000) / 1000;
  $('spA').value = String(deg === 360 ? 0 : deg);
  const free = frame.editable && !frame.locked;
  for (const f of SP_FIELDS) $(f).disabled = !free;
  $('sizePos').title = free ? '' : frame.locked ? 'The shape is locked' : 'This shape cannot be edited (flipped group)';
}
function applySizePos(field) {
  const id = selected, frame = id && frames.get(id);
  if (!frame || !frame.editable || frame.locked) return;
  const value = Number($(field).value);
  if ($(field).value.trim() === '' || !Number.isFinite(value)) { updateSizePos(); return; }
  const [u, v] = spPoint(), [w, h] = frame.size;
  run(() => {
    if (field === 'spX' || field === 'spY') {
      const [cx, cy] = worldPoint(id, u * w, v * h);
      const target = value / spUnit.k;
      engine.moveMembers(page, [id], field === 'spX' ? target - cx : 0, field === 'spY' ? target - cy : 0, $('refit').checked);
    } else if (field === 'spW' || field === 'spH') {
      const size = value / spUnit.k;
      if (!(size >= 0.05)) throw new Error('A shape must be at least 0.05 in wide and high');
      engine.resizeMember(page, id, field === 'spW' ? size : w, field === 'spH' ? size : h, [u, v], $('refit').checked);
    } else {
      engine.rotateShape(page, id, ((((value % 360) + 360) % 360) * Math.PI) / 180);
    }
    render();
  });
  updateSizePos();
}
for (const f of SP_FIELDS) $(f).addEventListener('change', () => applySizePos(f));
$('spPin').addEventListener('change', updateSizePos);

// ---- stencil panel: the masters of the open document -------------------------------------
let masterList = [];
function loadMasters() {
  try { masterList = engine.masters(); } catch { masterList = []; }
  for (const key of [...thumbs.keys()]) if (key.startsWith('doc:')) thumbs.delete(key); // the drawing's own masters changed
  renderMasters();
}
// External stencils (.vssx / .vstx) opened next to the drawing: {index, name, masters}. The Shapes panel shows the
// masters of the drawing or of one stencil; a stencil master is copied into the drawing when it is first dropped.
const stencils = [];
const thumbs = new Map(); // "index:master" -> data URL of its thumbnail SVG
const thumbObserver = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    thumbObserver.unobserve(entry.target);
    const li = entry.target;
    const key = `${li.dataset.stencil}:${li.dataset.master}`;
    if (!thumbs.has(key)) {
      let url = '';
      try {
        const svgText = li.dataset.stencil === 'doc' ? engine.masterThumb(li.dataset.master) : engine.stencilThumb(Number(li.dataset.stencil), li.dataset.master);
        url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svgText)}`;
      } catch { /* no thumbnail */ }
      thumbs.set(key, url);
    }
    const url = thumbs.get(key);
    const slot = li.querySelector('.thumb');
    if (slot && url) { const img = document.createElement('img'); img.src = url; img.alt = ''; slot.append(img); }
  }
}, { root: $('masters').parentElement, rootMargin: '120px' });
function currentMasters() {
  const source = $('stencilSource').value;
  return source === 'doc' ? masterList : stencils.find((s) => String(s.index) === source)?.masters ?? [];
}
// Favourite shapes: starred masters, remembered in the browser (a per-viewer convenience, best effort) by "source name :: master name".
const FAV_KEY = 'layoutcraft-favourite-shapes';
const loadFavourites = () => { try { return new Set(JSON.parse(localStorage.getItem(FAV_KEY) || '[]')); } catch { return new Set(); } };
let favourites = loadFavourites();
const saveFavourites = () => { try { localStorage.setItem(FAV_KEY, JSON.stringify([...favourites])); } catch { /* not remembered */ } };
const favKey = (sourceName, master) => `${sourceName}::${master.name || `Master ${master.id}`}`;
/** Every source of shapes: the drawing's own masters and each opened stencil. */
const allSources = () => [{ key: 'doc', name: 'This drawing', masters: masterList }, ...stencils.map((s) => ({ key: String(s.index), name: s.name, masters: s.masters }))];
/** The masters the panel lists: the favourites, a search over every source, or the shown source (optionally filtered). */
function listedMasters() {
  const source = $('stencilSource').value;
  const filter = $('stencilFilter').value.trim().toLowerCase();
  const match = (m) => !filter || (m.name || '').toLowerCase().includes(filter);
  const out = [];
  if (source === 'fav' || (filter && $('stencilAll').checked)) {
    for (const s of allSources()) for (const m of s.masters) if (match(m) && (source !== 'fav' || favourites.has(favKey(s.name, m)))) out.push({ master: m, key: s.key, name: s.name });
    return { entries: out, crossSource: true };
  }
  const s = allSources().find((x) => x.key === source);
  for (const m of s?.masters ?? []) if (match(m)) out.push({ master: m, key: s.key, name: s.name });
  return { entries: out, crossSource: false };
}
function renderMasters() {
  const list = $('masters');
  const source = $('stencilSource').value;
  list.innerHTML = '';
  const { entries, crossSource } = listedMasters();
  for (const { master, key, name } of entries) {
    const li = document.createElement('li');
    li.dataset.master = master.id;
    li.dataset.name = master.name || `Master ${master.id}`;
    // Thumbnails for the masters of a stencil and for those of the drawing itself (key "doc:<id>", cleared when the masters change).
    li.dataset.stencil = key;
    const slot = document.createElement('span');
    slot.className = 'thumb';
    li.append(slot);
    const label = document.createElement('span');
    label.textContent = master.name || `Master ${master.id}`;
    if (crossSource) label.dataset.src = name; // shown under the name by CSS, so the item's text stays the master's name
    li.append(label);
    const star = document.createElement('button');
    const on = favourites.has(favKey(name, master));
    star.className = `fav${on ? ' on' : ''}`;
    star.title = on ? 'Remove from the favourites' : 'Add to the favourites';
    star.setAttribute('aria-label', `${on ? 'Remove from' : 'Add to'} the favourites: ${master.name || `Master ${master.id}`}`);
    star.setAttribute('aria-pressed', String(on));
    star.dataset.fav = favKey(name, master);
    li.append(star);
    li.title = `${master.name} (${master.w.toFixed(2)} x ${master.h.toFixed(2)} in): drag onto the page, or double-click to place it in the middle of the view`;
    list.append(li);
  }
  for (const li of list.children) { thumbObserver.observe(li); li.classList.toggle('armed', !!armedMaster && li.dataset.master === armedMaster.master && li.dataset.stencil === armedMaster.stencil); }
  const filtering = $('stencilFilter').value.trim() !== '';
  $('stencilHint').textContent = entries.length ? '' : source === 'fav' ? (filtering ? 'No favourite matches.' : 'No favourites yet: click the star of a shape.') : filtering ? 'No shape matches.' : source === 'doc' ? 'This drawing has no masters.' : 'This stencil has no masters.';
}
$('masters').addEventListener('click', (event) => {
  const star = event.target.closest('.fav');
  if (!star) return;
  event.stopPropagation();
  const key = star.dataset.fav;
  if (favourites.has(key)) favourites.delete(key); else favourites.add(key);
  saveFavourites();
  renderMasters();
});
// A click on a shape of the panel picks it for AutoConnect (a second click lets it go).
$('masters').addEventListener('click', (event) => {
  if (event.target.closest('.fav')) return;
  const li = event.target.closest('li');
  if (!li) return;
  const same = !!armedMaster && armedMaster.master === li.dataset.master && armedMaster.stencil === li.dataset.stencil;
  armedMaster = same ? null : { master: li.dataset.master, stencil: li.dataset.stencil };
  for (const e of $('masters').children) e.classList.toggle('armed', !!armedMaster && e.dataset.master === armedMaster.master && e.dataset.stencil === armedMaster.stencil);
  say(armedMaster ? `AutoConnect will place "${li.dataset.name}": point at a shape and click an arrow.` : 'AutoConnect will copy the shape you point at.');
});
$('stencilFilter').oninput = renderMasters;
$('stencilAll').onchange = renderMasters;
$('stencilSource').onchange = renderMasters;
$('addStencil').onclick = () => $('stencilFile').click();
/** Register the built-in "Basic shapes" stencil as a source (once); `select` makes it the shown source. */
function useBuiltinStencil(select) {
  try {
    const opened = engine.openBuiltinStencil();
    if (!stencils.some((s) => s.index === opened.index)) {
      stencils.push({ index: opened.index, name: 'Basic shapes', masters: opened.masters });
      $('stencilSource').append(new Option('Basic shapes', String(opened.index)));
    }
    if (select) { $('stencilSource').value = String(opened.index); renderMasters(); }
  } catch (error) { say(error.message); }
}
useBuiltinStencil(false);
// Opened stencils are remembered across a page reload: their bytes are kept in IndexedDB (best effort, a browser
// without it just forgets them) and opened again at start-up; "Close stencil" forgets the shown one.
const stencilStore = {
  db: null,
  async open() {
    if (this.db) return this.db;
    this.db = await new Promise((resolve, reject) => {
      const req = indexedDB.open('layoutcraft-stencils', 1);
      req.onupgradeneeded = () => req.result.createObjectStore('stencils', { keyPath: 'name' });
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
    return this.db;
  },
  async run(mode, work) {
    const db = await this.open();
    return new Promise((resolve, reject) => {
      const tx = db.transaction('stencils', mode);
      const result = work(tx.objectStore('stencils'));
      tx.oncomplete = () => resolve(result?.result);
      tx.onerror = () => reject(tx.error);
    });
  },
  put(name, bytes) { return this.run('readwrite', (st) => st.put({ name, bytes })); },
  remove(name) { return this.run('readwrite', (st) => st.delete(name)); },
  all() { return this.run('readonly', (st) => st.getAll()); },
};
function addOpenedStencil(name, opened, select) {
  stencils.push({ index: opened.index, name, masters: opened.masters });
  $('stencilSource').append(new Option(name, String(opened.index)));
  if (select) { $('stencilSource').value = String(opened.index); renderMasters(); }
}
(async () => {
  try {
    for (const saved of await stencilStore.all()) {
      try { addOpenedStencil(saved.name, engine.openStencil(saved.bytes), false); } catch { await stencilStore.remove(saved.name); }
    }
  } catch { /* no IndexedDB: nothing remembered */ }
})();
$('stencilFile').onchange = async (event) => {
  const file = event.target.files[0];
  event.target.value = '';
  if (!file) return;
  try {
    say('Opening the stencil...');
    const bytes = await file.arrayBuffer();
    const opened = engine.openStencil(bytes);
    addOpenedStencil(file.name, opened, true);
    stencilStore.put(file.name, bytes).catch(() => {});
    say();
  } catch (error) { say(error.message); }
};
$('closeStencil').onclick = () => {
  const index = $('stencilSource').value;
  const entry = stencils.find((s) => String(s.index) === index);
  if (!entry || entry.name === 'Basic shapes') { say('Select an opened stencil file to close it.'); return; }
  engine.closeStencil(entry.index);
  stencils.splice(stencils.indexOf(entry), 1);
  [...$('stencilSource').options].find((o) => o.value === index)?.remove();
  stencilStore.remove(entry.name).catch(() => {});
  $('stencilSource').value = 'doc';
  renderMasters();
  say(`Closed ${entry.name}.`);
};

/** Page inches (y up) under a point of the browser window, the inverse of `toStage`. */
function fromClient(clientX, clientY) {
  const svg = svgEl();
  const vb = svg.viewBox.baseVal;
  const r = svg.getBoundingClientRect();
  const k = r.width / vb.width;
  return [((clientX - r.x) / k + vb.x) / PX_PER_INCH, pageH - ((clientY - r.y) / k + vb.y) / PX_PER_INCH];
}
function dropMaster(masterId, wx, wy, stencil) {
  if ($('snapGrid').checked) { const g = Number($('gridStep').value); wx = Math.round(wx / g) * g; wy = Math.round(wy / g) * g; }
  run(() => {
    const id = stencil === undefined ? engine.addMaster(page, masterId, wx, wy) : engine.dropStencilMaster(page, Number(stencil), masterId, wx, wy);
    if (stencil !== undefined) masterList = engine.masters(); // the drawing has the master now
    selection = [id];
    render();
    offerFit(id);
  });
}
/** A dropped shape larger than the page (a 72 in rack PDU on a letter page): ask whether to shrink it, in proportion, to fit the page. */
function offerFit(id) {
  const frame = engine.groups(page).find((f) => f.id === id);
  const size = engine.pageSize(page);
  if (!window.layoutcraft.askFit || !frame || frame.angle || !(frame.size[0] > size.w + 1e-6 || frame.size[1] > size.h + 1e-6)) return;
  const unit = size.metric ? 'mm' : 'in';
  const show = (v) => (size.metric ? Math.round(v * 25.4) : Math.round(v * 100) / 100);
  const k = Math.min((size.w * 0.9) / frame.size[0], (size.h * 0.9) / frame.size[1]);
  $('fitText').textContent = `It is ${show(frame.size[0])} x ${show(frame.size[1])} ${unit}, the page ${show(size.w)} x ${show(size.h)} ${unit}. Shrink it to ${Math.round(k * 100)} % of its size (same proportions, centred on the page)?`;
  $('fitKeep').onclick = (event) => { event.preventDefault(); $('fitDialog').close(); };
  $('fitForm').onsubmit = (event) => {
    event.preventDefault();
    $('fitDialog').close();
    run(() => {
      const w = frame.size[0] * k, h = frame.size[1] * k;
      engine.resizeGroup(page, id, w, h);
      const now = engine.groups(page).find((f) => f.id === id);
      engine.moveMembers(page, [id], size.w / 2 - (now.pin[0] - now.loc[0] + now.size[0] / 2), size.h / 2 - (now.pin[1] - now.loc[1] + now.size[1] / 2));
      selection = [id];
      render();
      say(`Shrunk to ${Math.round(k * 100)} % to fit the page (Undo restores the size).`);
    });
  };
  $('fitDialog').showModal();
}
// Design > Themes (round 12, Q4; Visio: Design > Themes gallery): one button per gallery theme with its six accent colours; a click applies the theme to
// the whole drawing (the theme part and the page sheets' scheme cells, see themes.rs); one undo step. Shapes keep their theme formulas, so they take the new colours.
let themeAllPages = false; // Design > Themes: false = the current page only (Visio's default), true = every page
function buildThemeGallery() {
  const box = $('themeGallery');
  if (!box) return;
  let info;
  try { info = engine.themes(); } catch { return; }
  // the theme of the SHOWN page: its own ThemeIndex when some page states one (Visio leaves the cell out of a page without a theme), else the drawing's single theme
  const pages = info.pages || [];
  const pageTheme = pages[page];
  const anyStated = pages.some((p) => p.theme != null && p.theme !== 0);
  const curName = anyStated ? (info.themes.find((t) => t.index === (pageTheme && pageTheme.theme))?.name || null) : info.current;
  const curVariant = anyStated ? (pageTheme ? pageTheme.variant : 0) : info.variant;
  const sig = `${curName}|${curVariant}|${info.themes.length}|${page}|${themeAllPages}|${pages.length}`;
  if (box.dataset.sig === sig) return;
  box.dataset.sig = sig;
  box.innerHTML = '';
  const all = document.createElement('label');
  all.className = 'themeAll';
  const allBox = document.createElement('input');
  allBox.type = 'checkbox';
  allBox.id = 'themeAllPages';
  allBox.checked = themeAllPages;
  allBox.onchange = () => { themeAllPages = allBox.checked; box.dataset.sig = ''; buildThemeGallery(); };
  all.append(allBox, document.createTextNode(' Apply to all pages'));
  box.append(all);
  for (const t of info.themes) {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = `themeBtn${t.name === curName ? ' on' : ''}`;
    b.dataset.theme = t.name;
    b.setAttribute('aria-pressed', String(t.name === curName));
    b.title = `${t.name} (fonts ${t.fonts.major || '-'} / ${t.fonts.minor || '-'})`;
    const sw = document.createElement('span');
    sw.className = 'sw';
    for (const c of t.accents) { const i = document.createElement('i'); i.style.background = `#${c}`; sw.append(i); }
    const label = document.createElement('span');
    label.textContent = t.name;
    b.append(sw, label);
    b.onclick = () => run(() => {
      if (themeAllPages) engine.applyTheme(page, t.name); else engine.applyThemeToPage(page, t.name);
      render();
      say(themeAllPages ? `Theme ${t.name} applied to all pages` : `Theme ${t.name} applied to this page`);
    });
    box.append(b);
  }
  // Design > Variants: the four colour variations of the drawing's theme (page sheet VariationColorIndex / VariationStyleIndex), each shown by its first two colours
  const vbox = $('variantGallery');
  if (vbox) {
    vbox.innerHTML = '';
    const theme = info.themes.find((t) => t.name === curName);
    for (let v = 0; v < 4; v++) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = `themeBtn${theme && curVariant === v ? ' on' : ''}`;
      b.dataset.variant = String(v);
      b.disabled = !theme;
      b.setAttribute('aria-pressed', String(!!theme && curVariant === v));
      b.title = theme ? `${theme.name}: variant ${v + 1}` : 'Apply a theme first (Design > Themes)';
      const sw = document.createElement('span');
      sw.className = 'sw';
      for (const c of (theme?.variants?.[v] || [])) { const i = document.createElement('i'); i.style.background = `#${c}`; sw.append(i); }
      const label = document.createElement('span');
      label.textContent = `Variant ${v + 1}`;
      b.append(sw, label);
      b.onclick = () => run(() => { if (themeAllPages) engine.setThemeVariant(page, v); else engine.setThemeVariantOnPage(page, v); render(); say(`Variant ${v + 1} of ${theme.name}`); });
      vbox.append(b);
    }
  }
}
// Change Shape (round 11b, SHAPE-17; Visio: Home > Editing > Change Shape): with one shape selected the button waits for a click on a shape of the Shapes panel
let editPointsMode = false; // Edit Points (declared early: updateButtons reads it)
let editPoints = { id: null, v: null, b: null }; // the shape, the selected vertex (index or null) and the selected arc handle (segment or null)
let changeShapeMode = false;
function setChangeShapeMode(on) {
  changeShapeMode = on;
  $('changeShapeBtn').classList.toggle('active', on);
  $('changeShapeBtn').setAttribute('aria-pressed', String(on));
  if (on) say('Change Shape: click a shape in the Shapes panel (Esc cancels).');
}
$('changeShapeBtn').onclick = () => { if (!$('changeShapeBtn').disabled) setChangeShapeMode(!changeShapeMode); };
/** The selected shapes Change Shape can work on: plain page-level shapes (no connectors, no group members), editable and not locked. */
const changeableShapes = () => turnable().filter((id) => !connectorMap.has(id) && !frames.get(id)?.parentId && !!frames.get(id)?.editable && !isLocked(id));
function changeSelectedShape(master) {
  const ids = changeableShapes();
  setChangeShapeMode(false);
  if (!ids.length) return;
  run(() => {
    const created = [];
    engine.batch(() => { for (const id of ids) created.push(engine.replaceShape(page, id, master.id, master.stencil === undefined ? -1 : Number(master.stencil))); }); // one undo step for all
    if (master.stencil !== undefined) masterList = engine.masters(); // the drawing has the master now
    selection = created;
    render();
    say(ids.length > 1 ? `Changed ${ids.length} shapes to ${master.name}` : `Changed to ${master.name}`);
  });
}
let masterDrag = null;
$('masters').addEventListener('pointerdown', (event) => {
  const li = event.target.closest('li');
  if (!li || !svgEl() || event.target.closest('.fav')) return;
  masterDrag = { id: li.dataset.master, stencil: li.dataset.stencil === 'doc' ? undefined : li.dataset.stencil, name: li.dataset.name || li.textContent, x: event.clientX, y: event.clientY, ghost: null };
  event.preventDefault();
});
window.addEventListener('pointermove', (event) => {
  if (!masterDrag) return;
  if (!masterDrag.ghost && Math.hypot(event.clientX - masterDrag.x, event.clientY - masterDrag.y) > 4) {
    masterDrag.ghost = document.createElement('div');
    masterDrag.ghost.id = 'dragghost';
    masterDrag.ghost.textContent = masterDrag.name;
    document.body.append(masterDrag.ghost);
  }
  if (masterDrag.ghost) Object.assign(masterDrag.ghost.style, { left: `${event.clientX + 10}px`, top: `${event.clientY + 6}px` });
});
window.addEventListener('pointerup', (event) => {
  if (!masterDrag) return;
  const d = masterDrag; masterDrag = null;
  d.ghost?.remove();
  if (!d.ghost) { if (changeShapeMode) changeSelectedShape(d); return; } // a plain click (in the Change Shape mode: the new shape)
  const over = document.elementFromPoint(event.clientX, event.clientY);
  if (over && $('stage').contains(over)) { const [wx, wy] = fromClient(event.clientX, event.clientY); dropMaster(d.id, wx, wy, d.stencil); }
});
$('masters').addEventListener('dblclick', (event) => {
  const li = event.target.closest('li');
  if (!li || !svgEl()) return;
  const r = $('stage').getBoundingClientRect();
  const [wx, wy] = fromClient(r.x + r.width / 2, r.y + r.height / 2);
  dropMaster(li.dataset.master, wx, wy, li.dataset.stencil === 'doc' ? undefined : li.dataset.stencil);
});

// ---- pages: add, copy, rename, delete, move (each ONE undo step) ---------------------------------------------------
function showPage(index) { page = Math.max(0, Math.min(index, engine.pageCount() - 1)); selection = []; selected = null; render(); fit(); }
function askPageName(title, value, done) {
  $('pageNameTitle').textContent = title;
  $('pageNameInput').value = value;
  $('pageNameDialog').dataset.mode = title;
  $('pageNameForm').onsubmit = (event) => { event.preventDefault(); const name = $('pageNameInput').value; $('pageNameDialog').close(); done(name); };
  $('pageNameCancel').onclick = (event) => { event.preventDefault(); $('pageNameDialog').close(); };
  $('pageNameDialog').showModal();
  $('pageNameInput').select();
}
$('pageAdd').onclick = () => askPageName('New page', '', (name) => run(() => showPage(engine.addPage(page, name.trim()))));
$('pageDup').onclick = () => run(() => showPage(engine.duplicatePage(page)));
$('pageRename').onclick = () => askPageName('Rename page', engine.pageNames()[page] || '', (name) => run(() => { engine.renamePage(page, name); render(); }));
$('pageDel').onclick = () => run(() => showPage(engine.deletePage(page)));
$('pageLeft').onclick = () => { if (page > 0) run(() => showPage(engine.movePage(page, page - 1))); };
$('pageRight').onclick = () => { if (page < engine.pageCount() - 1) run(() => showPage(engine.movePage(page, page + 1))); };
// Design > Page Setup: Orientation and Size of the shown page (the same engine call as the Page setup dialog, ONE undo step each).
const SIZE_EPS = 0.01;
function syncDesign() {
  let size = null;
  try { size = engine.pageSize(page); } catch { /* no drawing */ }
  if (!size) return;
  $('designOrientation').value = size.w > size.h ? 'landscape' : 'portrait';
  const [short, long] = size.w <= size.h ? [size.w, size.h] : [size.h, size.w];
  const match = [...$('designSize').options].find((o) => o.value && o.value.split('x').map(Number).every((v, i) => Math.abs(v - (i === 0 ? short : long)) < SIZE_EPS));
  $('designSize').value = match ? match.value : '';
}
function applyPageShape(preset) {
  const size = engine.pageSize(page);
  const [short, long] = preset ? preset.split('x').map(Number) : (size.w <= size.h ? [size.w, size.h] : [size.h, size.w]);
  const portrait = $('designOrientation').value === 'portrait';
  const [w, h] = portrait ? [short, long] : [long, short];
  if (Math.abs(w - size.w) < 1e-9 && Math.abs(h - size.h) < 1e-9) return;
  run(() => { engine.setPageSize(page, w, h); render(); fit(); });
}
$('designOrientation').onchange = () => applyPageShape(null);
$('designSize').onchange = () => { if ($('designSize').value) applyPageShape($('designSize').value); else syncDesign(); };
$('insertTextBox').onclick = () => $('drawTools').querySelector('[data-kind="5"]').click();
function syncPageTools() {
  const n = engine.pageCount();
  syncDesign();
  $('pageDel').disabled = n < 2; $('pageLeft').disabled = page <= 0; $('pageRight').disabled = page >= n - 1;
}

// ---- Export (SVG, PNG of the page or the selection) and Print / PDF ---------------------------------------------------
/** The box (SVG user units) of the selected shapes in the live SVG, or null. */
function selectionBox() {
  const svg = svgEl();
  if (!svg || !selection.length) return null;
  const inv = svg.getScreenCTM()?.inverse();
  if (!inv) return null;
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const id of selection) {
    const node = nodeOf(id);
    if (!node) continue;
    const r = node.getBoundingClientRect();
    for (const [cx, cy] of [[r.left, r.top], [r.right, r.bottom]]) {
      const p = new DOMPoint(cx, cy).matrixTransform(inv);
      x0 = Math.min(x0, p.x); y0 = Math.min(y0, p.y); x1 = Math.max(x1, p.x); y1 = Math.max(y1, p.y);
    }
  }
  return Number.isFinite(x0) ? [x0 - 4, y0 - 4, x1 - x0 + 8, y1 - y0 + 8] : null;
}
/** SVG text of the page (`scope` 'page': the page rectangle) or the selection, without editing overlays; size in SVG units. */
function exportSvg(scope) {
  let text = engine.svg(page, { print: true });
  const { w, h } = engine.pageSize(page);
  let box = [0, 0, w * PX_PER_INCH, h * PX_PER_INCH];
  if (scope === 'selection') {
    box = selectionBox();
    if (!box) throw new Error('Select a shape to export the selection');
  } else if (scope !== 'printpage' && !$('fixedPage').checked) {
    // the page rectangle is only the viewBox of the fixed-page view; the plain drawing is cropped to its content box
    const match = /viewBox="([^"]+)"/.exec(text);
    if (match && engine.groups(page).length) box = match[1].split(/\s+/).map(Number);
  }
  text = text.replace(/<svg ([^>]*)>/, (all, attrs) => {
    attrs = attrs.replace(/ ?viewBox="[^"]*"/, '').replace(/ ?width="[^"]*"/, '').replace(/ ?height="[^"]*"/, '');
    return `<svg ${attrs} width="${box[2]}" height="${box[3]}" viewBox="${box.join(' ')}">`;
  });
  return { text, width: box[2], height: box[3] };
}
/** The export as bytes: `{bytes, mime, width, height, name}` (PNG at `scale` times 96 dpi). */
async function exportData({ format = 'svg', scope = 'page', scale = 2 } = {}) {
  // draw.io (FILE-20): every page as one diagram of an uncompressed .drawio file (web/drawio-export.js), built from the document model
  if (format === 'drawio') {
    const xml = exportDrawio(engine);
    return { bytes: new TextEncoder().encode(xml), mime: 'application/vnd.jgraph.mxfile', width: 0, height: 0, name: 'drawing.drawio', stats: { pages: engine.pageCount() } };
  }
  // Excalidraw (FILE-19): the page as an .excalidraw scene (web/excalidraw-export.js), built from the document model
  if (format === 'excalidraw') {
    const scene = exportExcalidraw(engine, page);
    const stem = (engine.pageNames()[page] || `page${page + 1}`).replace(/[^\w.-]+/g, '_');
    return { bytes: new TextEncoder().encode(JSON.stringify(scene, null, 1)), mime: 'application/json', width: 0, height: 0, name: `${stem}.excalidraw`, stats: { elements: scene.elements.length } };
  }
  const { text, width, height } = exportSvg(scope);
  const stem = (engine.pageNames()[page] || `page${page + 1}`).replace(/[^\w.-]+/g, '_');
  if (format === 'svg') return { bytes: new TextEncoder().encode(text), mime: 'image/svg+xml', width, height, name: `${stem}.svg` };
  // DXF (FILE-17): the outlines of the shapes, in inches in page coordinates (web/svgdxf.js); fills, gradients and images are not exported
  if (format === 'dxf') {
    // layers (step 307): the Visio layers of the page become DXF layers (a shape on several layers goes on its first); hidden layers are frozen, locked ones locked
    const pageModel = engine.model().pages[page] || {};
    const names = pageModel.layers || [];
    const layers = {}, layerFlags = {};
    for (const sh of pageModel.shapes || []) { const first = (sh.layers || [])[0]; if (first !== undefined && names[first]) layers[sh.id ?? sh.visioId] = names[first].name; }
    for (const l of names) layerFlags[l.name] = (l.visible ? 0 : 1) | (l.locked ? 4 : 0);
    const made = svgToDxf(text, { pxPerInch: PX_PER_INCH, heightIn: engine.pageSize(page).h, layers, layerFlags });
    return { bytes: new TextEncoder().encode(made.dxf), mime: 'application/dxf', width, height, name: `${stem}.dxf`, stats: { polylines: made.polylines, circles: made.circles, texts: made.texts, layers: made.layers } };
  }
  scale = Math.max(1, Math.min(8, Number(scale) || 1));
  const pw = Math.round(width * scale), ph = Math.round(height * scale);
  if (pw * ph > 120e6) throw new Error('The image would be too large: choose a smaller scale');
  const url = URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' }));
  try {
    const img = new Image();
    await new Promise((ok, fail) => { img.onload = ok; img.onerror = () => fail(new Error('The browser could not draw the page')); img.src = url; });
    const canvas = document.createElement('canvas');
    canvas.width = pw; canvas.height = ph;
    const g = canvas.getContext('2d');
    g.fillStyle = $('paperColor').value; g.fillRect(0, 0, pw, ph);
    g.drawImage(img, 0, 0, pw, ph);
    const blob = await new Promise((ok) => canvas.toBlob(ok, 'image/png'));
    return { bytes: new Uint8Array(await blob.arrayBuffer()), mime: 'image/png', width: pw, height: ph, name: `${stem}.png` };
  } finally { URL.revokeObjectURL(url); }
}
/** A PDF of ALL pages written by the app itself (web/pdf.js): every page is drawn like the PNG export and stored as one picture on a PDF page of the page's size. */
async function exportPdf({ scale = 2, vector = true } = {}) {
  scale = Math.max(1, Math.min(4, Number(scale) || 1));
  const saved = page;
  const pages = [];
  let vectorPages = 0;
  const fallbacks = [];
  let fontSource = null;
  try {
    // the fonts of fonts/manifest.json that the drawing's families ask for are embedded as subsets (none listed or found: the standard fonts)
    if (vector) {
      const texts = [];
      for (let p = 0; p < engine.pageCount(); p++) { page = p; texts.push(exportSvg('page').text); }
      try { fontSource = await loadPdfFonts(familiesInSvg(texts)); } catch { fontSource = null; }
    }
    for (let p = 0; p < engine.pageCount(); p++) {
      page = p;
      const { text, width, height } = exportSvg('page');
      // vector page when the converter can draw everything on it; otherwise (patterns, filters, pictures ...) the page stays a picture
      if (vector) {
        try { const v = svgToPdfPage(text, { paper: $('paperColor').value, fontSource }); pages.push({ vector: v, widthPt: v.widthPt, heightPt: v.heightPt }); vectorPages++; continue; } catch (error) { if (!(error instanceof Unsupported)) throw error; fallbacks.push(`page ${p + 1}: ${error.message}`); }
      }
      const pw = Math.round(width * scale), ph = Math.round(height * scale);
      if (pw * ph > 120e6) throw new Error(`Page ${p + 1} would be too large: choose a smaller scale`);
      const url = URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' }));
      try {
        const img = new Image();
        await new Promise((ok, fail) => { img.onload = ok; img.onerror = () => fail(new Error(`The browser could not draw page ${p + 1}`)); img.src = url; });
        const canvas = document.createElement('canvas');
        canvas.width = pw; canvas.height = ph;
        const g = canvas.getContext('2d');
        g.fillStyle = $('paperColor').value; g.fillRect(0, 0, pw, ph);
        g.drawImage(img, 0, 0, pw, ph);
        const blob = await new Promise((ok) => canvas.toBlob(ok, 'image/jpeg', 0.92));
        if (!blob) throw new Error(`The browser could not encode page ${p + 1}`);
        pages.push({ jpeg: new Uint8Array(await blob.arrayBuffer()), pixelWidth: pw, pixelHeight: ph, widthPt: width * 0.75, heightPt: height * 0.75 });
      } finally { URL.revokeObjectURL(url); }
    }
  } finally { page = saved; }
  const names = engine.pageNames();
  return { bytes: buildPdf(pages, { title: names[0] || 'Drawing' }), mime: 'application/pdf', pages: pages.length, vectorPages, fallbacks, embeddedFonts: fontSource ? fontSource.fonts().filter((f) => f.used.size).length : 0, name: `${(names[0] || 'drawing').replace(/[^\w.-]+/g, '_')}.pdf` };
}
/** Every page as its own SVG or PNG file, in one ZIP written by the app (web/zip.js): "01-<page name>.svg" ... in page order. */
async function exportAllPages({ format = 'svg', scale = 2 } = {}) {
  if (format !== 'svg' && format !== 'png') throw new Error('Choose SVG or PNG');
  const saved = page;
  const files = [];
  try {
    for (let p = 0; p < engine.pageCount(); p++) {
      page = p;
      const one = await exportData({ format, scope: 'page', scale });
      files.push({ name: `${String(p + 1).padStart(2, '0')}-${one.name}`, bytes: one.bytes });
    }
  } finally { page = saved; }
  const names = engine.pageNames();
  return { bytes: buildZip(files), mime: 'application/zip', pages: files.length, name: `${(names[0] || 'drawing').replace(/[^\w.-]+/g, '_')}-${format}.zip`, files: files.map((f) => f.name) };
}
/** Save as web page (FILE-18): ONE self-contained HTML file; every page is a picture (an SVG data URI, so the pages cannot clash over ids) under a tab bar. */
async function exportWebPage() {
  // FILE-18 (steps 292, 306): one self-contained HTML file, a tab per page; the pages' SVG are inline (live, selectable text) and the shapes' hyperlinks are real links (web/webpage.js)
  const saved = page;
  const pages = [];
  try {
    for (let p = 0; p < engine.pageCount(); p++) {
      page = p;
      const one = await exportData({ format: 'svg', scope: 'page' });
      const svg = new TextDecoder().decode(one.bytes);
      const links = {};
      for (const vid of new Set([...svg.matchAll(/<g data-vid="([^"]+)"/g)].map((m) => m[1]))) {
        try { const list = engine.hyperlinks(p, vid); if (list.length) links[vid] = list; } catch { /* a shape without links data */ }
      }
      pages.push({ name: engine.pageNames()[p] || `Page ${p + 1}`, width: one.width, height: one.height, svg, links });
    }
  } finally { page = saved; }
  const made = buildWebPage(pages, { title: engine.pageNames()[0] || 'Drawing' });
  return { bytes: new TextEncoder().encode(made.html), mime: 'text/html', pages: pages.length, links: made.links, droppedLinks: made.dropped, name: `${(engine.pageNames()[0] || 'drawing').replace(/[^\w.-]+/g, '_')}.html` };
}
$('exportBtn').onclick = () => { $('exScope').querySelector('option[value=selection]').disabled = !selection.length; $('exScope').value = selection.length ? $('exScope').value : 'page'; $('exportDialog').showModal(); };
/** Copy as image (PRINT-10): the selection (the page when nothing is selected) as a PNG on the clipboard. */
async function copyAsImage() {
  const out = await exportData({ format: 'png', scope: selection.length ? 'selection' : 'page', scale: 2 });
  if (!navigator.clipboard || typeof ClipboardItem === 'undefined') throw new Error('This browser cannot copy pictures to the clipboard');
  await navigator.clipboard.write([new ClipboardItem({ 'image/png': new Blob([out.bytes], { type: 'image/png' }) })]);
  return { width: out.width, height: out.height, scope: selection.length ? 'selection' : 'page' };
}
$('copyImgBtn').onclick = async () => { try { say(); const r = await copyAsImage(); say(`Copied the ${r.scope} as a ${r.width} x ${r.height} picture`); } catch (error) { say(error.message); } };
$('exClose').onclick = () => $('exportDialog').close();
$('exFormat').onchange = () => { const f = $('exFormat').value; $('exScale').disabled = f === 'svg' || f === 'svg-all' || f === 'dxf' || f === 'html-all' || f === 'excalidraw' || f === 'drawio'; $('exScope').disabled = f === 'pdf' || f === 'excalidraw' || f === 'drawio' || f.endsWith('-all'); };
$('exGo').onclick = async () => {
  try {
    say();
    const format = $('exFormat').value;
    const out = format === 'pdf' ? await exportPdf({ scale: $('exScale').value })
      : format === 'html-all' ? await exportWebPage()
      : format.endsWith('-all') ? await exportAllPages({ format: format.slice(0, -4), scale: $('exScale').value })
      : await exportData({ format, scope: $('exScope').value, scale: $('exScale').value });
    const link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob([out.bytes], { type: out.mime }));
    link.download = out.name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 10000);
    $('exportDialog').close();
  } catch (error) { say(error.message); }
};
/** Put the page into the print sheet and set the paper size to the page size (the browser's print dialog also saves PDF). */
function preparePrint() {
  const { w, h } = engine.pageSize(page);
  const { text, width: boxW, height: boxH } = exportSvg('printpage'); // Visio prints the PAGE (round 11b): shapes outside it are cut off, never a wider content box
  $('printSheet').innerHTML = text;
  let style = document.getElementById('printPageStyle');
  if (!style) { style = document.createElement('style'); style.id = 'printPageStyle'; document.head.append(style); }
  // the page's print setup: its margins around the drawing, the print zoom as the width of the drawing, centring. A drawing printed larger
  // than the paper (zoom over 100 %, or "fit on N x M sheets" with room for more than the page) is cut into sheets, left to right, top to
  // bottom (tiling); at 100 % or less the drawing is shown on one sheet inside the margins.
  const pr = engine.pageSize(page).print || { margins: [0, 0, 0, 0], scale: [1, 1], fit: false, center: [false, false], pages: [1, 1] };
  const [ml, mr, mt, mb] = pr.margins;
  // the sheet is the PRINTER PAPER of the page setup (PaperKind + orientation; probe-print-sizes.ps1, step 255), not the drawing page: a 10 x 7 in page on A4
  // prints on 11.69 x 8.27 in sheets. Without a known paper the drawing page stands for it.
  const [pw, ph] = pr.paper || [w, h];
  const area = Math.max(0.1, pw - ml - mr);
  const areaH = Math.max(0.1, ph - mt - mb);
  const factor = pr.fit ? Math.min(pr.pages[0] * area / w, pr.pages[1] * areaH / h) : pr.scale[0];
  const tiled = w * factor > pw + 1e-6 || h * factor > ph + 1e-6;
  // the first sheet shows the paper from the drawing's bottom-left corner; every further sheet moves on by the printable area (paper minus margins)
  const count = (content, paper, stride) => Math.max(1, 1 + Math.ceil((content - paper) / stride - 1e-9));
  const tiles = tiled ? [count(w * factor, pw, area), count(h * factor, ph, areaH)] : [1, 1];
  // at 100 % or less the drawing must fit ONE sheet: its height too (a plain view cropped to a taller box than the page, or a page whose aspect differs from the printable area, used to run onto a second sheet)
  const widthIn = tiled ? w * factor : pr.fit ? area : Math.min(area, w * pr.scale[0], areaH * (boxW / boxH));
  if (tiled) {
    let sheets = '';
    for (let j = 0; j < tiles[1]; j++) for (let i = 0; i < tiles[0]; i++) {
      // Visio (measured, step 225): every sheet shows the FULL paper, the grid advances by the printable area (paper minus margins, so
      // neighbouring sheets overlap by the margins) and is anchored at the drawing's bottom-left; the partial row is the top one.
      const top = ph - (h * factor - (tiles[1] - 1 - j) * areaH);
      sheets += `<div class="ps-tile" style="width:${pw}in;height:${ph}in"><div style="left:${-i * area}in;top:${top}in;width:${widthIn}in">${text}</div></div>`;
    }
    $('printSheet').innerHTML = sheets;
  }
  style.textContent = `@page{size:${pw}in ${ph}in;margin:${tiled ? '0' : `${mt}in ${mr}in ${mb}in ${ml}in`}}` + (tiled
    ? `@media print{#printSheet .ps-tile{overflow:hidden;position:relative;break-after:page}#printSheet .ps-tile:last-child{break-after:auto}#printSheet .ps-tile>div{position:absolute}#printSheet .ps-tile svg{width:100%!important;height:auto}}`
    : `@media print{#printSheet svg{width:${widthIn}in!important;margin:${pr.center[1] ? 'auto' : '0'} ${pr.center[0] ? 'auto' : '0'} 0 ${pr.center[0] ? 'auto' : '0'}}}`);
  return { w, h, paper: [pw, ph], print: pr, widthIn, tiles };
}
$('printBtn').onclick = () => { try { say(); preparePrint(); window.print(); } catch (error) { say(error.message); } };
window.addEventListener('afterprint', () => { $('printSheet').innerHTML = ''; });
Object.assign(window.layoutcraft, { exportData, exportPdf, exportAllPages, exportWebPage, preparePrint, copyAsImage, pageBreaks });

// ---- Shape Data: the Prop rows of the selected shape (label, value, type, list items), add and remove -----------------
const dataKinds = [['0', 'Text'], ['2', 'Number'], ['3', 'Yes / no'], ['1', 'List'], ['4', 'List (free)'], ['5', 'Date'], ['6', 'Duration'], ['7', 'Currency']];
function dataRows(id) { return (engine.model().pages[page]?.shapes.find((s) => s.visioId === id)?.data) || []; }
/** The rows in the order the Shape Data window shows them: by SortKey (rows without one first), the file order within equal keys. */
function sortedDataRows(id) {
  let keys = [];
  try { keys = engine.dataSortKeys(page, id); } catch { /* a shape that is not on the page itself: file order */ }
  const keyOf = (name) => keys.find((k) => k.name === name)?.sortKey ?? '';
  return dataRows(id).map((row, i) => ({ row, i, key: keyOf(row.name) })).sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : a.i - b.i)).map((e) => e.row);
}
/** Move a row one place up or down: every row of the shape gets the SortKey of its new place (0001, 0002, ...), one undo step. */
function moveDataRow(id, name, delta) {
  const order = sortedDataRows(id);
  const at = order.findIndex((r) => r.name === name), to = at + delta;
  if (at < 0 || to < 0 || to >= order.length) return;
  [order[at], order[to]] = [order[to], order[at]];
  run(() => { engine.batch(() => { order.forEach((r, n) => engine.setShapeData(page, id, { ...r, sortKey: String(n + 1).padStart(4, '0') })); }); render(); drawData(); });
}
/** The shapes the Shape data dialog works on: the selected ones that are not connectors. */
function dataTargets() { return selection.filter((id) => !isConnector(id)); }
function drawData() {
  const ids = dataTargets();
  const id = ids.length === 1 ? ids[0] : null;
  $('dataShape').textContent = id ? `(shape ${id})` : ids.length ? `(${ids.length} shapes)` : '(nothing selected)';
  // alt text (step 335): Visio's Shape > Alt Text (User.visAltDescription); one shape at a time
  const nameBox = $('dataName');
  nameBox.disabled = !id;
  nameBox.value = id ? (engine.shapeNames(page)[id] || '') : '';
  const alt = $('dataAlt');
  alt.disabled = !id;
  alt.value = id ? (engine.model().pages[page]?.shapes.find((s) => s.visioId === id)?.altText || '') : '';
  const table = $('dataTable');
  table.innerHTML = '<tr><th>Label</th><th>Value</th><th>Type</th><th>Prompt</th><th>Hidden</th><th>Order</th><th></th></tr>';
  if (ids.length > 1) {
    const tr = document.createElement('tr'); const td = document.createElement('td'); td.colSpan = 7;
    td.textContent = `${ids.length} shapes selected: the row you add below is written to every one of them (one undo step); select one shape to edit its rows.`;
    tr.append(td); table.append(tr);
    return;
  }
  if (!id) return;
  const save = (row, changes) => run(() => { engine.setShapeData(page, id, { ...row, ...changes }); render(); drawData(); });
  for (const row of sortedDataRows(id)) {
    const tr = document.createElement('tr');
    tr.dataset.row = row.name;
    const cell = (node) => { const td = document.createElement('td'); td.append(node); tr.append(td); return node; };
    const label = cell(Object.assign(document.createElement('input'), { value: row.label || row.name }));
    label.className = 'dlabel'; label.setAttribute('aria-label', `Label of ${row.name}`);
    label.onchange = () => save(row, { label: label.value });
    let value;
    const kind = row.type;
    if (kind === 3) {
      value = cell(Object.assign(document.createElement('input'), { type: 'checkbox', checked: /^(1|true)$/i.test(row.value) }));
      value.onchange = () => save(row, { value: value.checked ? '1' : '0' });
    } else if (kind === 1 || kind === 4) {
      value = cell(document.createElement('select'));
      const items = (row.format || '').split(';').filter(Boolean);
      if (row.value && !items.includes(row.value)) items.unshift(row.value);
      for (const item of items) value.append(Object.assign(document.createElement('option'), { value: item, textContent: item, selected: item === row.value }));
      value.onchange = () => save(row, { value: value.value });
    } else {
      value = cell(Object.assign(document.createElement('input'), { type: kind === 2 ? 'text' : kind === 5 ? 'date' : 'text', value: kind === 5 ? String(row.value || '').slice(0, 10) : kind === 6 ? (Number.isFinite(Number(row.value)) && row.value !== '' ? `${Number(row.value)} d` : '') : row.value || '' }));
      if (kind === 6) value.title = 'A duration: a number and a unit (2 d, 3 h, 90 min, 30 s, 1 w); stored as days';
      value.onchange = () => save(row, { value: value.value });
    }
    value.className = 'dvalue'; value.setAttribute('aria-label', `Value of ${row.label || row.name}`);
    cell(document.createTextNode((dataKinds.find(([k]) => Number(k) === kind) || ['', `type ${kind}`])[1]));
    const prompt = cell(Object.assign(document.createElement('input'), { value: row.prompt || '' }));
    prompt.className = 'dprompt'; prompt.setAttribute('aria-label', `Prompt of ${row.label || row.name}`); prompt.placeholder = 'prompt';
    prompt.onchange = () => save(row, { prompt: prompt.value });
    const hid = cell(Object.assign(document.createElement('input'), { type: 'checkbox', checked: !!row.invisible }));
    hid.setAttribute('aria-label', `Hide ${row.label || row.name}`);
    hid.onchange = () => save(row, { invisible: hid.checked });
    const mv = cell(document.createElement('span'));
    for (const [sym, delta, cls, word] of [['▲', -1, 'dup', 'up'], ['▼', 1, 'ddown', 'down']]) {
      const b = document.createElement('button'); b.textContent = sym; b.className = cls; b.setAttribute('aria-label', `Move ${row.label || row.name} ${word}`);
      b.onclick = () => moveDataRow(id, row.name, delta); mv.append(b);
    }
    const del = cell(document.createElement('button')); del.textContent = 'Remove'; del.className = 'ddel';
    del.onclick = () => run(() => { engine.removeShapeData(page, id, row.name); render(); drawData(); });
    table.append(tr);
  }
}
$('dataBtn').onclick = () => { drawData(); $('dataDialog').showModal(); };

// ---- Inventory report (round 11, item 8; user decision f): a table of the shapes with chosen Shape Data columns, CSV export ----------------------
// LayoutCraft's own report, NOT Visio's report format (the functions live in inventory.js since step 351, shared with the MCP server)
const inventoryData = (allPages = false) => inventoryDataOf(engine.model(), engine.pageNames ? engine.pageNames() : [], page, allPages);
const inventoryCsv = (columns, allPages = false) => inventoryCsvOf(engine.model(), engine.pageNames ? engine.pageNames() : [], page, columns, allPages);
function drawReport() {
  const all = $('reportAllPages').checked;
  const { rows, dataCols } = inventoryData(all);
  const box = $('reportColumns');
  const chosen = new Set([...box.querySelectorAll('input:checked')].map((i) => i.dataset.key));
  const first = !box.querySelector('input');
  box.querySelectorAll('label').forEach((l) => l.remove());
  const keys = [['id', 'Shape ID'], ['text', 'Text'], ...(all ? [['page', 'Page']] : []), ...dataCols.map((c) => [c.key, c.label])];
  for (const [key, text] of keys) {
    const l = document.createElement('label');
    const i = document.createElement('input');
    i.type = 'checkbox'; i.dataset.key = key; i.checked = first ? (key === 'id' || key === 'text' || dataCols.some((c) => c.key === key)) : chosen.has(key);
    i.setAttribute('aria-label', `Column ${text}`);
    i.onchange = drawReport;
    l.append(i, ` ${text} `);
    box.append(l);
  }
  const cols = [...box.querySelectorAll('input:checked')].map((i) => i.dataset.key);
  const table = $('reportTable');
  table.textContent = '';
  const tr = (cells, tag) => { const r = document.createElement('tr'); for (const c of cells) { const d = document.createElement(tag); d.textContent = c; r.append(d); } table.append(r); };
  const label = (k) => ({ page: 'Page', id: 'Shape ID', text: 'Text' }[k] || dataCols.find((c) => c.key === k)?.label || k);
  tr(cols.map(label), 'th');
  for (const r of rows) tr(cols.map((k) => (k === 'page' ? r.page : k === 'id' ? r.id : k === 'text' ? r.text : r.values[k] ?? '')), 'td');
  $('reportCount').textContent = `${rows.length} shape${rows.length === 1 ? '' : 's'}`;
  return cols;
}
$('reportBtn').onclick = () => { drawReport(); $('reportDialog').showModal(); };
$('reportAllPages').onchange = () => { $('reportColumns').querySelectorAll('label').forEach((l) => l.remove()); drawReport(); };
$('reportClose').onclick = () => $('reportDialog').close();
$('reportCsv').onclick = () => {
  const cols = drawReport();
  if (!cols.length) { say('Choose at least one column.'); return; }
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob(['\ufeff' + inventoryCsv(cols, $('reportAllPages').checked)], { type: 'text/csv;charset=utf-8' }));
  link.download = `${(engine.model().name || 'drawing').replace(/[^\w.-]+/g, '_')}-inventory.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 10000);
};
Object.assign(window.layoutcraft, { inventoryCsv, inventoryData });
$('dataClose').onclick = () => $('dataDialog').close();
// Shape name (step 337): Visio's Shape.Name; unique on the page (a taken name gets .<shape id> added, like Visio), empty = the automatic name again
$('dataName').onchange = () => run(() => {
  const ids = dataTargets();
  if (ids.length !== 1) return;
  const wanted = $('dataName').value.trim();
  const given = engine.setShapeName(page, ids[0], wanted);
  render(); drawData();
  say(!wanted ? 'Name removed' : given === wanted ? 'Name saved' : `Name saved as ${given} (the name was taken)`);
});
$('dataAlt').onchange = () => run(() => {
  const ids = dataTargets();
  if (ids.length !== 1) return;
  engine.setAltText(page, ids[0], $('dataAlt').value.trim());
  render(); drawData();
  say($('dataAlt').value.trim() ? 'Alt text saved' : 'Alt text removed');
});
// Shape Data sets (step 329): named lists of rows kept in the browser, saved from one shape and applied to any selection
function drawDataSets() {
  const sets = loadSets(), sel = $('dataSetSel'), keep = sel.value;
  sel.textContent = '';
  for (const name of Object.keys(sets).sort((a, b) => a.localeCompare(b))) { const o = document.createElement('option'); o.value = name; o.textContent = `${name} (${sets[name].length} rows)`; sel.append(o); }
  if (keep && sets[keep]) sel.value = keep;
  $('dataSetApply').disabled = $('dataSetDelete').disabled = !sel.options.length;
}
$('dataSetSave').onclick = () => run(() => {
  const ids = dataTargets();
  if (ids.length !== 1) throw new Error('Select one shape: its Shape Data rows become the set');
  const name = $('dataSetName').value.trim();
  if (!name) throw new Error('Type a name for the set');
  const rows = setFromRows(dataRows(ids[0]));
  if (!rows.length) throw new Error('This shape has no Shape Data rows to save');
  const sets = loadSets(); sets[name] = rows; saveSets(sets);
  $('dataSetName').value = ''; drawDataSets(); $('dataSetSel').value = name;
  say(`Saved the set "${name}" (${rows.length} rows)`);
});
$('dataSetApply').onclick = () => run(() => {
  const ids = dataTargets();
  if (!ids.length) throw new Error('Select a shape first');
  const set = loadSets()[$('dataSetSel').value];
  if (!set) throw new Error('Choose a data set');
  const n = applySet(engine, page, ids, set, dataRows);
  render(); drawData();
  say(n ? `Added ${n} row${n === 1 ? '' : 's'} to ${ids.length} shape${ids.length === 1 ? '' : 's'}` : 'The shapes have all these rows already');
});
$('dataSetDelete').onclick = () => run(() => {
  const name = $('dataSetSel').value, sets = loadSets();
  if (!(name in sets)) return;
  delete sets[name]; saveSets(sets); drawDataSets(); say(`Deleted the set "${name}"`);
});
drawDataSets();
$('dataAdd').onclick = () => run(() => {
  const ids = dataTargets();
  if (!ids.length) throw new Error('Select a shape first');
  const label = $('dataNewLabel').value.trim();
  if (!label) throw new Error('Type a label for the data row');
  let base = label.replace(/[^A-Za-z0-9_]/g, '_'); if (!/^[A-Za-z_]/.test(base)) base = 'Row_' + base;
  const type = Number($('dataNewType').value);
  engine.batch(() => {
    for (const id of ids) {
      const taken = new Set(dataRows(id).map((r) => r.name.toLowerCase()));
      let name = base, n = 2; while (taken.has(name.toLowerCase())) name = `${base}_${n++}`;
      engine.setShapeData(page, id, { name, label, value: type === 3 ? '0' : '', type, format: type === 1 || type === 4 ? $('dataNewFormat').value : '' });
    }
  });
  $('dataNewLabel').value = ''; $('dataNewFormat').value = ''; render(); drawData();
});

// ---- Layers: list, visibility / print / lock, rename, add, assign the selection (Layer section rows + LayerMember) ----
function layerList() { return (engine.model().pages[page]?.layers) || []; }
function drawLayers() {
  const rows = layerList();
  const members = selection.length ? selection : [];
  $('layersSel').textContent = members.length ? `(${members.length} selected)` : '(nothing selected)';
  const table = $('layersTable');
  table.innerHTML = '<tr><th>Name</th><th>Visible</th><th>Print</th><th>Lock</th><th>Colour</th><th>Selection</th></tr>';
  for (const l of rows) {
    const tr = document.createElement('tr');
    tr.dataset.layer = String(l.index);
    const cell = (node) => { const td = document.createElement('td'); td.append(node); tr.append(td); return node; };
    const name = cell(Object.assign(document.createElement('input'), { value: l.name }));
    name.setAttribute('aria-label', `Name of layer ${l.name}`);
    name.className = 'lname';
    name.onchange = () => run(() => { engine.setLayer(page, l.index, { name: name.value }); render(); drawLayers(); });
    for (const [key, field] of [['visible', 'visible'], ['print', 'print'], ['locked', 'locked']]) {
      const box = cell(Object.assign(document.createElement('input'), { type: 'checkbox', checked: key === 'locked' ? !!l.locked : l.visible !== false && key === 'visible' ? true : key === 'print' ? l.print !== false : false }));
      box.className = 'l' + key;
      box.setAttribute('aria-label', `${key} for layer ${l.name}`);
      box.onchange = () => run(() => { engine.setLayer(page, l.index, { [field]: box.checked }); render(); drawLayers(); });
    }
    const colour = cell(Object.assign(document.createElement('input'), { type: 'color', value: l.color || '#000000' }));
    colour.className = 'lcolor'; colour.setAttribute('aria-label', `Colour of layer ${l.name}`); colour.title = l.color ? 'Layer colour (shapes on it draw in this colour)' : 'No layer colour';
    colour.style.opacity = l.color ? '1' : '0.4';
    colour.onchange = () => run(() => { engine.setLayerColor(page, l.index, colour.value); render(); drawLayers(); });
    const noColour = document.createElement('button'); noColour.textContent = 'x'; noColour.className = 'lcolorclear'; noColour.title = 'No layer colour'; noColour.setAttribute('aria-label', `No colour for layer ${l.name}`); noColour.disabled = !l.color;
    noColour.onclick = () => run(() => { engine.setLayerColor(page, l.index, 'none'); render(); drawLayers(); });
    colour.parentNode.append(noColour);
    const put = cell(document.createElement('button')); put.textContent = 'Put on'; put.className = 'lput'; put.disabled = !members.length;
    put.onclick = () => run(() => { engine.layerMembers(page, members, l.index, true); render(); drawLayers(); });
    const off = document.createElement('button'); off.textContent = 'Take off'; off.className = 'loff'; off.disabled = !members.length;
    off.onclick = () => run(() => { engine.layerMembers(page, members, l.index, false); render(); drawLayers(); });
    tr.lastChild.append(off);
    const del = document.createElement('button'); del.textContent = 'Delete'; del.className = 'ldel'; del.setAttribute('aria-label', `Delete layer ${l.name}`);
    del.onclick = () => run(() => { engine.deleteLayer(page, l.index); render(); drawLayers(); });
    tr.lastChild.append(del);
    table.append(tr);
  }
}
$('layersBtn').onclick = () => { drawLayers(); $('layersDialog').showModal(); };
$('layersClose').onclick = () => $('layersDialog').close();
$('layerAdd').onclick = () => run(() => { const n = $('layerNew').value.trim(); if (!n) throw new Error('Type a name for the layer'); engine.addLayer(page, n); $('layerNew').value = ''; render(); drawLayers(); });

// ---- Page setup: the size and orientation of the shown page (the shapes keep their positions) ----------------------
// the Print setup rows of Page setup <-> the page sheet's print cells
function readPrintFields() {
  const pct = Number($('psPct').value) / 100;
  return {
    orientation: $('psOrientation').value === 'portrait' ? 1 : 2,
    margins: [$('psmL').value, $('psmR').value, $('psmT').value, $('psmB').value].map(Number),
    grid: $('psGrid').checked, scale: [pct, pct], fit: $('psZoomFit').checked,
    pages: [Number($('psFitX').value), Number($('psFitY').value)], center: [$('psCenterH').checked, $('psCenterV').checked],
  };
}
function openPageSetup() {
  const { w, h, metric } = engine.pageSize(page);
  const k = metric ? 25.4 : 1;
  $('psUnit').textContent = metric ? 'mm' : 'in';
  $('psCurrent').textContent = `(now ${(w * k).toFixed(metric ? 0 : 2)} x ${(h * k).toFixed(metric ? 0 : 2)} ${metric ? 'mm' : 'in'})`;
  $('psSize').value = 'custom';
  $('psW').value = String(Math.round(w * k * 100) / 100);
  $('psH').value = String(Math.round(h * k * 100) / 100);
  $('psOrientation').value = w > h ? 'landscape' : 'portrait';
  $('psCustom').hidden = false;
  const pr = engine.pageSize(page).print;
  if (pr) {
    [$('psmL').value, $('psmR').value, $('psmT').value, $('psmB').value] = pr.margins.map((v) => String(Math.round(v * 1e4) / 1e4));
    $('psPct').value = String(Math.round(pr.scale[0] * 1e4) / 100); $('psFitX').value = String(pr.pages[0]); $('psFitY').value = String(pr.pages[1]);
    $('psZoomFit').checked = pr.fit; $('psZoomPct').checked = !pr.fit;
    $('psCenterH').checked = pr.center[0]; $('psCenterV').checked = pr.center[1]; $('psGrid').checked = pr.grid;
  }
  $('psPrint').dataset.was = JSON.stringify(readPrintFields());
  const sc = engine.pageSize(page).scale;
  $('psScalePaper').value = String(Math.round(sc.paper * 1e6) / 1e6); $('psScalePaperUnit').value = sc.paperUnit;
  $('psScaleDrawing').value = String(Math.round(sc.drawing * 1e6) / 1e6); $('psScaleDrawingUnit').value = sc.drawingUnit;
  $('psScale').dataset.was = scaleText();
  $('psScale').value = [...$('psScale').options].find((o) => o.value === 'none' ? isNoScale() : sameScale(o.value))?.value || 'custom';
  const info = engine.backgroundInfo();
  const current = info.assigned[page] || '';
  $('psBack').innerHTML = '';
  for (const [id, name] of [['', '(none)'], ...info.backgrounds.map((b) => [b.id, b.name])]) $('psBack').add(new Option(name, id));
  $('psBack').value = current;
  $('psBack').dataset.was = current;
  $('psBackList').innerHTML = '';
  for (const b of info.backgrounds) $('psBackList').add(new Option(b.name, b.id));
  $('psBackListRow').hidden = info.backgrounds.length === 0;
  $('psMakeBack').disabled = engine.pageCount() < 2 || !!current;
  $('psMakeBack').title = current ? "Clear this page's own background first" : engine.pageCount() < 2 ? 'A drawing needs one page that is not a background' : 'This page leaves the page tabs and can be shown beneath other pages';
  $('pageSetupDialog').showModal();
}
$('pageSetup').onclick = openPageSetup;
// the drawing scale rows of Page setup: "paper unit = drawing unit"; a preset fills the fields, typing in them makes it custom
const scaleText = () => [$('psScalePaper').value, $('psScalePaperUnit').value, $('psScaleDrawing').value, $('psScaleDrawingUnit').value].join('|');
const isNoScale = () => { const f = { in: 1, ft: 12, mm: 1 / 25.4, cm: 1 / 2.54, m: 1000 / 25.4 }; return Math.abs($('psScalePaper').value * f[$('psScalePaperUnit').value] - $('psScaleDrawing').value * f[$('psScaleDrawingUnit').value]) < 1e-9; };
const sameScale = (preset) => { const [p, pu, d, du] = preset.split('|'); return Number($('psScalePaper').value) === Number(p) && $('psScalePaperUnit').value === pu && Number($('psScaleDrawing').value) === Number(d) && $('psScaleDrawingUnit').value === du; };
$('psScale').addEventListener('change', () => {
  const v = $('psScale').value;
  if (v === 'custom') return;
  const unit = engine.pageSize(page).metric ? 'mm' : 'in';
  const [p, pu, d, du] = v === 'none' ? ['1', unit, '1', unit] : v.split('|');
  $('psScalePaper').value = p; $('psScalePaperUnit').value = pu; $('psScaleDrawing').value = d; $('psScaleDrawingUnit').value = du;
});
for (const id of ['psScalePaper', 'psScalePaperUnit', 'psScaleDrawing', 'psScaleDrawingUnit']) $(id).addEventListener('input', () => { $('psScale').value = isNoScale() ? 'none' : [...$('psScale').options].find((o) => o.value !== 'none' && o.value !== 'custom' && sameScale(o.value))?.value || 'custom'; });
$('psSize').onchange = () => {
  $('psCustom').hidden = $('psSize').value !== 'custom';
  if ($('psSize').value !== 'custom') {
    const [w, h] = $('psSize').value.split('x').map(Number);
    const [short, long] = w <= h ? [w, h] : [h, w];
    $('psOrientation').value = $('psOrientation').value; // the choice below decides
    const metric = engine.pageSize(page).metric, k = metric ? 25.4 : 1;
    $('psW').value = String(Math.round((($('psOrientation').value === 'portrait' ? short : long) * k) * 100) / 100);
    $('psH').value = String(Math.round((($('psOrientation').value === 'portrait' ? long : short) * k) * 100) / 100);
  }
};
$('psCancel').onclick = (event) => { event.preventDefault(); $('pageSetupDialog').close(); };
$('psMakeBack').onclick = () => {
  try { say(); const name = engine.pageNames()[page]; showPage(engine.makeBackground(page)); $('pageSetupDialog').close(); say(`${name} is now a background page. Page setup > Background page shows it beneath another page.`); } catch (error) { say(error.message); }
};
$('psMakeFore').onclick = () => {
  try { say(); const id = $('psBackList').value; if (!id) return; showPage(engine.makeForeground(id)); $('pageSetupDialog').close(); } catch (error) { say(error.message); }
};
$('pageSetupForm').onsubmit = (event) => {
  event.preventDefault();
  const metric = engine.pageSize(page).metric, k = metric ? 1 / 25.4 : 1;
  let w, h;
  if ($('psSize').value === 'custom') { w = Number($('psW').value) * k; h = Number($('psH').value) * k; }
  else [w, h] = $('psSize').value.split('x').map(Number);
  const [short, long] = w <= h ? [w, h] : [h, w];
  [w, h] = $('psOrientation').value === 'portrait' ? [short, long] : [long, short];
  try {
    say();
    const back = $('psBack').value;
    const printNow = JSON.stringify(readPrintFields());
    run(() => { engine.batch(() => { engine.setPageSize(page, w, h); if (printNow !== $('psPrint').dataset.was) engine.setPrintSetup(page, JSON.parse(printNow)); if (back !== $('psBack').dataset.was) engine.setBackPage(page, back || null); if (scaleText() !== $('psScale').dataset.was) engine.setDrawingScale(page, Number($('psScalePaper').value), $('psScalePaperUnit').value, Number($('psScaleDrawing').value), $('psScaleDrawingUnit').value); }); render(); fit(); });
    $('pageSetupDialog').close();
  } catch (error) { say(error.message); }
};

// ---- Hyperlinks: a shape's Hyperlink section (hyperlinks.rs). Insert > Hyperlink... / Ctrl+K edits the links of the selected shape, Ctrl+click follows the
// default one, the shape menu lists them, a small badge on the selected shape opens the default link. Links are not opened by the engine: the shell asks
// the browser (http, https, mailto, tel) or goes to a page of the drawing; a link to a file or another kind is refused with a message.
const linksOf = (id) => {
  const key = `${page}:${id}`;
  if (!linkCache.has(key)) { let list = []; try { list = engine.hyperlinks(page, id); } catch { /* a shape without a section, or no drawing */ } linkCache.set(key, list); }
  return linkCache.get(key);
};
const visibleLinks = (id) => linksOf(id).filter((l) => !l.invisible);
const defaultLink = (id) => { const list = visibleLinks(id); return list.find((l) => l.default) || list[0] || null; };
const linkLabel = (l) => l.description || l.address || l.subAddress || '(empty link)';
function openLink(link) {
  if (!link.address) { // a page of this drawing
    const at = engine.pageNames().indexOf(link.subAddress);
    if (at < 0) { say(`The page "${link.subAddress}" is not in this drawing.`); return false; }
    showPage(at);
    return true;
  }
  let url = null;
  try { url = new URL(link.address); } catch { /* not an absolute address */ }
  if (!url || !['http:', 'https:', 'mailto:', 'tel:'].includes(url.protocol)) { say(`This link cannot be opened in the browser: ${link.address}`); return false; }
  const target = link.subAddress && url.protocol.startsWith('http') ? `${link.address}#${link.subAddress}` : link.address;
  window.open(target, '_blank', 'noopener,noreferrer');
  return true;
}
/** The entries the shape menu shows for the selected shape: Open: <link>, one per visible link (contextmenu.js). */
window.layoutcraftLinks = {
  menuItems: () => (selection.length === 1 ? visibleLinks(selection[0]).map((l) => [`Open: ${linkLabel(l)}`, null, false, () => openLink(l)]) : []),
  open: openLink, linksOf, defaultLink,
};
function addLinkBadge(ov, id) {
  const link = defaultLink(id);
  const box = boxOf(id);
  if (!link || !box) return;
  const b = document.createElement('button');
  b.type = 'button';
  b.className = 'linkbadge';
  b.textContent = 'Link';
  b.setAttribute('aria-label', `Open the hyperlink: ${linkLabel(link)}`);
  b.title = `Open: ${linkLabel(link)}`;
  Object.assign(b.style, { left: `${box.x + box.w - 30}px`, top: `${Math.max(0, box.y - 18)}px` });
  b.addEventListener('pointerdown', (event) => event.stopPropagation());
  b.addEventListener('click', (event) => { event.stopPropagation(); openLink(link); });
  ov.append(b);
}
let lkWork = [];
let lkSel = -1;
let lkId = null;
const lkText = (l) => linkLabel(l);
function lkRefreshList() {
  const list = $('lkList');
  list.innerHTML = '';
  lkWork.forEach((l, i) => { const o = document.createElement('option'); o.value = String(i); o.textContent = lkText(l); list.append(o); });
  if (lkSel >= 0) list.value = String(lkSel);
}
function lkShowFields() {
  const l = lkWork[lkSel];
  for (const id of ['lkAddress', 'lkSub', 'lkDesc', 'lkNewWin', 'lkDefault', 'lkHidden']) $(id).disabled = !l;
  $('lkRemove').disabled = !l;
  $('lkAddress').value = l?.address ?? ''; $('lkSub').value = l?.subAddress ?? ''; $('lkDesc').value = l?.description ?? '';
  $('lkNewWin').checked = !!l?.newWindow; $('lkDefault').checked = !!l?.default; $('lkHidden').checked = !!l?.invisible;
}
function lkSelect(i) { lkSel = i; lkRefreshList(); lkShowFields(); }
// Review > Compare Documents (step 331): the open drawing against a file; the file is read by a second engine instance, nothing in the open drawing changes
$('compareBtn').onclick = () => $('compareFile').click();
let compareDiffs = [], compareName = 'file'; // the differences shown now (for Save list)
$('compareSave').onclick = () => {
  if (!compareDiffs.length) return;
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob(['\ufeff' + diffsToCsv(compareDiffs)], { type: 'text/csv;charset=utf-8' }));
  link.download = `${(engine.model().name || 'drawing').replace(/[^\w.-]+/g, '_')}-compare.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 10000);
  say(`Saved ${compareDiffs.length} difference${compareDiffs.length === 1 ? '' : 's'} as ${link.download}`);
};
$('compareFile').onchange = async () => {
  const file = $('compareFile').files[0];
  $('compareFile').value = '';
  if (!file) return;
  try {
    say();
    const other = await loadEngine('./layoutcraft.wasm');
    // the file is read the way File > Open reads it (a .drawio or .excalidraw file through its importer into the second engine, so the open drawing is never touched)
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (/\.excalidraw$/i.test(file.name)) importExcalidraw(other, new TextDecoder().decode(bytes));
    else if (/\.drawio$/i.test(file.name)) await importDrawio(other, bytes);
    else other.import(bytes);
    const diffs = compareModels(engine.model(), other.model());
    compareDiffs = diffs; compareName = file.name;
    $('compareSave').disabled = !diffs.length;
    const list = $('compareList');
    list.textContent = '';
    for (const d of diffs) {
      const li = document.createElement('li'), b = document.createElement('button'), kind = document.createElement('span');
      b.type = 'button'; b.dataset.kind = d.kind; b.dataset.id = d.id; kind.className = 'kind'; kind.textContent = `${d.kind} ${d.what}`;
      b.append(kind, document.createTextNode(`${d.pageName}: ${d.label} (${d.detail})`));
      if (d.kind !== 'removed' && d.page >= 0) b.onclick = () => { $('compareDialog').close(); showPage(d.page); if (d.what === 'shape' && d.id) setSelection([d.id]); };
      else b.disabled = true; // a removed item is not in the open drawing: there is nothing to select
      li.append(b); list.append(li);
    }
    const n = (k) => diffs.filter((d) => d.kind === k).length;
    $('compareSummary').textContent = diffs.length ? `${file.name}: ${n('added')} added, ${n('removed')} removed, ${n('changed')} changed (the open drawing against the file; added = only in the open drawing). Click one to go to it.` : `${file.name}: no differences found.`;
    $('compareDialog').showModal();
    list.querySelector('button:not(:disabled)')?.focus();
  } catch (error) { say(`Cannot compare with ${file.name}: ${error.message}`); }
};
// ---- Help > AI assistant (MCP) (step 358): the configuration to put into an MCP client; the desktop builds tell the page where their bundled server is (window.layoutcraftDesktop.mcp = { command, args }) ----
const MCP_KEY = 'layoutcraft-mcp';
function drawMcp() {
  const desktop = window.layoutcraftDesktop && window.layoutcraftDesktop.mcp;
  $('mcpDesktop').hidden = !desktop;
  $('mcpServer').disabled = !!desktop;
  const cfg = mcpConfig({
    command: desktop ? desktop.command : 'node', args: desktop ? desktop.args || [] : null,
    server: $('mcpServer').value.trim() || $('mcpServer').placeholder, root: $('mcpRoot').value.trim() || $('mcpRoot').placeholder,
  });
  $('mcpJson').value = cfg.json; $('mcpCmd').value = cfg.claudeCode;
  try { localStorage.setItem(MCP_KEY, JSON.stringify({ server: $('mcpServer').value, root: $('mcpRoot').value })); } catch { /* not remembered */ }
}
const mcpCopy = async (text, what) => {
  try { await navigator.clipboard.writeText(text); $('mcpMsg').textContent = `${what} copied.`; }
  catch { $('mcpMsg').textContent = `This browser did not allow copying: select the ${what.toLowerCase()} and copy it.`; }
};
$('mcpInfo').onclick = () => {
  try { const saved = JSON.parse(localStorage.getItem(MCP_KEY) || '{}'); $('mcpServer').value = saved.server || ''; $('mcpRoot').value = saved.root || ''; } catch { /* nothing remembered */ }
  $('mcpMsg').textContent = '';
  drawMcp();
  $('mcpDialog').showModal();
  $('mcpServer').focus();
};
$('mcpServer').oninput = drawMcp;
$('mcpRoot').oninput = drawMcp;
$('mcpCopyJson').onclick = () => mcpCopy($('mcpJson').value, 'Configuration');
$('mcpCopyCmd').onclick = () => mcpCopy($('mcpCmd').value, 'Command');
// Process > Check Diagram (step 328): a list of things that are usually mistakes (loose connector ends, shapes nothing connects to, overlaps, shapes outside the page); a click on a line selects the shape
let checkIssues = []; // the problems shown now (for Save list)
function drawCheck() {
  // one page, or (check box "All pages", step 343) every page: each problem carries its page index; the messages of a many-page check start with the page name
  const everyPage = $('checkAll').checked, model = engine.model(), names = engine.pageNames();
  const pages = everyPage ? [...Array(engine.pageCount()).keys()] : [page];
  const issues = pages.flatMap((p) => checkDiagram(model, p, engine.pageSize(p), { altText: $('checkAlt').checked }).map((issue) => ({ ...issue, page: p, message: everyPage ? `${names[p] || `Page ${p + 1}`}: ${issue.message}` : issue.message })));
  const list = $('checkList');
  list.textContent = '';
  checkIssues = issues;
  $('checkSave').disabled = !issues.length;
  for (const issue of issues) {
    const li = document.createElement('li'), b = document.createElement('button'), kind = document.createElement('span');
    b.type = 'button'; b.dataset.id = issue.id; kind.className = 'kind'; kind.textContent = issue.kind;
    b.append(kind, document.createTextNode(issue.message));
    b.dataset.page = String(issue.page);
    b.onclick = () => { $('checkDialog').close(); if (issue.page !== page) showPage(issue.page); setSelection([issue.id]); };
    li.append(b);
    if (issue.fix) {
      const fix = document.createElement('button');
      fix.type = 'button'; fix.className = 'fix'; fix.textContent = issue.fix; fix.title = `${issue.fix}: ${issue.message}`;
      fix.onclick = () => run(() => { if (applyFix(engine, issue.page, issue, engine.model(), engine.pageSize(issue.page))) { render(); drawCheck(); say(`${issue.fix}: done`); } });
      li.append(fix);
    }
    list.append(li);
  }
  const where = everyPage ? `in this drawing (${pages.length} page${pages.length === 1 ? '' : 's'})` : 'on this page';
  $('checkSummary').textContent = issues.length ? `${issues.length} problem${issues.length === 1 ? '' : 's'} found ${where}. Click one to go to it.` : `No problems found ${where}.`;
  if (!$('checkDialog').open) $('checkDialog').showModal();
  list.querySelector('button')?.focus();
}
$('checkDiagram').onclick = () => run(drawCheck);
$('checkAlt').onchange = () => run(drawCheck);
$('checkAll').onchange = () => run(drawCheck);
$('checkSave').onclick = () => {
  if (!checkIssues.length) return;
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob(['\ufeff' + issuesToCsv(checkIssues, engine.pageNames())], { type: 'text/csv;charset=utf-8' }));
  link.download = `${(engine.model().name || 'drawing').replace(/[^\w.-]+/g, '_')}-check.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 10000);
  say(`Saved ${checkIssues.length} problem${checkIssues.length === 1 ? '' : 's'} as ${link.download}`);
};
// Design > Borders & Titles (step 325): a border is a background page of plain shapes, shown beneath the current page
for (const [name, tip] of Object.entries(BORDER_STYLES)) { const o = document.createElement('option'); o.value = name; o.textContent = name; o.title = tip; $('borderStyle').append(o); }
$('borderApply').onclick = () => run(() => {
  const style = $('borderStyle').value;
  applyBorder(engine, page, style, $('borderTitle').value.trim());
  render();
  say(`Added the border "${style}" to ${engine.pageNames()[page] || 'this page'}`);
});
$('borderRemove').onclick = () => run(() => {
  const info = engine.backgroundInfo();
  if (!info.assigned[page]) { say('This page has no border or background page.'); return; }
  removeBorder(engine, page);
  render();
  say('Took the background page off this page');
});
// Insert > Picture (step 323): a PNG, JPEG, GIF or BMP file becomes a picture shape in the middle of the page, at its own size (a tiny one is enlarged to one inch, a big one reduced to half the page), selected; one undo step
$('insertPicture').onclick = () => $('pictureFile').click();
const PICTURE_TYPES = /^image\/(png|jpe?g|gif|bmp)$/i;
/** Insert the picture `file` on the current page, its centre at `at` ([x, y] inches, y up; the middle of the page when not given), at its own size (see above). One undo step; the picture is selected. */
async function insertPictureFile(file, at = null) {
  try {
    say();
    const bytes = new Uint8Array(await file.arrayBuffer());
    let w = 2, h = 2;
    try { const bmp = await createImageBitmap(new Blob([bytes], { type: file.type })); w = bmp.width / 96; h = bmp.height / 96; if (bmp.close) bmp.close(); } catch { /* the engine still checks the bytes */ }
    const size = engine.pageSize(page);
    const grow = Math.max(1, 1 / Math.max(w, h)); // a tiny picture (a 1 x 1 pixel PNG is 0.01 in) gets a longer side of one inch
    w *= grow; h *= grow;
    const k = Math.min(1, (size.w * 0.5) / w, (size.h * 0.5) / h); // a big one is reduced to half the page
    w *= k; h *= k;
    let cx = at ? at[0] : size.w / 2, cy = at ? at[1] : size.h / 2;
    cx = Math.max(w / 2, Math.min(size.w - w / 2, cx)); cy = Math.max(h / 2, Math.min(size.h - h / 2, cy)); // a picture dropped near the edge stays on the page
    const id = engine.insertPicture(page, bytes, cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2);
    selection = [id];
    render();
    say(`Inserted ${file.name || 'the picture'}`);
  } catch (error) { say(error.message); }
}
$('pictureFile').onchange = () => {
  const file = $('pictureFile').files[0];
  $('pictureFile').value = '';
  if (file) insertPictureFile(file);
};
// drag and drop of picture files onto the page (step 340): the picture's centre goes where it is dropped; other files are left to the browser
const DRAWING_FILE = /\.(vsdx|vssx|vstx|excalidraw|drawio)$/i;
$('stage').addEventListener('dragover', (event) => {
  // a picture file, or any other file (its name is not known before the drop): taken, so the browser does not navigate to it; the drop says what it can open
  if ([...(event.dataTransfer?.items || [])].some((item) => item.kind === 'file')) { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; }
});
$('stage').addEventListener('drop', (event) => {
  const all = [...(event.dataTransfer?.files || [])];
  if (!all.length) return;
  event.preventDefault();
  const pictures = all.filter((f) => PICTURE_TYPES.test(f.type));
  if (pictures.length && svgEl()) {
    const [x, y] = fromClient(event.clientX, event.clientY);
    pictures.forEach((file, n) => insertPictureFile(file, [x + n * 0.3, y - n * 0.3])); // several files fan out a little
    return;
  }
  // a drawing file (step 341) is opened like File > Open does it (the first one when several are dropped)
  const drawing = all.find((f) => DRAWING_FILE.test(f.name));
  if (drawing) {
    const dt = new DataTransfer();
    dt.items.add(drawing);
    $('file').files = dt.files;
    $('file').dispatchEvent(new Event('change', { bubbles: true }));
    return;
  }
  say(`${all[0].name}: LayoutCraft opens .vsdx, .vssx, .vstx, .drawio and .excalidraw files and takes PNG, JPEG, GIF and BMP pictures.`);
});
// paste of a picture (a screenshot, an image copied in a browser or an app) when no text field has the focus (step 340)
document.addEventListener('paste', (event) => {
  if (['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName) || textEdit || !svgEl()) return;
  const file = [...(event.clipboardData?.files || [])].find((f) => PICTURE_TYPES.test(f.type));
  if (!file) return;
  event.preventDefault();
  insertPictureFile(file);
});
$('insertHyperlink').onclick = () => run(() => {
  if (selection.length !== 1) { say('Select one shape to give it a hyperlink.'); return; }
  lkId = selection[0];
  lkWork = engine.hyperlinks(page, lkId).map((l) => ({ ...l }));
  $('lkShape').textContent = lkId;
  $('lkPages').innerHTML = engine.pageNames().map((n) => `<option value="${n.replace(/"/g, '&quot;')}">`).join('');
  $('lkMsg').textContent = '';
  lkSelect(lkWork.length ? 0 : -1);
  $('linkDialog').showModal();
  (lkWork.length ? $('lkAddress') : $('lkAdd')).focus();
});
$('lkAdd').onclick = () => { lkWork.push({ name: '', description: '', address: '', subAddress: '', extraInfo: '', frame: '', newWindow: false, default: false, invisible: false }); lkSelect(lkWork.length - 1); $('lkAddress').focus(); };
$('lkRemove').onclick = () => { if (lkSel < 0) return; lkWork.splice(lkSel, 1); lkSelect(Math.min(lkSel, lkWork.length - 1)); };
$('lkList').addEventListener('change', () => lkSelect(Number($('lkList').value)));
for (const [id, field, flag] of [['lkAddress', 'address'], ['lkSub', 'subAddress'], ['lkDesc', 'description'], ['lkNewWin', 'newWindow', true], ['lkDefault', 'default', true], ['lkHidden', 'invisible', true]]) {
  $(id).addEventListener('input', () => {
    const l = lkWork[lkSel];
    if (!l) return;
    l[field] = flag ? $(id).checked : $(id).value;
    if (id === 'lkDefault' && l.default) lkWork.forEach((o) => { if (o !== l) o.default = false; });
    const keep = lkSel; lkRefreshList(); $('lkList').value = String(keep);
  });
}
$('lkCancel').onclick = (event) => { event.preventDefault(); $('linkDialog').close(); };
$('linkForm').onsubmit = (event) => {
  event.preventDefault();
  const bad = lkWork.findIndex((l) => !l.address.trim() && !l.subAddress.trim());
  if (bad >= 0) { lkSelect(bad); $('lkMsg').textContent = 'A hyperlink needs an address or a sub-address.'; return; }
  const before = JSON.stringify(engine.hyperlinks(page, lkId));
  if (JSON.stringify(lkWork) === before) { $('linkDialog').close(); return; } // nothing changed: no undo step
  try {
    say();
    run(() => { engine.setHyperlinks(page, lkId, lkWork); render(); });
    $('linkDialog').close();
  } catch (error) { $('lkMsg').textContent = error.message; }
};

// ---- Shape Sheet pane (View > Task panes > Shape Sheet pane): the ShapeSheet of the selected shape, READ ONLY - the cells with their values and formulas,
// the sections with their rows; what the master supplies is merged in (a section the shape does not carry itself is marked). Editing a cell is not built.
const SHEET_TRANSFORM = ['Width', 'Height', 'PinX', 'PinY', 'LocPinX', 'LocPinY', 'Angle', 'FlipX', 'FlipY', 'ResizeMode'];
function sheetTable(cells) {
  const table = document.createElement('table');
  const head = table.createTHead().insertRow();
  for (const h of ['Cell', 'Value', 'Formula']) { const th = document.createElement('th'); th.scope = 'col'; th.textContent = h; head.append(th); }
  const body = table.createTBody();
  for (const c of cells) {
    const row = body.insertRow();
    row.insertCell().textContent = c.name;
    row.insertCell().textContent = `${c.value ?? ''}${c.unit ? ` ${c.unit}` : ''}`;
    row.insertCell().textContent = c.formula ?? '';
  }
  return table;
}
function sheetDetails(title, open) {
  const d = document.createElement('details');
  d.open = !!open;
  const s = document.createElement('summary');
  s.textContent = title;
  d.append(s);
  return d;
}
function renderSheetPane() {
  const pane = $('sheetPane');
  const on = $('paneSheet').checked;
  pane.hidden = !on;
  if (!on) return;
  const body = $('shBody');
  body.innerHTML = '';
  if (selection.length !== 1) { body.textContent = selection.length ? 'Select one shape to see its ShapeSheet.' : 'Nothing is selected.'; $('shHeading').textContent = 'Shape Sheet'; return; }
  let sheet = null;
  try { sheet = engine.shapeSheet(page, selection[0]); } catch (error) { body.textContent = error.message; return; }
  $('shHeading').textContent = `Shape Sheet - Shape ${sheet.id}${sheet.name ? ` (${sheet.name})` : ''}${sheet.master ? `, master ${sheet.master}` : ''}`;
  const transform = SHEET_TRANSFORM.map((n) => sheet.cells.find((c) => c.name === n)).filter(Boolean);
  const rest = sheet.cells.filter((c) => !SHEET_TRANSFORM.includes(c.name));
  const t = sheetDetails('Shape Transform', true);
  t.append(sheetTable(transform));
  body.append(t);
  if (rest.length) { const d = sheetDetails(`Other cells (${rest.length})`, false); d.append(sheetTable(rest)); body.append(d); }
  for (const sec of sheet.sections) {
    const label = `${sec.name}${sec.index !== null && sec.index !== undefined ? ` ${sec.index}` : ''} (${sec.rows.length} row${sec.rows.length === 1 ? '' : 's'})${sec.fromMaster ? ' - from the master' : ''}`;
    const d = sheetDetails(label, false);
    if (sec.cells.length) d.append(sheetTable(sec.cells));
    for (const r of sec.rows) {
      const p = document.createElement('p');
      p.className = 'shrow';
      p.textContent = `Row ${r.name ?? r.index ?? ''}${r.type ? ` (${r.type})` : ''}`;
      d.append(p, sheetTable(r.cells));
    }
    body.append(d);
  }
}
$('paneSheet').addEventListener('change', () => renderSheetPane());

// ---- Comments: visio/comments.xml (comments.rs). Review > New comment adds one to the selected shape (or the page); the Comments pane lists them all
// (a click goes to the shape and its page, Edit and Delete change them); a yellow tag marks every shape that has comments. Every change is ONE undo step.
const commentsAll = () => (commentCache ??= (() => { try { return engine.comments(); } catch { return []; } })());
const nowText = () => { const d = new Date(), p = (n) => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`; };
const initialsOf = (name) => { const w = name.trim().split(/\s+/).filter(Boolean); return (w.length > 1 ? w.slice(0, 3).map((x) => x[0]).join('') : (w[0] || '?')[0]); };
const storedAuthor = () => { try { return localStorage.getItem('layoutcraft-author') || ''; } catch { return ''; } };
const commentTarget = (c) => (c.shape ? `Shape ${c.shape}` : `Page ${(() => { try { return engine.pageNames()[c.page] ?? ''; } catch { return ''; } })()}`);
let cmOnShape = null; // the shape whose comments the pane highlights
function renderCommentsPane() {
  const pane = $('commentsPane');
  const on = $('showComments').checked;
  pane.hidden = !on;
  if (!on) return;
  const all = commentsAll();
  // the reviewer filter (step 334): one entry per author, "All reviewers" first; the choice is kept while the authors still exist
  const authors = [...new Set(all.map((c) => c.author))].sort((a, b) => a.localeCompare(b));
  const sel = $('cmReviewer'), keep = sel.value;
  sel.textContent = '';
  for (const [value, text] of [['', 'All reviewers'], ...authors.map((a) => [a, `${a} (${all.filter((c) => c.author === a).length})`])]) { const o = document.createElement('option'); o.value = value; o.textContent = text; sel.append(o); }
  sel.value = authors.includes(keep) ? keep : '';
  const list = sel.value ? all.filter((c) => c.author === sel.value) : all;
  $('cmHeading').textContent = sel.value ? `Comments (${list.length} of ${all.length})` : `Comments (${all.length})`;
  $('cmPrev').disabled = $('cmNext').disabled = !list.length;
  const ol = $('cmList');
  ol.innerHTML = '';
  for (const c of list) {
    const li = document.createElement('li');
    li.dataset.id = c.id;
    if (cmOnShape && c.shape === cmOnShape && c.page === page) li.classList.add('on');
    const go = document.createElement('button');
    go.type = 'button'; go.className = 'cmgo';
    const meta = document.createElement('span');
    meta.className = 'cmmeta';
    meta.textContent = `${c.author} (${c.initials}) ${c.editDate.replace('T', ' ')} - ${commentTarget(c)}\n`;
    go.append(meta, document.createTextNode(c.text));
    go.setAttribute('aria-label', `${c.author}, ${commentTarget(c)}: ${c.text}. Go to it.`);
    go.addEventListener('click', () => goToComment(c));
    const btns = document.createElement('div');
    btns.className = 'cmbtns';
    const edit = document.createElement('button'); edit.type = 'button'; edit.textContent = 'Edit'; edit.addEventListener('click', () => openCommentDialog(c));
    const del = document.createElement('button'); del.type = 'button'; del.textContent = 'Delete'; del.addEventListener('click', () => run(() => { engine.deleteComment(page, c.id); render(); }));
    btns.append(edit, del);
    li.append(go, btns);
    ol.append(li);
  }
}
$('cmReviewer').onchange = () => renderCommentsPane();
/** The comments the pane shows now (the chosen reviewer's), in the pane's order. */
const shownComments = () => { const who = $('cmReviewer').value; return who ? commentsAll().filter((c) => c.author === who) : commentsAll(); };
function stepComment(dir) {
  const list = shownComments();
  if (!list.length) return;
  const at = list.findIndex((c) => c.page === page && c.shape === cmOnShape && cmOnShape !== null);
  const next = list[(at < 0 ? (dir > 0 ? 0 : list.length - 1) : (at + dir + list.length) % list.length)];
  goToComment(next);
  say(`Comment ${list.indexOf(next) + 1} of ${list.length}: ${next.author}`);
}
$('cmPrev').onclick = () => stepComment(-1);
$('cmNext').onclick = () => stepComment(1);
function goToComment(c) {
  if (c.page >= 0 && c.page !== page) showPage(c.page);
  cmOnShape = c.shape;
  if (c.shape && frames.has(c.shape)) setSelection([c.shape]); else setSelection([]);
  renderCommentsPane();
}
function addCommentMarkers(ov) {
  if (!svgEl()) return;
  const seen = new Set();
  for (const c of commentsAll()) {
    if (c.page !== page || !c.shape || seen.has(c.shape) || !frames.has(c.shape)) continue;
    seen.add(c.shape);
    const box = boxOf(c.shape);
    if (!box) continue;
    const n = commentsAll().filter((o) => o.page === page && o.shape === c.shape).length;
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'cmtag'; b.textContent = n > 1 ? `C${n}` : 'C';
    b.setAttribute('aria-label', `${n} comment${n > 1 ? 's' : ''} on shape ${c.shape}. Show them.`);
    b.title = `${n} comment${n > 1 ? 's' : ''}`;
    Object.assign(b.style, { left: `${box.x - 6}px`, top: `${Math.max(0, box.y - 16)}px` });
    b.addEventListener('pointerdown', (event) => event.stopPropagation());
    b.addEventListener('click', (event) => { event.stopPropagation(); cmOnShape = c.shape; $('showComments').checked = true; renderCommentsPane(); $('cmList').querySelector('li.on .cmgo')?.focus(); });
    ov.append(b);
  }
}
let cmEditing = null; // the comment being edited, or null for a new one
let cmTargetShape = null;
function openCommentDialog(comment) {
  cmEditing = comment;
  cmTargetShape = comment ? comment.shape : (selection.length === 1 ? selection[0] : null);
  $('cmTitle').textContent = comment ? 'Edit comment' : 'New comment';
  $('cmTarget').textContent = comment ? commentTarget(comment) : (cmTargetShape ? `On shape ${cmTargetShape}` : 'On the page');
  $('cmAuthor').value = comment ? comment.author : storedAuthor();
  $('cmAuthor').disabled = !!comment;
  $('cmText').value = comment ? comment.text : '';
  $('cmMsg').textContent = '';
  $('commentDialog').showModal();
  ($('cmAuthor').value && !comment ? $('cmText') : $('cmAuthor').disabled ? $('cmText') : $('cmAuthor')).focus();
}
$('newComment').onclick = () => { if (!svgEl()) { say('Open or create a drawing first.'); return; } openCommentDialog(null); };
$('showComments').addEventListener('change', () => { cmOnShape = null; renderCommentsPane(); });
$('cmCancel').onclick = (event) => { event.preventDefault(); $('commentDialog').close(); };
$('commentForm').onsubmit = (event) => {
  event.preventDefault();
  const text = $('cmText').value;
  const author = $('cmAuthor').value.trim();
  if (!text.trim()) { $('cmMsg').textContent = 'Write the comment first.'; return; }
  if (!cmEditing && !author) { $('cmMsg').textContent = 'Give your name: it is stored with the comment.'; return; }
  try {
    say();
    run(() => {
      if (cmEditing) { if (text !== cmEditing.text) engine.setCommentText(page, cmEditing.id, text, nowText()); }
      else { engine.addComment(page, cmTargetShape, author, initialsOf(author), text, nowText()); try { localStorage.setItem('layoutcraft-author', author); } catch { /* private window */ } }
      render();
    });
    $('commentDialog').close();
    if (!cmEditing) { $('showComments').checked = true; cmOnShape = cmTargetShape; renderCommentsPane(); }
  } catch (error) { $('cmMsg').textContent = error.message; }
};

// ---- Document properties: title, subject, author, manager, company, category, keywords, comments (docProps/core.xml, app.xml) ----
const DOC_PROP_FIELDS = { title: 'dpTitle', subject: 'dpSubject', creator: 'dpCreator', manager: 'dpManager', company: 'dpCompany', category: 'dpCategory', keywords: 'dpKeywords', description: 'dpDescription' };
// ---- Define Styles (THEME-07, step 293): the drawing's named styles, apply one to the selection, create or change a custom one ----
function fillStyleForm() {
  const styles = engine.listStyles();
  const keep = $('stStyle').value;
  const options = styles.map((s) => `<option value="${s.id}">${s.name}${s.custom ? '' : ' (built in)'}</option>`).join('');
  $('stStyle').innerHTML = options; $('stBased').innerHTML = options;
  if (styles.some((s) => s.id === keep)) $('stStyle').value = keep;
  const normal = styles.find((s) => s.name === 'Normal'); if (normal) $('stBased').value = normal.id;
  showStyle();
  return styles;
}
function showStyle() {
  const s = engine.listStyles().find((st) => st.id === $('stStyle').value);
  if (!s) return;
  $('stName').value = s.custom ? s.name : '';
  if (/^#[0-9a-f]{6}$/i.test(s.lineColor || '')) $('stLineColor').value = s.lineColor;
  if (s.lineWeight) $('stLineWeight').value = Math.round(parseFloat(s.lineWeight) * 72 * 100) / 100;
  if (/^#[0-9a-f]{6}$/i.test(s.fillColor || '')) $('stFillColor').value = s.fillColor;
  if (/^#[0-9a-f]{6}$/i.test(s.textColor || '')) $('stTextColor').value = s.textColor;
  if (s.textSize) $('stFontSize').value = Math.round(parseFloat(s.textSize) * 72 * 100) / 100;
  const bits = Number(s.textStyle || 0);
  $('stBold').checked = !!(bits & 1); $('stItalic').checked = !!(bits & 2);
  // paragraph cells and the style this one is based on (a custom style shows its own base; a built-in one offers Normal)
  // (a built-in style's own values are not copied into a new style: the fields stay empty unless the person sets them)
  $('stAlign').value = s.custom ? (s.paraAlign ?? '') : ''; $('stIndLeft').value = s.custom ? (s.indentLeft ?? '') : ''; $('stLineSp').value = s.custom && Number(s.lineSpacing) < 0 ? String(-Number(s.lineSpacing)) : '';
  if (s.custom && s.line && [...$('stBased').options].some((o) => o.value === s.line)) $('stBased').value = s.line;
  else { const normal = engine.listStyles().find((st) => st.name === 'Normal'); if (normal) $('stBased').value = normal.id; }
}
$('stylesBtn').onclick = () => run(() => { fillStyleForm(); $('stylesDialog').showModal(); });
$('stDelete').onclick = () => run(() => {
  const s = engine.listStyles().find((st) => st.id === $('stStyle').value);
  if (!s) throw new Error('Choose a style first');
  if (!s.custom) throw new Error(`'${s.name}' is a built-in style: it cannot be deleted`);
  engine.deleteStyle(s.id);
  render(); fillStyleForm();
  say(`Style '${s.name}' deleted`);
});
$('stClose').onclick = () => $('stylesDialog').close();
$('stStyle').onchange = showStyle;
$('stApply').onclick = () => run(() => {
  if (!selection.length) throw new Error('Select a shape first');
  engine.applyStyle(page, selection, $('stStyle').value, $('stScope').value);
  render(); say(`Applied the style to ${selection.length} shape(s)`);
});
$('stSave').onclick = () => run(() => {
  const name = $('stName').value.trim();
  if (!name) throw new Error('Type a name for the style');
  engine.defineStyle({ name, basedOn: $('stBased').value, lineColor: $('stLineColor').value, lineWeight: Number($('stLineWeight').value), fillColor: $('stFillColor').value, textColor: $('stTextColor').value, fontSize: Number($('stFontSize').value), bold: $('stBold').checked, italic: $('stItalic').checked, ...($('stAlign').value !== '' ? { align: Number($('stAlign').value) } : {}), ...($('stIndLeft').value !== '' ? { indentLeft: Number($('stIndLeft').value) } : {}), ...($('stLineSp').value !== '' ? { lineSpacing: Number($('stLineSp').value) } : {}) });
  fillStyleForm();
  const made = engine.listStyles().find((s) => s.name === name); if (made) { $('stStyle').value = made.id; showStyle(); }
  say(`Style '${name}' saved`);
});
$('docPropsBtn').onclick = () => run(() => {
  const props = engine.docProps();
  for (const [key, id] of Object.entries(DOC_PROP_FIELDS)) $(id).value = props[key] || '';
  $('docPropsDialog').showModal();
});
$('dpCancel').onclick = (event) => { event.preventDefault(); $('docPropsDialog').close(); };
$('docPropsForm').onsubmit = (event) => {
  event.preventDefault();
  const props = {};
  for (const [key, id] of Object.entries(DOC_PROP_FIELDS)) props[key] = $(id).value;
  const before = engine.docProps();
  if (Object.keys(props).every((key) => props[key] === (before[key] || ''))) { $('docPropsDialog').close(); return; } // nothing changed: no undo step
  try {
    say();
    engine.setDocProps(props); render();
    $('docPropsDialog').close();
  } catch (error) { say(error.message); }
};

// ---- New drawing: a small dialog (page size, orientation, unit, name) and engine.newDrawing --------------------
function openNewDialog() {
  $('newCustom').hidden = $('newSize').value !== 'custom';
  $('newDialog').showModal();
}
$('newBtn').onclick = openNewDialog;
$('newSize').onchange = () => { $('newCustom').hidden = $('newSize').value !== 'custom'; };
$('newCancel').onclick = (event) => { event.preventDefault(); $('newDialog').close(); };
$('newForm').onsubmit = (event) => {
  event.preventDefault();
  const metric = $('newUnit').value === 'mm';
  let w, h;
  if ($('newSize').value === 'custom') {
    const k = metric ? 1 / 25.4 : 1;
    w = Number($('newW').value) * k; h = Number($('newH').value) * k;
  } else [w, h] = $('newSize').value.split('x').map(Number);
  const [short, long] = w <= h ? [w, h] : [h, w];
  [w, h] = $('newOrientation').value === 'portrait' ? [short, long] : [long, short];
  try {
    say();
    engine.newDrawing({ width: w, height: h, metric, name: $('newName').value });
    $('newDialog').close();
    page = 0; selection = []; selected = null;
    $('fixedPage').checked = true; $('file').value = '';
    loadMasters(); useBuiltinStencil(true); render(); fit();
  } catch (error) { say(error.message); }
};
window.addEventListener('keydown', (event) => {
  if (event.ctrlKey && event.altKey && event.key.toLowerCase() === 'n') { event.preventDefault(); openNewDialog(); }
});

$('file').onchange = async (event) => {
  const file = event.target.files[0];
  if (!file) return;
  say();
  const bytes = await file.arrayBuffer();
  // A file that cannot be read leaves the open drawing exactly as it was (the engine keeps its state when an import fails).
  let note = '';
  try {
    if (/\.excalidraw$/i.test(file.name)) {
      // an Excalidraw scene becomes a new drawing; a file that is no scene is refused before the open drawing is touched
      const text = new TextDecoder().decode(bytes);
      const before = engine.pageCount() ? engine.export() : null;
      try { const res = importExcalidraw(engine, text); const n = Object.values(res.skipped).reduce((a, b) => a + b, 0); note = n ? `Opened ${file.name}; not imported: ${Object.entries(res.skipped).map(([k, v]) => `${v} ${k}`).join(', ')}.` : ''; }
      catch (error) { if (before) engine.import(before); throw error; }
    } else if (/\.drawio$/i.test(file.name)) {
      // a draw.io file (plain or compressed diagrams) becomes a drawing with one page per diagram; a bad file leaves the open drawing as it was
      const before = engine.pageCount() ? engine.export() : null;
      try {
        const res = await importDrawio(engine, bytes);
        const parts = [...Object.entries(res.approximated).map(([k, v]) => `${v} ${k} drawn as rectangle`), ...Object.entries(res.skipped).map(([k, v]) => `${v} ${k} not imported`)];
        note = parts.length ? `Opened ${file.name}; ${parts.join(', ')}.` : '';
      } catch (error) { if (before) engine.import(before); throw error; }
    } else engine.import(bytes);
  } catch (error) {
    say(`Could not open ${file.name}: ${error.message}${engine.pageCount() ? ' The drawing that was open is unchanged.' : ''}`);
    event.target.value = '';
    return;
  }
  event.target.value = '';
  page = 0; selection = []; selected = null;
  try { loadMasters(); render(); fit(); if (note) say(note); } catch (error) { say(`Opened ${file.name}, but showing it failed: ${error.message}`); }
};
$('print').onchange = () => run(render);
$('undo').onclick = () => run(() => { engine.undo(); render(); });
$('redo').onclick = () => run(() => { engine.redo(); render(); });
$('zin').onclick = () => zoomAt(1.25, ...stageCentre());
$('zout').onclick = () => zoomAt(1 / 1.25, ...stageCentre());
$('fit').onclick = fit;
$('zoom100').onclick = () => { const c = stageCentre(); zoomAt(1 / zoom, ...c); };
$('zoomWidth').onclick = () => { const svg = svgEl(); if (!svg) return; for (let pass = 0; pass < 2; pass++) setZoom(($('stage').clientWidth - 30) / svg.viewBox.baseVal.width); placePage(); /* a scrollbar that appears at the first zoom narrows the window: measure again */ $('stage').scrollLeft = 0; drawOverlay(); };
// View > Full Screen: the whole shell goes full screen (Esc leaves it, as in the browser); a browser that refuses says so.
$('fullScreen').onclick = () => {
  if (document.fullscreenElement) { document.exitFullscreen?.(); return; }
  const root = document.documentElement;
  Promise.resolve(root.requestFullscreen?.()).catch((error) => say(`Full screen was refused: ${error.message}`));
};
// View > Show > Guides and the task panes (Shapes, Size & Position): view settings, not part of the drawing.
$('showGuides').addEventListener('change', () => { selectedGuide = null; if (svgEl()) drawGuides(); });
$('paneShapes').addEventListener('change', () => { $('stencil').hidden = !$('paneShapes').checked; });
$('paneSizePos').addEventListener('change', () => { updateSizePos(); });
$('save').onclick = () => {
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob([engine.export()], { type: 'application/vnd.ms-visio.drawing' }));
  link.download = 'edited.vsdx';
  link.click();
};
$('apply').onclick = () => run(() => {
  const anchor = $('anchor').value.split(',').map(Number);
  engine.resizeGroup(page, selected, Number($('w').value), Number($('h').value), anchor);
  render();
});

// Screen pixels -> page inches (y up), at the current zoom.
const inchesPerPx = () => {
  const svg = svgEl();
  return (svg.viewBox.baseVal.width / svg.getBoundingClientRect().width) / PX_PER_INCH;
};

// ---- snapping -------------------------------------------------------------------------------
/** Axis-aligned page box (inches) of a shape, also when it or an ancestor is rotated. */
function worldBox(id) {
  const [w, h] = frames.get(id).size;
  const pts = [[0, 0], [w, 0], [w, h], [0, h]].map(([u, v]) => worldPoint(id, u, v));
  const xs = pts.map((q) => q[0]), ys = pts.map((q) => q[1]);
  return { x0: Math.min(...xs), x1: Math.max(...xs), y0: Math.min(...ys), y1: Math.max(...ys) };
}
/** Left/centre/right and bottom/centre/top of every shape the selection may snap to. */
function snapTargets(moving) {
  const ex = new Set(moving);
  const xs = [], ys = [];
  for (const [id, f] of frames) {
    if (ex.has(id) || isBackground(id)) continue;
    let inside = false;
    for (let q = f.parentId; q; q = frames.get(q)?.parentId) if (ex.has(q)) inside = true;
    if (inside) continue;
    const b = worldBox(id);
    xs.push(b.x0, (b.x0 + b.x1) / 2, b.x1);
    ys.push(b.y0, (b.y0 + b.y1) / 2, b.y1);
  }
  return { xs, ys };
}
/** Best snap on one axis: `mv` are the moving selection's three positions, returns {delta, at} or null. */
function snapAxis(mv, shapeTargets, grid, threshold) {
  let best = null;
  const consider = (target, m) => {
    const delta = target - m;
    if (Math.abs(delta) <= threshold && (!best || Math.abs(delta) < Math.abs(best.delta) - 1e-12)) best = { delta, at: target };
  };
  for (const m of mv) {
    if (grid) consider(Math.round(m / grid) * grid, m);
    for (const t of shapeTargets) consider(t, m);
  }
  return best;
}
/** The page delta (inches) for a pointer delta, after snapping; also reports the guide positions. */
function snappedDelta(d, dxIn, dyIn) {
  const grid = $('snapGrid').checked ? Number($('gridStep').value) : 0;
  const shapes = $('snapShapes').checked;
  const gs = $('snapGuides').checked ? guidesOf() : [];
  const gxs = gs.filter((g) => g.vertical).map((g) => g.x), gys = gs.filter((g) => !g.vertical).map((g) => g.y);
  if (!grid && !shapes && !gxs.length && !gys.length) return { dx: dxIn, dy: dyIn };
  const threshold = 8 / pxPerInch();
  const b = d.box;
  const sx = snapAxis([b.x0 + dxIn, (b.x0 + b.x1) / 2 + dxIn, b.x1 + dxIn], [...(shapes ? d.targets.xs : []), ...gxs], grid, threshold);
  const sy = snapAxis([b.y0 + dyIn, (b.y0 + b.y1) / 2 + dyIn, b.y1 + dyIn], [...(shapes ? d.targets.ys : []), ...gys], grid, threshold);
  return { dx: dxIn + (sx ? sx.delta : 0), dy: dyIn + (sy ? sy.delta : 0), gx: sx?.at, gy: sy?.at };
}
function drawGuides(g = {}) {
  const el = $('guides');
  if (!el) return;
  el.innerHTML = '';
  const r = svgEl().getBoundingClientRect(), s = $('stage').getBoundingClientRect();
  const left = r.x - s.x + $('stage').scrollLeft, top = r.y - s.y + $('stage').scrollTop;
  const line = (cls, x, y, w, h) => { const d = document.createElement('div'); d.className = cls; Object.assign(d.style, { left: `${x}px`, top: `${y}px`, width: `${w}px`, height: `${h}px` }); el.append(d); };
  if (g.gx !== undefined) line('guide', toStage(g.gx, 0)[0], top, 1, r.height);
  if (g.gy !== undefined) line('guide', left, toStage(0, g.gy)[1], r.width, 1);
  // the ruler guides of the page (a guide that is being dragged is drawn at its new place)
  for (const guide of $('showGuides').checked ? guidesOf() : []) {
    const at = guideDrag && guideDrag.id === guide.id ? guideDrag.position : null;
    const cls = `vguide${guide.id === selectedGuide ? ' on' : ''}`;
    if (guide.vertical) line(cls, toStage(at ?? guide.x, 0)[0], top, 2, r.height);
    else line(cls, left, toStage(0, at ?? guide.y)[1], r.width, 2);
  }
  if ($('showBreaks').checked) {
    const pb = pageBreaks();
    for (const x of pb.x) line('pbreak v', toStage(x, 0)[0], top, 0, r.height);
    for (const y of pb.y) line('pbreak h', left, toStage(0, y)[1], r.width, 0);
  }
  drawVertexHandles(el);
  if (guideDrag && guideDrag.id === null) { // a new guide being pulled out of a ruler
    if (guideDrag.vertical) line('vguide on', toStage(guideDrag.position, 0)[0], top, 2, r.height);
    else line('vguide on', left, toStage(0, guideDrag.position)[1], r.width, 2);
  }
}

/** Page breaks (RIB-12): where the printed page is cut into sheets, in page inches (x from the left, y from the bottom). The same rule as preparePrint (Visio-measured, step 225):
 * the first sheet shows the drawing from its bottom-left corner, every further sheet moves on by the printable area (paper minus margins) divided by the print scale; nothing
 * is cut when the drawing fits one sheet. */
function pageBreaks() {
  const size = engine.pageSize(page);
  const { w, h } = size;
  const pr = size.print || { margins: [0, 0, 0, 0], scale: [1, 1], fit: false, center: [false, false], pages: [1, 1] };
  const [ml, mr, mt, mb] = pr.margins;
  const [pw, ph] = pr.paper || [w, h];
  const area = Math.max(0.1, pw - ml - mr), areaH = Math.max(0.1, ph - mt - mb);
  const factor = pr.fit ? Math.min(pr.pages[0] * area / w, pr.pages[1] * areaH / h) : pr.scale[0];
  const tiled = w * factor > pw + 1e-6 || h * factor > ph + 1e-6;
  if (!tiled || !(factor > 0)) return { x: [], y: [], sheets: [1, 1] };
  const count = (content, paper, stride) => Math.max(1, 1 + Math.ceil((content - paper) / stride - 1e-9));
  const nx = count(w * factor, pw, area), ny = count(h * factor, ph, areaH);
  const x = [], y = [];
  for (let k = 1; k < nx; k++) x.push((k * area) / factor + 0); // the sheet k starts here; the sheet before it ends (margins overlap) at paper width
  for (let k = 1; k < ny; k++) y.push((k * areaH) / factor);
  return { x: x.filter((v) => v < w), y: y.filter((v) => v < h), sheets: [nx, ny] };
}
$('showBreaks').addEventListener('change', () => { if (svgEl()) drawGuides(); });

// ---- ruler guides: pull one out of a ruler, move it, delete it (Delete key, or drop it on a ruler); shapes snap to them --------------
// A guide is a Type='Guide' page shape (guides.rs): horizontal = PinY, vertical = PinX with a quarter turn.
let guideCache = null;
let selectedGuide = null;
let guideDrag = null; // {id|null (new), vertical, position, moved}
const guidesOf = () => (guideCache ??= (() => { try { return engine.guides(page); } catch { return []; } })());
const RULER_PX = 12; // the width of the ruler band drawn by fixPageView
function pageSizeIn() { const s = engine.pageSize(page); return [s.w, s.h]; }
/** Which ruler band (if any) a window point is in: 'top' (makes a horizontal guide) or 'left' (a vertical one). */
function rulerAt(clientX, clientY) {
  if (!$('showRulers').checked || !$('pagerulers')) return null;
  const [wx, wy] = fromClient(clientX, clientY);
  const [w, h] = pageSizeIn();
  const band = RULER_PX / PX_PER_INCH;
  if (wy > h && wy < h + band && wx > -band && wx < w + band) return 'top';
  if (wx < 0 && wx > -band && wy > -band && wy < h) return 'left';
  return null;
}
function guideAt(clientX, clientY) {
  if (!$('showGuides').checked) return null;
  const s = $('stage').getBoundingClientRect();
  const px = clientX - s.x + $('stage').scrollLeft, py = clientY - s.y + $('stage').scrollTop;
  let best = null;
  for (const g of guidesOf()) {
    const d = g.vertical ? Math.abs(px - toStage(g.x, 0)[0]) : Math.abs(py - toStage(0, g.y)[1]);
    if (d <= 4 && (!best || d < best.d)) best = { g, d };
  }
  return best?.g ?? null;
}
function guidePosition(vertical, clientX, clientY) {
  const [wx, wy] = fromClient(clientX, clientY);
  let p = vertical ? wx : wy;
  if ($('snapGrid').checked) { const step = Number($('gridStep').value); p = Math.round(p / step) * step; }
  return Math.round(p * 1e6) / 1e6;
}
function finishGuideDrag(event) {
  const d = guideDrag;
  guideDrag = null;
  window.removeEventListener('pointermove', onGuideMove, true);
  window.removeEventListener('pointerup', finishGuideDrag, true);
  if (!d) return;
  const [w, h] = pageSizeIn();
  const over = rulerAt(event.clientX, event.clientY);
  const inside = d.vertical ? d.position >= 0 && d.position <= w : d.position >= 0 && d.position <= h;
  run(() => {
    if (d.id === null) {
      if (inside && !over) { engine.addGuide(page, d.vertical, d.position); selectedGuide = engine.guides(page).at(-1)?.id ?? null; $('showGuides').checked = true; } // a new guide shows the guides again
    } else if (over || !inside) {
      engine.deleteGuide(page, d.id); selectedGuide = null; // dropped on a ruler (or off the page): the guide goes
    } else if (d.moved) {
      engine.moveGuide(page, d.id, d.position);
    }
    render();
  });
}
function onGuideMove(event) {
  if (!guideDrag) return;
  guideDrag.position = guidePosition(guideDrag.vertical, event.clientX, event.clientY);
  guideDrag.moved = true;
  drawGuides();
}
$('stage').addEventListener('pointerdown', (event) => {
  if (event.button !== 0 || drawKind !== null || connectMode || connPointMode || textEdit || !svgEl()) return;
  const ruler = rulerAt(event.clientX, event.clientY);
  let hit = null;
  if (!ruler) hit = guideAt(event.clientX, event.clientY);
  if (!ruler && !hit) { if (selectedGuide) { selectedGuide = null; drawGuides(); } return; }
  event.preventDefault();
  event.stopImmediatePropagation();
  if (hit) { selectedGuide = hit.id; setSelection([]); }
  const vertical = hit ? hit.vertical : ruler === 'left';
  guideDrag = { id: hit ? hit.id : null, vertical, position: hit ? (vertical ? hit.x : hit.y) : guidePosition(vertical, event.clientX, event.clientY), moved: false };
  window.addEventListener('pointermove', onGuideMove, true);
  window.addEventListener('pointerup', finishGuideDrag, true);
  drawGuides();
}, true);
window.addEventListener('keydown', (event) => {
  if (!selectedGuide || textEdit || (event.key !== 'Delete' && event.key !== 'Backspace')) return;
  if (['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName)) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  const id = selectedGuide;
  selectedGuide = null;
  run(() => { engine.deleteGuide(page, id); render(); });
}, true);

// ---- draw tools: rectangle, rounded rectangle, ellipse, diamond, triangle, line, text box -------------------------------
// Pick a tool and drag on the page (a plain click makes a default-sized shape); the new shape is selected and the tool
// goes off again. Grid snapping applies. A text box opens its text editor at once.
let drawKind = null;
const DEFAULT_DRAW = { w: 1.5, h: 1 };
function setDrawKind(kind) {
  drawKind = kind;
  pencil = null;
  for (const b of $('drawTools').children) { const on = Number(b.dataset.kind) === kind; b.classList.toggle('active', on); b.setAttribute('aria-pressed', String(on)); }
  $('stage').style.cursor = kind === null ? (connectMode ? 'crosshair' : '') : 'crosshair';
  if (kind !== null) { if (connectMode) setConnectMode(false); if (connPointMode) setConnPointMode(false); setSelection([]); } else drawGuides();
}
$('drawTools').addEventListener('click', (event) => {
  const b = event.target.closest('button');
  if (!b) return;
  const kind = Number(b.dataset.kind);
  setDrawKind(drawKind === kind ? null : kind);
});
function snapPoint(wx, wy) {
  if (!$('snapGrid').checked) return [wx, wy];
  const g = Number($('gridStep').value);
  return [Math.round(wx / g) * g, Math.round(wy / g) * g];
}
// ---- Pencil tool (draw kind 7): a click adds a corner, a double-click or Enter finishes an open polyline, a click on the first corner closes a polygon, Esc cancels ----
let pencil = null; // { pts: [[x, y] in page inches], cursor: [x, y] }
const PENCIL_CLOSE_PX = 10;
function pencilClick(event) {
  const p = snapPoint(...fromClient(event.clientX, event.clientY));
  if (!pencil) pencil = { pts: [], cursor: p };
  if (pencil.pts.length >= 3) {
    const [fx, fy] = toStage(...pencil.pts[0]), at = stagePoint(event);
    if (Math.hypot(fx - at.x, fy - at.y) <= PENCIL_CLOSE_PX) { finishPencil(true); return; }
  }
  const last = pencil.pts[pencil.pts.length - 1];
  if (!last || Math.hypot(last[0] - p[0], last[1] - p[1]) > 1e-6) pencil.pts.push(p);
  pencil.cursor = p;
  drawPencilPreview();
}
function drawPencilPreview() {
  const layer = $('guides');
  layer.innerHTML = '';
  if (!pencil || !pencil.pts.length) return;
  const pts = [...pencil.pts, pencil.cursor].map((p) => toStage(p[0], p[1]));
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('style', 'position:absolute;left:0;top:0;width:1px;height:1px;overflow:visible;pointer-events:none');
  const line = document.createElementNS('http://www.w3.org/2000/svg', 'polyline');
  line.setAttribute('points', pts.map((p) => p.join(',')).join(' ')); line.setAttribute('fill', 'none'); line.setAttribute('stroke', 'var(--accent, #1971c2)'); line.setAttribute('stroke-width', '2'); line.setAttribute('stroke-dasharray', '5 3');
  svg.append(line); layer.append(svg);
  pts.slice(0, pencil.pts.length).forEach((p, i) => {
    const d = document.createElement('div');
    d.className = `dot${i === 0 && pencil.pts.length >= 3 ? ' on' : ''}`;
    d.dataset.pencil = String(i);
    Object.assign(d.style, { left: `${p[0]}px`, top: `${p[1]}px` });
    layer.append(d);
  });
}
function finishPencil(closed) {
  const pts = pencil ? pencil.pts : [];
  pencil = null;
  drawGuides();
  if (pts.length < (closed ? 3 : 2)) { setDrawKind(null); say('Pencil: nothing drawn (a polyline needs two corners, a polygon three).'); return; }
  run(() => {
    let id;
    try { id = engine.drawPolyline(page, pts, closed); } catch (e) { setDrawKind(null); say(String(e.message || e)); return; }
    setDrawKind(null);
    selection = [id];
    render();
    say(closed ? 'Polygon drawn. Edit Points changes its corners.' : 'Polyline drawn. Edit Points changes its corners.');
  });
}
// ---- Freeform tool (draw kind 8): drag a stroke; the stroke is thinned (Douglas-Peucker) and becomes Visio's spline through the kept points ----
function simplifyStroke(pts, tol) {
  if (pts.length < 3) return pts.slice();
  const [a, b] = [pts[0], pts[pts.length - 1]];
  const dx = b[0] - a[0], dy = b[1] - a[1], len = Math.hypot(dx, dy);
  let worst = -1, at = 0;
  for (let i = 1; i < pts.length - 1; i++) {
    const d = len < 1e-9 ? Math.hypot(pts[i][0] - a[0], pts[i][1] - a[1]) : Math.abs((pts[i][0] - a[0]) * dy - (pts[i][1] - a[1]) * dx) / len;
    if (d > worst) { worst = d; at = i; }
  }
  if (worst <= tol) return [a, b];
  return [...simplifyStroke(pts.slice(0, at + 1), tol).slice(0, -1), ...simplifyStroke(pts.slice(at), tol)];
}
/** 4 to 40 well spread points for the spline: thinned by the tolerance, then topped up along the stroke when fewer than 4 are left. */
function strokeToCurvePoints(stroke) {
  let tol = 0.03, pts = simplifyStroke(stroke, tol);
  while (pts.length > 40) { tol *= 1.5; pts = simplifyStroke(stroke, tol); }
  if (pts.length >= 4) return pts;
  const lens = [0];
  for (let i = 1; i < stroke.length; i++) lens.push(lens[i - 1] + Math.hypot(stroke[i][0] - stroke[i - 1][0], stroke[i][1] - stroke[i - 1][1]));
  const total = lens[lens.length - 1], out = [];
  for (let k = 0; k < 4; k++) {
    const target = (total * k) / 3;
    let i = 1;
    while (i < lens.length - 1 && lens[i] < target) i++;
    const span = lens[i] - lens[i - 1] || 1, t = Math.min(1, Math.max(0, (target - lens[i - 1]) / span));
    out.push([stroke[i - 1][0] + (stroke[i][0] - stroke[i - 1][0]) * t, stroke[i - 1][1] + (stroke[i][1] - stroke[i - 1][1]) * t]);
  }
  return out;
}
function finishFreeform(d, event) {
  drawGuides();
  const stroke = [...d.stroke, snapPoint(...fromClient(event.clientX, event.clientY))].filter((p, i, a) => i === 0 || Math.hypot(p[0] - a[i - 1][0], p[1] - a[i - 1][1]) > 1e-6);
  const length = stroke.reduce((s, p, i) => s + (i ? Math.hypot(p[0] - stroke[i - 1][0], p[1] - stroke[i - 1][1]) : 0), 0);
  if (stroke.length < 2 || length < 0.15) { setDrawKind(null); say('Freeform: drag a longer stroke to draw a curve.'); return; }
  const pts = strokeToCurvePoints(stroke);
  run(() => {
    let id;
    try { id = engine.drawCurve(page, pts); } catch (e) { setDrawKind(null); say(String(e.message || e)); return; }
    setDrawKind(null);
    selection = [id];
    render();
    say('Curve drawn. Edit Points moves its control points.');
  });
}
function startDraw(event) {
  if (drawKind === 7) { pencilClick(event); event.preventDefault(); return; }
  drag = { kind: 'draw', shape: drawKind, x: event.clientX, y: event.clientY, from: fromClient(event.clientX, event.clientY), stroke: drawKind === 8 ? [fromClient(event.clientX, event.clientY)] : null };
  event.preventDefault();
}
const ARC_BOW_RATIO = 0.2071; // the sagitta of a quarter circle over its chord c: r = c / sqrt(2), r * (1 - cos 45 deg) = 0.2071 * c
function drawPreview(d, event) {
  const layer = $('guides');
  layer.innerHTML = '';
  const a = stagePoint({ clientX: d.x, clientY: d.y }), b = stagePoint(event);
  if (d.shape === 8) { // Freeform: the stroke so far, dashed
    const p = fromClient(event.clientX, event.clientY), last = d.stroke[d.stroke.length - 1];
    if (Math.hypot(p[0] - last[0], p[1] - last[1]) > 0.02) d.stroke.push(p);
    const pts = d.stroke.map((q) => toStage(q[0], q[1]));
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('style', 'position:absolute;left:0;top:0;width:1px;height:1px;overflow:visible;pointer-events:none');
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'polyline');
    line.setAttribute('points', pts.map((q) => q.join(',')).join(' ')); line.setAttribute('fill', 'none'); line.setAttribute('stroke', 'var(--accent, #1971c2)'); line.setAttribute('stroke-width', '2'); line.setAttribute('stroke-dasharray', '5 3');
    svg.append(line); layer.append(svg);
    return;
  }
  if (d.shape === 9) { // Arc: the chord and the quarter circle it will get, dashed
    const pa = fromClient(d.x, d.y), pb = fromClient(event.clientX, event.clientY);
    const chord = Math.hypot(pb[0] - pa[0], pb[1] - pa[1]);
    const pts = (chord < 1e-6 ? [pa, pb] : arcSamples(pa, pb, chord * ARC_BOW_RATIO)).map((p) => toStage(p[0], p[1]));
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('style', 'position:absolute;left:0;top:0;width:1px;height:1px;overflow:visible;pointer-events:none');
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'polyline');
    line.setAttribute('points', pts.map((p) => p.join(',')).join(' ')); line.setAttribute('fill', 'none'); line.setAttribute('stroke', 'var(--accent, #1971c2)'); line.setAttribute('stroke-width', '2'); line.setAttribute('stroke-dasharray', '5 3');
    svg.append(line); layer.append(svg);
    return;
  }
  if (d.shape === 6) {
    const line = document.createElement('div');
    line.className = 'drawline';
    Object.assign(line.style, { left: `${a.x}px`, top: `${a.y}px`, width: `${Math.hypot(b.x - a.x, b.y - a.y)}px`, transform: `rotate(${Math.atan2(b.y - a.y, b.x - a.x)}rad)` });
    layer.append(line);
    return;
  }
  const box = document.createElement('div');
  box.className = 'drawprev';
  Object.assign(box.style, { left: `${Math.min(a.x, b.x)}px`, top: `${Math.min(a.y, b.y)}px`, width: `${Math.abs(b.x - a.x)}px`, height: `${Math.abs(b.y - a.y)}px`, borderRadius: d.shape === 2 ? '50%' : d.shape === 1 ? '10px' : '0' });
  layer.append(box);
}
function endDraw(d, event) {
  if (d.shape === 8) { finishFreeform(d, event); return; }
  drawGuides();
  const kind = d.shape;
  let [x0, y0] = snapPoint(...d.from);
  let [x1, y1] = snapPoint(...fromClient(event.clientX, event.clientY));
  if (Math.hypot(event.clientX - d.x, event.clientY - d.y) < 4) { // a click: a default-sized shape around the point
    if (kind === 6 || kind === 9) { x1 = x0 + DEFAULT_DRAW.w; y1 = y0; } else { x1 = x0 + DEFAULT_DRAW.w / 2; y1 = y0 + DEFAULT_DRAW.h / 2; x0 -= DEFAULT_DRAW.w / 2; y0 -= DEFAULT_DRAW.h / 2; }
  }
  run(() => {
    const id = kind === 9 ? engine.drawArc(page, x0, y0, x1, y1, Math.hypot(x1 - x0, y1 - y0) * ARC_BOW_RATIO) : engine.drawShape(page, kind, x0, y0, x1, y1);
    setDrawKind(null);
    selection = [id];
    render();
    if (kind === 5) beginTextEdit(id);
  });
}

// ---- connector tool ---------------------------------------------------------------------
// With the tool on, hovering a shape shows its connection points (green dots); dragging from one
// shape to another adds a right-angle connector glued at both ends: to the connection point
// nearest the pointer when it is within 16 px, else to the whole shape.
let connectMode = false;
const SITE_PX = 16;
function setConnectMode(on) {
  if (on && handMode) setHandMode(false);
  if (on && drawKind !== null) setDrawKind(null);
  connectMode = on;
  $('connectTool').classList.toggle('active', on);
  $('connectTool').setAttribute('aria-pressed', String(on));
  $('stage').style.cursor = on ? 'crosshair' : '';
  if (on) setSelection([]); else drawGuides();
}
$('connectTool').onclick = () => setConnectMode(!connectMode);
// Connection Point tool (round 11b; Visio: Home > Tools > Connection Point): a click on a shape adds a connection point at the click (Visio writes a new
// Connection row, see connpoints.rs); hovering a shape shows its points. One undo step per point; Esc leaves the tool.
let connPointMode = false;
let cpSelected = null; // {id, n}: the connection point a click selected in the tool (Delete removes it)
function setConnPointMode(on) {
  if (on && connectMode) setConnectMode(false);
  if (on && handMode) setHandMode(false);
  if (on && drawKind !== null) setDrawKind(null);
  connPointMode = on;
  cpSelected = null;
  $('connPointTool').classList.toggle('active', on);
  $('connPointTool').setAttribute('aria-pressed', String(on));
  $('stage').style.cursor = on ? 'crosshair' : '';
  if (on) setSelection([]); else drawGuides();
}
$('connPointTool').onclick = () => setConnPointMode(!connPointMode);
/** The point (wx, wy) of the page as (fx, fy) = fractions of shape `id`'s own box (0..1 from its bottom left), whatever its turn or flip. */
function boxFractions(id, wx, wy) {
  const f = frames.get(id);
  if (!f || !(f.size[0] > 1e-9) || !(f.size[1] > 1e-9)) return null;
  const o = worldPoint(id, 0, 0), a = worldPoint(id, 1, 0), b = worldPoint(id, 0, 1);
  const ax = a[0] - o[0], ay = a[1] - o[1], bx = b[0] - o[0], by = b[1] - o[1];
  const det = ax * by - ay * bx;
  if (Math.abs(det) < 1e-12) return null;
  const dx = wx - o[0], dy = wy - o[1];
  const u = (dx * by - dy * bx) / det, v = (ax * dy - ay * dx) / det;
  const clamp = (t) => Math.min(1, Math.max(0, t));
  return [clamp((u - 0) / f.size[0]), clamp(v / f.size[1])];
}
/** Pointer down in the Connection Point tool: on an existing point of the shape under the pointer (within 16 px) it is dragged, anywhere else on a shape a new point is added. */
function startConnPoint(event) {
  const id = shapeUnder(event);
  const near = id ? nearestSite(sitesOf(id), stagePoint(event)) : null;
  if (id && near && frames.get(id)?.editable) { drag = { kind: 'cpmove', id, n: near.n, x: event.clientX, y: event.clientY }; event.preventDefault(); return; }
  addConnectionPointAt(event);
}
function addConnectionPointAt(event) {
  const id = shapeUnder(event);
  if (!id) { say('Click a shape to add a connection point'); return; }
  if (!frames.get(id)?.editable) { say('This shape cannot be edited'); return; }
  const [wx, wy] = fromClient(event.clientX, event.clientY);
  const fr = boxFractions(id, wx, wy);
  if (!fr) { say('This shape has no size to put a connection point on'); return; }
  run(() => { const n = engine.addConnectionPoint(page, id, fr[0], fr[1]); render(); say(n ? `Connection point ${n} added` : 'No connection point was added'); });
  event.preventDefault();
}
// Edit Points (round 12, GEOM-15; Visio: Shape > Edit Vertices): the vertices of the selected freeform shape are handles; a drag moves one (grid snap),
// a click on a segment adds a vertex there, a click on a vertex selects it and Delete removes it; each edit is one undo step, the box follows the vertices.
function setEditPointsMode(on) {
  if (on) {
    if (connectMode) setConnectMode(false);
    if (connPointMode) setConnPointMode(false);
    if (handMode) setHandMode(false);
    if (drawKind !== null) setDrawKind(null);
    if (changeShapeMode) setChangeShapeMode(false);
    editPoints = { id: selected, v: null, b: null };
  }
  editPointsMode = on;
  $('editPointsBtn').classList.toggle('active', on);
  $('editPointsBtn').setAttribute('aria-pressed', String(on));
  $('stage').style.cursor = on ? 'crosshair' : '';
  if (on) say('Edit Points: drag a vertex, click a segment to add one, drag a segment to bend it into an arc, select a vertex and press Delete (Esc leaves).');
  if (svgEl()) drawGuides();
}
$('fitCurveBtn').onclick = () => {
  const id = selected;
  if (!id || $('fitCurveBtn').disabled) return;
  run(() => { try { engine.fitCurve(page, id); render(); say('Fit Curve: the polyline is now a smooth curve through its corners. Edit Points moves its control points.'); } catch (e) { say(String(e.message || e)); } });
};
$('editPointsBtn').onclick = () => { if (editPointsMode) setEditPointsMode(false); else if (!$('editPointsBtn').disabled) setEditPointsMode(true); };
// arcs (step 276): a segment carries a bow = the signed sagitta (inches); the arc's middle is the chord's middle moved by the bow along (dy, -dx) / chord (page inches, y up)
const arcMidPoint = (a, b, bow) => { const dx = b[0] - a[0], dy = b[1] - a[1], c = Math.hypot(dx, dy); return c < 1e-9 ? null : [(a[0] + b[0]) / 2 + (dy / c) * bow, (a[1] + b[1]) / 2 - (dx / c) * bow]; };
const bowOfPoint = (a, b, p) => { const dx = b[0] - a[0], dy = b[1] - a[1], c = Math.hypot(dx, dy); return c < 1e-9 ? 0 : (p[0] - (a[0] + b[0]) / 2) * (dy / c) + (p[1] - (a[1] + b[1]) / 2) * (-dx / c); };
function arcSamples(a, b, bow, n = 24) {
  const dx = b[0] - a[0], dy = b[1] - a[1], chord = Math.hypot(dx, dy);
  if (chord < 1e-9 || Math.abs(bow) < 1e-9) return [a, b];
  const mx = (a[0] + b[0]) / 2, my = (a[1] + b[1]) / 2, nx = dy / chord, ny = -dx / chord;
  const h = (bow * bow - (chord * chord) / 4) / (2 * bow), cx = mx + nx * h, cy = my + ny * h, r = Math.hypot(chord / 2, h);
  const ang = (p) => Math.atan2(p[1] - cy, p[0] - cx), tau = Math.PI * 2, norm = (v) => ((v % tau) + tau) % tau;
  const ps = ang(a), pe = ang(b), pm = ang([mx + nx * bow, my + ny * bow]);
  const sweep = norm(pm - ps) < norm(pe - ps) ? norm(pe - ps) : -norm(ps - pe);
  return Array.from({ length: n + 1 }, (_, i) => { const t = ps + (sweep * i) / n; return [cx + r * Math.cos(t), cy + r * Math.sin(t)]; });
}
function drawVertexHandles(layer) {
  if (!editPointsMode) return;
  const v = frames.get(editPoints.id)?.vertices;
  if (!v) { editPointsMode = false; $('editPointsBtn').classList.remove('active'); $('stage').style.cursor = ''; return; }
  if (v.curve) { // a Bezier / spline: the control polygon (dashed) behind the points
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('style', 'position:absolute;left:0;top:0;width:1px;height:1px;overflow:visible;pointer-events:none');
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'polyline');
    line.setAttribute('points', v.points.map((p) => toStage(p[0], p[1]).join(',')).join(' ')); line.setAttribute('fill', 'none'); line.setAttribute('stroke', '#1971c2'); line.setAttribute('stroke-width', '1'); line.setAttribute('stroke-dasharray', '3 3'); line.setAttribute('opacity', '.7');
    svg.append(line); layer.append(svg);
  }
  v.points.forEach((p, i) => {
    if (v.closed && i === v.points.length - 1) return; // the closing vertex is the first one
    const [px, py] = toStage(p[0], p[1]);
    const d = document.createElement('div');
    const control = v.curve && i > 0 && i < v.points.length - 1;
    d.className = `dot${control ? ' ctl' : ''}${editPoints.v === i || (v.closed && editPoints.v === v.points.length - 1 && i === 0) ? ' on' : ''}`;
    d.dataset.vertex = String(i);
    Object.assign(d.style, { left: `${px}px`, top: `${py}px` });
    layer.append(d);
  });
  (v.bows || []).forEach((bow, i) => { // the handle of an arc segment sits at the middle of the arc
    if (Math.abs(bow) < 1e-9) return;
    const m = arcMidPoint(v.points[i], v.points[i + 1], bow);
    if (!m) return;
    const [px, py] = toStage(m[0], m[1]);
    const d = document.createElement('div');
    d.className = `dot arc${editPoints.b === i ? ' on' : ''}`;
    d.dataset.bow = String(i);
    Object.assign(d.style, { left: `${px}px`, top: `${py}px` });
    layer.append(d);
  });
}
/** Pointer down in the Edit Points mode: on a vertex (within 10 px) it is dragged / selected; on a segment (within 7 px) a vertex is added there. */
function startEditPoint(event) {
  const v = frames.get(editPoints.id)?.vertices;
  if (!v) return;
  const at = stagePoint(event);
  const px = v.points.map((p) => { const [x, y] = toStage(p[0], p[1]); return { x, y }; });
  let best = null;
  px.forEach((p, i) => { if (v.closed && i === px.length - 1) return; const d = Math.hypot(p.x - at.x, p.y - at.y); if (d <= 10 && (!best || d < best.d)) best = { i, d }; });
  if (best) { drag = { kind: 'vmove', id: editPoints.id, v: best.i, x: event.clientX, y: event.clientY }; event.preventDefault(); return; }
  let handle = null; // the handle at the middle of an arc: drag to change the bow, click to select (Delete straightens it)
  (v.bows || []).forEach((bow, i) => {
    if (Math.abs(bow) < 1e-9) return;
    const m = arcMidPoint(v.points[i], v.points[i + 1], bow);
    if (!m) return;
    const [mx, my] = toStage(m[0], m[1]);
    const d = Math.hypot(mx - at.x, my - at.y);
    if (d <= 10 && (!handle || d < handle.d)) handle = { i, d };
  });
  if (handle) { drag = { kind: 'vbow', id: editPoints.id, seg: handle.i, x: event.clientX, y: event.clientY }; event.preventDefault(); return; }
  let seg = null;
  for (let i = 0; i + 1 < px.length; i++) {
    const bow = v.bows?.[i] || 0;
    const pts = (Math.abs(bow) < 1e-9 ? [v.points[i], v.points[i + 1]] : arcSamples(v.points[i], v.points[i + 1], bow)).map((p) => { const [x, y] = toStage(p[0], p[1]); return { x, y }; });
    for (let k = 0; k + 1 < pts.length; k++) {
      const a = pts[k], b = pts[k + 1], dx = b.x - a.x, dy = b.y - a.y, len2 = dx * dx + dy * dy;
      if (len2 < 1e-9) continue;
      const t = Math.min(1, Math.max(0, ((at.x - a.x) * dx + (at.y - a.y) * dy) / len2));
      const d = Math.hypot(a.x + t * dx - at.x, a.y + t * dy - at.y);
      if (d <= 7 && (!seg || d < seg.d)) seg = { i, d, t: (k + t) / (pts.length - 1) };
    }
  }
  event.preventDefault();
  if (!seg || v.curve || v.oval) { editPoints.v = null; editPoints.b = null; drawGuides(); return; } // a curve has only its points (ends and control points) to drag, an oval its four vertices
  // a click adds a vertex there, a drag bends the segment into an arc (decided when the pointer goes up)
  drag = { kind: 'vseg', id: editPoints.id, seg: seg.i, t: seg.t, x: event.clientX, y: event.clientY };
}
$('handTool').onclick = () => { if (!handMode && connectMode) setConnectMode(false); if (!handMode && connPointMode) setConnPointMode(false); setHandMode(!handMode); };
function shapeUnder(event) {
  const id = document.elementFromPoint(event.clientX, event.clientY)?.closest?.('[data-vid]')?.dataset.vid;
  return id && frames.has(id) && !isBackground(id) ? id : null;
}
const worldSiteCache = new Map();
/** The connection points of a shape in stage pixels: [{n, x, y}]. */
function sitesOf(id) {
  if (!worldSiteCache.has(id)) {
    let points = [];
    try { points = engine.connectionPoints(page, id); } catch { /* none */ }
    worldSiteCache.set(id, points);
  }
  return worldSiteCache.get(id).map((p) => { const [x, y] = toStage(p.x, p.y); return { n: p.n, x, y }; });
}
/**
 * The shape a pointer refers to. A connection point sits on a shape's edge, where the pixel under
 * the pointer is often the background or a neighbour, so a pointer within reach of a point of
 * `previous` (the shape whose points are shown) still means that shape.
 */
function resolveTarget(event, previous, exclude) {
  if (previous && previous !== exclude && nearestSite(sitesOf(previous), stagePoint(event))) return previous;
  const under = shapeUnder(event);
  return under && under !== exclude ? under : null;
}
/** Whether shape `id` is `owner` itself or lies inside it. */
function ownOrInside(id, owner) {
  for (let cur = id; cur; cur = frames.get(cur)?.parentId) if (cur === owner) return true;
  return false;
}
function nearestSite(sites, at) {
  let best = null;
  for (const s of sites) { const d = Math.hypot(s.x - at.x, s.y - at.y); if (d <= SITE_PX && (!best || d < best.d)) best = { ...s, d }; }
  return best;
}
function drawConnectFeedback(targetId, at, from) {
  const layer = $('guides');
  layer.innerHTML = '';
  const dot = (s, on) => { const d = document.createElement('div'); d.className = on ? 'dot on' : 'dot'; Object.assign(d.style, { left: `${s.x}px`, top: `${s.y}px` }); layer.append(d); };
  let end = at;
  if (targetId) {
    const box = boxOf(targetId);
    if (box) { const t = document.createElement('div'); t.className = 'target'; Object.assign(t.style, { left: `${box.x}px`, top: `${box.y}px`, width: `${box.w}px`, height: `${box.h}px` }); layer.append(t); }
    const sites = sitesOf(targetId);
    const near = nearestSite(sites, at);
    for (const s of sites) dot(s, near && s.n === near.n);
    if (near) end = near;
  }
  if (from) {
    const dx = end.x - from.pt.x, dy = end.y - from.pt.y;
    const line = document.createElement('div');
    line.className = 'rubber';
    Object.assign(line.style, { left: `${from.pt.x}px`, top: `${from.pt.y}px`, width: `${Math.hypot(dx, dy)}px`, transform: `rotate(${Math.atan2(dy, dx)}rad)` });
    layer.append(line);
  }
}
let connectHint = null; // the shape whose connection points are shown while the tool is idle
function hoverConnect(event) {
  connectHint = resolveTarget(event, connectHint, null);
  drawConnectFeedback(connectHint, stagePoint(event), null);
}
function startConnect(event) {
  const id = resolveTarget(event, connectHint, null);
  if (!id) return;
  const at = stagePoint(event);
  const near = nearestSite(sitesOf(id), at);
  drag = { kind: 'connect', from: { id, n: near ? near.n : 0, pt: near ? { x: near.x, y: near.y } : at } };
  event.preventDefault();
}
function moveConnect(event) {
  drag.candidate = resolveTarget(event, drag.candidate, drag.from.id);
  drawConnectFeedback(drag.candidate, stagePoint(event), drag.from);
}
function endConnect(d, event) {
  const target = resolveTarget(event, d.candidate, d.from.id);
  drawGuides();
  if (!target || target === d.from.id) return;
  const near = nearestSite(sitesOf(target), stagePoint(event));
  run(() => {
    const id = engine.addConnector(page, { id: d.from.id, n: d.from.n }, { id: target, n: near ? near.n : 0 });
    selection = [id];
    render();
  });
}

// Click selects (Shift toggles); dragging moves the selection live; dragging a handle resizes the
// single selected shape; dragging on empty space (or on a page-sized background shape) draws a
// marquee that selects the shapes it fully encloses.
let drag = null;
const isBackground = (id) => {
  const frame = frames.get(id);
  const svg = svgEl();
  if (!frame || !svg) return false;
  const vb = svg.viewBox.baseVal;
  return frame.size[0] * PX_PER_INCH >= vb.width * 0.9 && frame.size[1] * PX_PER_INCH >= vb.height * 0.9;
};
const stagePoint = (event) => {
  const s = $('stage').getBoundingClientRect();
  return { x: event.clientX - s.x + $('stage').scrollLeft, y: event.clientY - s.y + $('stage').scrollTop };
};
// A right click is not a left click: it used to start a move / connect drag whose release the browser's context menu swallowed,
// leaving a stuck drag (the next clicks then did nothing). It now ends any drag and opens no browser menu over the drawing.
$('stage').addEventListener('contextmenu', (event) => {
  if (!svgEl()) return;
  event.preventDefault();
  if (drag) cancelDrag();
});
$('stage').addEventListener('pointerdown', (event) => {
  // a handle under the open text editor (the editor box is as big as the shape, so a corner handle lies on its edge and the press used to hit the box):
  // the press ends the editing, the text is kept, and then grabs the handle (user bug 2026-11-16: a shape with typed text would not resize)
  let hitTarget = event.target;
  if (textEdit && event.button === 0 && document.elementsFromPoint(event.clientX, event.clientY).some((e) => e.matches?.('#ov .h, #ov .rot, #ov .te, #ov .ch'))) {
    endTextEdit();
    hitTarget = document.elementFromPoint(event.clientX, event.clientY) || event.target;
  }
  if (event.button === 2) { if (drag) cancelDrag(); return; }
  if (event.button === 1 || ((spaceDown || handMode) && event.button === 0)) { startPan(event); return; }
  if (drawKind !== null) { startDraw(event); return; }
  if (connectMode) { startConnect(event); return; }
  if (connPointMode) { startConnPoint(event); return; }
  if (editPointsMode) { startEditPoint(event); return; }
  const tallEnd = hitTarget.closest('.te');
  if (tallEnd && selected && frames.get(selected)?.ends) { // an end of a tall 1-D group
    drag = { kind: 'tend', id: selected, end: Number(tallEnd.dataset.end), x: event.clientX, y: event.clientY };
    event.preventDefault();
    return;
  }
  const endHandle = hitTarget.closest('.ch');
  if (endHandle && selected && isConnector(selected)) { // drag a connector end to glue it elsewhere or set it free
    const c = connectorMap.get(selected);
    const end = Number(endHandle.dataset.end);
    const path = connectorPath(selected);
    drag = { kind: 'cend', id: selected, end, x: event.clientX, y: event.clientY, fixed: path[end === 0 ? path.length - 1 : 0], keep: c[end === 0 ? 'end' : 'begin'].glue?.sheet || null };
    event.preventDefault();
    return;
  }
  const handle = hitTarget.closest('.h');
  if (handle && selected) {
    const frame = frames.get(selected);
    drag = { kind: 'resize', id: selected, x: event.clientX, y: event.clientY, fx: Number(handle.dataset.fx), fy: Number(handle.dataset.fy), size: frame.size.slice(), theta: worldAngle(selected), axes: worldAxes(selected), targets: snapTargets([selected]) };
    event.preventDefault();
    return;
  }
  if (hitTarget.closest('.rot') && selected) {
    const frame = frames.get(selected);
    const [px, py] = toStage(...worldPoint(selected, frame.loc[0], frame.loc[1]));
    const at = stagePoint(event);
    drag = { kind: 'rotate', id: selected, x: event.clientX, y: event.clientY, centre: [px, py], theta0: worldAngle(selected), start: Math.atan2(-(at.y - py), at.x - px), node: nodeOf(selected) };
    event.preventDefault();
    return;
  }
  // a connector within a few pixels is hit, but a shape that the pointer is directly over (not a background shape) is on top of it and wins; a connector's own line is its own target
  const overNode = hitTarget.closest?.('[data-vid]');
  const overShape = overNode && !hitTarget.closest('[data-cid]') && !isBackground(overNode.dataset.vid);
  let hitConnector = connectorAt(event, 6);
  if (hitConnector && overShape) { const own = nodeOf(hitConnector); if (!own || !overNode.contains(own)) hitConnector = null; } // a connector that is part of the shape under the pointer stays hittable
  if (hitConnector) { // a connector is selected by its line; a glued one cannot be moved by dragging (its ends follow the shapes), a free line can
    if (event.shiftKey) setSelection(selection.includes(hitConnector) ? selection.filter((id) => id !== hitConnector) : [...selection, hitConnector]);
    else if (!selection.includes(hitConnector)) select(hitConnector);
    if (selection.includes(hitConnector) && !event.shiftKey && movable().length) drag = { kind: 'move', id: hitConnector, x: event.clientX, y: event.clientY, nodes: previewNodes(movable()), ids: movable() };
    return;
  }
  const node = hitTarget.closest('[data-vid]');
  let vid = node?.dataset.vid;
  let sub = null; // pressing a part of a selected group drags the group; only a click without a drag selects the part (Visio)
  if (vid && !event.shiftKey) {
    const first = groupFirst(vid);
    const owner = selection.find((id) => id !== first && id !== vid && frames.has(vid) && (() => { for (let up = frames.get(vid)?.parentId, g = 0; up && g < 64; up = frames.get(up)?.parentId, g++) if (up === id) return true; return false; })());
    if (owner && !selection.includes(first)) { sub = first; vid = owner; } else vid = first;
  } // a click selects the whole group; a second click on a member of the selected group selects the member (Visio)
  if (vid && (event.ctrlKey || event.metaKey) && !event.shiftKey && !isLocked(vid)) { const link = defaultLink(vid); if (link) { event.preventDefault(); openLink(link); return; } } // Ctrl+click follows the default link
  if (vid && isLocked(vid)) vid = undefined; // a shape on a locked layer is not there for the pointer
  if (vid && event.shiftKey) { // toggle membership
    setSelection(selection.includes(vid) ? selection.filter((id) => id !== vid) : [...selection, vid]);
    return;
  }
  if (!vid || (isBackground(vid) && !selection.includes(vid))) {
    // Empty space: a marquee. A plain click on a background shape selects it on release.
    drag = { kind: 'marquee', at: vid || null, x: event.clientX, y: event.clientY, start: stagePoint(event), keep: [] };
    setSelection([]);
    return;
  }
  if (!selection.includes(vid)) select(vid);
  drag = { kind: 'move', id: vid, x: event.clientX, y: event.clientY, nodes: previewNodes(movable()), ids: movable(), sub };
});
window.addEventListener('pointermove', (event) => {
  if (pencil && !drag) { pencil.cursor = snapPoint(...fromClient(event.clientX, event.clientY)); drawPencilPreview(); return; }
  if (connectMode && !drag) { hoverConnect(event); return; }
  if (connPointMode && !drag) { hoverConnect(event); return; }
  if (!drag) return;
  if (drag.kind === 'pan') { movePan(event); return; }
  if (drag.kind === 'connect') { moveConnect(event); return; }
  if (drag.kind === 'vmove') { // a dot follows the pointer (snapped)
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 3 && !drag.active) return;
    drag.active = true;
    drawGuides();
    const [sx, sy] = toStage(...snapPoint(...fromClient(event.clientX, event.clientY)));
    const dot = document.createElement('div'); dot.className = 'dot on'; Object.assign(dot.style, { left: `${sx}px`, top: `${sy}px` }); $('guides').append(dot);
    return;
  }
  if (drag.kind === 'vseg' || drag.kind === 'vbow') { // the segment follows the pointer as an arc (dashed preview)
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 4 && !drag.active) return;
    drag.active = true;
    const v = frames.get(drag.id)?.vertices;
    if (!v) return;
    const a = v.points[drag.seg], b = v.points[drag.seg + 1];
    drawGuides();
    const pts = arcSamples(a, b, bowOfPoint(a, b, fromClient(event.clientX, event.clientY))).map((p) => toStage(p[0], p[1]));
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('style', 'position:absolute;left:0;top:0;width:1px;height:1px;overflow:visible;pointer-events:none');
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'polyline');
    line.setAttribute('points', pts.map((p) => p.join(',')).join(' ')); line.setAttribute('fill', 'none'); line.setAttribute('stroke', '#1971c2'); line.setAttribute('stroke-width', '1.5'); line.setAttribute('stroke-dasharray', '5 3');
    svg.append(line); $('guides').append(svg);
    return;
  }
  if (drag.kind === 'cpmove') { // a dot follows the pointer, kept inside the shape's box
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 3 && !drag.active) return;
    drag.active = true;
    const [wx, wy] = fromClient(event.clientX, event.clientY);
    const fr = boxFractions(drag.id, wx, wy);
    const layer = $('guides');
    layer.innerHTML = '';
    if (fr) {
      const f = frames.get(drag.id);
      const [px, py] = toStage(...worldPoint(drag.id, fr[0] * f.size[0], fr[1] * f.size[1]));
      const d = document.createElement('div'); d.className = 'dot on'; Object.assign(d.style, { left: `${px}px`, top: `${py}px` }); layer.append(d);
    }
    return;
  }
  if (drag.kind === 'draw') { drawPreview(drag, event); return; }
  if (drag.kind === 'tend') { // the other end stays; a dashed line shows the new Begin-End line the shape will stretch and turn to
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 3 && !drag.active) return;
    drag.active = true;
    const ov = $('ov');
    ov.querySelector('.tprev')?.remove();
    const f = frames.get(drag.id);
    const [fx, fy] = toStage(...f.ends[1 - drag.end]);
    const [sx, sy] = snapPoint(...fromClient(event.clientX, event.clientY)); // the dragged end snaps to the grid like any other drag (Snap to grid option)
    const at = { x: toStage(sx, sy)[0], y: toStage(sx, sy)[1] };
    const seg = document.createElement('div');
    seg.className = 'cseg tprev';
    Object.assign(seg.style, { left: `${fx}px`, top: `${fy}px`, width: `${Math.hypot(at.x - fx, at.y - fy)}px`, transform: `rotate(${Math.atan2(at.y - fy, at.x - fx)}rad)` });
    ov.append(seg);
    return;
  }
  if (drag.kind === 'cend') { // the connector end follows the pointer; shapes under it show their connection points
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 3 && !drag.active) return;
    drag.active = true;
    drag.candidate = ownOrInside(resolveTarget(event, drag.candidate, drag.keep), drag.id) ? null : resolveTarget(event, drag.candidate, drag.keep);
    drawConnectFeedback(drag.candidate, stagePoint(event), { pt: { x: drag.fixed[0], y: drag.fixed[1] } });
    return;
  }
  const dx = event.clientX - drag.x, dy = event.clientY - drag.y;
  if (drag.kind === 'move') {
    // a free line has no element of its own to translate: the overlay (its end handles) shows the move, the line follows on release
    if (Math.hypot(dx, dy) < 3 || (!drag.nodes.length && !drag.ids.length)) return;
    if (!drag.box) { // first move: the selection's page box and the shapes it can snap to
      const boxes = drag.ids.map(worldBox);
      drag.box = { x0: Math.min(...boxes.map((b) => b.x0)), x1: Math.max(...boxes.map((b) => b.x1)), y0: Math.min(...boxes.map((b) => b.y0)), y1: Math.max(...boxes.map((b) => b.y1)) };
      drag.targets = snapTargets(drag.ids);
    }
    const ppi = pxPerInch();
    const snap = snappedDelta(drag, dx / ppi, -dy / ppi);
    drag.sdx = snap.dx; drag.sdy = snap.dy; // page inches, y up
    for (const node of drag.nodes) node.style.transform = `translate(${snap.dx * PX_PER_INCH}px, ${-snap.dy * PX_PER_INCH}px)`;
    $('ov').style.transform = `translate(${snap.dx * ppi}px, ${-snap.dy * ppi}px)`;
    drawGuides(snap);
    showConnectorPreview(drag, snap.dx, snap.dy);
  } else if (drag.kind === 'marquee') {
    if (Math.hypot(dx, dy) < 3) return;
    const a = drag.start, b = stagePoint(event);
    const box = { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
    drag.box = box;
    let m = $('marquee');
    if (!m) { m = document.createElement('div'); m.id = 'marquee'; $('ov').append(m); }
    Object.assign(m.style, { left: `${box.x}px`, top: `${box.y}px`, width: `${box.w}px`, height: `${box.h}px` });
  } else if (drag.kind === 'rotate') {
    drag.angle = rotationAt(drag, event);
    previewRotation(drag);
  } else {
    drag.next = resizedSize(drag, dx, dy, event.shiftKey);
    drawOverlay({ rect: drag.next.rect });
    drawGuides(drag.next.guides);
  }
});
/** The angle (radians, page orientation) the rotation handle points to; Shift snaps to 15 degrees. */
function rotationAt(d, event) {
  const at = stagePoint(event);
  let angle = d.theta0 + (Math.atan2(-(at.y - d.centre[1]), at.x - d.centre[0]) - d.start);
  if (event.shiftKey) angle = Math.round(angle / (Math.PI / 12)) * (Math.PI / 12);
  return angle;
}
function previewRotation(d) {
  const turn = d.angle - d.theta0;
  const [px, py] = worldPoint(d.id, ...frames.get(d.id).loc);
  const ux = px * PX_PER_INCH, uy = (pageH - py) * PX_PER_INCH; // the pin in SVG user units
  if (d.node) d.node.style.transform = `translate(${ux}px, ${uy}px) rotate(${-turn}rad) translate(${-ux}px, ${-uy}px)`;
  drawOverlay({ turn });
  $('sel').textContent = `Shape ${d.id}, angle ${(((d.angle * 180) / Math.PI) % 360 + 360) % 360 | 0}°`;
}
/**
 * New size of a shape while a handle is dragged. The pointer delta is taken along the shape's own
 * axes (so rotated shapes resize correctly); the opposite edge stays fixed.
 */
function resizedSize(d, dx, dy, shift = false) {
  const [w, h] = d.size;
  const ppi = pxPerInch();
  const wx = dx / ppi, wy = -dy / ppi; // page inches, y up
  // along the shape's own width and height: the pointer delta on the page directions of its axes (mirrored ones included)
  const du = wx * d.axes.ex[0] + wy * d.axes.ex[1], dv = wx * d.axes.ey[0] + wy * d.axes.ey[1];
  const sn = d.axes.ex[1], cs = d.axes.ex[0];
  const mirrored = d.axes.ex[0] * d.axes.ey[1] - d.axes.ex[1] * d.axes.ey[0] < 0;
  const min = 0.05;
  let nw = d.fx === 1 ? Math.max(min, w + du) : d.fx === 0 ? Math.max(min, w - du) : w;
  let nh = d.fy === 0 ? Math.max(min, h + dv) : d.fy === 1 ? Math.max(min, h - dv) : h;
  const guides = {};
  if (shift && d.fx !== 0.5 && d.fy !== 0.5) {
    // Shift on a corner handle keeps the proportions (the larger change wins); no snapping then.
    const s = Math.max(nw / w, nh / h, min / Math.min(w, h));
    nw = w * s; nh = h * s;
  } else if (!mirrored && Math.abs(Math.sin(2 * Math.atan2(sn, cs))) < 1e-6) {
    // Snapping while resizing, for shapes whose sides are horizontal and vertical on the page (turned by a
    // multiple of 90 degrees): the edge that moves goes to the grid or to an edge/centre of another shape.
    const grid = $('snapGrid').checked ? Number($('gridStep').value) : 0;
    const shapes = $('snapShapes').checked;
    if (grid || shapes) {
      const thr = 8 / ppi;
      const cx = Math.round(cs), sx = Math.round(sn);
      const snapEdge = (local, axisOfU) => { // `local`: the moving edge in the shape's own u (or v) coordinate
        const p = axisOfU ? worldPoint(d.id, local, 0) : worldPoint(d.id, 0, local);
        const xAxis = axisOfU ? cx !== 0 : cx === 0; // which page axis the shape's own axis lies on
        const coord = xAxis ? p[0] : p[1];
        const sign = axisOfU ? (cx !== 0 ? cx : sx) : (cx !== 0 ? cx : -sx);
        const best = snapAxis([coord], shapes ? (xAxis ? d.targets.xs : d.targets.ys) : [], grid, thr);
        return best ? { local: local + best.delta * sign, at: best.at, xAxis } : null;
      };
      if (d.fx !== 0.5) {
        const e = snapEdge(d.fx === 1 ? nw : w - nw, true);
        if (e) { nw = Math.max(min, d.fx === 1 ? e.local : w - e.local); if (e.xAxis) guides.gx = e.at; else guides.gy = e.at; }
      }
      if (d.fy !== 0.5) {
        const e = snapEdge(d.fy === 0 ? nh : h - nh, false);
        if (e) { nh = Math.max(min, d.fy === 0 ? e.local : h - e.local); if (e.xAxis) guides.gx = e.at; else guides.gy = e.at; }
      }
    }
  }
  // The rectangle in the old box coordinates: the edge opposite the handle stays put.
  const u0 = d.fx === 1 ? 0 : d.fx === 0 ? w - nw : (w - nw) / 2, v0 = d.fy === 0 ? 0 : d.fy === 1 ? h - nh : (h - nh) / 2;
  return { w: nw, h: nh, rect: { u0, v0, u1: u0 + nw, v1: v0 + nh }, guides };
}
/** Shapes whose box lies completely inside the marquee (topmost only: groups beat their members). */
function enclosed(box) {
  const hit = [];
  for (const id of frames.keys()) {
    const b = boxOf(id);
    if (!b || isBackground(id)) continue;
    if (b.x >= box.x && b.y >= box.y && b.x + b.w <= box.x + box.w && b.y + b.h <= box.y + box.h) hit.push(id);
  }
  const set = new Set(hit);
  return hit.filter((id) => { for (let p = frames.get(id)?.parentId; p; p = frames.get(p)?.parentId) if (set.has(p)) return false; return true; });
}
window.addEventListener('pointerup', (event) => {
  if (!drag) return;
  const d = drag; drag = null;
  if (d.kind === 'pan') { $('stage').style.cursor = spaceDown || handMode ? 'grab' : (connectMode ? 'crosshair' : ''); return; }
  if (d.kind === 'connect') { endConnect(d, event); return; }
  if (d.kind === 'vmove') {
    if (!d.active) { editPoints.v = d.v; drawGuides(); say(`Vertex ${d.v + 1} selected. Press Delete to remove it.`); return; }
    const [x, y] = snapPoint(...fromClient(event.clientX, event.clientY));
    editPoints.v = d.v;
    run(() => { engine.moveVertex(page, d.id, d.v, x, y); render(); say('Vertex moved'); });
    return;
  }
  if (d.kind === 'vseg' || d.kind === 'vbow') {
    const v = frames.get(d.id)?.vertices;
    if (!v) return;
    const a = v.points[d.seg], b = v.points[d.seg + 1];
    if (!d.active) {
      if (d.kind === 'vbow') { editPoints.b = d.seg; editPoints.v = null; drawGuides(); say('Arc handle selected. Drag it to bend the arc, press Delete to straighten the segment.'); return; }
      const bow0 = v.bows?.[d.seg] || 0;
      const at = Math.abs(bow0) < 1e-9 ? snapPoint(a[0] + d.t * (b[0] - a[0]), a[1] + d.t * (b[1] - a[1])) : fromClient(event.clientX, event.clientY); // an arc is split at its nearest point
      run(() => { engine.insertVertex(page, d.id, d.seg, at[0], at[1]); editPoints.v = d.seg + 1; editPoints.b = null; render(); say('Vertex added'); });
      return;
    }
    const bow = bowOfPoint(a, b, fromClient(event.clientX, event.clientY));
    run(() => { engine.setSegmentBow(page, d.id, d.seg, bow); editPoints.b = Math.abs(bow) < 1e-6 ? null : d.seg; editPoints.v = null; render(); say(Math.abs(bow) < 1e-6 ? 'Segment straight' : 'Segment bent into an arc'); });
    return;
  }
  if (d.kind === 'cpmove') {
    $('guides').innerHTML = '';
    if (!d.active) { // a click on a point (no drag) selects it: Delete removes it
      cpSelected = { id: d.id, n: d.n };
      const site = sitesOf(d.id).find((s) => s.n === d.n);
      if (site) { const dot = document.createElement('div'); dot.className = 'dot on'; Object.assign(dot.style, { left: `${site.x}px`, top: `${site.y}px` }); $('guides').append(dot); }
      say(`Connection point ${d.n} selected. Press Delete to remove it.`);
      return;
    }
    cpSelected = null;
    const [wx, wy] = fromClient(event.clientX, event.clientY);
    const fr = boxFractions(d.id, wx, wy);
    if (fr) run(() => { engine.moveConnectionPoint(page, d.id, d.n, fr[0], fr[1]); render(); say(`Connection point ${d.n} moved`); });
    return;
  }
  if (d.kind === 'draw') { endDraw(d, event); return; }
  if (d.kind === 'tend') {
    $('ov').querySelector('.tprev')?.remove();
    if (!d.active) return;
    run(() => {
      const [x, y] = snapPoint(...fromClient(event.clientX, event.clientY));
      engine.moveConnectorEnd(page, d.id, d.end, { x, y });
      selection = [d.id];
      render();
    });
    return;
  }
  if (d.kind === 'cend') {
    drawGuides();
    if (!d.active) return;
    const found = resolveTarget(event, d.candidate, d.keep);
    // a connector (a one-dimensional group such as a stencil rack server included) is never glued to itself or to its own parts
    // (a one-dimensional group that turns by its ends - its frame lists them - is never glued either: the end goes where the pointer is)
    const target = ownOrInside(found, d.id) || frames.get(d.id)?.ends ? null : found;
    if (!target && d.keep && shapeUnder(event) === d.keep) { say('A connector joins two different shapes: its other end is already on this one.'); return; }
    const near = target && nearestSite(sitesOf(target), stagePoint(event));
    run(() => {
      if (target) engine.moveConnectorEnd(page, d.id, d.end, { id: target, n: near ? near.n : 0 });
      else { const [x, y] = fromClient(event.clientX, event.clientY); engine.moveConnectorEnd(page, d.id, d.end, { x, y }); }
      selection = [d.id];
      render();
    });
    return;
  }
  const dx = event.clientX - d.x, dy = event.clientY - d.y;
  const moved = Math.hypot(dx, dy) >= 3;
  if (d.kind === 'marquee') {
    $('marquee')?.remove();
    if (!moved) { if (d.at) select(d.at); return; }
    setSelection(enclosed(d.box));
    return;
  }
  $('ov').style.transform = '';
  clearConnectorPreview(d);
  drawGuides();
  if (d.kind === 'rotate') {
    if (!moved || d.angle === undefined) { if (d.node) d.node.style.transform = ''; select(d.id); return; }
    run(() => { engine.rotateShape(page, d.id, d.angle); render(); });
    return;
  }
  if (!moved) { for (const node of d.nodes || []) node.style.transform = ''; if (d.sub && d.kind === 'move') select(d.sub); else drawOverlay(); return; }
  const k = inchesPerPx();
  if (d.kind === 'move') {
    const ids = movable();
    if (!ids.length) { for (const node of d.nodes) node.style.transform = ''; drawOverlay(); return; }
    const mx = d.sdx !== undefined ? d.sdx : dx * k, my = d.sdy !== undefined ? d.sdy : -dy * k;
    drawGuides();
    run(() => { engine.moveMembers(page, ids, mx, my, $('refit').checked); render(); });
  } else {
    const next = resizedSize(d, dx, dy, event.shiftKey);
    const anchor = [d.fx === 0 ? 1 : d.fx === 1 ? 0 : 0.5, d.fy === 0 ? 0 : d.fy === 1 ? 1 : 0.5]; // y up: top handle keeps the bottom edge
    run(() => {
      engine.resizeMember(page, d.id, next.w, next.h, anchor, $('refit').checked);
      render();
    });
  }
});

// ---- in-place text editing ----------------------------------------------------------------
// Double-click (or F2) opens a text box over the shape. Ctrl+Enter, Escape or clicking away
// commits (as in Visio); Enter is a new line. Only the characters that changed are rewritten, so
// the shape's formatting runs stay. The text of a group lives in one of its members.
let textEdit = null;
let recentRange = null;
function beginTextEdit(id) {
  if (textEdit || !frames.get(id)) return;
  let info;
  try { say(); info = engine.textInfo(page, id); } catch (error) { say(error.message); return; }
  const box = boxOf(id);
  const ta = document.createElement('textarea');
  ta.id = 'textedit';
  ta.spellcheck = true; // the browser's own spell check, in the text edit box only (user decision d); nothing is drawn in the SVG
  ta.setAttribute('spellcheck', 'true');
  ta.value = info.text;
  const w = Math.max(box.w, 140), h = Math.max(box.h, 44);
  Object.assign(ta.style, { left: `${box.x + box.w / 2 - w / 2}px`, top: `${box.y + box.h / 2 - h / 2}px`, width: `${w}px`, height: `${h}px` });
  $('stage').append(ta);
  ta.focus();
  ta.select();
  textEdit = { id: info.owner, shape: id, original: info.text, ta };
  ta.addEventListener('keydown', (event) => {
    event.stopPropagation();
    if (event.key === 'Escape' || ((event.ctrlKey || event.metaKey) && event.key === 'Enter')) { event.preventDefault(); endTextEdit(); }
  });
  ta.addEventListener('blur', () => endTextEdit());
  for (const type of ['select', 'keyup', 'mouseup', 'input']) ta.addEventListener(type, showTextState);
  showTextState();
}
// The text controls show the formatting at the caret (the character before it) or of the selected characters while
// the text editor is open: pressed buttons for bold / italic / underline / strike / bullet / alignment, "mixed" when
// the selection differs, the size and colour fields. Reads the run and paragraph rows through engine.textState.
function showTextState() {
  const live = textEdit;
  if (!live) return;
  let state;
  try {
    const ta = live.ta;
    const at = (n) => [...ta.value.slice(0, n)].length;
    state = engine.textState(page, live.id, at(ta.selectionStart), at(ta.selectionEnd));
  } catch { return; }
  const c = state.char || {}, p = state.para || {};
  const set = (id, value) => {
    const button = $(id);
    button.setAttribute('aria-pressed', value === 'mixed' ? 'mixed' : String(!!value));
    button.classList.toggle('active', value === true);
    button.classList.toggle('mixed', value === 'mixed');
  };
  const bit = (n) => (c.Style === undefined ? false : c.Style === 'mixed' ? 'mixed' : (Number(c.Style) & n) !== 0);
  set('bold', bit(1)); set('italic', bit(2)); set('underline', bit(4));
  set('strike', c.Strikethru === undefined ? false : c.Strikethru === 'mixed' ? 'mixed' : Number(c.Strikethru) === 1);
  set('bullet', p.Bullet === undefined ? false : p.Bullet === 'mixed' ? 'mixed' : Number(p.Bullet) > 0);
  for (const [id, align] of [['alignL', 0], ['alignC', 1], ['alignR', 2], ['alignJ', 3]]) {
    $(id).classList.toggle('active', p.HorzAlign !== undefined && p.HorzAlign !== 'mixed' && Number(p.HorzAlign) === align);
  }
  const num = (v) => (v !== undefined && v !== 'mixed' && Number.isFinite(Number(v)) ? Number(v) : null);
  if (num(p.IndLeft) !== null) $('indLeft').value = String(Math.round(num(p.IndLeft) * 100) / 100);
  if (num(p.IndFirst) !== null) $('indFirst').value = String(Math.round(num(p.IndFirst) * 100) / 100);
  if (num(p.SpBefore) !== null) $('spBefore').value = String(Math.round(num(p.SpBefore) * 72 * 10) / 10);
  if (num(p.SpAfter) !== null) $('spAfter').value = String(Math.round(num(p.SpAfter) * 72 * 10) / 10);
  if (num(p.SpLine) !== null && num(p.SpLine) < 0) { const m = String(-num(p.SpLine)); if ([...$('lineSp').options].some((o) => Number(o.value) === Number(m))) $('lineSp').value = [...$('lineSp').options].find((o) => Number(o.value) === Number(m)).value; }
  if (typeof p.BulletStr === 'string' && [...$('bulletChar').options].some((o) => o.value === p.BulletStr)) $('bulletChar').value = p.BulletStr;
  // the font name at the caret / of the selection (blank and 'mixed' when the selected characters differ)
  if (typeof c.FontName === 'string') { $('fontFamily').value = c.FontName === 'mixed' ? '' : c.FontName; $('fontFamily').placeholder = c.FontName === 'mixed' ? 'mixed' : ''; }
  if (c.Size !== undefined && c.Size !== 'mixed' && Number.isFinite(Number(c.Size))) $('fontSize').value = String(Math.round(Number(c.Size) * 72 * 10) / 10);
  if (typeof c.Color === 'string' && /^#[0-9a-f]{6}$/i.test(c.Color)) $('textColor').value = c.Color.toLowerCase();
  if (c.Case !== undefined && c.Case !== 'mixed') caseState = Number(c.Case) || 0;
}
function endTextEdit() {
  if (!textEdit) return;
  const edit = textEdit;
  textEdit = null;
  const value = edit.ta.value;
  // Remember the selected part for a moment: a colour or font control clicked next takes the focus (and so ends the
  // editing) before it can apply its change to that range.
  const a = edit.ta.selectionStart, b = edit.ta.selectionEnd;
  recentRange = b > a ? { id: edit.id, start: [...value.slice(0, a)].length, end: [...value.slice(0, b)].length, time: Date.now() } : null;
  edit.ta.remove();
  if (value !== edit.original) run(() => { engine.setText(page, edit.id, value); render(); });
}
$('stage').addEventListener('dblclick', (event) => {
  if (drawKind === 7) { if (pencil) finishPencil(false); return; }
  const node = event.target.closest('[data-vid]');
  if (!node) return;
  select(node.dataset.vid);
  beginTextEdit(node.dataset.vid);
});

// ---- copy / paste / duplicate ---------------------------------------------------------------
// The clipboard lives in the engine (the shapes' XML), so pasting still works after the originals
// are deleted and on another page of the same drawing. Shapes inside groups and connectors cannot be
// copied. Repeated pastes step 0.25 in right and down so copies do not hide each other.
const PASTE_STEP = 0.25;
let pasteCount = 0;
const copyable = () => topmost(selection);
// Rotate / flip act on the selected shapes that sit on the page itself (connectors follow their shapes).
const turnable = () => topmost(selection).filter((id) => !isConnector(id));
function turnSelected(delta) {
  const ids = turnable();
  if (!ids.length) { say('Select shapes to rotate them.'); return; }
  run(() => { engine.rotateShapes(page, ids, delta); render(); });
}
function flipSelected(vertical) {
  const ids = turnable();
  if (!ids.length) { say('Select shapes to flip them.'); return; }
  run(() => { engine.flipShapes(page, ids, vertical); render(); });
}
// Group / ungroup (page-level shapes); the new group, or the freed shapes, are selected afterwards.
function groupSelected() {
  const ids = turnable(); // members of a group may be picked too: they leave it and form a new group on the page (Visio)
  if (ids.length < 2) { say('Select at least two shapes to group them.'); return; }
  run(() => { const group = engine.groupShapes(page, ids); selection = [group]; render(); });
}
function ungroupSelected() {
  const ids = turnable().filter((id) => frames.get(id)?.isGroup);
  if (ids.length !== 1) { say('Select one group to ungroup it.'); return; }
  run(() => { selection = engine.ungroupShape(page, ids[0]); render(); });
}
// Drawing order (Visio's Bring to Front / Send to Back / Forward / Backward): the selected shapes among their siblings, selection kept.
function reorderSelected(how) {
  const ids = topmost(selection);
  if (!ids.length) { say('Select shapes to change their drawing order.'); return; }
  run(() => { engine.reorderShapes(page, ids, how); render(); });
}
for (const [id, how] of [['zFront', 'front'], ['zFwd', 'forward'], ['zBwd', 'backward'], ['zBack', 'back']]) $(id).onclick = () => reorderSelected(how);
// Shape operations (Visio's Operations: Union, Combine, Fragment, Intersect, Subtract) on page-level shapes: the first one selected gives the look.
const OPS = [['opUnion', 'union', 'Union'], ['opCombine', 'combine', 'Combine'], ['opFragment', 'fragment', 'Fragment'], ['opIntersect', 'intersect', 'Intersect'], ['opSubtract', 'subtract', 'Subtract']];
for (const [button, how, label] of OPS) {
  $(button).onclick = () => {
    const ids = turnable().filter((id) => !frames.get(id)?.parentId && !connectorMap.has(id));
    if (ids.length < 2) { say(`Select at least two shapes to ${label.toLowerCase()} them.`); return; }
    run(() => { selection = engine.shapeOperation(page, how, ids); render(); say(`${label}: ${selection.length === 1 ? 'one shape' : selection.length + ' shapes'} made.`); });
  };
}
// Join (Visio: Operations > Join): lines (free lines are connectors in the shell) and shapes of straight segments; one shape, or more when some do not touch.
$('opJoin').onclick = () => {
  const ids = topmost(selection).filter((id) => !frames.get(id)?.parentId && (!connectorMap.has(id) || isFreeLine(id)));
  if (!ids.length) { say('Select lines or open shapes to join them.'); return; }
  run(() => { try { selection = engine.shapeOperation(page, 'join', ids); render(); say(`Join: ${selection.length === 1 ? 'one shape' : selection.length + ' shapes'} made.`); } catch (e) { say(String(e.message || e)); } });
};
// Trim (Visio: Operations > Trim): lines and outlines are cut where they cross; the pieces are selected.
$('opTrim').onclick = () => {
  const ids = topmost(selection).filter((id) => !frames.get(id)?.parentId && (!connectorMap.has(id) || isFreeLine(id)));
  if (ids.length < 2) { say('Select at least two lines or shapes that cross one another to trim them.'); return; }
  run(() => { try { selection = engine.shapeOperation(page, 'trim', ids); render(); say(`Trim: ${selection.length} pieces made.`); } catch (e) { say(String(e.message || e)); } });
};
// ---- AutoConnect (CONN-13, step 288; Visio: Shape.AutoConnect, probe-autoconnect.ps1) -------------------------------------------------------------------------------------
// Pointing at a 2-D shape shows four blue arrows; a click on one places the shape picked in the Shapes panel (else a copy of the shape pointed at) NEXT to it, one avenue
// (the page's AvenueSize, 0.295 in on Visio's A4 default page) away from its edge and centred on its axis, and glues a connector from the first shape to the new one
// (the whole shapes, like the Connector tool). One undo step; the new shape is selected.
const AC_DIRS = [['right', 1, 0], ['left', -1, 0], ['up', 0, 1], ['down', 0, -1]];
function acEnabled() {
  return $('autoConnectToggle').checked && !editPointsMode && !connectMode && !connPointMode && drawKind === null && !drag && !handMode && !textEdit && !painter && !pencil;
}
function acHide() { acShape = null; const layer = $('acLayer'); if (layer) layer.innerHTML = ''; }
function acShow(id) {
  if (acShape === id || !$('acLayer')) return;
  acHide();
  const f = frames.get(id), b = boxOf(id);
  if (!f || !b || f.parentId || isConnector(id) || f.ends || isLocked(id)) return;
  acShape = id;
  for (const [dir, dx, dy] of AC_DIRS) {
    const a = document.createElement('div');
    a.className = `acarrow ${dir}`;
    a.dataset.dir = dir;
    a.title = `AutoConnect: ${armedMaster ? 'place the picked shape' : 'copy this shape'} ${dir === 'up' ? 'above' : dir === 'down' ? 'below' : 'to the ' + dir} and connect them`;
    Object.assign(a.style, { left: `${b.x + b.w / 2 + dx * (b.w / 2 + 16)}px`, top: `${b.y + b.h / 2 - dy * (b.h / 2 + 16)}px` });
    a.addEventListener('pointerenter', () => { clearTimeout(acTimer); acTimer = null; });
    for (const type of ['pointerdown', 'mousedown']) a.addEventListener(type, (event) => { event.preventDefault(); event.stopPropagation(); if (type === 'pointerdown') acPress(id, dir, event); });
    $('acLayer').append(a);
  }
}
window.addEventListener('pointermove', (event) => {
  if (!acEnabled()) { if (acShape) acHide(); return; }
  if (event.target.closest?.('#acLayer')) { clearTimeout(acTimer); acTimer = null; return; }
  const id = shapeUnder(event);
  if (id) { clearTimeout(acTimer); acTimer = null; acShow(id); } else if (acShape && !acTimer) acTimer = setTimeout(() => { acTimer = null; acHide(); }, 250);
});
function armedSize() {
  if (!armedMaster) return null;
  const list = armedMaster.stencil === 'doc' ? masterList : stencils.find((s) => String(s.index) === armedMaster.stencil)?.masters ?? [];
  return list.find((m) => String(m.id) === armedMaster.master) || null;
}
/**
 * A press on an arrow (step 338, Visio: AutoConnect to an existing shape): released where it was pressed (moved at most 6 px) it is the click of before (a new shape is placed and connected);
 * dragged and released over ANOTHER shape it glues a connector from the first shape to that one (whole shapes), nothing new is placed; released anywhere else it does nothing.
 */
function acPress(id, dir, event) {
  const x0 = event.clientX, y0 = event.clientY;
  const finish = (e) => {
    window.removeEventListener('pointerup', finish, true);
    window.removeEventListener('pointercancel', finish, true);
    if (e.type === 'pointercancel') { acHide(); return; }
    if (Math.hypot(e.clientX - x0, e.clientY - y0) <= 6) { autoConnect(id, dir); return; }
    const target = shapeUnder(e);
    acHide();
    if (!target || target === id || isConnector(target)) { say('AutoConnect: drop the arrow on another shape to connect the two.'); return; }
    run(() => {
      const connector = engine.addConnector(page, { id, n: 0 }, { id: target, n: 0 });
      selection = [connector];
      render();
      say('AutoConnect: connected to the shape.');
    });
  };
  window.addEventListener('pointerup', finish, true);
  window.addEventListener('pointercancel', finish, true);
}
function autoConnect(id, dir) {
  acHide();
  const armed = armedSize();
  const pick = armed ? {
    size: [armed.w, armed.h],
    place: (x, y) => {
      if (armedMaster.stencil === 'doc') return engine.addMaster(page, armed.id, x, y);
      const created = engine.dropStencilMaster(page, Number(armedMaster.stencil), armed.id, x, y);
      masterList = engine.masters();
      return created;
    },
  } : null;
  run(() => {
    let created;
    try { created = engine.autoConnect(page, id, dir, pick); } catch (e) { say(String(e.message || e)); return; }
    selection = [created];
    render();
    say('AutoConnect: shape placed and connected.');
  });
}
$('autoConnectToggle').addEventListener('change', () => { if (!$('autoConnectToggle').checked) acHide(); });
$('grp').onclick = groupSelected;
$('ungrp').onclick = ungroupSelected;
// Align and distribute (page-level shapes): aligned to the FIRST shape selected, like Visio; distribute needs three.
function arrange(kind, arg) {
  const ids = turnable().filter((id) => !frames.get(id)?.parentId);
  const need = kind === 'distribute' ? 3 : 2;
  if (ids.length < need) { say(`Select at least ${need} shapes of the page to ${kind === 'distribute' ? 'distribute' : 'align'} them.`); return; }
  run(() => { if (kind === 'distribute') engine.distributeShapes(page, ids, arg); else engine.alignShapes(page, ids, arg); render(); });
}
for (const [id, mode] of [['arrL', 0], ['arrCx', 1], ['arrRt', 2], ['arrT', 3], ['arrM', 4], ['arrB', 5]]) $(id).onclick = () => arrange('align', mode);
$('arrDistH').onclick = () => arrange('distribute', false);
$('arrDistV').onclick = () => arrange('distribute', true);
// Same size: every other page-level shape gets the width and / or height of the FIRST one selected (kept about its pin, one undo step).
function sameSize(w, h) {
  const ids = turnable().filter((id) => !frames.get(id)?.parentId);
  if (ids.length < 2) { say('Select at least 2 shapes of the page to make them the same size.'); return; }
  const first = frames.get(ids[0]);
  run(() => { engine.batch(() => { for (const id of ids.slice(1)) { const f = frames.get(id); engine.resizeMember(page, id, w ? first.size[0] : f.size[0], h ? first.size[1] : f.size[1], [0.5, 0.5]); } }); render(); });
}
$('sameW').onclick = () => sameSize(true, false);
$('sameH').onclick = () => sameSize(false, true);
$('sameWH').onclick = () => sameSize(true, true);
// Lock shape: toggles the move / size / turn / delete protection cells of the selection (all locked -> unlock).
$('lockShape').onclick = () => {
  const ids = topmost(selection);
  if (!ids.length) { say('Select shapes to lock them.'); return; }
  const lock = !ids.every((id) => frames.get(id)?.locked);
  run(() => { engine.styleShapes(page, ids, { lock }); render(); say(lock ? 'Locked.' : 'Unlocked.'); });
};
$('reverseEnds').onclick = () => {
  const ids = selection.filter((id) => isConnector(id));
  if (!ids.length) { say('Select a connector to reverse its ends.'); return; }
  run(() => { engine.batch(() => { for (const id of ids) engine.reverseEnds(page, id); }); render(); say(ids.length === 1 ? 'Reversed the ends of the connector.' : `Reversed the ends of ${ids.length} connectors.`); });
};
$('rotL').onclick = () => turnSelected(Math.PI / 2);
$('rotR').onclick = () => turnSelected(-Math.PI / 2);
$('flipH').onclick = () => flipSelected(false);
$('flipV').onclick = () => flipSelected(true);
function syncClipboardButtons() {
  for (const id of ['rotL', 'rotR', 'flipH', 'flipV']) $(id).disabled = turnable().length === 0;
  const pageLevel = turnable().filter((id) => !frames.get(id)?.parentId).length;
  for (const id of ['arrL', 'arrCx', 'arrRt', 'arrT', 'arrM', 'arrB']) $(id).disabled = pageLevel < 2;
  for (const id of ['sameW', 'sameH', 'sameWH']) $(id).disabled = pageLevel < 2;
  $('lockShape').disabled = selection.length === 0;
  $('reverseEnds').disabled = !selection.some((id) => isConnector(id));
  $('painter').disabled = selection.length !== 1 && !painter;
  for (const id of ['arrDistH', 'arrDistV']) $(id).disabled = pageLevel < 3;
  $('grp').disabled = turnable().length < 2;
  for (const [button] of OPS) $(button).disabled = turnable().filter((id) => !frames.get(id)?.parentId && !connectorMap.has(id)).length < 2;
  $('opJoin').disabled = topmost(selection).filter((id) => !frames.get(id)?.parentId && (!connectorMap.has(id) || isFreeLine(id))).length < 1;
  $('opTrim').disabled = topmost(selection).filter((id) => !frames.get(id)?.parentId && (!connectorMap.has(id) || isFreeLine(id))).length < 2;
  for (const id of ['zFront', 'zFwd', 'zBwd', 'zBack']) $(id).disabled = !selection.length;
  $('dataBtn').disabled = !dataTargets().length;
  $('ungrp').disabled = !(turnable().length === 1 && frames.get(turnable()[0])?.isGroup);
  $('copy').disabled = copyable().length === 0;
  $('cut').disabled = copyable().length === 0;
  $('dup').disabled = copyable().length === 0;
  $('paste').disabled = engine.clipboardCount() === 0;
  // Change Shape: one plain page-level shape (not a connector, not a member of a group) that can be edited
  const changeable = changeableShapes().length > 0;
  $('changeShapeBtn').disabled = !changeable;
  const one = topmost(selection).filter((id) => !isConnector(id) || isFreeLine(id)); // a free line has edit points too (a glued connector has none)
  const editable = one.length === 1 && !!frames.get(one[0])?.vertices && !isLocked(one[0]);
  $('editPointsBtn').disabled = !editable && !editPointsMode;
  const fv = one.length === 1 ? frames.get(one[0])?.vertices : null;
  $('fitCurveBtn').disabled = !(fv && !fv.curve && !fv.oval && !fv.closed && !fv.line && fv.points.length >= 4 && fv.points.length <= 200 && (fv.bows || []).every((b) => Math.abs(b) < 1e-9) && !isLocked(one[0]));
  if (editPointsMode && (!editable || one[0] !== editPoints.id)) setEditPointsMode(false);
  if (!changeable && changeShapeMode) setChangeShapeMode(false);
}
function copySelected() {
  const ids = copyable();
  if (!ids.length) { say('Select shapes to copy them.'); return false; }
  try { say(); engine.copy(page, ids); pasteCount = 0; syncClipboardButtons(); return true; } catch (error) { say(error.message); return false; }
}
// Cut = copy, then delete the originals (one undo step); the first paste lands where they were.
function cutSelected() {
  const ids = copyable();
  if (!copySelected()) return;
  pasteCount = -1;
  selection = [];
  run(() => { engine.deleteShapes(page, ids); render(); });
}
function pasteClipboard() {
  if (!engine.clipboardCount()) { say('Nothing to paste: copy shapes first.'); return; }
  pasteCount += 1;
  run(() => { const ids = engine.paste(page, PASTE_STEP * pasteCount, -PASTE_STEP * pasteCount); selection = ids; render(); });
}
function duplicateSelected() {
  if (!copySelected()) return;
  pasteCount = 1;
  run(() => { const ids = engine.paste(page, PASTE_STEP, -PASTE_STEP); selection = ids; render(); });
}
$('copy').onclick = copySelected;
$('cut').onclick = cutSelected;
$('paste').onclick = pasteClipboard;
$('dup').onclick = duplicateSelected;

// ---- find and replace text (round 7, B5) -----------------------------------------------------------------------
// Searches the text of every shape of the shown page (document order); Find next selects the shape, Replace changes the
// match in the selected shape, Replace all changes every match on the page as ONE undo step (formatting runs stay).
function textShapes() {
  const seen = new Map();
  for (const id of frames.keys()) {
    try { const info = engine.textInfo(page, id); if (info.text && !seen.has(info.owner)) seen.set(info.owner, { owner: info.owner, id, text: info.text }); } catch { /* no text */ }
  }
  return [...seen.values()];
}
const findRegex = (all) => {
  const needle = $('findText').value;
  if (!needle) return null;
  return new RegExp(needle.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), `${$('findCase').checked ? '' : 'i'}${all ? 'g' : ''}`);
};
let findFrom = -1;
function findNext() {
  const re = findRegex(false);
  if (!re) { $('findStatus').textContent = 'Type the text to find.'; return null; }
  const list = textShapes().filter((t) => re.test(t.text));
  if (!list.length) { $('findStatus').textContent = 'Not found.'; return null; }
  const order = [...frames.keys()];
  const at = (t) => order.indexOf(t.id);
  const after = list.find((t) => at(t) > findFrom) || list[0];
  findFrom = at(after);
  $('findStatus').textContent = `${list.length} shape${list.length === 1 ? '' : 's'} match.`;
  select(after.id);
  return after;
}
function replaceIn(shapes, re, withText) {
  run(() => {
    engine.batch(() => { for (const t of shapes) engine.setText(page, t.owner, t.text.replace(re, () => withText)); });
    render();
  });
}
$('findBtn').onclick = () => { $('findDialog').show(); $('findText').focus(); $('findText').select(); };
$('findClose').onclick = () => $('findDialog').close();
$('findNext').onclick = findNext;
$('findText').oninput = () => { findFrom = -1; };
$('findText').onkeydown = (event) => { if (event.key === 'Enter') { event.preventDefault(); findNext(); } };
$('replaceOne').onclick = () => {
  const re = findRegex(false);
  const target = selected && textShapes().find((t) => (t.id === selected || t.owner === selected) && re && re.test(t.text)) || findNext();
  if (!target || !re) return;
  replaceIn([target], re, $('replaceText').value);
  $('findStatus').textContent = 'Replaced 1.';
};
$('replaceAll').onclick = () => {
  const re = findRegex(true);
  if (!re) { $('findStatus').textContent = 'Type the text to find.'; return; }
  const hits = textShapes().filter((t) => { re.lastIndex = 0; return re.test(t.text); });
  if (!hits.length) { $('findStatus').textContent = 'Not found.'; return; }
  let n = 0;
  for (const t of hits) n += (t.text.match(re) || []).length;
  replaceIn(hits, re, $('replaceText').value);
  $('findStatus').textContent = `Replaced ${n} in ${hits.length} shape${hits.length === 1 ? '' : 's'}.`;
};

// ---- format panel ----------------------------------------------------------------------------
// Fill, line and font changes are applied the moment a control changes, to the selected shapes and
// everything inside them (one undo step); the values shown are those of a single selected shape.
let modelCache = null;
const hexOf = (value, fallback) => {
  const v = String(value || '').trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(v)) return v;
  if (/^#[0-9a-f]{3}$/.test(v)) return `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}`;
  return fallback;
};
function updateFormatPanel() {
  const ids = topmost(selection);
  $('format').hidden = ids.length === 0;
  if (ids.length !== 1 || selection.length !== 1) return;
  const id = ids[0];
  modelCache ??= engine.model().pages[page]?.shapes ?? [];
  const shape = modelCache.find((s) => s.visioId === id) || {};
  let info = {};
  try { info = engine.styleInfo(page, id); } catch { /* keep defaults */ }
  const noFill = !shape.fill || shape.fill === 'none';
  $('fillNone').checked = noFill;
  $('fillColor').value = hexOf(shape.fill, '#ffffff');
  $('lineColor').value = hexOf(shape.stroke, '#000000');
  $('lineWeight').value = shape.strokeWidth !== undefined ? (shape.strokeWidth * 0.75).toFixed(2) : info.lineWeight ? (info.lineWeight * 72).toFixed(2) : '';
  $('linePattern').value = String(info.linePattern ?? 1);
  $('fontSize').value = shape.fontSize !== undefined ? (shape.fontSize * 0.75).toFixed(1) : info.fontPt ? info.fontPt.toFixed(1) : '';
  $('fontFamily').value = shape.fontFamily || info.fontFamily || '';
  const pct = (opacity, trans) => Math.round((opacity !== undefined && opacity !== null ? 1 - opacity : trans || 0) * 100);
  $('fillTrans').value = String(pct(shape.fillOpacity, info.fillTrans)); $('fillTransOut').textContent = `${$('fillTrans').value}%`;
  $('lineTrans').value = String(pct(shape.strokeOpacity, info.lineTrans)); $('lineTransOut').textContent = `${$('lineTrans').value}%`;
  $('shadow').checked = !!(shape.shadow || info.shadow);
  if (shape.shadow) {
    $('shadowDx').value = String(Math.round(shape.shadow.dx ?? 4));
    $('shadowDy').value = String(Math.round(shape.shadow.dy ?? 4));
    $('shadowBlur').value = String(Math.round(shape.shadow.blur ?? 4));
    $('shadowColor').value = hexOf(shape.shadow.color, '#000000');
    $('shadowType').value = String(shape.shadow.kind || 1);
    $('shadowScale').value = String(Math.round((shape.shadow.scale || 1) * 100));
    $('shadowAngle').value = String(Math.round((shape.shadow.oblique || 0) * 180 / Math.PI));
  }
  $('fillPattern').value = String(info.fillPattern ?? 1);
  showGradient(info.gradient);
  $('glowPx').value = info.glow ? String(Math.round(info.glow)) : '';
  $('softEdges').value = info.softEdges ? String(Math.round(info.softEdges)) : '';
  $('reflection').checked = !!info.reflection;
  $('bevelType').value = String(Math.round(info.bevel || 0));
  $('lineCap').value = String(info.lineCap ?? 0);
  $('rounding').value = info.rounding ? String(Math.round(info.rounding * 96)) : '';
  for (const id of ['beginArrow', 'endArrow', 'beginArrowSize', 'endArrowSize']) {
    const value = String(info[id] ?? (id.endsWith('Size') ? 2 : 0));
    const select = $(id);
    if (![...select.options].some((o) => o.value === value)) select.append(new Option(`Kind ${value}`, value));
    select.value = value;
  }
  for (const [id, key] of [['bold', 'bold'], ['italic', 'italic'], ['underline', 'underline']]) {
    const on = shape[key] !== undefined ? !!shape[key] : !!info[key];
    $(id).setAttribute('aria-pressed', String(on));
    $(id).classList.toggle('active', on);
  }
  $('textColor').value = hexOf(shape.textColor, '#000000');
}
function applyStyle(style) {
  const ids = topmost(selection);
  if (!ids.length) return;
  run(() => { engine.styleShapes(page, ids, style); render(); });
}
$('painter').onclick = () => {
  if (painter) { setPainter(null); return; }
  const ids = topmost(selection);
  if (ids.length !== 1) { say('Select the shape whose format you want to copy.'); return; }
  updateFormatPanel();
  const style = { fill: $('fillNone').checked ? 'none' : $('fillColor').value, line: $('lineColor').value, pattern: Number($('linePattern').value), fillTrans: Number($('fillTrans').value) / 100, lineTrans: Number($('lineTrans').value) / 100, shadow: $('shadow').checked, textColor: $('textColor').value };
  const pt = Number($('lineWeight').value); if (Number.isFinite(pt) && pt >= 0 && $('lineWeight').value !== '') style.weight = (pt * 96) / 72;
  const fs = Number($('fontSize').value); if (Number.isFinite(fs) && fs > 0) style.fontPt = fs;
  if ($('fontFamily').value) style.fontFamily = $('fontFamily').value;
  for (const k of ['bold', 'italic', 'underline']) style[k] = $(k).classList.contains('active');
  setPainter({ source: ids[0], style });
  say('Format copied: click the shape to format (Esc cancels).');
};
$('fillColor').onchange = () => { $('fillNone').checked = false; applyStyle({ fill: $('fillColor').value }); };
$('fillNone').onchange = () => applyStyle({ fill: $('fillNone').checked ? 'none' : $('fillColor').value });
$('lineColor').onchange = () => applyStyle({ line: $('lineColor').value });
$('lineWeight').onchange = () => { const pt = Number($('lineWeight').value); if (Number.isFinite(pt) && pt >= 0) applyStyle({ weight: (pt * 96) / 72 }); };
$('linePattern').onchange = () => applyStyle({ pattern: Number($('linePattern').value) });
$('fillTrans').oninput = () => { $('fillTransOut').textContent = `${$('fillTrans').value}%`; };
$('lineTrans').oninput = () => { $('lineTransOut').textContent = `${$('lineTrans').value}%`; };
$('fillTrans').onchange = () => applyStyle({ fillTrans: Number($('fillTrans').value) / 100 });
$('lineTrans').onchange = () => applyStyle({ lineTrans: Number($('lineTrans').value) / 100 });
$('shadow').onchange = () => applyStyle({ shadow: $('shadow').checked });
// Fill pattern (1 solid, 2-24 patterns, 25-40 gradients between the fill colour and the second colour), the line cap
// and the corner radius; the shadow in detail (colour, offset, blur) turns the shadow on.
$('fillPattern').onchange = () => applyStyle({ fillPattern: Number($('fillPattern').value), fillBack: $('fillBack').value });
$('fillBack').onchange = () => { const p = Number($('fillPattern').value); applyStyle({ fillBack: $('fillBack').value, fillPattern: p >= 1 ? p : 1 }); $('fillPattern').value = String(p >= 1 ? p : 1); };
// Gradient editor: custom stops (position %, colour, transparency %), the angle, apply / switch off. The stops live in
// gradStops while the panel is open and are read from the selected shape's FillGradient rows (engine.styleInfo().gradient).
let gradStops = [[0, '#ffffff', 0], [1, '#1971c2', 0]];
function renderGradStops() {
  const box = $('gradStops');
  box.innerHTML = '';
  gradStops.forEach((stop, i) => {
    const row = document.createElement('div');
    row.className = 'gradstop';
    const color = document.createElement('input'); color.type = 'color'; color.value = stop[1]; color.setAttribute('aria-label', `Stop ${i + 1} colour`);
    color.onchange = () => { gradStops[i][1] = color.value; };
    const pos = document.createElement('input'); pos.type = 'number'; pos.min = '0'; pos.max = '100'; pos.step = '1'; pos.value = String(Math.round(stop[0] * 100)); pos.style.width = '3.6em'; pos.setAttribute('aria-label', `Stop ${i + 1} position in percent`);
    pos.onchange = () => { gradStops[i][0] = Math.min(1, Math.max(0, (Number(pos.value) || 0) / 100)); };
    const trans = document.createElement('input'); trans.type = 'number'; trans.min = '0'; trans.max = '100'; trans.step = '5'; trans.value = String(Math.round(stop[2] * 100)); trans.style.width = '3.6em'; trans.setAttribute('aria-label', `Stop ${i + 1} transparency in percent`);
    trans.onchange = () => { gradStops[i][2] = Math.min(1, Math.max(0, (Number(trans.value) || 0) / 100)); };
    const del = document.createElement('button'); del.textContent = '×'; del.title = 'Remove this stop'; del.setAttribute('aria-label', `Remove stop ${i + 1}`);
    del.onclick = () => { if (gradStops.length <= 2) { say('A gradient needs at least 2 stops.'); return; } gradStops.splice(i, 1); renderGradStops(); };
    row.append(`${i + 1} `, color, ' at ', pos, '% transp ', trans, '% ', del);
    box.append(row);
  });
}
renderGradStops();
$('gradAdd').onclick = () => {
  if (gradStops.length >= 8) { say('A gradient has at most 8 stops.'); return; }
  const last = gradStops[gradStops.length - 1];
  const before = gradStops[gradStops.length - 2];
  gradStops.splice(gradStops.length - 1, 0, [(before[0] + last[0]) / 2, '#a5d8ff', 0]);
  renderGradStops();
};
$('gradApply').onclick = () => {
  const stops = gradStops.map((s) => [s[0], s[1], s[2]]).sort((a, b) => a[0] - b[0]);
  const angle = Number($('gradAngle').value);
  applyStyle({ gradientStops: stops, gradientAngle: Number.isFinite(angle) ? angle : 0, gradientDir: Number($('gradType').value) });
};
$('gradOff').onclick = () => applyStyle({ gradientEnabled: false });
/** Fill the editor from the selected shape (called by the format panel sync). */
function showGradient(gradient) {
  if (!gradient || !gradient.enabled || !gradient.stops || gradient.stops.length < 2) return;
  gradStops = gradient.stops.map((s) => [Number(s[0]), String(s[1]), Number(s[2]) || 0]).slice(0, 8);
  if (Number.isFinite(gradient.angleDeg)) $('gradAngle').value = String(Math.round(gradient.angleDeg)); { const dir = String(Number.isFinite(gradient.dir) ? gradient.dir : 0); if (![...$('gradType').options].some((o) => o.value === dir)) $('gradType').append(Object.assign(document.createElement('option'), { value: dir, textContent: `Type ${dir}` })); $('gradType').value = dir; }
  renderGradStops();
}

// Effects: glow (radius px, colour), soft edges (radius px), reflection, bevel (type); each writes the effect cells of the selected shapes.
$('glowPx').onchange = () => {
  const v = Number($('glowPx').value);
  if (!Number.isFinite(v) || v < 0 || v > 200) { say('The glow radius must be between 0 and 200 pixels.'); return; }
  applyStyle(v > 0 ? { glowPx: v, glowColor: $('glowColor').value, glowTrans: 0.3 } : { glowPx: 0 });
};
$('glowColor').onchange = () => { if (Number($('glowPx').value) > 0) applyStyle({ glowColor: $('glowColor').value }); };
$('softEdges').onchange = () => {
  const v = Number($('softEdges').value);
  if (!Number.isFinite(v) || v < 0 || v > 200) { say('The soft edge radius must be between 0 and 200 pixels.'); return; }
  applyStyle({ softEdgesPx: v });
};
$('reflection').onchange = () => applyStyle({ reflection: $('reflection').checked });
$('bevelType').onchange = () => applyStyle({ bevel: Number($('bevelType').value), bevelPx: 8 });

$('lineCap').onchange = () => applyStyle({ lineCap: Number($('lineCap').value) });
$('rounding').onchange = () => { const px = Number($('rounding').value); if (Number.isFinite(px) && px >= 0) applyStyle({ rounding: px }); else say('The corner radius must be 0 or more.'); };
const applyShadowDetail = () => {
  applyStyle({ shadowColor: $('shadowColor').value, shadowDx: Number($('shadowDx').value) || 0, shadowDy: Number($('shadowDy').value) || 0, shadowBlur: Math.max(0, Number($('shadowBlur').value) || 0), shadowType: Number($('shadowType').value), shadowScale: Math.max(1, Number($('shadowScale').value) || 100) / 100, shadowAngle: Number($('shadowAngle').value) || 0 });
  $('shadow').checked = true;
};
for (const id of ['shadowColor', 'shadowDx', 'shadowDy', 'shadowBlur', 'shadowType', 'shadowScale', 'shadowAngle']) $(id).onchange = applyShadowDetail;
for (const id of ['beginArrow', 'endArrow', 'beginArrowSize', 'endArrowSize']) $(id).onchange = () => applyStyle({ [id]: Number($(id).value) });
// Text formatting of a RANGE: while the in-place editor is open, the selected characters (a caret formats the whole
// text, a paragraph control the paragraph of the caret); a control used just after the editor closed (a colour or font
// control takes the focus first) still uses the range that was selected; otherwise the whole text of the selected shapes.
function formatText(spec, { paragraph = false } = {}) {
  const live = textEdit;
  let jobs = [];
  if (live) {
    const ta = live.ta;
    const at = (n) => [...ta.value.slice(0, n)].length;
    let start = at(ta.selectionStart), end = at(ta.selectionEnd);
    if (!paragraph && end === start) { start = 0; end = [...ta.value].length; }
    jobs = [{ id: live.id, start, end }];
  } else if (recentRange && Date.now() - recentRange.time < 3000 && !paragraph) {
    jobs = [{ id: recentRange.id, start: recentRange.start, end: recentRange.end }];
  } else {
    for (const id of topmost(selection)) {
      try { const info = engine.textInfo(page, id); jobs.push({ id: info.owner, start: 0, end: [...info.text].length }); } catch { /* no text */ }
    }
  }
  if (!jobs.length) { say('Select a shape, or select text in the text editor, to format it.'); return; }
  const reopen = live ? { shape: live.shape, a: live.ta.selectionStart, b: live.ta.selectionEnd } : null;
  run(() => {
    if (live) {
      // keep what was typed so far, then close the editor without the blur handler running twice
      const value = live.ta.value;
      textEdit = null;
      live.ta.remove();
      if (value !== live.original) engine.setText(page, live.id, value);
    }
    for (const job of jobs) engine.formatText(page, job.id, { ...spec, start: job.start, end: job.end });
    render();
  });
  if (reopen) {
    beginTextEdit(reopen.shape);
    if (textEdit) textEdit.ta.setSelectionRange(reopen.a, reopen.b);
  }
}
// A panel button must not take the focus from the text editor (the selection would be lost).
for (const id of ['bold', 'italic', 'underline', 'strike', 'caseBtn', 'alignL', 'alignC', 'alignR', 'alignJ', 'bullet']) {
  $(id).addEventListener('mousedown', (event) => { if (textEdit) event.preventDefault(); });
}
const hasRange = () => !!textEdit || (recentRange && Date.now() - recentRange.time < 3000);
const STYLE_BIT = { bold: 1, italic: 2, underline: 4 };
const toggleText = (id) => {
  const on = $(id).getAttribute('aria-pressed') !== 'true';
  if (!hasRange()) { applyStyle({ [id]: on }); return; }
  $(id).setAttribute('aria-pressed', String(on));
  $(id).classList.toggle('active', on);
  formatText(on ? { set: STYLE_BIT[id] } : { clear: STYLE_BIT[id] });
};
for (const id of ['bold', 'italic', 'underline']) $(id).onclick = () => toggleText(id);
$('fontFamily').onchange = () => {
  const name = $('fontFamily').value.trim();
  if (!name) { say('Type or pick a font name.'); return; }
  if (hasRange()) formatText({ cells: { Font: name } }); else applyStyle({ fontFamily: name });
};
$('textColor').onchange = () => { if (hasRange()) formatText({ cells: { Color: $('textColor').value } }); else applyStyle({ textColor: $('textColor').value }); };
$('fontSize').onchange = () => {
  const pt = Number($('fontSize').value);
  if (!Number.isFinite(pt) || pt < 1) { say('The font size must be at least 1 pt.'); return; }
  if (hasRange()) formatText({ cells: { Size: String(pt / 72) } }); else applyStyle({ fontPt: pt });
};
$('strike').onclick = () => {
  const on = $('strike').getAttribute('aria-pressed') !== 'true';
  $('strike').setAttribute('aria-pressed', String(on));
  $('strike').classList.toggle('active', on);
  formatText({ cells: { Strikethru: on ? '1' : '0' } });
};
let caseState = 0;
$('caseBtn').onclick = () => { caseState = (caseState + 1) % 3; formatText({ cells: { Case: String(caseState) } }); say(['Normal case', 'ALL CAPITALS', 'Initial Capitals'][caseState]); };
$('spacing').onchange = () => formatText({ cells: { Letterspace: String(Number($('spacing').value) / 72) } });
for (const [id, align] of [['alignL', '0'], ['alignC', '1'], ['alignR', '2'], ['alignJ', '3']]) $(id).onclick = () => formatText({ para: { HorzAlign: align } }, { paragraph: true });
$('bullet').onclick = () => {
  const on = $('bullet').getAttribute('aria-pressed') !== 'true';
  $('bullet').setAttribute('aria-pressed', String(on));
  $('bullet').classList.toggle('active', on);
  formatText({ para: { Bullet: on ? '1' : '0' } }, { paragraph: true });
};

// Paragraph spacing and indents (cells of the paragraph rows, written for the paragraphs the selection touches):
// IndLeft / IndFirst in inches, SpLine as a negative multiple of the line height, SpBefore / SpAfter in points (stored in inches).
const paraNumber = (id) => { const v = Number($(id).value); return Number.isFinite(v) ? v : null; };
$('indLeft').onchange = () => { const v = paraNumber('indLeft'); if (v === null || v < 0) { say('The left indent must be 0 or more.'); return; } formatText({ para: { IndLeft: String(v) } }, { paragraph: true }); };
$('indFirst').onchange = () => { const v = paraNumber('indFirst'); if (v === null) { say('Type the first-line indent in inches (negative for a hanging indent).'); return; } formatText({ para: { IndFirst: String(v) } }, { paragraph: true }); };
$('lineSp').onchange = () => formatText({ para: { SpLine: String(-Number($('lineSp').value)) } }, { paragraph: true });
$('spBefore').onchange = () => { const v = paraNumber('spBefore'); if (v === null || v < 0) { say('The space before must be 0 pt or more.'); return; } formatText({ para: { SpBefore: String(v / 72) } }, { paragraph: true }); };
$('spAfter').onchange = () => { const v = paraNumber('spAfter'); if (v === null || v < 0) { say('The space after must be 0 pt or more.'); return; } formatText({ para: { SpAfter: String(v / 72) } }, { paragraph: true }); };
$('bulletChar').onchange = () => formatText({ para: { Bullet: '1', BulletStr: $('bulletChar').value } }, { paragraph: true });
for (const id of ['bulletChar', 'indLeft', 'indFirst', 'lineSp', 'spBefore', 'spAfter']) {
  $(id).addEventListener('mousedown', (event) => { if (textEdit && event.target.tagName === 'BUTTON') event.preventDefault(); });
}

function deleteSelected() {
  const ids = topmost(selection).filter((id) => !frames.get(id)?.locked);
  if (!ids.length) return;
  selection = [];
  run(() => { engine.deleteShapes(page, ids); render(); });
}
$('del').onclick = () => deleteSelected();

// Arrow keys nudge the selection (Shift = 10x); Delete removes it; Ctrl+A selects all top-level
// shapes; Escape deselects; Ctrl+Z undoes; Ctrl +/- zooms.
/** Names the shape `id` for the live region ("Process, 3 of 12"). */
function shapeName(id) {
  const g = $('stage').querySelector(`svg g[data-vid="${CSS.escape(String(id))}"]`);
  return g?.getAttribute('aria-label') || (isConnector(id) ? `Connector ${id}` : `Shape ${id}`);
}
/** Tab / Shift+Tab on the stage: the next / previous shape of the page (shapes, then connectors). At the ends the focus moves on. */
function tabThroughShapes(event) {
  const list = [...frames.keys()].filter((id) => !frames.get(id)?.parentId && !isLocked(id)).concat([...connectorMap.keys()].filter((id) => !frames.has(id)));
  if (!list.length) return false;
  const at = selected ? list.indexOf(selected) : -1;
  const next = event.shiftKey ? (at < 0 ? list.length - 1 : at - 1) : at + 1;
  if (next < 0 || next >= list.length) return false;
  event.preventDefault();
  select(list[next]);
  say(`${shapeName(list[next])}, ${next + 1} of ${list.length}`);
  return true;
}
/** Arrow keys on a focused resize or rotate handle. */
function keyOnHandle(event) {
  const target = event.target;
  if (selected && target.classList?.contains('te')) { // an end handle of a tall 1-D group: the arrow keys move that end by 0.05 in (Shift 0.5), the shape stretches and turns as when dragging
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[event.key];
    const fr = frames.get(selected);
    if (!d || !fr?.ends) return false;
    event.preventDefault();
    const end = Number(target.dataset.end), step = event.shiftKey ? 0.5 : 0.05;
    const to = { x: fr.ends[end][0] + d[0] * step, y: fr.ends[end][1] + d[1] * step };
    run(() => { engine.moveConnectorEnd(page, selected, end, to); render(); });
    $('ov').querySelector(`.te[data-end="${end}"]`)?.focus();
    say(`${end ? 'End' : 'Begin'} point at ${to.x.toFixed(2)}, ${to.y.toFixed(2)} inches`);
    return true;
  }
  if (selected && target.classList?.contains('ch') && !target.classList.contains('te') && isConnector(selected)) { // an end handle of a line / an unturned flat 1-D group: arrow keys move a LOOSE end by 0.05 in (Shift 0.5)
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[event.key];
    const c = connectorMap.get(selected);
    if (!d || !c) return false;
    event.preventDefault();
    const end = Number(target.dataset.end), which = end ? c.end : c.begin;
    if (which.glue) { say('This end is glued; drag it to move it, or select the shape and use the arrow keys to move the whole shape'); return true; }
    const step = event.shiftKey ? 0.5 : 0.05;
    const to = { x: which.x + d[0] * step, y: which.y + d[1] * step };
    const id = selected;
    run(() => { engine.moveConnectorEnd(page, id, end, to); selection = [id]; render(); });
    $('ov').querySelector(`.ch[data-end="${end}"]`)?.focus();
    say(`${end ? 'End' : 'Begin'} point at ${to.x.toFixed(2)}, ${to.y.toFixed(2)} inches`);
    return true;
  }
  if (!selected || !(target.classList?.contains('h') || target.classList?.contains('rot'))) return false;
  const frame = frames.get(selected);
  if (!frame || !frame.editable) return false;
  if (event.key === 'Tab' || event.key === 'Escape') return false;
  if (target.classList.contains('rot')) {
    const dir = { ArrowLeft: 1, ArrowUp: 1, ArrowRight: -1, ArrowDown: -1 }[event.key];
    if (!dir) return false;
    event.preventDefault();
    const delta = ((event.shiftKey ? 15 : 5) * Math.PI) / 180;
    const turned = (worldAngle(selected) + dir * delta + 2 * Math.PI) % (2 * Math.PI);
    run(() => { engine.rotateShape(page, selected, turned); render(); });
    $('ov').querySelector('.rot')?.focus();
    say(`Rotated to ${Math.round((turned * 180) / Math.PI)} degrees`);
    return true;
  }
  const dir = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] }[event.key];
  if (!dir) return false;
  event.preventDefault();
  const fx = Number(target.dataset.fx), fy = Number(target.dataset.fy);
  const step = event.shiftKey ? 0.5 : 0.05, min = 0.05;
  const axes = worldAxes(selected, 0);
  const du = step * (dir[0] * axes.ex[0] + dir[1] * axes.ex[1]), dv = step * (dir[0] * axes.ey[0] + dir[1] * axes.ey[1]);
  const [w, h] = frame.size;
  const nw = fx === 1 ? Math.max(min, w + du) : fx === 0 ? Math.max(min, w - du) : w;
  const nh = fy === 0 ? Math.max(min, h + dv) : fy === 1 ? Math.max(min, h - dv) : h;
  if (nw === w && nh === h) return true;
  const anchor = [fx === 0 ? 1 : fx === 1 ? 0 : 0.5, fy === 0 ? 0 : fy === 1 ? 1 : 0.5];
  run(() => { engine.resizeMember(page, selected, nw, nh, anchor, $('refit').checked); render(); });
  $('ov').querySelector(`.h[data-fx="${fx}"][data-fy="${fy}"]`)?.focus();
  say(`Size ${nw.toFixed(2)} by ${nh.toFixed(2)} inches`);
  return true;
}
window.addEventListener('keydown', (event) => {
  if (event.target.matches('input, select, textarea')) return;
  if (connPointMode && cpSelected && (event.key === 'Delete' || event.key === 'Backspace')) { // the Connection Point tool: Delete removes the selected point
    event.preventDefault();
    const { id, n } = cpSelected;
    cpSelected = null;
    run(() => { engine.deleteConnectionPoint(page, id, n); render(); say(`Connection point ${n} deleted`); });
    return;
  }
  if (editPointsMode && editPoints.b !== null && editPoints.v === null && (event.key === 'Delete' || event.key === 'Backspace')) { // Edit Points: Delete straightens the selected arc
    event.preventDefault();
    const seg = editPoints.b;
    editPoints.b = null;
    run(() => { try { engine.setSegmentBow(page, editPoints.id, seg, 0); render(); say('Segment straight'); } catch (e) { say(String(e.message || e)); } });
    return;
  }
  if (editPointsMode && editPoints.v !== null && (event.key === 'Delete' || event.key === 'Backspace')) { // Edit Points: Delete removes the selected vertex
    event.preventDefault();
    const v = editPoints.v;
    editPoints.v = null;
    run(() => { try { engine.deleteVertex(page, editPoints.id, v); render(); say('Vertex deleted'); } catch (e) { say(String(e.message || e)); } });
    return;
  }
  if (keyOnHandle(event)) return;
  if (event.key === 'Tab' && event.target === $('stage') && !event.ctrlKey && !event.altKey && tabThroughShapes(event)) return;
  if (event.code === 'Space') { event.preventDefault(); if (!event.repeat) setSpace(true); return; }
  if ((event.ctrlKey || event.metaKey) && ['b', 'i', 'u'].includes(event.key.toLowerCase())) {
    event.preventDefault();
    if (!$('format').hidden) toggleText({ b: 'bold', i: 'italic', u: 'underline' }[event.key.toLowerCase()]);
    return;
  }
  if ((event.ctrlKey || event.metaKey) && (event.key.toLowerCase() === 'y' || (event.shiftKey && event.key.toLowerCase() === 'z'))) { event.preventDefault(); if (engine.redoDepth()) run(() => { engine.redo(); render(); }); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') { event.preventDefault(); if (engine.undoDepth()) run(() => { engine.undo(); render(); }); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') { event.preventDefault(); $('findBtn').click(); return; }
  if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'k') { event.preventDefault(); if (!$('insertHyperlink').disabled) $('insertHyperlink').click(); else say('Select one shape to give it a hyperlink.'); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'c') { event.preventDefault(); copySelected(); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'x') { event.preventDefault(); cutSelected(); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v') { event.preventDefault(); pasteClipboard(); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'd') { event.preventDefault(); duplicateSelected(); return; }
  if ((event.ctrlKey || event.metaKey) && event.shiftKey && ['f', 'b'].includes(event.key.toLowerCase()) && selection.length) { event.preventDefault(); reorderSelected(event.key.toLowerCase() === 'f' ? 'front' : 'back'); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'g') { event.preventDefault(); if (event.shiftKey) ungroupSelected(); else groupSelected(); return; }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') { event.preventDefault(); setSelection([...frames.values()].filter((f) => !f.parentId && !isBackground(f.id)).map((f) => f.id)); return; }
  if ((event.ctrlKey || event.metaKey) && (event.key === '+' || event.key === '=')) { event.preventDefault(); zoomAt(1.25, ...stageCentre()); return; }
  if ((event.ctrlKey || event.metaKey) && event.key === '-') { event.preventDefault(); zoomAt(1 / 1.25, ...stageCentre()); return; }
  if (!event.ctrlKey && !event.metaKey && !event.altKey && ['[', ']', 'h', 'v'].includes(event.key.toLowerCase())) {
    event.preventDefault();
    if (event.key === '[') turnSelected(Math.PI / 2); else if (event.key === ']') turnSelected(-Math.PI / 2); else flipSelected(event.key.toLowerCase() === 'v');
    return;
  }
  if (event.key === 'F2' && selected) { event.preventDefault(); beginTextEdit(selected); return; }
  if (event.key === 'Enter' && drawKind === 7 && pencil) { event.preventDefault(); finishPencil(false); return; }
  if (event.key === 'Escape' && drawKind !== null) { drag = null; setDrawKind(null); return; }
  if (event.key === 'Escape' && painter) { setPainter(null); return; }
  if (event.key === 'Escape') { if (editPointsMode) { drag = null; setEditPointsMode(false); return; } if (changeShapeMode) { setChangeShapeMode(false); return; } if (handMode) { setHandMode(false); return; } if (connectMode) { drag = null; drawGuides(); setConnectMode(false); return; } if (connPointMode) { drag = null; drawGuides(); setConnPointMode(false); return; } select(null); return; }
  if (event.key === 'Delete' || event.key === 'Backspace') { event.preventDefault(); deleteSelected(); return; }
  const step = (event.shiftKey ? 0.5 : 0.05);
  const delta = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] }[event.key];
  const ids = movable();
  if (!delta || !ids.length) return;
  event.preventDefault();
  run(() => { engine.moveMembers(page, ids, delta[0], delta[1], $('refit').checked); render(); });
});
window.addEventListener('resize', () => drawOverlay());
$('stage').addEventListener('scroll', () => drawOverlay());
