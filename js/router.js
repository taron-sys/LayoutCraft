// Connector geometry: glue points, side selection and route generation.
import { rot, clamp } from './util.js';

export const SIDES = ['n', 'e', 's', 'w'];

const SIDE_VEC = {
  n: { x: 0, y: -1 },
  e: { x: 1, y: 0 },
  s: { x: 0, y: 1 },
  w: { x: -1, y: 0 },
};

export const sideVector = (side, angle = 0) => rot(SIDE_VEC[side] || SIDE_VEC.e, angle);

/** Local box point for a side (before rotation). */
export function sideLocalPoint(s, side) {
  switch (side) {
    case 'n': return { x: s.w / 2, y: 0 };
    case 's': return { x: s.w / 2, y: s.h };
    case 'w': return { x: 0, y: s.h / 2 };
    default:  return { x: s.w, y: s.h / 2 };
  }
}

/** Shape rotation in radians. The model stores `rot` in degrees. */
const shapeRadians = (s) => ((s.rot || 0) * Math.PI) / 180;

/** Convert a point in a shape's local box to world space. */
export function localToWorld(s, p) {
  const cx = s.x + s.w / 2, cy = s.y + s.h / 2;
  const fx = s.flipX ? -1 : 1, fy = s.flipY ? -1 : 1;
  const r = rot({ x: fx * (p.x - s.w / 2), y: fy * (p.y - s.h / 2) }, shapeRadians(s));
  return { x: cx + r.x, y: cy + r.y };
}

/** Outward direction of a side, taking rotation and mirroring into account. */
export function sideVectorFor(s, side) {
  const v = sideVector(side, shapeRadians(s));
  return { x: v.x * (s.flipX ? -1 : 1), y: v.y * (s.flipY ? -1 : 1) };
}

/** World-space point where a connector glues onto a shape. */
export function anchorOnShape(s, side) {
  return localToWorld(s, sideLocalPoint(s, side));
}

/**
 * The point on `side` closest to `toward`, not the side's midpoint. This is
 * what keeps a connector on a large background shape from reaching all the
 * way to a far-away midpoint.
 */
export function anchorOnShapeAt(s, side, toward) {
  if (!toward) return anchorOnShape(s, side);
  const cx = s.x + s.w / 2, cy = s.y + s.h / 2;
  const dx = toward.x - cx, dy = toward.y - cy;
  const a = -((s.rot || 0) * Math.PI) / 180;
  const rx = dx * Math.cos(a) - dy * Math.sin(a);
  const ry = dx * Math.sin(a) + dy * Math.cos(a);
  const lx = rx / (s.flipX ? -1 : 1) + s.w / 2;
  const ly = ry / (s.flipY ? -1 : 1) + s.h / 2;
  let p;
  switch (side) {
    case 'n': p = { x: clamp(lx, 0, s.w), y: 0 }; break;
    case 's': p = { x: clamp(lx, 0, s.w), y: s.h }; break;
    case 'w': p = { x: 0, y: clamp(ly, 0, s.h) }; break;
    default: p = { x: s.w, y: clamp(ly, 0, s.h) }; break;
  }
  return localToWorld(s, p);
}

export function shapeCenter(s) {
  return { x: s.x + s.w / 2, y: s.y + s.h / 2 };
}

/** Pick the side of `s` that best faces the world direction (dx,dy). */
export function bestSide(s, dx, dy) {
  let best = 'e', score = -Infinity;
  for (const side of SIDES) {
    const v = sideVectorFor(s, side);
    const sc = v.x * dx + v.y * dy;
    if (sc > score) { score = sc; best = side; }
  }
  return best;
}

/** Choose glue sides for a connector between two shapes. */
export function autoSides(a, b) {
  const ca = shapeCenter(a), cb = shapeCenter(b);
  const dx = cb.x - ca.x, dy = cb.y - ca.y;
  if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return { from: 'e', to: 'w' };
  return { from: bestSide(a, dx, dy), to: bestSide(b, -dx, -dy) };
}

/** Resolve one end of a connector to a concrete world point. */
export function endpointPoint(end, shapesById, toward) {
  if (end.shapeId) {
    const s = shapesById.get(end.shapeId);
    if (s) {
      // an imported glue point keeps Visio's exact attachment, scaled with
      // the shape; a point made by dragging an end slides along its side,
      // staying as close as possible to the other end
      if (end.local) return localToWorld(s, { x: end.local.x * s.w, y: end.local.y * s.h });
      return anchorOnShapeAt(s, end.side || 'e', toward);
    }
  }
  return end.point ? { x: end.point.x, y: end.point.y } : { x: 0, y: 0 };
}

/** Direction the connector leaves/arrives at an end. */
export function endDirection(end, shapesById, fallback) {
  if (end.shapeId) {
    const s = shapesById.get(end.shapeId);
    if (s) return sideVectorFor(s, end.side || 'e');
  }
  return fallback;
}

const ortho = (v) => (Math.abs(v.x) >= Math.abs(v.y) ? 'h' : 'v');

/**
 * Drop consecutive duplicate points. Visio connectors often store a
 * waypoint equal to the end point; the resulting zero-length last segment
 * leaves SVG's marker orientation undefined, so a vertical arrowhead is
 * drawn horizontally. Keeping the last real segment fixes that.
 */
