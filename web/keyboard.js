// H7 (round 8): keyboard access to the ribbon and the window regions.
//  * Alt (pressed and released on its own) shows KEY TIPS, a letter on every ribbon tab; the letter opens that tab (File opens the backstage).
//    Evidence: Microsoft's public "Keyboard shortcuts for Visio" page states only Alt+H for the Home tab (checked 2026-11-25; it says Alt shows key tips
//    but does not list the other tab letters). The other letters (F File, N Insert, G Design, A Data, R Review, W View, E Help, P Process) are OUR OWN choice,
//    following the common Office convention; they are not verified against any public source. Key tips for single commands are not built.
//  * F6 / Shift+F6 move the focus round the regions: ribbon, shapes pane, drawing, side panel, page tabs.
//  * Ctrl+F1 collapses / expands the ribbon (only the tab strip stays); a click on a tab then shows its panel until Esc or a click elsewhere.
const $ = (id) => document.getElementById(id);

// ---- key tips ----
const TIPS = { File: 'F', Home: 'H', Insert: 'N', Design: 'G', Data: 'A', Process: 'P', Review: 'R', View: 'W', Help: 'E' }; // only Home = H is documented publicly (see the note at the top); Draw, Org Chart and other tabs have no key tip yet
let altArmed = false;
let tipsOn = false;
function showTips() {
  if (tipsOn || !$('ribbonTabs')) return;
  tipsOn = true;
  for (const tab of $('ribbonTabs').querySelectorAll('[role=tab]')) {
    const letter = TIPS[tab.textContent];
    if (!letter) continue;
    tab.dataset.tip = letter; // drawn by CSS (::after): the tab's text, and so its name, stays as it is
  }
  $('msg').textContent = `Key tips: ${Object.entries(TIPS).map(([name, k]) => `${k} ${name}`).join(', ')}. Press a letter, or Esc to cancel.`;
}
function hideTips() {
  if (!tipsOn) return;
  tipsOn = false;
  for (const tab of document.querySelectorAll('#ribbonTabs [data-tip]')) delete tab.dataset.tip;
  $('msg').textContent = '';
}
window.addEventListener('keydown', (event) => {
  if (event.key === 'Alt' && !event.repeat && !event.ctrlKey && !event.shiftKey && !event.metaKey) { altArmed = true; return; }
  altArmed = false; // Alt+<key> is not a tap of Alt
  if (!tipsOn) return;
  const key = event.key.length === 1 ? event.key.toUpperCase() : event.key;
  const name = Object.keys(TIPS).find((n) => TIPS[n] === key);
  event.preventDefault();
  event.stopPropagation();
  hideTips();
  if (name) {
    window.layoutcraftRibbon?.openTab(name);
    if (name !== 'File') $(`tab-${name}`)?.focus();
  }
}, true);
window.addEventListener('keyup', (event) => {
  if (event.key !== 'Alt') return;
  if (altArmed) { event.preventDefault(); tipsOn ? hideTips() : showTips(); }
  altArmed = false;
}, true);
window.addEventListener('pointerdown', () => { altArmed = false; hideTips(); }, true);

// ---- F6: the regions of the window ----
const REGIONS = [
  { name: 'ribbon', root: () => $('ribbonTabs'), target: () => document.querySelector('#ribbonTabs [aria-selected=true]') },
  { name: 'shapes pane', root: () => $('stencil'), target: () => $('stencilFilter') },
  { name: 'drawing', root: () => $('stage'), target: () => $('stage') },
  { name: 'side panel', root: () => document.querySelector('main > aside:last-of-type'), target: () => [...document.querySelectorAll('main > aside:last-of-type input, main > aside:last-of-type select, main > aside:last-of-type button')].find((e) => !e.disabled && e.type !== 'hidden' && e.getClientRects().length > 0) || $('sel') },
  { name: 'page tabs', root: () => $('pageBar'), target: () => document.querySelector('#pageBar button:not(:disabled)') || $('pageBar') }, // with one page its only tab is the (disabled) current one: the bar itself takes the focus
];
const usable = (r) => { const t = r.target(); return t && t.getClientRects().length > 0; };
window.addEventListener('keydown', (event) => {
  if (event.key !== 'F6' || event.ctrlKey || event.altKey || event.metaKey) return;
  if (window.layoutcraftBackstage?.isOpen() || document.querySelector('dialog[open]')) return;
  event.preventDefault();
  const list = REGIONS.filter(usable);
  if (!list.length) return;
  const here = list.findIndex((r) => r.root()?.contains(document.activeElement));
  const next = list[((here < 0 ? (event.shiftKey ? 0 : -1) : here) + (event.shiftKey ? -1 : 1) + list.length) % list.length];
  const t = next.target();
  if (t === $('sel') || t === $('pageBar')) t.tabIndex = -1;
  t.focus();
  $('msg').textContent = `Region: ${next.name}`;
}, true);

