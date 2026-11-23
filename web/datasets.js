// Shape Data sets (step 329, DATA-19): a named list of Shape Data rows (label, type, list items, prompt, default value) that can be saved from a shape and applied to other shapes, so
// that many shapes get the same fields. The sets are kept in this browser (localStorage layoutcraft-data-sets), not in the drawing: they are the user's own library; the rows
// that were applied are ordinary Shape Data rows of the shapes and travel with the file.
const KEY = 'layoutcraft-data-sets';

/** All sets: { name: [row, ...] }. */
export function loadSets() {
  try { const o = JSON.parse(localStorage.getItem(KEY) || '{}'); return o && typeof o === 'object' && !Array.isArray(o) ? o : {}; } catch { return {}; }
}
export function saveSets(sets) { try { localStorage.setItem(KEY, JSON.stringify(sets)); } catch { /* no storage */ } }

/** The rows of a shape as the rows of a set (name, label, type, list items, prompt, default value, hidden). */
export function setFromRows(rows) {
  return rows.filter((r) => r && r.name).map((r) => ({ name: String(r.name), label: String(r.label || r.name), type: r.type ?? 0, format: String(r.format || ''), prompt: String(r.prompt || ''), value: String(r.value ?? ''), invisible: !!r.invisible }));
}

/**
 * Write the rows of `set` to the shapes `ids` of page `page` (one undo step). A row whose label the shape has already is left alone; a name that is taken gets a number.
 * `rowsOf(id)` gives the rows a shape has now. Returns the number of rows written.
 */
export function applySet(engine, page, ids, set, rowsOf) {
  let written = 0;
  engine.batch(() => {
    for (const id of ids) {
      const have = rowsOf(id);
      const labels = new Set(have.map((r) => String(r.label || r.name).toLowerCase()));
      const names = new Set(have.map((r) => String(r.name).toLowerCase()));
      for (const row of set) {
        if (labels.has(row.label.toLowerCase())) continue;
        let name = row.name, n = 2;
        while (names.has(name.toLowerCase())) name = `${row.name}_${n++}`;
        names.add(name.toLowerCase()); labels.add(row.label.toLowerCase());
        engine.setShapeData(page, id, { ...row, name });
        written++;
      }
    }
  });
  return written;
}
