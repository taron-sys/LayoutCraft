// Design > Borders & Titles (step 325, RIB-08). A border is an ordinary background page (a page that other pages show beneath their own shapes, Page setup > Background page) made of
// plain shapes: the styles below are our own designs, not Visio's border masters. `applyBorder` makes the background page, assigns it to the page and returns its id; one undo step.
export const BORDER_STYLES = {
  'Frame': 'A thin frame around the page',
  'Double frame': 'Two frames, the inner one thinner',
  'Frame with title block': 'A frame and a title block at the lower right corner',
  'Title bar': 'A bar with the title along the top edge',
};
const INK = '#1f2933';

/** The shapes of a border for a page of w x h inches: [{kind, x0, y0, x1, y1, text?, style}] (kind 0 = rectangle), y up from the lower left corner. */
export function borderShapes(styleName, w, h, title = '') {
  const m = Math.min(0.25, Math.min(w, h) / 10); // the margin
  const frame = (inset, weight) => ({ kind: 0, x0: inset, y0: inset, x1: w - inset, y1: h - inset, style: { fill: 'none', line: INK, weight } });
  switch (styleName) {
    case 'Frame': return [frame(m, 1.5)];
    case 'Double frame': return [frame(m, 2.25), frame(m * 1.6, 0.75)];
    case 'Frame with title block': {
      const bw = Math.min(3, (w - 2 * m) * 0.6), bh = Math.min(0.8, (h - 2 * m) * 0.25);
      return [frame(m, 1.5), { kind: 0, x0: w - m - bw, y0: m, x1: w - m, y1: m + bh, text: title, style: { fill: '#ffffff', line: INK, weight: 1, fontPt: 14, bold: true } }];
    }
    case 'Title bar': {
      const bh = Math.min(0.6, h * 0.12);
      return [frame(m, 1), { kind: 0, x0: m, y0: h - m - bh, x1: w - m, y1: h - m, text: title, style: { fill: '#e6e9ee', line: INK, weight: 1, fontPt: 16, bold: true } }];
    }
    default: throw new Error(`no border style named ${styleName}`);
  }
}

/** Add the border `styleName` (with `title` where the style has one) beneath page `page`. Returns the id of the background page. One undo step. */
export function applyBorder(engine, page, styleName, title = '') {
  const size = engine.pageSize(page);
  const shapes = borderShapes(styleName, size.w, size.h, title);
  let id = null;
  engine.batch(() => {
    const index = engine.pageCount();
    engine.addPage(index - 1, `Border - ${styleName}`);
    engine.setBackPage(index, null); // a new page follows the background of the page before it: the border page must have none of its own
    engine.setPageSize(index, size.w, size.h);
    for (const s of shapes) {
      const shape = engine.drawShape(index, s.kind, s.x0, s.y0, s.x1, s.y1);
      engine.styleShapes(index, [shape], s.style);
      if (s.text) engine.setText(index, shape, s.text);
    }
    const before = new Set(engine.backgroundInfo().backgrounds.map((b) => b.id));
    engine.makeBackground(index);
    const added = engine.backgroundInfo().backgrounds.find((b) => !before.has(b.id)); // the list is not in the order of creation
    id = added.id;
    engine.setBackPage(page, id);
  });
  return id;
}

/** Take the border (the background page) off page `page`; the background page stays in the drawing. One undo step. */
export function removeBorder(engine, page) { engine.setBackPage(page, null); }
