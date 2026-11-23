// Flatten SVG path data into polylines.
//
// Visio's Geometry section can express lines and arcs, but not arbitrary
// cubic/quadratic curves. The editor's shape catalogue and imported part
// paths are SVG, so new VSDX shapes are written as fine polyline
// approximations. Keeping this pure makes it testable outside the browser.

const r3 = (n) => Math.round(n * 1000) / 1000;

function tokenize(d) {
  return String(d || '').match(/[a-zA-Z]|-?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?/gi) || [];
}

/**
 * @returns {Array<{points: Array<{x:number,y:number}>, closed: boolean}>}
 */
export function flattenPath(d, steps = 12) {
  const tokens = tokenize(d);
  if (!tokens.length) return [];

  const out = [];
  let sub = null;
  let i = 0;
  let cmd = null;
  let x = 0, y = 0;
  let sx = 0, sy = 0;
  let lastCtrl = null;
  let prevCmd = null;

  const isNum = (t) => /^[-+.\d]/.test(t);
  const num = () => {
    const v = parseFloat(tokens[i]);
    i += 1;
    return Number.isFinite(v) ? v : 0;
  };
  const hasNums = (n) => {
    let j = i, seen = 0;
    while (j < tokens.length && seen < n && isNum(tokens[j])) { seen += 1; j += 1; }
    return seen === n;
  };
  const point = (px, py) => {
    if (!sub) {
      sub = { points: [], closed: false };
      out.push(sub);
    }
    const last = sub.points[sub.points.length - 1];
    if (!last || Math.hypot(last.x - px, last.y - py) > 1e-6) sub.points.push({ x: px, y: py });
    x = px; y = py;
  };
  const startSub = (px, py) => {
    sub = { points: [{ x: px, y: py }], closed: false };
    out.push(sub);
    x = sx = px;
    y = sy = py;
  };
  const lineTo = (px, py) => point(px, py);
  const cubic = (x1, y1, x2, y2, x3, y3) => {
    const x0 = x, y0 = y;
    const n = Math.max(4, steps);
    for (let k = 1; k <= n; k++) {
      const t = k / n, u = 1 - t;
      const px = u * u * u * x0 + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3;
      const py = u * u * u * y0 + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3;
      point(px, py);
    }
    lastCtrl = { x: x2, y: y2 };
  };
  const quad = (x1, y1, x2, y2) => {
    const x0 = x, y0 = y;
    const n = Math.max(4, steps);
    for (let k = 1; k <= n; k++) {
      const t = k / n, u = 1 - t;
      const px = u * u * x0 + 2 * u * t * x1 + t * t * x2;
      const py = u * u * y0 + 2 * u * t * y1 + t * t * y2;
      point(px, py);
    }
    lastCtrl = { x: x1, y: y1 };
  };
  const arc = (rx, ry, phiDeg, largeArc, sweep, ex, ey) => {
    rx = Math.abs(rx); ry = Math.abs(ry);
    if (!rx || !ry) { lineTo(ex, ey); return; }
    const phi = (phiDeg * Math.PI) / 180;
    const cos = Math.cos(phi), sin = Math.sin(phi);
    const dx = (x - ex) / 2, dy = (y - ey) / 2;
    const x1 = cos * dx + sin * dy;
    const y1 = -sin * dx + cos * dy;
    let rx2 = rx * rx, ry2 = ry * ry;
    const x12 = x1 * x1, y12 = y1 * y1;
    const lambda = x12 / rx2 + y12 / ry2;
    if (lambda > 1) {
      const s = Math.sqrt(lambda);
      rx *= s; ry *= s; rx2 = rx * rx; ry2 = ry * ry;
    }
    const sign = largeArc === sweep ? -1 : 1;
    const coef = sign * Math.sqrt(Math.max(0,
      (rx2 * ry2 - rx2 * y12 - ry2 * x12) / (rx2 * y12 + ry2 * x12)));
    const cx1 = coef * (rx * y1) / ry;
    const cy1 = coef * (-ry * x1) / rx;
    const cx = cos * cx1 - sin * cy1 + (x + ex) / 2;
    const cy = sin * cx1 + cos * cy1 + (y + ey) / 2;
    const ang = (ux, uy, vx, vy) => {
      const dot = ux * vx + uy * vy;
      const len = Math.hypot(ux, uy) * Math.hypot(vx, vy);
      let a = Math.acos(Math.max(-1, Math.min(1, dot / (len || 1))));
      if (ux * vy - uy * vx < 0) a = -a;
      return a;
    };
    const ux = (x1 - cx1) / rx, uy = (y1 - cy1) / ry;
    const vx = (-x1 - cx1) / rx, vy = (-y1 - cy1) / ry;
    const theta0 = ang(1, 0, ux, uy);
    let dtheta = ang(ux, uy, vx, vy);
    if (!sweep && dtheta > 0) dtheta -= Math.PI * 2;
    if (sweep && dtheta < 0) dtheta += Math.PI * 2;
    const n = Math.max(4, Math.ceil(Math.abs(dtheta) / (Math.PI / 12)));
    for (let k = 1; k <= n; k++) {
      const th = theta0 + (dtheta * k) / n;
      const px = cx + cos * rx * Math.cos(th) - sin * ry * Math.sin(th);
      const py = cy + sin * rx * Math.cos(th) + cos * ry * Math.sin(th);
      point(px, py);
    }
    lastCtrl = null;
  };

  while (i < tokens.length) {
    if (/^[a-zA-Z]$/.test(tokens[i])) {
      cmd = tokens[i];
      i += 1;
      if (cmd === 'Z' || cmd === 'z') {
        if (sub) {
          sub.closed = true;
          const first = sub.points[0];
          const last = sub.points[sub.points.length - 1];
          if (first && last && Math.hypot(first.x - last.x, first.y - last.y) > 1e-6) {
            sub.points.push({ x: first.x, y: first.y });
          }
          x = sx; y = sy;
        }
        prevCmd = cmd;
        continue;
      }
    } else if (!cmd) {
      i += 1;
      continue;
    }

    const rel = cmd === cmd.toLowerCase();
    const C = cmd.toUpperCase();
    if (C === 'M') {
      const px = num(), py = num();
      const ax = rel ? x + px : px, ay = rel ? y + py : py;
      if (prevCmd && prevCmd.toUpperCase() !== 'M' && sub && !sub.closed) {
        // A new M starts a new subpath; the previous one stays open.
      }
      startSub(ax, ay);
      lastCtrl = null;
      while (i < tokens.length && isNum(tokens[i])) {
        const lx = num(), ly = num();
        lineTo(rel ? x + lx : lx, rel ? y + ly : ly);
      }
      prevCmd = cmd;
      continue;
    }
    if (C === 'L') {
      while (i < tokens.length && isNum(tokens[i])) {
        const px = num(), py = num();
        lineTo(rel ? x + px : px, rel ? y + py : py);
      }
      lastCtrl = null;
    } else if (C === 'H') {
      while (i < tokens.length && isNum(tokens[i])) {
        const px = num();
        lineTo(rel ? x + px : px, y);
      }
      lastCtrl = null;
    } else if (C === 'V') {
      while (i < tokens.length && isNum(tokens[i])) {
        const py = num();
        lineTo(x, rel ? y + py : py);
      }
      lastCtrl = null;
    } else if (C === 'C') {
      while (i < tokens.length && isNum(tokens[i])) {
        const x1 = num(), y1 = num(), x2 = num(), y2 = num(), ex = num(), ey = num();
        const ax1 = rel ? x + x1 : x1, ay1 = rel ? y + y1 : y1;
        const ax2 = rel ? x + x2 : x2, ay2 = rel ? y + y2 : y2;
        const aex = rel ? x + ex : ex, aey = rel ? y + ey : ey;
        cubic(ax1, ay1, ax2, ay2, aex, aey);
      }
    } else if (C === 'S') {
      while (i < tokens.length && isNum(tokens[i])) {
        const x2 = num(), y2 = num(), ex = num(), ey = num();
        const ax2 = rel ? x + x2 : x2, ay2 = rel ? y + y2 : y2;
        const aex = rel ? x + ex : ex, aey = rel ? y + ey : ey;
        const refl = lastCtrl ? { x: 2 * x - lastCtrl.x, y: 2 * y - lastCtrl.y } : { x, y };
        cubic(refl.x, refl.y, ax2, ay2, aex, aey);
      }
    } else if (C === 'Q') {
      while (i < tokens.length && isNum(tokens[i])) {
        const x1 = num(), y1 = num(), ex = num(), ey = num();
        const ax1 = rel ? x + x1 : x1, ay1 = rel ? y + y1 : y1;
        const aex = rel ? x + ex : ex, aey = rel ? y + ey : ey;
        quad(ax1, ay1, aex, aey);
      }
    } else if (C === 'T') {
      while (i < tokens.length && isNum(tokens[i])) {
        const ex = num(), ey = num();
        const aex = rel ? x + ex : ex, aey = rel ? y + ey : ey;
        const refl = lastCtrl ? { x: 2 * x - lastCtrl.x, y: 2 * y - lastCtrl.y } : { x, y };
        quad(refl.x, refl.y, aex, aey);
      }
    } else if (C === 'A') {
      while (i < tokens.length && isNum(tokens[i])) {
        const rx = num(), ry = num(), phi = num(), large = num(), sweep = num();
        const ex = num(), ey = num();
        const aex = rel ? x + ex : ex, aey = rel ? y + ey : ey;
        arc(rx, ry, phi, large !== 0, sweep !== 0, aex, aey);
      }
    } else {
      // Unknown command: skip one token to avoid an infinite loop.
      i += 1;
    }
    prevCmd = cmd;
  }

  return out.filter((s) => s.points.length > 0);
}

/** Bounding box of flattened paths, in the same units as the input. */
export function pathBounds(subpaths) {
  let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
  for (const sub of subpaths) {
    for (const p of sub.points) {
      if (p.x < x1) x1 = p.x;
      if (p.y < y1) y1 = p.y;
      if (p.x > x2) x2 = p.x;
      if (p.y > y2) y2 = p.y;
    }
  }
  if (!Number.isFinite(x1)) return { x: 0, y: 0, w: 0, h: 0 };
  return { x: r3(x1), y: r3(y1), w: r3(x2 - x1), h: r3(y2 - y1) };
}
