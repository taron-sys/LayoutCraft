// H6 (round 8): the File "backstage" and the Help dialogs.
// The File tab opens a full-window panel with a list on the left (New, Open, Info, Save As, Print, Export, Options) as Visio's File tab does.
// Like the context menus, an entry RUNS AN EXISTING COMMAND by clicking its element (the ribbon controls keep their ids); the panel only
// shows information (Info) and proxies the view options (Options). Options are remembered in localStorage (view settings, not the drawing).
// Help: a keyboard shortcut list (taken from the real key handlers of shell.js) and an About box.
const $ = (id) => document.getElementById(id);
const BUILD = 'round 8';
const KEY = 'layoutcraft-options';

// ---- options remembered between sessions ----
const OPTION_CHECKS = [['snapGrid', 'Snap to grid'], ['snapShapes', 'Snap to shapes'], ['snapGuides', 'Snap to guides'], ['showGrid', 'Show the grid'], ['showRulers', 'Show the rulers'], ['wheelZoom', 'The mouse wheel zooms']];
const readStore = () => { try { return JSON.parse(localStorage.getItem(KEY) || '{}'); } catch { return {}; } };
const writeStore = (o) => { try { localStorage.setItem(KEY, JSON.stringify(o)); } catch { /* private window: the options just last for the session */ } };
function fire(el) { el.dispatchEvent(new Event('change', { bubbles: true })); el.dispatchEvent(new Event('input', { bubbles: true })); }
(function applyStored() {
  const o = readStore();
  for (const [id] of OPTION_CHECKS) if (typeof o[id] === 'boolean' && $(id) && $(id).checked !== o[id]) { $(id).checked = o[id]; fire($(id)); }
  if (o.gridStep && $('gridStep') && [...$('gridStep').options].some((x) => x.value === o.gridStep) && $('gridStep').value !== o.gridStep) { $('gridStep').value = o.gridStep; fire($('gridStep')); }
})();

// ---- the backstage ----
// round 11: Visio's order is Home, New, Open | Info, Save, Save As, Print, Share, Export, Close | Account, Feedback, Options; the entries that exist here keep that order
const SECTIONS = ['New', 'Open', 'Info', 'Save As', 'Print', 'Export', 'Options'];
let root = null;
let opener = null;
const esc = (t) => String(t).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const cmdButton = (text, id, note) => `<p><button type="button" data-cmd="${id}">${esc(text)}</button>${note ? ` <span class="bsnote">${esc(note)}</span>` : ''}</p>`;

function infoHtml() {
  const engine = window.layoutcraft?.engine;
  let rows = [];
  let pages = 0;
  try {
    pages = engine.pageCount();
    const props = engine.docProps();
    const names = [['title', 'Title'], ['subject', 'Subject'], ['creator', 'Author'], ['manager', 'Manager'], ['company', 'Company'], ['category', 'Category'], ['keywords', 'Keywords'], ['description', 'Comments']];
    rows = names.filter(([k]) => props[k]).map(([k, label]) => `<tr><th scope="row">${label}</th><td>${esc(props[k])}</td></tr>`);
    const size = engine.pageSize(0);
    rows.push(`<tr><th scope="row">Pages</th><td>${pages}</td></tr>`);
    const k = size.metric ? 25.4 : 1;
    rows.push(`<tr><th scope="row">Size of page 1</th><td>${(size.w * k).toFixed(size.metric ? 0 : 2)} x ${(size.h * k).toFixed(size.metric ? 0 : 2)} ${size.metric ? 'mm' : 'in'}</td></tr>`);
  } catch { return '<p>No drawing is open.</p>'; }
  return `<table class="bsinfo">${rows.join('')}</table>${cmdButton('Edit properties...', 'docPropsBtn')}`;
}
function optionsHtml() {
  const checks = OPTION_CHECKS.map(([id, label]) => `<label><input type="checkbox" data-proxy="${id}"${$(id)?.checked ? ' checked' : ''}> ${esc(label)}</label>`).join('');
  const steps = $('gridStep') ? [...$('gridStep').options].map((o) => `<option value="${esc(o.value)}"${o.value === $('gridStep').value ? ' selected' : ''}>${esc(o.textContent)}</option>`).join('') : '';
  return `${checks}<label>Grid spacing <select data-proxy="gridStep">${steps}</select></label><p class="bsnote">These are view settings of this browser, not part of the drawing.</p>`;
}
const BODY = {
  Info: infoHtml,
  New: () => cmdButton('New drawing...', 'newBtn', 'Starts an empty drawing with the page size you choose.'),
  Open: () => cmdButton('Open a Visio file...', 'file', '.vsdx drawings and .vssx / .vstx stencils') + cmdButton('Add a stencil...', 'addStencil', 'Opens a stencil file next to the drawing.'),
  'Save As': () => cmdButton('Download VSDX', 'save', 'The browser saves the drawing as a .vsdx file; Visio opens it.'),
  Print: () => cmdButton('Print / PDF...', 'printBtn', 'The paper size of the page setup is used.'),
  Export: () => cmdButton('Export...', 'exportBtn', 'SVG, PNG or PDF; one page, the selection, or every page.'),
  Options: optionsHtml,
};