function dedupePoints(points) {
  const out = [];
  for (const p of points) {
    const q = out[out.length - 1];
    if (!q || Math.hypot(p.x - q.x, p.y - q.y) > 0.01) out.push(p);
  }
  if (out.length === 1) out.push({ x: out[0].x, y: out[0].y });
  return out;
}

const poly = (points) => ({ kind: 'poly', points: dedupePoints(points) });

/**
 * Build the polyline / curve for a connector.
 * Returns { kind: 'line'|'poly'|'curve', points?, d? }
 */
export function routeConnector(conn, shapesById) {
  const A0 = endpointPoint(conn.from, shapesById);
  const B0 = endpointPoint(conn.to, shapesById);
  const waypoints = conn.waypoints || [];
  const centerOf = (end) => {
    const s = end.shapeId ? shapesById.get(end.shapeId) : null;
    return s ? shapeCenter(s) : null;
  };
  // A side-glued end slides towards the other end, so a connector attached
  // to a page-sized background does not run to the far side's midpoint.
  const towardA = waypoints.length ? waypoints[0] : (centerOf(conn.to) || B0);
  const towardB = waypoints.length ? waypoints[waypoints.length - 1] : (centerOf(conn.from) || A0);
  const A = endpointPoint(conn.from, shapesById, towardA);
  const B = endpointPoint(conn.to, shapesById, towardB);

  if (waypoints.length) {
    return poly([A, ...waypoints, B]);
  }

  const dirA = endDirection(conn.from, shapesById, norm({ x: B.x - A.x, y: B.y - A.y }));
  const dirB = endDirection(conn.to, shapesById, norm({ x: A.x - B.x, y: A.y - B.y }));

  if (conn.route === 'curved') {
    const d = Math.hypot(B.x - A.x, B.y - A.y);
    const k = clamp(d * 0.35, 16, 90);
    // control points sit on the outward normal of each end, so the curve
    // leaves and enters the shapes along their glued sides
    const c1 = { x: A.x + dirA.x * k, y: A.y + dirA.y * k };
    const c2 = { x: B.x + dirB.x * k, y: B.y + dirB.y * k };
    return {
      kind: 'curve',
      d: `M${f(A.x)} ${f(A.y)}C${f(c1.x)} ${f(c1.y)} ${f(c2.x)} ${f(c2.y)} ${f(B.x)} ${f(B.y)}`,
      points: [A, B],
    };
  }

  if (conn.route === 'elbow') {
    // Visio's ShapeRouteStyle variants 3-6 force a first segment direction and
    // a fixed fraction of the span, instead of choosing from the glue sides.
    const rs = Number(conn.routeStyle);
    if (rs === 3 || rs === 5) {
      const t = rs === 5 ? 0.25 : 0.75;
      const my = A.y + (B.y - A.y) * t;
      return poly([A, { x: A.x, y: my }, { x: B.x, y: my }, B]);
    }
    if (rs === 4 || rs === 6) {
      const t = rs === 6 ? 0.1 : 0.9;
      const mx = A.x + (B.x - A.x) * t;
      return poly([A, { x: mx, y: A.y }, { x: mx, y: B.y }, B]);
    }
    const a = ortho(dirA), b = ortho(dirB);
    if (a === 'h' && b === 'h') {
      const my = (A.y + B.y) / 2;
      return poly([A, { x: A.x, y: my }, { x: B.x, y: my }, B]);
    }
    if (a === 'v' && b === 'v') {
      const mx = (A.x + B.x) / 2;
      return poly([A, { x: mx, y: A.y }, { x: mx, y: B.y }, B]);
    }
    if (a === 'h') {
      const mx = (A.x + B.x) / 2;
      return poly([A, { x: mx, y: A.y }, { x: mx, y: B.y }, B]);
    }
    const my = (A.y + B.y) / 2;
    return poly([A, { x: A.x, y: my }, { x: B.x, y: my }, B]);
  }

  return { kind: 'line', points: [A, B] };
}

/** Point at the middle of a route (for labels and bend handles). */
export function routeMidpoint(route) {
  if (route.kind === 'curve') {
    const p = route.points;
    return { x: (p[0].x + p[1].x) / 2, y: (p[0].y + p[1].y) / 2 };
  }
  const pts = route.points;
  if (pts.length === 2) return { x: (pts[0].x + pts[1].x) / 2, y: (pts[0].y + pts[1].y) / 2 };
  let total = 0;
  const segs = [];
  for (let i = 0; i < pts.length - 1; i++) {
    const l = Math.hypot(pts[i + 1].x - pts[i].x, pts[i + 1].y - pts[i].y);
    segs.push(l); total += l;
  }
  let want = total / 2;
  for (let i = 0; i < segs.length; i++) {
    if (want <= segs[i]) {
      const t = segs[i] ? want / segs[i] : 0;
      return {
        x: pts[i].x + (pts[i + 1].x - pts[i].x) * t,
        y: pts[i].y + (pts[i + 1].y - pts[i].y) * t,
      };
    }
    want -= segs[i];
  }
  return pts[Math.floor(pts.length / 2)];
}

function norm(v) {
  const l = Math.hypot(v.x, v.y) || 1;
  return { x: v.x / l, y: v.y / l };
}

const f = (n) => Math.round(n * 100) / 100;
