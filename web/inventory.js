// Inventory report (round 11, item 8; user decision f; moved out of shell.js in step 351 so the MCP server can use it too): LayoutCraft's own report, NOT Visio's report format.
// Rows = the shapes of one page (or of every page), columns = Page, Shape ID, Text and each Shape Data label. Pure functions of the drawing model.

/** { rows: [{page, id, text, values}], dataCols: [{key, label}] } for page `pageIndex` of `model` (every page with allPages). `names` are the page names (index = page). */
export function inventoryData(model, names, pageIndex, allPages = false) {
  const pages = model.pages;
  const rows = [], dataCols = [];
  (allPages ? pages.map((p, i) => i) : [pageIndex]).forEach((pi) => {
    for (const shape of pages[pi]?.shapes || []) {
      const values = {};
      for (const row of shape.data || []) {
        if (!dataCols.some((c) => c.key === row.name)) dataCols.push({ key: row.name, label: row.label || row.name });
        values[row.name] = row.value ?? '';
      }
      rows.push({ page: (names || [])[pi] ?? String(pi + 1), id: shape.visioId, text: String(shape.label ?? '').replace(/\s+/g, ' ').trim(), values });
    }
  });
  return { rows, dataCols };
}

/** CSV text (RFC 4180: quotes doubled, fields with comma / quote / line break quoted, CRLF) for the chosen columns (keys: 'page', 'id', 'text' or a Shape Data name). */
export function inventoryCsv(model, names, pageIndex, columns, allPages = false) {
  const { rows, dataCols } = inventoryData(model, names, pageIndex, allPages);
  const head = { page: 'Page', id: 'Shape ID', text: 'Text' };
  const q = (v) => { const t = String(v ?? ''); return /[",\r\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t; };
  const label = (k) => head[k] || dataCols.find((c) => c.key === k)?.label || k;
  const cell = (r, k) => (k === 'page' ? r.page : k === 'id' ? r.id : k === 'text' ? r.text : r.values[k] ?? '');
  return [columns.map((k) => q(label(k))).join(','), ...rows.map((r) => columns.map((k) => q(cell(r, k))).join(','))].join('\r\n') + '\r\n';
}
