// draw.io edge fidelity (step 311): pure helpers for the importer (web/drawio.js). Public draw.io style names only.
// Arrowheads use Visio's own numbering (measured on the installed Visio, docs/27: 1 open V, 4 / 13 filled triangle, 5 barbed filled, 10 filled circle, 11 filled square, 14 hollow
// triangle, 19 hollow barbed, 20 hollow circle, 21 hollow square, 22 hollow diamond, 23 slash, 24 one tick, 25 two ticks, 27 crow's foot, 28 crow's foot + tick, 29 crow's foot + circle,
// 31 tick + circle).

const FILLED = { block: 4, blockThin: 4, classic: 5, classicThin: 5, open: 1, openThin: 1, oval: 10, diamond: 22, diamondThin: 22, box: 11, halfCircle: 10, dash: 24, cross: 23, ERone: 24, ERmandOne: 25, ERmany: 27, ERoneToMany: 28, ERzeroToOne: 31, ERzeroToMany: 29, doubleBlock: 4 };
const HOLLOW = { block: 14, blockThin: 14, classic: 19, classicThin: 19, oval: 20, diamond: 22, diamondThin: 22, box: 21, halfCircle: 20 };

/** The Visio arrowhead number of a draw.io arrow name (`filled` = endFill / startFill is not 0); 0 for none, null for an unknown name. */
export function arrowIndex(name, filled = true) {
  if (name === undefined) return undefined;
  if (name === 'none' || name === '') return 0;
  const table = filled ? FILLED : { ...FILLED, ...HOLLOW };
  return table[name] ?? null;
}

/** draw.io's arrow size in px (default 6) as Visio's size index 0..6 (2 = medium). */
export function arrowSize(px) {
  const v = Number(px);
  if (!Number.isFinite(v)) return 2;
  return v <= 4 ? 1 : v <= 8 ? 2 : v <= 11 ? 3 : v <= 15 ? 4 : v <= 20 ? 5 : 6;
}

/** The connection point (1 top, 2 right, 3 bottom, 4 left, as the engine numbers the four points of a drawn shape) nearest to a position given as fractions (exitX / exitY, entryX / entryY; y down). */
export function sideOf(fx, fy) {
  const dx = fx - 0.5, dy = fy - 0.5;
  if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0 ? 2 : 4;
  return dy >= 0 ? 3 : 1;
}

/** Make a polyline axis-parallel (edgeStyle orthogonal / elbow): between two points that differ in both x and y a corner is added, alternating the direction. `firstHorizontal`: the first leg's direction (null: the longer axis). */
export function orthogonalize(points, firstHorizontal = null) {
  if (points.length < 2) return points.slice();
  const out = [points[0]];
  let horiz = firstHorizontal ?? Math.abs(points[1][0] - points[0][0]) >= Math.abs(points[1][1] - points[0][1]);
  for (let i = 1; i < points.length; i++) {
    const a = out[out.length - 1], b = points[i];
    if (Math.abs(a[0] - b[0]) < 1e-6 || Math.abs(a[1] - b[1]) < 1e-6) { out.push(b); horiz = Math.abs(a[1] - b[1]) < 1e-6; continue; }
    out.push(horiz ? [b[0], a[1]] : [a[0], b[1]], b);
  }
  return out;
}

/** The leg direction an exit side gives: true horizontal (left / right), false vertical (top / bottom), null unknown. */
export function exitHorizontal(fx, fy) {
  if (fx === undefined || fy === undefined) return null;
  return Math.abs(fx - 0.5) >= Math.abs(fy - 0.5);
}

/** Points through a curve: the polyline `pts` (at least two) as at least four points on a smooth Catmull-Rom curve through them (for engine.drawCurve). */
export function curveThrough(pts) {
  if (pts.length < 2) return pts.slice();
  if (pts.length === 2) { const [a, b] = pts; return [a, [a[0] + (b[0] - a[0]) / 3, a[1] + (b[1] - a[1]) / 3], [a[0] + ((b[0] - a[0]) * 2) / 3, a[1] + ((b[1] - a[1]) * 2) / 3], b]; }
  const out = [pts[0]];
  const p = (i) => pts[Math.max(0, Math.min(pts.length - 1, i))];
  for (let i = 0; i < pts.length - 1; i++) {
    const [p0, p1, p2, p3] = [p(i - 1), p(i), p(i + 1), p(i + 2)];
    for (const t of [1 / 3, 2 / 3]) {
      const t2 = t * t, t3 = t2 * t;
      out.push([0.5 * (2 * p1[0] + (-p0[0] + p2[0]) * t + (2 * p0[0] - 5 * p1[0] + 4 * p2[0] - p3[0]) * t2 + (-p0[0] + 3 * p1[0] - 3 * p2[0] + p3[0]) * t3), 0.5 * (2 * p1[1] + (-p0[1] + p2[1]) * t + (2 * p0[1] - 5 * p1[1] + 4 * p2[1] - p3[1]) * t2 + (-p0[1] + 3 * p1[1] - 3 * p2[1] + p3[1]) * t3)]);
    }
    out.push(p2);
  }
  return out;
}

/** Where the label of an edge goes: the point at `t` (draw.io: mxGeometry x -1..1 -> 0..1) along the path `pts`, `perp` px to the side (mxGeometry y), moved by the offset point. y down. */
export function labelPosition(pts, geoX = 0, geoY = 0, offset = [0, 0]) {
  const lens = [0];
  for (let i = 1; i < pts.length; i++) lens.push(lens[i - 1] + Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]));
  const total = lens[lens.length - 1] || 1;
  const want = Math.min(1, Math.max(0, (geoX + 1) / 2)) * total;
  let k = 1;
  while (k < pts.length - 1 && lens[k] < want) k++;
  const a = pts[k - 1], b = pts[k], seg = lens[k] - lens[k - 1] || 1;
  const f = (want - lens[k - 1]) / seg;
  const px = a[0] + (b[0] - a[0]) * f, py = a[1] + (b[1] - a[1]) * f;
  const tx = (b[0] - a[0]) / seg, ty = (b[1] - a[1]) / seg;
  return [px - ty * geoY + offset[0], py + tx * geoY + offset[1]];
}
