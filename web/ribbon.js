// (round 11: the strip is File Home Insert Draw Design Data Process Review View Help, as in the screenshots)
// H1 (round 8): the ribbon frame. The shell's controls used to sit in one flat row. This module, loaded before shell.js, builds the Visio-style
// frame around them and MOVES the existing elements (by id, so every id and every listener stays as it is) into tabs and groups:
//   Quick Access Toolbar (Save, Undo, Redo, Print) / tab strip File Home Insert Design Data Review View Help / one panel of named groups per tab
//   / page tabs at the bottom (as Visio) / status bar (page n of m, selection, zoom, messages).
// The layout follows Visio's menu structure. Commands that have no place yet land in Home > More, so nothing is ever lost.
const $ = (id) => document.getElementById(id);

const QAT = ['save', 'undo', 'redo', 'printBtn'];
// Icons (step 312): the SVG files of web/assets/icons, shown through a CSS mask in the text colour (see assets/icons/README.md). button id -> icon file name (without .svg).
const ICONS = {
  save: 'save', undo: 'undo', redo: 'redo', printBtn: 'print', newBtn: 'new', exportBtn: 'export', copyImgBtn: 'copy-image', docPropsBtn: 'properties',
  paste: 'paste', cut: 'cut', copy: 'copy', painter: 'format-painter', alignL: 'align-left', alignC: 'align-center', alignR: 'align-right', alignJ: 'align-justify', bullet: 'bullets',
  connectTool: 'connector', connPointTool: 'connection-point', editPointsBtn: 'edit-points', fitCurveBtn: 'fit-curve', handTool: 'pan', stylesBtn: 'styles',
  arrL: 'align-objects-left', arrCx: 'align-objects-center', arrRt: 'align-objects-right', arrT: 'align-objects-top', arrM: 'align-objects-middle', arrB: 'align-objects-bottom',
  arrDistH: 'distribute-horizontal', arrDistV: 'distribute-vertical', sameW: 'same-width', sameH: 'same-height', sameWH: 'same-size',
  zFront: 'bring-to-front', zFwd: 'bring-forward', zBwd: 'send-backward', zBack: 'send-to-back', grp: 'group', ungrp: 'ungroup', rotL: 'rotate-left', rotR: 'rotate-right',
  flipH: 'flip-horizontal', flipV: 'flip-vertical', reverseEnds: 'reverse-ends', lockShape: 'lock',
  opUnion: 'op-union', opCombine: 'op-combine', opFragment: 'op-fragment', opIntersect: 'op-intersect', opSubtract: 'op-subtract', opJoin: 'op-join', opTrim: 'op-trim',
  changeShapeBtn: 'change-shape', findBtn: 'find', layersBtn: 'layers', dup: 'duplicate', del: 'delete',
  pageAdd: 'page-add', pageDup: 'page-copy', pageRename: 'page-rename', pageDel: 'page-delete', pageLeft: 'page-previous', pageRight: 'page-next', insertHyperlink: 'hyperlink', insertPicture: 'picture', checkDiagram: 'check', compareBtn: 'compare', insertTextBox: 'text-box',
  pageSetup: 'page-setup', dataBtn: 'shape-data', newComment: 'new-comment', reportBtn: 'report',
  fullScreen: 'full-screen', zout: 'zoom-out', zin: 'zoom-in', fit: 'zoom-fit', zoomWidth: 'zoom-width', zoom100: 'zoom-100', newWindow: 'new-window', helpShortcuts: 'keyboard', helpAbout: 'about', mcpInfo: 'mcp',
};
// the drawing tools have no ids: their data-kind picks the icon
const DRAW_ICONS = { 0: 'draw-rectangle', 1: 'draw-rounded', 2: 'draw-ellipse', 3: 'draw-diamond', 4: 'draw-triangle', 5: 'draw-textbox', 6: 'draw-line', 7: 'draw-pencil', 8: 'draw-freeform', 9: 'draw-arc' };
// buttons whose text is only a symbol (or whose icon says it all): the text stays for screen readers (visually hidden) and the accessible name is the title
const ICON_ONLY = new Set([
  // quick access toolbar and symbols
  'save', 'undo', 'redo', 'printBtn', 'painter', 'alignL', 'alignC', 'alignR', 'alignJ', 'bullet', 'arrL', 'arrCx', 'arrRt', 'arrT', 'arrM', 'arrB', 'arrDistH', 'arrDistV', 'sameW', 'sameH', 'sameWH',
  'reverseEnds', 'lockShape', 'opUnion', 'opCombine', 'opFragment', 'opIntersect', 'opSubtract', 'opJoin', 'opTrim', 'pageLeft', 'pageRight', 'zin', 'zout',
  // well-known commands: the icon says it, the name is the tooltip (step 313)
  'paste', 'cut', 'copy', 'dup', 'del', 'findBtn', 'grp', 'ungrp', 'rotL', 'rotR', 'flipH', 'flipV', 'zFront', 'zFwd', 'zBwd', 'zBack',
  'pageAdd', 'pageDup', 'pageRename', 'pageDel', 'insertHyperlink', 'fullScreen', 'newWindow', 'handTool',
]);
const ICON_DIR = 'assets/icons/';
const TABS = [
  ['File', [['New and open', ['newBtn', 'file']], ['Export', ['exportBtn', 'copyImgBtn']], ['Info', ['docPropsBtn']]]],
  // 'p:id' = the whole row (<p>) of the Format side panel that holds control `id`: the label and its controls move together.
  ['Home', [
    ['Clipboard', ['paste', 'cut', 'copy', 'painter']],
    ['Font', [['Font', ['p:fontFamily', 'p:fontSize', 'p:bold', 'p:strike']]]],
    ['Paragraph', [['Paragraph', ['p:alignL', 'p:indLeft', 'p:lineSp']]]],
    ['Tools', [['Shapes', ['drawTools']], 'connectTool', 'connPointTool', 'editPointsBtn', 'fitCurveBtn', 'handTool']],
    ['Shape Styles', [['Styles', ['p:fillColor', 'p:lineColor', 'p:shadow']]]],
    // an entry [label, [ids]] is a DROP-DOWN (round 12, Q6): one button that opens a menu holding those commands (every id and listener stays)
    ['Arrange', ['arrange', ['Size', ['sameW', 'sameH', 'sameWH']], ['Order', ['zFront', 'zFwd', 'zBwd', 'zBack']], 'grp', 'ungrp', ['Rotate', ['rotL', 'rotR', 'flipH', 'flipV', 'reverseEnds']], 'lockShape', ['Operations', ['opUnion', 'opCombine', 'opFragment', 'opIntersect', 'opSubtract', 'opJoin', 'opTrim']]]], // Visio: Align and Position live inside Arrange
    ['Editing', ['stylesBtn', 'changeShapeBtn', 'findBtn', 'layersBtn', 'dup', 'del']],
  ]],
  ['Insert', [['Pages', ['pageTools']], ['Illustrations', ['insertPicture', 'pictureFile']], ['Links', ['insertHyperlink']], ['Text', ['insertTextBox']]]],
  // round 11 item 11: Visio's Draw tab (pen / ink tools) and Process tab (sub-process, diagram validation, SharePoint) have no commands here;
  // the tabs stand in the order of the user's screenshots and say so, nothing is faked
  ['Draw', [], 'Ink and touch drawing (pens, stroke eraser, lasso, ink to text) is not built. The shape tools are on Home > Tools.'],
  ['Design', [['Page Setup', ['pageSetup', 'designOrientation', 'designSize']], ['Themes', [['Themes', ['themeGallery']]]], ['Variants', [['Variants', ['variantGallery']]]], ['Borders & Titles', ['borderStyle', 'borderTitle', 'borderApply', 'borderRemove']], ['Backgrounds', ['paperColor']]]],
  ['Data', [['Show/Hide', ['dataBtn']]]],
  ['Process', [['Verify', ['checkDiagram']]], 'Sub-process links and the SharePoint workflow commands are not built.'],
  ['Review', [['Comments', ['newComment', 'showComments']], ['Compare', ['compareBtn', 'compareFile']], ['Reports', ['reportBtn']]], 'Spelling is the browser\'s own check inside the text editor; reports are LayoutCraft\'s own simple Inventory report.'],
  ['View', [
    ['Views', ['fullScreen', 'print', 'fixedPage']],
    ['Show', ['showRulers', 'showGrid', 'showGuides', 'showBreaks', 'paneShapes', 'paneSizePos', 'paneSheet', 'panePanZoom']], // Visio: Task Panes is a drop-down inside Show
    ['Zoom', ['zout', 'zl', 'zin', 'fit', 'zoomWidth', 'zoom100', 'wheelZoom']],
    ['Visual Aids', ['snapGrid', 'gridStep', 'snapShapes', 'snapGuides', 'autoConnectToggle', 'refit']],
    ['Palette', ['paletteSel', 'tipDelay']], // step 275: the application colour palette (Light, Dark, Black, Ocean, Sunset, System)
    ['Window', ['newWindow']], // round 11: Visio keeps New Window in its own Window group, after Visual Aids
  ]],
  ['Help', [['Help', ['helpShortcuts', 'helpAbout']], ['AI assistant', ['mcpInfo']]]],
];

