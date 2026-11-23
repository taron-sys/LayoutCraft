// Excalidraw import (TODO H, step 301): a scene (.excalidraw JSON, the open format: {type: 'excalidraw', version, elements: [...], appState, files}) becomes a new drawing in the engine.
// Written from the public file format only. Pixels become inches (96 per inch), y is flipped (Excalidraw y grows downwards, the page's grows upwards).
// Mapped: rectangle (rounded when it has roundness), ellipse, diamond, text (free text box; text bound to a container goes into the container's own text), line / arrow / freedraw (open or closed polylines),
// arrows bound at both ends become connectors glued to the two shapes, stroke / fill colour, stroke width and style (solid, dashed, dotted), opacity, rotation, font size and family, arrowheads.
// Frames (step 332) are drawn as rectangles with their name in a text box above the top left corner; images (step 326) are placed. Not mapped (counted in `skipped`): embeddables, groups' nesting (the shapes arrive ungrouped), roughness / hand-drawn look, fill hatch styles (a hatch fills solid), z-order beyond file order.

const PX_PER_IN = 96;
const KINDS = { rectangle: 0, ellipse: 2, diamond: 3, frame: 0, magicframe: 0 }; // a frame is a rectangle around other elements (its name goes above it)
const FONTS = { 1: 'Segoe Print', 2: 'Arial', 3: 'Consolas', 5: 'Segoe Print', 6: 'Calibri', 7: 'Cambria', 8: 'Consolas' };
const ARROWS = { arrow: 13, triangle: 13, bar: 6, dot: 10, circle: 10, diamond: 15, circle_outline: 10, diamond_outline: 15, triangle_outline: 13 };
const colour = (c) => (typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c) ? c.toLowerCase() : /^#[0-9a-f]{3}$/i.test(c || '') ? '#' + c.slice(1).split('').map((d) => d + d).join('').toLowerCase() : null);

/** Parse and check an Excalidraw scene; throws an Error with a plain message for anything that is not one. */
export function parseScene(input) {
  let scene = input;
  if (typeof input === 'string') {
    try { scene = JSON.parse(input); } catch { throw new Error('This is not an Excalidraw file (the JSON cannot be read)'); }
  }
  if (!scene || scene.type !== 'excalidraw' || !Array.isArray(scene.elements)) throw new Error('This is not an Excalidraw file (no "type": "excalidraw" and elements list)');
  return scene;
}

/** The bytes of the picture an `image` element shows (`scene.files[fileId].dataURL`, base64), or null when there is none or it is not a PNG / JPEG / GIF / BMP. */
function pictureBytes(scene, e) {
  const file = scene.files && scene.files[e.fileId];
  const m = file && typeof file.dataURL === 'string' ? /^data:image\/(png|jpe?g|gif|bmp);base64,(.+)$/is.exec(file.dataURL) : null;
  if (!m || !(e.width > 0) || !(e.height > 0)) return null;
  try { const bin = atob(m[2].replace(/\s+/g, '')); const bytes = new Uint8Array(bin.length); for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i); return bytes; } catch { return null; }
}

function boundsOf(elements) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  const add = (x, y) => { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); };
  for (const e of elements) {
    if (Array.isArray(e.points) && e.points.length) for (const p of e.points) add(e.x + p[0], e.y + p[1]);
    else { add(e.x, e.y); add(e.x + (e.width || 0), e.y + (e.height || 0)); }
  }
  return Number.isFinite(x0) ? { x0, y0, x1, y1 } : { x0: 0, y0: 0, x1: 0, y1: 0 };
}

/**
 * Build a new drawing in `engine` from the scene. Returns {page, shapes, connectors, skipped: {type: count}, map: {excalidrawId: shapeId}}.
 * One undo step per shape, as drawing is; the drawing replaces the engine's document.
 */