// ---- Ctrl+F1: collapse / expand the ribbon ----
const header = () => document.querySelector('header.ribbon');
const STORE_KEY = 'layoutcraft-ribbon-collapsed';
function syncExpanded() {
  const h = header();
  if (!h) return;
  const open = !h.classList.contains('collapsed') || h.classList.contains('peek');
  for (const tab of h.querySelectorAll('[role=tab]')) tab.setAttribute('aria-expanded', String(open));
  // the button at the end of the tab strip: a chevron up collapses, a chevron down expands, a pin (while a collapsed ribbon is shown for one command) keeps it open
  const t = $('ribbonToggle');
  if (t) {
    const collapsed = h.classList.contains('collapsed'), peek = h.classList.contains('peek');
    const [icon, label] = !collapsed ? ['chevron-up', 'Collapse the ribbon (Ctrl+F1)'] : peek ? ['pin', 'Keep the ribbon open (Ctrl+F1)'] : ['chevron-down', 'Expand the ribbon (Ctrl+F1)'];
    t.title = label; t.setAttribute('aria-label', label.replace(/ \(Ctrl\+F1\)/, '')); t.setAttribute('aria-expanded', String(!collapsed));
    t.querySelector('.ricon')?.style.setProperty('--icon', `url(assets/icons/${icon}.svg)`);
  }
}
function setCollapsed(on, { remember = true, say = true } = {}) {
  const h = header();
  if (!h) return;
  h.classList.toggle('collapsed', on);
  h.classList.remove('peek');
  syncExpanded();
  if (say) $('msg').textContent = on ? 'Ribbon collapsed. Ctrl+F1 expands it.' : 'Ribbon expanded.';
  if (remember) { try { localStorage.setItem(STORE_KEY, on ? '1' : '0'); } catch { /* private window: the state is not kept */ } }
}
// the state of the last session (collapsed or not) comes back when the page has built its ribbon
function restore() { try { if (localStorage.getItem(STORE_KEY) === '1' && header()) setCollapsed(true, { remember: false, say: false }); } catch { /* no storage */ } }
if (document.readyState === 'complete') restore(); else window.addEventListener('load', restore);
// the button and a double click on a tab (as in Office) collapse / expand the ribbon
document.addEventListener('click', (event) => {
  if (!event.target.closest?.('#ribbonToggle')) return;
  const h = header();
  setCollapsed(h.classList.contains('peek') ? false : !h.classList.contains('collapsed'));
}, true);
document.addEventListener('dblclick', (event) => {
  const tab = event.target.closest?.('#ribbonTabs [role=tab]');
  if (tab && tab.textContent !== 'File') setCollapsed(!header().classList.contains('collapsed')); // expanded: collapse; collapsed (a click already showed the panel): expand
});
window.addEventListener('keydown', (event) => {
  if (event.key === 'F1' && event.ctrlKey && !event.altKey && !event.shiftKey) { event.preventDefault(); setCollapsed(!header()?.classList.contains('collapsed')); return; }
  if (event.key === 'Escape' && header()?.classList.contains('peek')) { header().classList.remove('peek'); syncExpanded(); }
}, true);
document.addEventListener('click', (event) => {
  const h = header();
  if (!h || !h.classList.contains('collapsed')) return;
  const tab = event.target.closest?.('#ribbonTabs [role=tab]');
  if (tab && tab.textContent !== 'File') { h.classList.add('peek'); syncExpanded(); return; }
  if (h.classList.contains('peek') && !event.target.closest?.('#ribbonPanels')) { h.classList.remove('peek'); syncExpanded(); }
}, true);
document.addEventListener('click', (event) => { // a command chosen from a peeking panel closes it again
  const h = header();
  if (h?.classList.contains('peek') && event.target.closest?.('#ribbonPanels button, #ribbonPanels input[type=checkbox]')) setTimeout(() => { h.classList.remove('peek'); syncExpanded(); }, 0);
});

window.layoutcraftKeys = { showTips, hideTips, setCollapsed, tipsOn: () => tipsOn, collapsed: () => !!header()?.classList.contains('collapsed') };
