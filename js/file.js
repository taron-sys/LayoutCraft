// Persistence + export helpers.
import { download } from './util.js';
import { normalizeDocument, ensureDocumentVersion } from './schema.js';

const KEY = 'layoutcraft.document.v1';

export function saveLocal(doc) {
  try {
    localStorage.setItem(KEY, JSON.stringify(ensureDocumentVersion(doc)));
    return true;
  } catch (err) {
    console.warn('autosave failed', err);
    return false;
  }
}

export function loadLocal() {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    return normalizeDocument(JSON.parse(raw));
  } catch (err) {
    if (err && /schema version/i.test(err.message || '')) console.warn(err.message);
    return null;
  }
}

export function exportJSON(doc, name) {
  const out = ensureDocumentVersion(doc);
  download(`${slug(name)}.layoutcraft.json`, JSON.stringify(out, null, 2), 'application/json');
}

export function importJSON(text) {
  return normalizeDocument(JSON.parse(text));
}

const NS = 'http://www.w3.org/2000/svg';

export function pageBounds(pg) {
  let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
  const grow = (x, y) => {
    x1 = Math.min(x1, x); y1 = Math.min(y1, y);
    x2 = Math.max(x2, x); y2 = Math.max(y2, y);
  };
  for (const s of pg.shapes) grow(s.x, s.y), grow(s.x + s.w, s.y + s.h);
  for (const c of pg.connectors) {
    for (const end of [c.from, c.to]) {
      if (end.point) grow(end.point.x, end.point.y);
    }
    for (const w of c.waypoints || []) grow(w.x, w.y);
  }
  if (!isFinite(x1)) return { x: 0, y: 0, w: 200, h: 150 };
  const pad = 20;
  return { x: x1 - pad, y: y1 - pad, w: x2 - x1 + pad * 2, h: y2 - y1 + pad * 2 };
}

/** Build a standalone SVG document for the current page. */
export function buildSVG(app) {
  const pg = app.getPage();
  const b = pageBounds(pg);
  const out = document.createElementNS(NS, 'svg');
  out.setAttribute('xmlns', NS);
  out.setAttribute('width', Math.ceil(b.w));
  out.setAttribute('height', Math.ceil(b.h));
  out.setAttribute('viewBox', `${b.x} ${b.y} ${b.w} ${b.h}`);
  const bg = document.createElementNS(NS, 'rect');
  bg.setAttribute('x', b.x); bg.setAttribute('y', b.y);
  bg.setAttribute('width', b.w); bg.setAttribute('height', b.h);
  bg.setAttribute('fill', '#ffffff');
  out.appendChild(bg);
  const defs = app.svg.querySelector('#canvasDefs');
  if (defs) out.appendChild(defs.cloneNode(true));
  for (const id of ['#layerContent']) {
    const layer = app.svg.querySelector(id);
    if (layer) out.appendChild(layer.cloneNode(true));
  }
  return out;
}

export function exportSVG(app) {
  const out = buildSVG(app);
  const text = new XMLSerializer().serializeToString(out);
  download(`${slug(app.doc.name)}.svg`, `<?xml version="1.0" encoding="UTF-8"?>\n${text}`, 'image/svg+xml');
}

export function exportPNG(app, scale = 2) {
  const out = buildSVG(app);
  const text = new XMLSerializer().serializeToString(out);
  const blob = new Blob([text], { type: 'image/svg+xml;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const img = new Image();
  img.onload = () => {
    const w = out.getAttribute('width'), h = out.getAttribute('height');
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(w * scale));
    canvas.height = Math.max(1, Math.round(h * scale));
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    URL.revokeObjectURL(url);
    canvas.toBlob((b) => {
      const a = document.createElement('a');
      a.href = URL.createObjectURL(b);
      a.download = `${slug(app.doc.name)}.png`;
      document.body.appendChild(a);
      a.click();
      a.remove();
    }, 'image/png');
  };
  img.onerror = () => {
    URL.revokeObjectURL(url);
    alert('Could not rasterise the diagram.');
  };
  img.src = url;
}

function slug(s) {
  return String(s || 'diagram').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'diagram';
}