/** The node to move for an id: a checkbox or colour input wrapped in a label moves with its label. */
function nodeFor(id) {
  if (id.startsWith('p:')) return $(id.slice(2))?.closest('p') ?? null;
  const el = $(id);
  if (!el) return null;
  const label = el.closest('label');
  return label && label.parentElement?.tagName === 'HEADER' ? label : el;
}

function build() {
  const header = document.querySelector('header');
  if (!header || $('ribbonTabs')) return;
  const taken = new Set();
  // every node is looked up BEFORE anything moves: a node that sits in a panel not yet attached to the page cannot be found by id
  const found = new Map();
  for (const id of [...QAT, 'msg', 'tabs', ...TABS.flatMap(([, groups]) => groups.flatMap(([, ids]) => ids.flatMap((i) => (Array.isArray(i) ? i[1] : [i]))))]) found.set(id, nodeFor(id));
  const take = (id) => { const n = found.get(id) ?? null; if (n) taken.add(n); return n; };

  // the elements that leave the header for other places
  const msg = take('msg');
  const pageTabs = take('tabs');
  const qatNodes = QAT.map(take).filter(Boolean);

  const el = (tag, attrs = {}, ...kids) => {
    const n = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) { if (k === 'text') n.textContent = v; else n.setAttribute(k, v); }
    n.append(...kids);
    return n;
  };

  /** A drop-down: one button that opens a small menu holding the moved command nodes; a click on a command (or elsewhere) closes it. */
  const openMenus = new Set();
  const closeMenus = (except = null) => { for (const m of [...openMenus]) if (m !== except) { m.hidden = true; m.previousElementSibling?.setAttribute('aria-expanded', 'false'); openMenus.delete(m); } };
  document.addEventListener('click', (event) => { if (!event.target.closest?.('.rdd')) closeMenus(); });
  document.addEventListener('keydown', (event) => { if (event.key === 'Escape' && openMenus.size) closeMenus(); });
  function dropdown(label, nodes) {
    if (!nodes.length) return null;
    const button = el('button', { type: 'button', class: 'rddbtn', 'aria-haspopup': 'true', 'aria-expanded': 'false', title: `${label}: shows the commands`, text: `${label} ▾` });
    const menu = el('div', { class: 'rddmenu', role: 'group', 'aria-label': label, hidden: '' }, ...nodes);
    button.addEventListener('click', () => { const open = menu.hidden; closeMenus(menu); menu.hidden = !open; button.setAttribute('aria-expanded', String(open)); if (open) openMenus.add(menu); else openMenus.delete(menu); });
    menu.addEventListener('click', (event) => { if (event.target.closest('button, label, input, select')) setTimeout(() => closeMenus(), 0); });
    return el('div', { class: 'rdd' }, button, menu);
  }

  const qat = el('div', { id: 'qat', role: 'toolbar', 'aria-label': 'Quick access toolbar' }, el('b', { class: 'appname', text: window.layoutcraftProduct.name }), ...qatNodes);
  const tablist = el('div', { id: 'ribbonTabs', role: 'tablist', 'aria-label': 'Ribbon tabs' });
  const panels = el('div', { id: 'ribbonPanels' });
  const tabButtons = [];
  const homePanel = { groups: null };

  TABS.forEach(([name, groups, note], index) => {
    const tab = el('button', { id: `tab-${name}`, role: 'tab', 'aria-selected': 'false', 'aria-controls': `ribbon-${name}`, tabindex: '-1', text: name });
    const panel = el('div', { id: `ribbon-${name}`, role: 'tabpanel', 'aria-labelledby': `tab-${name}`, class: 'ribbonpanel', hidden: '' });
    for (const [groupName, ids] of groups) {
      const items = el('div', { class: 'rgitems' });
      for (const id of ids) {
        if (Array.isArray(id)) { const dd = dropdown(id[0], id[1].map(take).filter(Boolean)); if (dd) items.append(dd); continue; }
        const n = take(id); if (n) items.append(n);
      }
      if (!items.children.length) continue;
      panel.append(el('div', { class: 'rgroup', role: 'group', 'aria-label': groupName }, items, el('div', { class: 'rglabel', text: groupName })));
    }
    if (note && !panel.children.length) panel.append(el('p', { class: 'rnote', text: note }));
    if (name === 'Home') homePanel.groups = panel;
    tablist.append(tab);
    panels.append(panel);
    tabButtons.push([tab, panel]);
    tab.dataset.index = String(index);
  });

  // whatever is still in the header (a control nobody listed) goes to Home > More
  const rest = [...header.children].filter((n) => !taken.has(n));
  if (rest.length) {
    const items = el('div', { class: 'rgitems' }, ...rest);
    homePanel.groups.append(el('div', { class: 'rgroup', role: 'group', 'aria-label': 'More' }, items, el('div', { class: 'rglabel', text: 'More' })));
  }

  // The moved Format rows work on the selection: like Visio's greyed-out buttons they are inert while nothing is selected
  // (the side panel's own `hidden` flag, set by shell.js, says so).
  const formatRows = [...found.entries()].filter(([id, n]) => id.startsWith('p:') && n).map(([, n]) => n);
  const formatPanel = $('format');
  const syncRows = () => { const off = !formatPanel || formatPanel.hidden; for (const r of formatRows) { r.inert = off; r.classList.toggle('rowoff', off); } };
  if (formatRows.length && formatPanel) { new MutationObserver(syncRows).observe(formatPanel, { attributes: true, attributeFilter: ['hidden'] }); syncRows(); }

  // the collapse button at the right end of the tab strip (Office: a chevron; keyboard.js sets its icon, label and action)
  const toggle = el('button', { id: 'ribbonToggle', type: 'button', 'aria-label': 'Collapse the ribbon', title: 'Collapse the ribbon (Ctrl+F1)' });
  const tabRow = el('div', { id: 'ribbonRow' }, tablist, toggle);
  header.append(qat, tabRow, panels);
  header.classList.add('ribbon');

  // icons: a mask span in front of the button's text (the text becomes visually hidden for the icon-only buttons)
  const addIcon = (node, name, iconOnly) => {
    if (!node || node.tagName !== 'BUTTON' || node.querySelector(':scope > .ricon')) return;
    if (iconOnly) {
      // the name of the command is its tooltip (the browser shows it after a short hover) and its accessible name
      const name = node.textContent.trim();
      if (!node.getAttribute('title') && name) node.setAttribute('title', name);
      if (!node.getAttribute('aria-label')) node.setAttribute('aria-label', node.getAttribute('title') || name);
      const hidden = el('span', { class: 'rhide' });
      while (node.firstChild) hidden.append(node.firstChild);
      node.append(hidden);
      node.classList.add('ricononly');
    }
    else if (node.textContent.trim()) {
      // a button with a text and an icon: the text sits in its own span so the compact mode (narrow windows, see shell.html) can hide it; the name stays as tooltip and accessible name
      if (!node.getAttribute('title')) node.setAttribute('title', node.textContent.trim());
      const text = el('span', { class: 'rtext' });
      while (node.firstChild) text.append(node.firstChild);
      node.append(text);
    }
    node.prepend(el('span', { class: 'ricon', 'aria-hidden': 'true', style: `--icon:url(${ICON_DIR}${name}.svg)` }));
  };
  for (const [id, name] of Object.entries(ICONS)) addIcon($(id), name, ICON_ONLY.has(id));
  for (const b of document.querySelectorAll('#drawTools [data-kind]')) { const name = DRAW_ICONS[b.dataset.kind]; if (name) addIcon(b, name, false); }
  toggle.append(el('span', { class: 'ricon', 'aria-hidden': 'true', style: `--icon:url(${ICON_DIR}chevron-up.svg)` }));

  function openTab(name) {
    // File is not a panel of commands: it opens the backstage (backstage.js), as in Visio; the tab strip keeps showing the tab that was open
    if (name === 'File' && window.layoutcraftBackstage) { window.layoutcraftBackstage.open(); return true; }
    const hit = tabButtons.find(([tab]) => tab.textContent === name);
    if (!hit) return false;
    for (const [tab, panel] of tabButtons) {
      const on = tab === hit[0];
      tab.setAttribute('aria-selected', String(on));
      tab.tabIndex = on ? 0 : -1;
      panel.hidden = !on;
    }
    return true;
  }
  tablist.addEventListener('click', (event) => { const t = event.target.closest('[role=tab]'); if (t) openTab(t.textContent); });
  tablist.addEventListener('keydown', (event) => {
    const keys = { ArrowRight: 1, ArrowLeft: -1 };
    const current = tabButtons.findIndex(([tab]) => tab.getAttribute('aria-selected') === 'true');
    let next = -1;
    if (event.key in keys) next = (current + keys[event.key] + tabButtons.length) % tabButtons.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabButtons.length - 1;
    if (next < 0) return;
    event.preventDefault();
    openTab(tabButtons[next][0].textContent);
    if (tabButtons[next][0].textContent !== 'File') tabButtons[next][0].focus(); // the backstage takes the focus itself
  });
  openTab('Home');

  // page tabs at the bottom, status bar under them
  const pageBar = el('nav', { id: 'pageBar', 'aria-label': 'Pages' });
  if (pageTabs) pageBar.append(pageTabs);
  const sbPage = el('span', { id: 'sbPage', text: 'Page 1 of 1' });
  const sbSel = el('span', { id: 'sbSel' });
  const sbZoom = el('span', { id: 'sbZoom' });
  const status = el('footer', { id: 'statusbar' }, sbPage, sbSel, el('span', { class: 'sbfill' }), ...(msg ? [msg] : []), sbZoom);
  const main = document.querySelector('main');
  main.after(pageBar, status);

  const refresh = () => {
    const buttons = pageTabs ? [...pageTabs.querySelectorAll('button')] : [];
    const at = Math.max(0, buttons.findIndex((b) => b.disabled));
    sbPage.textContent = buttons.length ? `Page ${at + 1} of ${buttons.length}` : 'No drawing';
    sbSel.textContent = $('sel')?.textContent || '';
    sbZoom.textContent = $('zl')?.textContent || '';
  };
  const watch = new MutationObserver(refresh);
  for (const target of [pageTabs, $('sel'), $('zl')]) if (target) watch.observe(target, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['disabled'] });
  refresh();

  window.layoutcraftRibbon = { openTab, tabs: () => tabButtons.map(([tab]) => tab.textContent) };
}

if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', build); else build();
