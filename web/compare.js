// Review > Compare Documents (step 331, RIB-11). Visio's Compare Documents shows the differences of two drawings; ours compares the drawing models: `current` (the open drawing, the "new" one) against `other`
// (a file the user chose, the "old" one). Pages are matched by name, shapes and connectors by their Visio id inside a page.
//   added    : in the open drawing, not in the file
//   removed  : in the file, not in the open drawing
//   changed  : in both, but the text, the alt text, the position or size (more than half a pixel), the fill, the line or the rotation differ
// compareModels(current, other) returns [{page, pageName, kind, what: 'page'|'shape'|'connector', id, label, detail}] in page order; `id` is the id in the open drawing (for removed ones the id in the file).
const EPS = 0.5;

const label = (s) => (s && s.label ? `"${String(s.label).split('\n')[0].slice(0, 30)}"` : s ? `shape ${s.visioId}` : '');
const near = (a, b) => Math.abs((a || 0) - (b || 0)) <= EPS;

export function compareShape(a, b) {
  const d = [];
  if ((a.label || '') !== (b.label || '')) d.push(`text "${b.label || ''}" -> "${a.label || ''}"`);
  if (!near(a.x, b.x) || !near(a.y, b.y)) d.push(`moved ${Math.round((a.x - b.x) / 0.96) / 100} in right, ${Math.round((a.y - b.y) / 0.96) / 100} in down`);
  if (!near(a.w, b.w) || !near(a.h, b.h)) d.push(`size ${Math.round(b.w / 0.96) / 100} x ${Math.round(b.h / 0.96) / 100} in -> ${Math.round(a.w / 0.96) / 100} x ${Math.round(a.h / 0.96) / 100} in`);
  if ((a.altText || '') !== (b.altText || '')) d.push(`alt text "${b.altText || ''}" -> "${a.altText || ''}"`);
  if ((a.fill || 'none') !== (b.fill || 'none')) d.push(`fill ${b.fill || 'none'} -> ${a.fill || 'none'}`);
  if ((a.stroke || 'none') !== (b.stroke || 'none')) d.push(`line ${b.stroke || 'none'} -> ${a.stroke || 'none'}`);
  if (!near(a.rot, b.rot)) d.push(`rotation ${Math.round(b.rot || 0)} -> ${Math.round(a.rot || 0)} degrees`);
  return d;
}

const endId = (e) => (e && e.shapeId ? String(e.shapeId).replace('shape:visio:', '') : '');

export function compareModels(current, other) {
  const out = [];
  const pagesA = current.pages || [], pagesB = other.pages || [];
  const nameOf = (p, i) => p.name || `Page-${i + 1}`;
  const byName = new Map(pagesB.map((p, i) => [nameOf(p, i), p]));
  const seen = new Set();
  pagesA.forEach((pa, i) => {
    const name = nameOf(pa, i);
    const pb = byName.get(name);
    if (!pb) { out.push({ page: i, pageName: name, kind: 'added', what: 'page', id: '', label: name, detail: 'page is not in the file' }); return; }
    seen.add(name);
    const sa = new Map((pa.shapes || []).map((s) => [String(s.visioId), s])), sb = new Map((pb.shapes || []).map((s) => [String(s.visioId), s]));
    for (const [id, s] of sa) {
      const o = sb.get(id);
      if (!o) { out.push({ page: i, pageName: name, kind: 'added', what: 'shape', id, label: label(s), detail: 'not in the file' }); continue; }
      const diff = compareShape(s, o);
      if (diff.length) out.push({ page: i, pageName: name, kind: 'changed', what: 'shape', id, label: label(s), detail: diff.join('; ') });
    }
    for (const [id, s] of sb) if (!sa.has(id)) out.push({ page: i, pageName: name, kind: 'removed', what: 'shape', id, label: label(s), detail: 'only in the file' });
    const ca = new Map((pa.connectors || []).map((c) => [String(c.visioId), c])), cb = new Map((pb.connectors || []).map((c) => [String(c.visioId), c]));
    for (const [id, c] of ca) {
      const o = cb.get(id);
      if (!o) { out.push({ page: i, pageName: name, kind: 'added', what: 'connector', id, label: `connector ${id}`, detail: 'not in the file' }); continue; }
      const diff = [];
      if (endId(c.from) !== endId(o.from) || endId(c.to) !== endId(o.to)) diff.push(`glued ${endId(o.from) || 'nothing'} - ${endId(o.to) || 'nothing'} -> ${endId(c.from) || 'nothing'} - ${endId(c.to) || 'nothing'}`);
      if ((c.label || '') !== (o.label || '')) diff.push(`text "${o.label || ''}" -> "${c.label || ''}"`);
      if ((c.stroke || '') !== (o.stroke || '')) diff.push(`line ${o.stroke || 'none'} -> ${c.stroke || 'none'}`);
      if (diff.length) out.push({ page: i, pageName: name, kind: 'changed', what: 'connector', id, label: `connector ${id}`, detail: diff.join('; ') });
    }
    for (const id of cb.keys()) if (!ca.has(id)) out.push({ page: i, pageName: name, kind: 'removed', what: 'connector', id, label: `connector ${id}`, detail: 'only in the file' });
  });
  pagesB.forEach((pb, i) => { const name = nameOf(pb, i); if (!seen.has(name) && !pagesA.some((pa, k) => nameOf(pa, k) === name)) out.push({ page: -1, pageName: name, kind: 'removed', what: 'page', id: '', label: name, detail: 'page is only in the file' }); });
  return out;
}

/** The differences as CSV text (step 347): columns Page, Difference (added / removed / changed), Kind (shape / connector / page), Id, Item, Detail. CRLF line ends; fields with a comma, quote or line break quoted. */
export function diffsToCsv(diffs) {
  const field = (v) => { const t = String(v ?? ''); return /[",\r\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t; };
  const rows = [['Page', 'Difference', 'Kind', 'Id', 'Item', 'Detail'], ...diffs.map((d) => [d.pageName, d.kind, d.what, d.id, d.label, d.detail])];
  return rows.map((r) => r.map(field).join(',')).join('\r\n') + '\r\n';
}
