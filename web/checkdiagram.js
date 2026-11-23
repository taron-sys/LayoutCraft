// Process > Check Diagram (step 328). Visio checks a diagram against rules of a rule set; ours are a few plain checks on the drawing model, all about things that are usually mistakes:
//   - a connector with an end that is glued to no shape ("loose end")
//   - a second connector from the same shape to the same shape ("duplicate connector", step 354)
//   - a shape with a text or a fill that no connector touches, on a page that has connectors ("not connected")
//   - two filled shapes that cover each other (one is not simply inside the other) ("overlap")
//   - a shape that lies partly outside the page ("outside the page")
// With the option `altText` shapes without text and without alt text are listed too ("no alt text"). checkDiagram(model, pageIndex, pageSize, {altText}) returns [{kind, id, message, fix?}] sorted by kind; `id` is the shape to select, `fix` the name of the repair applyFix can do. Checking changes nothing.
const PX = 96;

const shapeIdOf = (end) => (end && end.shapeId ? String(end.shapeId).replace('shape:visio:', '') : null);

export function checkDiagram(model, pageIndex, size, { altText = false } = {}) {
  const page = model.pages[pageIndex];
  if (!page) return [];
  const shapes = (page.shapes || []).filter((s) => !s.group && s.w > 0 && s.h > 0);
  const connectors = page.connectors || [];
  const issues = [];
  const name = (s) => (s.label ? `"${String(s.label).split('\n')[0].slice(0, 30)}"` : `shape ${s.visioId}`);

  for (const c of connectors) {
    const ends = [['start', shapeIdOf(c.from)], ['end', shapeIdOf(c.to)]].filter(([, id]) => !id);
    if (ends.length) issues.push({ kind: 'loose end', id: String(c.visioId), fix: 'Delete connector', message: `Connector ${c.visioId} has a loose ${ends.map(([w]) => w).join(' and ')} (not glued to a shape)` });
  }

  // step 354: two connectors that join the same two shapes in the same direction (a copy that was pasted over the first one, or the same link drawn twice); the later one is reported, 'Delete connector' removes it
  const firstOf = new Map();
  for (const c of connectors) {
    const from = shapeIdOf(c.from), to = shapeIdOf(c.to);
    if (!from || !to) continue;
    const key = `${from}>${to}`;
    if (!firstOf.has(key)) { firstOf.set(key, c); continue; }
    const byId = new Map(shapes.map((s) => [String(s.visioId), s]));
    const nm = (id) => (byId.get(id) ? name(byId.get(id)) : `shape ${id}`);
    issues.push({ kind: 'duplicate connector', id: String(c.visioId), fix: 'Delete connector', message: `Connector ${c.visioId} joins ${nm(from)} to ${nm(to)} like connector ${firstOf.get(key).visioId} does` });
  }

  if (connectors.length) {
    const used = new Set();
    for (const c of connectors) for (const id of [shapeIdOf(c.from), shapeIdOf(c.to)]) if (id) used.add(id);
    for (const s of shapes) {
      const filled = s.fill && s.fill !== 'none';
      if (s.type === 'image' || !(s.label || filled) || used.has(String(s.visioId))) continue;
      // a text box (no fill, no line) is a caption, not a node of the diagram
      if (!filled && (!s.stroke || s.strokeWidth === 0)) continue;
      issues.push({ kind: 'not connected', id: String(s.visioId), message: `${name(s)} is not connected to anything` });
    }
  }

  const top = shapes.filter((s) => s.fill && s.fill !== 'none' && s.type !== 'image');
  for (let i = 0; i < top.length; i++) {
    for (let j = i + 1; j < top.length; j++) {
      const a = top[i], b = top[j];
      const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x), h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      if (!(w > 0 && h > 0)) continue;
      const inside = (p, q) => p.x >= q.x - 0.5 && p.y >= q.y - 0.5 && p.x + p.w <= q.x + q.w + 0.5 && p.y + p.h <= q.y + q.h + 0.5;
      if (inside(a, b) || inside(b, a)) continue; // a shape on a container is not an overlap
      const smaller = Math.min(a.w * a.h, b.w * b.h);
      if ((w * h) / smaller >= 0.25) issues.push({ kind: 'overlap', id: String(a.visioId), message: `${name(a)} and ${name(b)} overlap` });
    }
  }

  if (size) {
    const W = size.w * PX, H = size.h * PX;
    for (const s of shapes) {
      if (s.x < -0.5 || s.y < -0.5 || s.x + s.w > W + 0.5 || s.y + s.h > H + 0.5) issues.push({ kind: 'outside the page', id: String(s.visioId), fix: s.w <= W + 0.5 && s.h <= H + 0.5 ? 'Move into page' : undefined, message: `${name(s)} lies partly outside the page` });
    }
  }
  // accessibility (step 336, off unless asked for): a shape that says nothing to a screen reader, i.e. no text of its own and no alt text (pictures, icons, plain symbols); connectors are not listed
  if (altText) for (const s of shapes) if (!(s.label && String(s.label).trim()) && !(s.altText && String(s.altText).trim())) issues.push({ kind: 'no alt text', id: String(s.visioId), message: `${s.type === 'image' ? 'The picture' : 'Shape'} ${s.visioId} has neither text nor alt text (Shape data > Alt text)` });
  const order = ['loose end', 'duplicate connector', 'not connected', 'overlap', 'outside the page', 'no alt text'];
  return issues.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind));
}

/**
 * Repair one problem (one undo step): 'Delete connector' removes a connector with a loose end, 'Move into page' moves a shape that sticks out of the page just far enough in
 * (a shape larger than the page has no fix). Returns true when something was done.
 */
export function applyFix(engine, page, issue, model, size) {
  if (!issue || !issue.fix) return false;
  if (issue.fix === 'Delete connector') { engine.deleteShapes(page, [issue.id]); return true; }
  if (issue.fix === 'Move into page') {
    const s = ((model.pages[page] || {}).shapes || []).find((q) => String(q.visioId) === String(issue.id));
    if (!s || !size) return false;
    const W = size.w * PX, H = size.h * PX;
    let dx = 0, dy = 0; // pixels, screen directions (y down)
    if (s.x < 0) dx = -s.x; else if (s.x + s.w > W) dx = W - (s.x + s.w);
    if (s.y < 0) dy = -s.y; else if (s.y + s.h > H) dy = H - (s.y + s.h);
    if (!dx && !dy) return false;
    engine.moveMembers(page, [String(issue.id)], dx / PX, -dy / PX); // the engine moves in inches with y up
    return true;
  }
  return false;
}

/**
 * The problems as CSV text (step 346; a report to keep or send): columns Page, Problem, Shape, Message, Fix. `names` are the page names (index = page); a field with a comma, a quote or a line
 * break is quoted, quotes doubled. Lines end with CRLF (what spreadsheets expect).
 */
export function issuesToCsv(issues, names = []) {
  const field = (v) => { const t = String(v ?? ''); return /[",\r\n]/.test(t) ? `"${t.replace(/"/g, '""')}"` : t; };
  const rows = [['Page', 'Problem', 'Shape', 'Message', 'Fix']];
  for (const i of issues) rows.push([names[i.page] || (i.page === undefined ? '' : `Page ${i.page + 1}`), i.kind, i.id, String(i.message).replace(/^[^:]*: /, (m) => (i.page !== undefined && m === `${names[i.page] || `Page ${i.page + 1}`}: ` ? '' : m)), i.fix || '']);
  return rows.map((r) => r.map(field).join(',')).join('\r\n') + '\r\n';
}
