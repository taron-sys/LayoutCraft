// LayoutCraft shell palettes (step 275). Loaded as a blocking script in <head>, so the saved palette is on <html> before the first paint (no flash).
// A palette is a class `theme-<id>` on <html>; all shell colours are CSS variables defined per class in shell.html. "System" follows prefers-color-scheme and
// updates live. Everything that is not CSS (JS-drawn parts) subscribes with layoutcraftTheme.onChange. The drawing itself never changes with the palette.
(() => {
  const KEY = 'layoutcraft-palette';
  const PALETTES = [['system', 'System'], ['light', 'Light'], ['dark', 'Dark'], ['black', 'Black (high contrast)'], ['ocean', 'Ocean'], ['sunset', 'Sunset']];
  const listeners = [];
  let mode = 'system';
  try { const saved = localStorage.getItem(KEY); if (saved && PALETTES.some((p) => p[0] === saved)) mode = saved; } catch { /* storage blocked: keep System */ }
  const query = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
  const resolve = () => (mode === 'system' ? (query && query.matches ? 'dark' : 'light') : mode);
  let shown = null;
  const apply = (notify) => {
    const id = resolve();
    const root = document.documentElement;
    for (const c of [...root.classList]) if (c.startsWith('theme-')) root.classList.remove(c);
    root.classList.add('theme-' + id);
    root.dataset.palette = mode;
    if (id !== shown) {
      shown = id;
      if (notify) {
        for (const f of listeners) { try { f(id, mode); } catch { /* a listener must not break the others */ } }
        document.dispatchEvent(new CustomEvent('layoutcraft-theme', { detail: { id, mode } }));
      }
    }
  };
  if (query) { const again = () => { if (mode === 'system') apply(true); }; query.addEventListener ? query.addEventListener('change', again) : query.addListener(again); }
  apply(false);
  window.layoutcraftTheme = {
    palettes: PALETTES.map((p) => ({ id: p[0], name: p[1] })),
    get mode() { return mode; },
    get resolved() { return resolve(); },
    set(next) {
      if (!PALETTES.some((p) => p[0] === next)) return false;
      mode = next;
      try { localStorage.setItem(KEY, mode); } catch { /* storage blocked: the choice lasts for this session */ }
      apply(true);
      return true;
    },
    onChange(callback) { listeners.push(callback); return () => { const i = listeners.indexOf(callback); if (i >= 0) listeners.splice(i, 1); }; },
  };
})();