function build() {
  root = document.createElement('div');
  root.id = 'backstage';
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-label', 'File');
  root.hidden = true;
  root.innerHTML = `<nav id="bsNav" aria-label="File"><button type="button" id="bsBack" aria-label="Back to the drawing">&#8592; Back</button><div role="tablist" aria-orientation="vertical" id="bsTabs">${SECTIONS.map((s) => `<button type="button" role="tab" id="bs-${s.replace(' ', '')}" aria-controls="bsBody" aria-selected="false" tabindex="-1">${s}</button>`).join('')}</div></nav><section id="bsBody" role="tabpanel" aria-live="polite"></section>`;
  document.body.append(root);
  $('bsBack').addEventListener('click', () => close());
  $('bsTabs').addEventListener('click', (event) => { const t = event.target.closest('[role=tab]'); if (t) show(t.textContent); });
  $('bsTabs').addEventListener('keydown', (event) => {
    const tabs = [...$('bsTabs').querySelectorAll('[role=tab]')];
    const at = tabs.indexOf(document.activeElement);
    const next = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: tabs.length - 1 }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    const t = tabs[(next + tabs.length) % tabs.length];
    t.focus(); show(t.textContent);
  });
  $('bsBody').addEventListener('click', (event) => {
    const b = event.target.closest('button[data-cmd]');
    if (!b) return;
    const el = $(b.dataset.cmd);
    close(false);
    if (el && !el.disabled) el.click();
  });
  $('bsBody').addEventListener('change', (event) => {
    const proxy = event.target.dataset?.proxy;
    const target = proxy && $(proxy);
    if (!target) return;
    if (target.type === 'checkbox') target.checked = event.target.checked; else target.value = event.target.value;
    fire(target);
    const o = readStore();
    o[proxy] = target.type === 'checkbox' ? target.checked : target.value;
    writeStore(o);
  });
  root.addEventListener('keydown', (event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); } });
}
function show(name) {
  for (const t of $('bsTabs').querySelectorAll('[role=tab]')) { const on = t.textContent === name; t.setAttribute('aria-selected', String(on)); t.tabIndex = on ? 0 : -1; }
  $('bsBody').innerHTML = `<h2>${esc(name)}</h2>${BODY[name]()}`;
  $('bsBody').setAttribute('aria-labelledby', `bs-${name.replace(' ', '')}`);
}
function open(section = 'Info') {
  if (!root) build();
  if (!root.hidden) return;
  opener = document.activeElement;
  root.hidden = false;
  show(section);
  $(`bs-${section.replace(' ', '')}`).focus();
}
function close(restore = true) {
  if (!root || root.hidden) return;
  root.hidden = true;
  if (restore && opener?.isConnected) opener.focus();
  opener = null;
}
window.layoutcraftBackstage = { open, close, isOpen: () => !!root && !root.hidden };

// ---- Help ----
const SHORTCUTS = [
  ['Editing', [['Ctrl+Z', 'Undo'], ['Ctrl+Y or Ctrl+Shift+Z', 'Redo'], ['Ctrl+X / Ctrl+C / Ctrl+V', 'Cut / copy / paste'], ['Ctrl+D', 'Duplicate'], ['Delete or Backspace', 'Delete the selection'], ['Ctrl+A', 'Select all shapes of the page'], ['Ctrl+F', 'Find and replace'], ['Ctrl+Alt+N', 'New drawing']]],
  ['Shapes', [['Click, Shift+click', 'Select, add to or remove from the selection'], ['Tab / Shift+Tab', 'Select the next / previous shape'], ['Arrow keys', 'Move the selection by 0.05 in (Shift: 0.5 in)'], ['F2', 'Edit the text of the selected shape'], ['Ctrl+Enter or Esc', 'Finish editing the text'], ['Ctrl+B / Ctrl+I / Ctrl+U', 'Bold / italic / underline'], ['Ctrl+G / Ctrl+Shift+G', 'Group / ungroup'], ['Ctrl+Shift+F / Ctrl+Shift+B', 'Bring to front / send to back'], ['[ and ]', 'Rotate left / right by 90 degrees'], ['H and V', 'Flip horizontally / vertically'], ['Esc', 'Leave the tool, then deselect']]],
  ['View', [['Ctrl + plus / Ctrl + minus', 'Zoom in / out'], ['Ctrl+mouse wheel', 'Zoom around the pointer'], ['Space+drag or middle button', 'Pan the page'], ['Menu key or Shift+F10', 'Open the right-click menu at the selection']]],
];
function helpDialog(id, label, html) {
  let d = $(id);
  if (!d) {
    d = document.createElement('dialog');
    d.id = id;
    d.setAttribute('aria-label', label);
    document.body.append(d);
  }
  d.innerHTML = `${html}<menu><button type="button" data-close>Close</button></menu>`;
  d.querySelector('[data-close]').onclick = () => d.close();
  d.showModal();
}
$('helpShortcuts')?.addEventListener('click', () => helpDialog('helpShortcutsDialog', 'Keyboard shortcuts', `<h2>Keyboard shortcuts</h2>${SHORTCUTS.map(([group, rows]) => `<h3>${group}</h3><table class="bskeys">${rows.map(([k, what]) => `<tr><th scope="row"><kbd>${esc(k)}</kbd></th><td>${esc(what)}</td></tr>`).join('')}</table>`).join('')}`));
$('helpAbout')?.addEventListener('click', () => helpDialog('helpAboutDialog', `About ${window.layoutcraftProduct.name}`, `<h2>${window.layoutcraftProduct.name}</h2><p>A viewer and editor for drawings in the .vsdx format used by Visio. LayoutCraft is an independent project and is not affiliated with or endorsed by Microsoft. The drawing core is written in Rust and runs in your browser as WebAssembly (${BUILD}).</p><p>Files are opened and saved on your computer; the drawing is never uploaded.</p>`));
