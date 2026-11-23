// Customisable quick access toolbar (step 324). Right-click a ribbon button: "Add to Quick Access Toolbar" puts a copy of it (its icon, its tooltip) in the title bar; right-click the copy: "Remove ...".
// A copy runs its original (a click on the copy clicks the ribbon button, so it does exactly what the button does and is disabled when the button is). The list is remembered
// (localStorage layoutcraft-qat). Save, Undo, Redo and Print stay where they are. "Reset Quick Access Toolbar" removes all added buttons.
const KEY = 'layoutcraft-qat';
const FIXED = new Set(['save', 'undo', 'redo', 'printBtn']);
const qat = document.getElementById('qat');
let ids = [];
try { const saved = JSON.parse(localStorage.getItem(KEY) || '[]'); if (Array.isArray(saved)) ids = saved.filter((x) => typeof x === 'string' && x); } catch { /* no storage */ }
const observers = [];
let menu = null;
let opener = null; // the button the menu was opened from: the keyboard focus goes back to it when the menu closes

const save = () => { try { localStorage.setItem(KEY, JSON.stringify(ids)); } catch { /* no storage */ } };
const nameOf = (el) => (el.getAttribute('aria-label') || el.getAttribute('title') || el.dataset.title || el.textContent || el.id).trim();

function build() {
  for (const o of observers.splice(0)) o.disconnect();
  for (const old of qat.querySelectorAll('.qatx')) old.remove();
  ids = ids.filter((id) => { const t = document.getElementById(id); return t && t.tagName === 'BUTTON' && !FIXED.has(id) && !qat.contains(t); });
  for (const id of ids) {
    const target = document.getElementById(id);
    const proxy = document.createElement('button');
    proxy.type = 'button';
    proxy.className = 'qatx ricononly';
    proxy.dataset.for = id;
    const icon = target.querySelector('.ricon');
    if (icon) proxy.append(icon.cloneNode(true)); else proxy.textContent = nameOf(target);
    const sync = () => {
      proxy.disabled = target.disabled;
      const tip = target.getAttribute('title') || target.dataset.title || nameOf(target);
      proxy.title = tip; proxy.setAttribute('aria-label', nameOf(target));
    };
    sync();
    const o = new MutationObserver(sync);
    o.observe(target, { attributes: true, attributeFilter: ['disabled', 'title', 'aria-label'], subtree: false });
    observers.push(o);
    proxy.addEventListener('click', () => { if (!target.disabled) target.click(); });
    qat.append(proxy);
  }
}

function closeMenu(restoreFocus = false) {
  if (!menu) return;
  menu.remove(); menu = null;
  window.removeEventListener('pointerdown', outside, true);
  window.removeEventListener('keydown', keys, true);
  if (restoreFocus && opener && opener.isConnected && !opener.disabled) opener.focus();
  opener = null;
}
const outside = (e) => { if (menu && !menu.contains(e.target)) closeMenu(); };
const keys = (e) => {
  if (!menu) return;
  if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeMenu(true); return; }
  if (e.key === 'Tab') { closeMenu(false); return; }
  const items = [...menu.querySelectorAll('button')];
  const at = items.indexOf(document.activeElement);
  const go = (i) => { e.preventDefault(); e.stopPropagation(); items[(i + items.length) % items.length]?.focus(); };
  if (e.key === 'ArrowDown') go(at + 1);
  else if (e.key === 'ArrowUp') go(at < 0 ? -1 : at - 1);
  else if (e.key === 'Home') go(0);
  else if (e.key === 'End') go(-1);
};

function openMenu(x, y, items, from = null) {
  closeMenu();
  opener = from;
  menu = document.createElement('div');
  menu.id = 'ctxmenu'; menu.className = 'qatmenu';
  menu.setAttribute('role', 'menu'); menu.setAttribute('aria-label', 'Quick access toolbar');
  for (const [text, run] of items) {
    const b = document.createElement('button');
    b.type = 'button'; b.setAttribute('role', 'menuitem'); b.textContent = text;
    b.addEventListener('click', () => { closeMenu(true); run(); });
    menu.append(b);
  }
  document.body.append(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(0, Math.min(x, innerWidth - r.width - 4))}px`;
  menu.style.top = `${Math.max(0, Math.min(y, innerHeight - r.height - 4))}px`;
  window.addEventListener('pointerdown', outside, true);
  window.addEventListener('keydown', keys, true);
  menu.querySelector('button')?.focus();
}

const api = {
  get ids() { return [...ids]; },
  add(id) { if (!ids.includes(id)) { ids.push(id); save(); build(); } },
  remove(id) { ids = ids.filter((x) => x !== id); save(); build(); },
  reset() { ids = []; save(); build(); },
  reload() { try { const saved = JSON.parse(localStorage.getItem(KEY) || '[]'); ids = Array.isArray(saved) ? saved.filter((x) => typeof x === 'string' && x) : []; } catch { ids = []; } build(); },
};
window.layoutcraftQat = api;

/** The menu for `button` (the right click, or the Menu key / Shift+F10 on a focused button); false when the button has none. */
function menuFor(button, x, y, fromKeyboard) {
  if (!button || !(button.closest('#ribbonPanels') || button.closest('#qat'))) return false;
  const proxy = button.classList.contains('qatx');
  const id = proxy ? button.dataset.for : button.id;
  if (!id || FIXED.has(id) || (!proxy && button.closest('#qat'))) return false; // the fixed four and the tab strip have no menu of ours
  const items = [];
  if (ids.includes(id)) items.push(['Remove from Quick Access Toolbar', () => api.remove(id)]);
  else items.push(['Add to Quick Access Toolbar', () => api.add(id)]);
  if (ids.length) items.push(['Reset Quick Access Toolbar', () => api.reset()]);
  openMenu(x, y, items, fromKeyboard ? button : null);
  return true;
}
document.addEventListener('contextmenu', (e) => {
  const button = e.target instanceof Element ? e.target.closest('button') : null;
  if (menuFor(button, e.clientX, e.clientY, false)) { e.preventDefault(); e.stopPropagation(); }
}, true);
// keyboard (step 342): the Menu key or Shift+F10 on a focused ribbon button opens the same menu below the button
document.addEventListener('keydown', (e) => {
  if (menu || !(e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))) return;
  const button = e.target instanceof Element ? e.target.closest('button') : null;
  if (!button) return;
  const r = button.getBoundingClientRect();
  if (menuFor(button, r.left, r.bottom + 2, true)) { e.preventDefault(); e.stopPropagation(); }
}, true);

build();
