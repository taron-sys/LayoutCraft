// H5 (round 8): the right-click menus. Three menus, as in Visio: a shape (or the selection), the empty page, a page tab.
// Every entry runs an EXISTING command by clicking its element (the ribbon controls keep their ids), so an entry is disabled exactly
// when its command is, and nothing here knows how a command works. A check entry toggles a view option (a checkbox of the ribbon).
// Keyboard: the Menu key / Shift+F10 opens the menu at the selection; arrows, Home, End move, Enter runs, Esc and Tab close.
const $ = (id) => document.getElementById(id);

const SHAPE = [
  ['Cut', 'cut'], ['Copy', 'copy'], ['Paste', 'paste'], ['Duplicate', 'dup'], ['Delete', 'del'], '-',
  ['Group', 'grp'], ['Ungroup', 'ungrp'], '-',
  ['Bring to front', 'zFront'], ['Bring forward', 'zFwd'], ['Send backward', 'zBwd'], ['Send to back', 'zBack'], '-',
  ['Rotate left', 'rotL'], ['Rotate right', 'rotR'], ['Flip horizontal', 'flipH'], ['Flip vertical', 'flipV'], '-',
  ['Reverse ends', 'reverseEnds'], ['Lock', 'lockShape'], ['Shape data...', 'dataBtn'], ['Hyperlink...', 'insertHyperlink'], ['New comment', 'newComment'],
];
const PAGE = [
  ['Paste', 'paste'], '-',
  ['Page setup...', 'pageSetup'], ['Fit page to window', 'fit'], ['New comment', 'newComment'], '-',
  ['Grid', 'showGrid', true], ['Rulers', 'showRulers', true], ['Guides', 'showGuides', true], ['Snap to grid', 'snapGrid', true],
];
const TAB = [
  ['Insert page', 'pageAdd'], ['Duplicate page', 'pageDup'], ['Rename page', 'pageRename'], ['Delete page', 'pageDel'], '-',
  ['Move left', 'pageLeft'], ['Move right', 'pageRight'], '-',
  ['Page setup...', 'pageSetup'],
];

let menu = null;
let opener = null;

function close(restoreFocus = true) {
  if (!menu) return;
  menu.remove();
  menu = null;
  window.removeEventListener('pointerdown', outside, true);
  window.removeEventListener('resize', closeNow);
  $('stage')?.removeEventListener('scroll', closeNow);
  if (restoreFocus && opener?.isConnected) opener.focus();
  opener = null;
}
const closeNow = () => close(false);
function outside(event) { if (menu && !menu.contains(event.target)) close(false); }

/** Whether the command behind element `id` can run now: it exists, is not disabled and does not sit in an inert row. */
function enabled(id) {
  const el = $(id);
  return !!el && !el.disabled && !el.closest('[inert]');
}

function open(kind, x, y, items, label) {
  close(false);
  opener = document.activeElement;
  menu = document.createElement('div');
  menu.id = 'ctxmenu';
  menu.setAttribute('role', 'menu');
  menu.setAttribute('aria-label', label);
  menu.dataset.kind = kind;
  for (const entry of items) {
    if (entry === '-') { const sep = document.createElement('div'); sep.setAttribute('role', 'separator'); menu.append(sep); continue; }
    const [text, id, check, action] = entry; // `action`: a function to run instead of clicking element `id`
    const item = document.createElement('button');
    item.type = 'button';
    item.setAttribute('role', check ? 'menuitemcheckbox' : 'menuitem');
    item.tabIndex = -1;
    item.dataset.cmd = id;
    item.textContent = text;
    if (check) item.setAttribute('aria-checked', String(!!$(id)?.checked));
    if (id && !enabled(id)) { item.disabled = true; item.setAttribute('aria-disabled', 'true'); }
    item.addEventListener('click', () => { const el = id ? $(id) : null; close(true); if (action) action(); else if (el && !el.disabled) el.click(); });
    menu.append(item);
  }
  document.body.append(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(0, Math.min(x, window.innerWidth - r.width - 4))}px`;
  menu.style.top = `${Math.max(0, Math.min(y, window.innerHeight - r.height - 4))}px`;
  window.addEventListener('pointerdown', outside, true);
  window.addEventListener('resize', closeNow);
  $('stage')?.addEventListener('scroll', closeNow);
  const first = menu.querySelector('button:not(:disabled)');
  first?.focus();
}

{
  document.addEventListener('keydown', (event) => {
    if (!menu) return;
    const items = [...menu.querySelectorAll('button:not(:disabled)')];
    const at = items.indexOf(document.activeElement);
    const go = (i) => { event.preventDefault(); event.stopPropagation(); items[(i + items.length) % items.length]?.focus(); };
    if (event.key === 'ArrowDown') go(at + 1);
    else if (event.key === 'ArrowUp') go(at < 0 ? -1 : at - 1);
    else if (event.key === 'Home') go(0);
    else if (event.key === 'End') go(-1);
    else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true); }
    else if (event.key === 'Tab') { close(false); }
  }, true);
}

/** Whether the pointer event came from the keyboard (Menu key, Shift+F10): the browser then reports no real position. */
const fromKeyboard = (event) => event.clientX === 0 && event.clientY === 0 && event.button === 0 && !event.pointerType;

$('stage').addEventListener('contextmenu', (event) => {
  const api = window.layoutcraft;
  if (!api || !document.querySelector('#stage svg')) return;
  const svg = document.querySelector('#stage svg').getBoundingClientRect();
  let x = event.clientX, y = event.clientY;
  let hit = event.target.closest?.('[data-vid]')?.dataset.vid || event.target.closest?.('[data-cid]')?.dataset.cid || null;
  const node = hit && (document.querySelector(`[data-vid="${CSS.escape(hit)}"]`) || document.querySelector(`[data-cid="${CSS.escape(hit)}"]`));
  if (node) { // a page-sized background shape counts as empty page, like a left click does
    const r = node.getBoundingClientRect();
    if (r.width >= svg.width * 0.9 && r.height >= svg.height * 0.9) hit = null;
  }
  if (fromKeyboard(event)) { // from the keyboard: at the selection, or at the middle of the page
    const id = api.selection[0];
    const target = id && (document.querySelector(`[data-vid="${CSS.escape(id)}"]`) || document.querySelector(`[data-cid="${CSS.escape(id)}"]`));
    const r = (target || document.querySelector('#stage svg')).getBoundingClientRect();
    x = r.x + r.width / 2; y = r.y + r.height / 2;
    hit = id || null;
  }
  if (hit) { if (!api.selection.includes(hit)) api.setSelection([hit]); } else if (api.selection.length) api.setSelection([]);
  // the selection-bound buttons update in a microtask after the selection changes: build the menu after that
  setTimeout(() => (hit ? open('shape', x, y, [...(window.layoutcraftLinks?.menuItems() ?? []), ...SHAPE], 'Shape') : open('page', x, y, PAGE, 'Page')), 0);
});

$('pageBar').addEventListener('contextmenu', (event) => {
  const tab = event.target.closest?.('#tabs button');
  if (!tab) return;
  event.preventDefault();
  if (!tab.disabled) tab.click(); // the commands act on the shown page: right-clicking another tab shows it first
  setTimeout(() => open('tab', event.clientX, event.clientY, TAB, 'Page'), 50);
});

window.layoutcraftMenu = { close: () => close(true), isOpen: () => !!menu };
