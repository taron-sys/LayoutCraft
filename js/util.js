// Small geometry / text helpers shared across the app.

let _seq = 0;
export const uid = (p = 'i') => `${p}${(++_seq).toString(36)}${Math.random().toString(36).slice(2, 6)}`;

export const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
export const dist = (a, b) => Math.hypot(b.x - a.x, b.y - a.y);
export const snapTo = (v, g) => (g > 0 ? Math.round(v / g) * g : v);

export function rot(p, angle = 0) {
  if (!angle) return { x: p.x, y: p.y };
  const c = Math.cos(angle), s = Math.sin(angle);
  return { x: p.x * c - p.y * s, y: p.x * s + p.y * c };
}

export function distToSegment(p, a, b) {
  const dx = b.x - a.x, dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  if (len2 === 0) return dist(p, a);
  let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
  t = clamp(t, 0, 1);
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}





// ---------------------------------------------------------------- text ---
const _mc = document.createElement('canvas');
const _mx = _mc.getContext('2d');

export const FONT_STACK = '"Segoe UI", Calibri, Helvetica, Arial, sans-serif';

export function fontString(st) {
  return `${st.italic ? 'italic ' : ''}${st.bold ? 'bold ' : ''}${st.fontSize || 12}px ${st.fontFamily || FONT_STACK}`;
}

export function measureText(text, font) {
  _mx.font = font;
  return _mx.measureText(text).width;
}

/** Greedy word wrap that honours explicit newlines. */
export function wrapText(text, maxWidth, font) {
  const out = [];
  for (const para of String(text ?? '').split('\n')) {
    if (para === '') { out.push(''); continue; }
    const words = para.split(/\s+/);
    let cur = '';
    for (const w of words) {
      const t = cur ? `${cur} ${w}` : w;
      if (!cur || measureText(t, font) <= maxWidth) cur = t;
      else { out.push(cur); cur = w; }
    }
    out.push(cur);
  }
  return out;
}

// --------------------------------------------------------------- files ---
export function download(filename, data, mime = 'text/plain') {
  const blob = data instanceof Blob ? data : new Blob([data], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

