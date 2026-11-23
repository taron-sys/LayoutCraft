// Ribbon tooltips with their own delay (step 320). The browser's title tooltip appears after about a second and cannot be tuned; for the icon-only buttons (Paste, Cut, Copy, ...)
// the name is the only label, so the ribbon shows its own small tooltip after `delay` ms of hovering (default 900 ms; `localStorage layoutcraft-tip-delay`, or
// `window.layoutcraftTips.delay = 1500`). While the pointer is over a button its `title` is held back in `data-title` (so the browser does not show a second tooltip) and put back when
// the pointer leaves; the accessible name stays (`aria-label` is set for a button whose only label is its title). A button reached with the keyboard (focus-visible) shows it too (step 339).
const AREAS = '#ribbonPanels, #qat, #ribbonRow';
let delay = 900;
try { const saved = Number(localStorage.getItem('layoutcraft-tip-delay')); if (Number.isFinite(saved) && saved >= 0 && saved <= 10000 && localStorage.getItem('layoutcraft-tip-delay') !== null) delay = saved; } catch { /* no storage */ }

let timer = null, current = null, box = null;

function hide() {
  clearTimeout(timer); timer = null;
  if (box) box.hidden = true;
}
/** Put the title back on the button the pointer left (a title written meanwhile, e.g. the Undo depth, wins). */
function release(el) {
  if (!el) return;
  if (el.dataset.title !== undefined) { if (!el.getAttribute('title')) el.setAttribute('title', el.dataset.title); delete el.dataset.title; }
}
function show(el) {
  const text = el.getAttribute('title') || el.dataset.title || '';
  if (el.getAttribute('title')) { el.dataset.title = el.getAttribute('title'); el.removeAttribute('title'); }
  if (!text || !el.isConnected) return;
  if (!box) { box = document.createElement('div'); box.id = 'lcTip'; box.setAttribute('role', 'tooltip'); document.body.append(box); }
  box.textContent = text; box.hidden = false;
  const r = el.getBoundingClientRect(), b = box.getBoundingClientRect();
  box.style.left = `${Math.max(4, Math.min(innerWidth - b.width - 4, r.left + r.width / 2 - b.width / 2))}px`;
  box.style.top = `${r.bottom + 8 + b.height > innerHeight ? Math.max(4, r.top - b.height - 8) : r.bottom + 8}px`;
}

/** Start the tooltip of `el` (the pointer is over it, or the keyboard focus is on it): it shows after the delay. */
function begin(el) {
  if (el === current) return;
  hide(); release(current); current = null;
  if (!el || !el.closest(AREAS)) return;
  current = el;
  const title = el.getAttribute('title');
  if (title) {
    if (!el.getAttribute('aria-label') && !el.textContent.trim()) el.setAttribute('aria-label', title);
    el.dataset.title = title; el.removeAttribute('title'); // no second, native tooltip
  }
  timer = setTimeout(() => { if (current === el) show(el); }, api.delay);
}
document.addEventListener('mouseover', (e) => begin(e.target instanceof Element ? e.target.closest('[title], [data-title]') : null), true);
// keyboard (step 339): a button reached with the keyboard (focus-visible) shows its tooltip after the same delay; moving the focus away or pressing a key hides it
document.addEventListener('focusin', (e) => { if (e.target instanceof Element && e.target.matches(':focus-visible')) begin(e.target.closest('[title], [data-title]')); }, true);
document.addEventListener('focusout', (e) => { if (current && e.target instanceof Element && current === e.target.closest('[title], [data-title]')) { hide(); release(current); current = null; } }, true);
document.addEventListener('mouseout', (e) => {
  if (!current) return;
  const to = e.relatedTarget;
  if (to instanceof Node && current.contains(to)) return;
  hide(); release(current); current = null;
}, true);
for (const type of ['pointerdown', 'keydown', 'wheel', 'blur']) addEventListener(type, () => { hide(); }, true);

const api = { get delay() { return delay; }, set delay(v) { const n = Number(v); if (Number.isFinite(n) && n >= 0 && n <= 10000) { delay = n; try { localStorage.setItem('layoutcraft-tip-delay', String(n)); } catch { /* no storage */ } } }, hide };
window.layoutcraftTips = api;

// View > Tooltips: Quick 400 ms, Normal 900 ms, Slow 1800 ms, Very slow 3000 ms (any other value set by hand shows as the nearest choice)
const pick = document.getElementById('tipDelay');
if (pick) {
  const sync = () => { const opts = [...pick.options].map((o) => Number(o.value)); pick.value = String(opts.reduce((best, v) => (Math.abs(v - api.delay) < Math.abs(best - api.delay) ? v : best), opts[0])); };
  sync();
  pick.addEventListener('change', () => { api.delay = Number(pick.value); });
}