export function importExcalidraw(engine, input, { margin = 0.5 } = {}) {
  const scene = parseScene(input);
  const all = scene.elements.filter((e) => e && typeof e === 'object' && !e.isDeleted);
  const skipped = {};
  const skip = (e) => { skipped[e.type] = (skipped[e.type] || 0) + 1; };
  const drawable = all.filter((e) => e.type in KINDS || e.type === 'text' || e.type === 'line' || e.type === 'arrow' || e.type === 'freedraw' || (e.type === 'image' && pictureBytes(scene, e)) || (skip(e), false));
  const b = boundsOf(drawable);
  const width = Math.max(2, (b.x1 - b.x0) / PX_PER_IN + 2 * margin), height = Math.max(2, (b.y1 - b.y0) / PX_PER_IN + 2 * margin);
  engine.newDrawing({ width: Math.round(width * 1000) / 1000, height: Math.round(height * 1000) / 1000, metric: false, name: 'Excalidraw' });
  const X = (x) => (x - b.x0) / PX_PER_IN + margin;
  const Y = (y) => height - ((y - b.y0) / PX_PER_IN + margin);
  const map = {}, shapes = [], boundText = {};
  for (const e of drawable) if (e.type === 'text' && e.containerId) boundText[e.containerId] = e;

  const style = (e, id, extra = {}) => {
    const s = { ...extra };
    const line = colour(e.strokeColor), fill = colour(e.backgroundColor);
    if (line) s.line = line;
    if (e.backgroundColor === 'transparent' || e.backgroundColor == null) s.fill = 'none'; else if (fill) s.fill = fill;
    if (e.strokeWidth > 0) s.weight = e.strokeWidth;
    s.pattern = e.strokeStyle === 'dashed' ? 2 : e.strokeStyle === 'dotted' ? 3 : 1;
    if (typeof e.opacity === 'number' && e.opacity < 100) { const t = Math.round((1 - e.opacity / 100) * 100) / 100; s.lineTrans = t; if (s.fill !== 'none') s.fillTrans = t; }
    engine.styleShapes(0, [id], s);
  };
  const fontOf = (e) => ({ fontPt: Math.round((e.fontSize || 20) * 0.75 * 10) / 10, fontFamily: FONTS[e.fontFamily] || 'Arial', textColor: colour(e.strokeColor) || '#000000' });
  const turn = (e, id) => { if (e.angle) engine.rotateShape(0, id, ((-e.angle % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI)); };

  // closed shapes first (connectors need them), then free text, then lines
  for (const e of drawable.filter((d) => d.type in KINDS)) {
    const kind = e.type === 'rectangle' && e.roundness ? 1 : KINDS[e.type];
    const id = engine.drawShape(0, kind, X(e.x), Y(e.y + e.height), X(e.x + e.width), Y(e.y));
    map[e.id] = id; shapes.push(id);
    style(e, id);
    const t = boundText[e.id];
    if (t && t.text) { engine.setText(0, id, t.text); engine.styleShapes(0, [id], fontOf(t)); }
    turn(e, id);
    if ((e.type === 'frame' || e.type === 'magicframe') && e.name) { // the frame's name sits above its top left corner, like Excalidraw shows it
      const nameBox = engine.drawShape(0, 5, X(e.x), Y(e.y), X(e.x + Math.min(Math.max(e.width, 60), (e.name.length * 9) + 12)), Y(e.y - 24));
      engine.styleShapes(0, [nameBox], { fill: 'none', line: 'none', fontPt: 11, textColor: '#666666', fontFamily: 'Arial' });
      engine.setText(0, nameBox, e.name);
      shapes.push(nameBox);
    }
  }
  // pictures (step 326): an `image` element names a file of the scene's `files` (a data: URL); PNG, JPEG, GIF and BMP are placed, other kinds (SVG) are counted as skipped
  for (const e of drawable.filter((d) => d.type === 'image')) {
    try {
      const id = engine.insertPicture(0, pictureBytes(scene, e), X(e.x), Y(e.y + e.height), X(e.x + e.width), Y(e.y));
      map[e.id] = id; shapes.push(id);
      turn(e, id);
    } catch { skip(e); }
  }
  for (const e of drawable.filter((d) => d.type === 'text' && !d.containerId)) {
    const id = engine.drawShape(0, 5, X(e.x), Y(e.y + e.height), X(e.x + e.width), Y(e.y));
    map[e.id] = id; shapes.push(id);
    engine.styleShapes(0, [id], { fill: 'none', line: 'none', ...fontOf(e) });
    engine.setText(0, id, e.text || '');
    engine.styleShapes(0, [id], fontOf(e));
    turn(e, id);
  }
  let connectors = 0;
  for (const e of drawable.filter((d) => d.type === 'line' || d.type === 'arrow' || d.type === 'freedraw')) {
    const pts = (e.points || []).map((p) => [X(e.x + p[0]), Y(e.y + p[1])]);
    if (pts.length < 2) { skip(e); continue; }
    const from = e.startBinding && map[e.startBinding.elementId], to = e.endBinding && map[e.endBinding.elementId];
    let id, closed = false;
    if (e.type === 'arrow' && from && to && from !== to) {
      id = engine.addConnector(0, { id: from, n: 0 }, { id: to, n: 0 });
      connectors++;
    } else {
      closed = e.type !== 'arrow' && pts.length > 2 && Math.hypot(pts[0][0] - pts[pts.length - 1][0], pts[0][1] - pts[pts.length - 1][1]) < 0.02;
      id = engine.drawPolyline(0, closed ? pts.slice(0, -1) : pts, closed);
    }
    map[e.id] = id; shapes.push(id);
    const extra = {};
    if (e.type === 'arrow') {
      const end = ARROWS[e.endArrowhead], begin = ARROWS[e.startArrowhead];
      if (end) extra.endArrow = end; if (begin) extra.beginArrow = begin;
    }
    style(e, id, closed ? extra : { ...extra, fill: 'none' });
  }
  return { page: 0, shapes, connectors, skipped, map };
}
