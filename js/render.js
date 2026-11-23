// SVG renderer: draws shapes, connectors and the selection overlay.
import {
  shapePath, shapeDetail, shapeTextBox,
} from './shapes.js';
import {
  routeConnector, routeMidpoint, localToWorld,
  anchorOnShapeAt, bestSide, shapeCenter,
} from './router.js';
import { fontString, wrapText, measureText } from './util.js';
import { calloutText, calloutTarget, barRatio, iconLevel, dataColorFill } from './model.js';

const NS = 'http://www.w3.org/2000/svg';
const DPI = 96;

function el(tag, attrs = {}) {
  const e = document.createElementNS(NS, tag);
  for (const k in attrs) {
    if (attrs[k] === undefined || attrs[k] === null) continue;
    e.setAttribute(k, attrs[k]);
  }
  return e;
}

export function dashArray(style, w) {
  if (style === 'dashed') return `${Math.max(4, w * 6)} ${Math.max(3, w * 4)}`;
  if (style === 'dotted') return `${Math.max(1, w)} ${Math.max(2, w * 2.2)}`;
  // Visio's higher LinePattern indices are the assorted dash/dot presets.
  if (style === 'dashFine') return `${Math.max(3, w * 3)} ${Math.max(3, w * 3)}`;
  return null;
}

const HANDLES = [
  ['nw', 0, 0], ['n', 0.5, 0], ['ne', 1, 0], ['e', 1, 0.5],
  ['se', 1, 1], ['s', 0.5, 1], ['sw', 0, 1], ['w', 0, 0.5],
];

/** Imported picture captions keep a frame-relative text block. */
function textBoxOf(s) {
  if (s.textBoxRel) {
    return {
      x: s.textBoxRel.x * s.w, y: s.textBoxRel.y * s.h,
      w: s.textBoxRel.w * s.w, h: s.textBoxRel.h * s.h,
    };
  }
  return s.textBox || shapeTextBox(s.type, s.w, s.h);
}

export class Renderer {
  constructor(svg, app) {
    this.svg = svg;
    this.app = app;
    this.defs = svg.querySelector('#canvasDefs');
    this.gridBg = svg.querySelector('#gridBg');
    this.scene = svg.querySelector('#scene');
    this.contentLayer = svg.querySelector('#layerContent');
    this.overlay = svg.querySelector('#layerOverlay');
    this.buildDefs();
  }

  buildDefs() {
    const pat = el('pattern', {
      id: 'gridPattern', width: 50, height: 50, patternUnits: 'userSpaceOnUse',
    });
    pat.appendChild(el('path', {
      d: 'M0 0H50V50H0Z M10 0V50 M20 0V50 M30 0V50 M40 0V50 M0 10H50 M0 20H50 M0 30H50 M0 40H50',
      fill: 'none', stroke: '#d3d9e2', 'stroke-width': 1,
    }));
    this.defs.appendChild(pat);

    const mk = (id, d, filled, opts = {}) => {
      const m = el('marker', {
        id, viewBox: '0 0 12 12', refX: opts.refX ?? 10.5, refY: 6,
        markerWidth: opts.width ?? 8, markerHeight: opts.height ?? 8,
        markerUnits: 'strokeWidth',
        orient: 'auto-start-reverse',
      });
      m.appendChild(el('path', {
        d, fill: filled ? 'context-stroke' : (opts.fill ?? 'none'),
        stroke: 'context-stroke',
        'stroke-width': filled ? 0 : (opts.strokeWidth ?? 1.4),
        'stroke-linecap': 'round', 'stroke-linejoin': 'round',
      }));
      this.defs.appendChild(m);
    };
    mk('mArrowOpen', 'M2 1.5L10.5 6L2 10.5', false);
    mk('mArrowSolid', 'M2 1.5L10.5 6L2 10.5Z', true);
    // UML relation ends: inheritance, aggregation and composition.
    mk('mHollowTriangle', 'M1.8 1.4L10.8 6L1.8 10.6Z', false,
      { fill: '#ffffff', refX: 10.8, width: 16, height: 16 });
    mk('mHollowDiamond', 'M11 6L6.3 1.4L1.6 6L6.3 10.6Z', false,
      { fill: '#ffffff', refX: 11, width: 16, height: 16, strokeWidth: 1.3 });
    mk('mFilledDiamond', 'M11 6L6.3 1.4L1.6 6L6.3 10.6Z', true,
      { refX: 11, width: 16, height: 16 });
  }

  /** Update grid pattern + scene transform from the current view. */
  updateView() {
    const { x, y, zoom } = this.app.state.view;
    const gs = this.app.state.grid.size;
    this.scene.setAttribute('transform', `translate(${x} ${y}) scale(${zoom})`);

    const mc = gs * 5 * zoom;
    const pat = this.defs.querySelector('#gridPattern');
    if (this.app.state.grid.on && mc >= 14) {
      const ox = ((x % mc) + mc) % mc;
      const oy = ((y % mc) + mc) % mc;
      pat.setAttribute('width', mc);
      pat.setAttribute('height', mc);
      pat.setAttribute('patternTransform', `translate(${ox} ${oy})`);
      const g = gs * zoom;
      pat.firstChild.setAttribute('d',
        `M0 0H${mc}V${mc}H0Z` +
        `M${g} 0V${mc}M${g * 2} 0V${mc}M${g * 3} 0V${mc}M${g * 4} 0V${mc}` +
        `M0 ${g}H${mc}M0 ${g * 2}H${mc}M0 ${g * 3}H${mc}M0 ${g * 4}H${mc}`);
      this.gridBg.setAttribute('fill', 'url(#gridPattern)');
      this.gridBg.style.display = '';
    } else {
      this.gridBg.style.display = 'none';
    }
  }

  render() {
    const app = this.app;
    const pg = app.getPage();
    const shapesById = new Map(pg.shapes.map((s) => [s.id, s]));

    this.contentLayer.textContent = '';
    this.overlay.textContent = '';
    // fresh gradient defs each render: imported bevels create them on demand
    this.partDefs = el('defs');
    this.contentLayer.appendChild(this.partDefs);
    this.gradSeq = 0;
    this.clipSeq = 0;

    // Group text sits behind the members, like Visio's group label.
    const groupFrames = this.groupFrames(pg);
    for (const g of pg.groups || []) {
      if (!g.label) continue;
      const frame = groupFrames.get(String(g.visioId));
      if (!g.rotated && !frame) continue;
      const node = this.buildGroupLabel(g, frame, pg);
      if (node) this.contentLayer.appendChild(node);
    }

    // shapes and connectors share one z-order, the way Visio stacks them
    const items = [];
    pg.connectors.forEach((c, i) => items.push({ z: c.z ?? -1, node: this.buildConnector(c, shapesById) }));
    // Data-callout leaders sit above the target but below the callout box.
    pg.shapes.forEach((s, i) => {
      // Shapes only on hidden Visio layers are not drawn.
      if (s.layerHidden) return;
      const z = s.z ?? 1000 + i;
      if (s.callout && s.callout.kind === 'text') {
        const target = calloutTarget(s, pg);
        if (target) {
          const leader = this.buildCalloutLeader(s, target);
          if (leader) items.push({ z: z - 0.5, node: leader });
        }
      }
      items.push({ z, node: this.buildShape(s) });
    });
    items.sort((a, b) => a.z - b.z);
    for (const it of items) this.contentLayer.appendChild(it.node);
    this.renderOverlay(pg, shapesById);
    this.updateView();
  }

  // ------------------------------------------------------------- groups --
  /** Current world-pixel frame for every imported group. */
  groupFrames(pg) {
    const groups = (pg.groups || []).filter((g) => g.visioId);
    const frames = new Map();
    if (!groups.length) return frames;
    const byParent = new Map();
    for (const g of groups) {
      const key = g.parentId == null ? '' : String(g.parentId);
      if (!byParent.has(key)) byParent.set(key, []);
      byParent.get(key).push(g);
    }
    const bottomUp = [...groups].sort((a, b) => (b.depth || 0) - (a.depth || 0));
    for (const g of bottomUp) {
      const vid = String(g.visioId);
      const direct = pg.shapes.filter((s) => s.group && String(s.group.id) === vid);
      const childGroups = byParent.get(vid) || [];
      let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
      const grow = (ax, ay, bx, by) => {
        x1 = Math.min(x1, ax); y1 = Math.min(y1, ay);
        x2 = Math.max(x2, bx); y2 = Math.max(y2, by);
      };
      for (const s of direct) grow(s.x, s.y, s.x + s.w, s.y + s.h);
      for (const h of childGroups) {
        const f = frames.get(String(h.visioId));
        if (f) grow(f.x, f.y, f.x + f.w, f.y + f.h);
      }
      if (!Number.isFinite(x1)) continue;
      frames.set(vid, { x: x1, y: y1, w: Math.max(1, x2 - x1), h: Math.max(1, y2 - y1) });
    }
    return frames;
  }

  buildGroupLabel(g, frame, pg) {
    const rel = g.labelRel || { x: 0, y: 0.45, w: 1, h: 0.1 };
    const st = g.labelStyle || {};
    let bx, by, bw, bh, rot = 0;
    if (g.rotated && g.labelCenter && g.labelSize) {
      // The label centre was mapped through the group's own transform at
      // import. Follow the live group by the same delta as one of its
      // members; rotated resizes are not representable and are warned about.
      const member = pg.shapes.find((s) => s.group
        && String(s.group.rootId) === String(g.rootId) && s.visio && s.visio.orig);
      let dx = 0, dy = 0;
      if (member) {
        const o = member.visio.orig;
        dx = (member.x + member.w / 2) - (o.x + o.w / 2);
        dy = (member.y + member.h / 2) - (o.y + o.h / 2);
      }
      const pageH = pg.visio?.pageH || 0;
      const cx = g.labelCenter.x * DPI + dx;
      const cy = (pageH - g.labelCenter.y) * DPI + dy;
      bw = Math.max(10, g.labelSize.w * DPI);
      bh = Math.max(10, g.labelSize.h * DPI);
      bx = cx - bw / 2;
      by = cy - bh / 2;
      rot = -(g.angle || 0) * 180 / Math.PI;
    } else {
      if (!frame) return null;
      bx = frame.x + rel.x * frame.w;
      // labelRel.y is measured from the group's lower-left in page space; the
      // frame here is world top-left, so flip the y offset.
      by = frame.y + (1 - rel.y - rel.h) * frame.h;
      bw = Math.max(10, rel.w * frame.w);
      bh = Math.max(10, rel.h * frame.h);
    }
    const pseudo = {
      label: g.label,
      type: 'rect',
      w: bw, h: bh,
      textBox: { x: 0, y: 0, w: bw, h: bh },
      fontFamily: st.fontFamily || 'Calibri',
      fontSize: st.fontSize || 12,
      bold: !!st.bold, italic: !!st.italic, underline: !!st.underline,
      textColor: st.textColor || '#1b2733',
      align: st.align || 'center',
      vAlign: st.vAlign || 'middle',
    };
    const transform = rot
      ? `translate(${bx + bw / 2} ${by + bh / 2}) rotate(${rot}) translate(${-bw / 2} ${-bh / 2})`
      : `translate(${bx} ${by})`;
    const node = el('g', {
      transform,
      'pointer-events': 'none',
      opacity: 0.92,
      class: 'group-label',
    });
    node.appendChild(this.renderText(pseudo));
    return node;
  }

  // ------------------------------------------------------------ callouts --
  /** A thin leader from the callout edge to the nearest target edge. */
  buildCalloutLeader(s, target) {
    if (!s.w || !s.h || !target.w || !target.h) return null;
    const a = { x: s.x + s.w / 2, y: s.y + s.h / 2 };
    const b = { x: target.x + target.w / 2, y: target.y + target.h / 2 };
    const p1 = boxEdgePoint({ x: s.x, y: s.y, w: s.w, h: s.h }, b);
    const p2 = boxEdgePoint({ x: target.x, y: target.y, w: target.w, h: target.h }, a);
    const len = Math.hypot(p2.x - p1.x, p2.y - p1.y);
    if (len < 4) return null;
    const ux = (p2.x - p1.x) / len;
    const uy = (p2.y - p1.y) / len;
    return el('line', {
      x1: p1.x + ux * 2, y1: p1.y + uy * 2,
      x2: p2.x - ux * 2, y2: p2.y - uy * 2,
      stroke: s.stroke || '#c9b458',
      'stroke-width': 1.2,
      'stroke-linecap': 'round',
    });
  }

  /**
   * A five-level icon set: boxes, diamonds or circles in a row. The first
   * `iconLevel` glyphs are filled, the rest only outlined.
   */
  buildDataIcon(s) {
    const level = iconLevel(s, this.app.getPage());
    const set = s.callout.iconSet || 'boxes';
    const pad = Math.max(0.5, Math.min(3, s.h * 0.06));
    const gap = Math.max(1, Math.min(4, s.h * 0.16));
    const cell = Math.max(1, Math.min(s.h - 2 * pad, (s.w - 2 * pad - 4 * gap) / 5));
    const total = cell * 5 + gap * 4;
    const x0 = (s.w - total) / 2;
    const y0 = (s.h - cell) / 2;
    const g = el('g');
    const outline = s.stroke && s.stroke !== 'none' && (s.strokeWidth || 0) > 0;
    const filled = s.fill && s.fill !== 'none';
    const glyph = (x, y, fill, stroke) => {
      if (set === 'circles') {
        return el('circle', {
          cx: x + cell / 2, cy: y + cell / 2, r: cell / 2,
          fill: fill || 'none', stroke: stroke || 'none',
          'stroke-width': stroke ? Math.max(0.6, s.strokeWidth || 1) : 0,
        });
      }
      if (set === 'diamonds') {
        return el('path', {
          d: `M${x + cell / 2} ${y}L${x + cell} ${y + cell / 2}` +
             `L${x + cell / 2} ${y + cell}L${x} ${y + cell / 2}Z`,
          fill: fill || 'none', stroke: stroke || 'none',
          'stroke-width': stroke ? Math.max(0.6, s.strokeWidth || 1) : 0,
          'stroke-linejoin': 'round',
        });
      }
      return el('rect', {
        x, y, width: cell, height: cell,
        fill: fill || 'none', stroke: stroke || 'none',
        'stroke-width': stroke ? Math.max(0.6, s.strokeWidth || 1) : 0,
      });
    };
    for (let i = 0; i < 5; i++) {
      const x = x0 + i * (cell + gap);
      g.appendChild(glyph(x, y0, null, outline ? s.stroke : null));
    }
    if (filled) {
      for (let i = 0; i < level; i++) {
        const x = x0 + i * (cell + gap);
        g.appendChild(glyph(x, y0, s.fill, null));
      }
    }
    return g;
  }

  // ------------------------------------------------------------- shapes --
  buildShape(s) {
    const fx = s.flipX ? -1 : 1, fy = s.flipY ? -1 : 1;
    const g = el('g', {
      transform: `translate(${s.x + s.w / 2} ${s.y + s.h / 2}) rotate(${s.rot || 0})` +
                 ` scale(${fx} ${fy}) translate(${-s.w / 2} ${-s.h / 2})`,
    });
    g.dataset.shapeId = s.id;
    const dataFill = s.callout || s.image ? null : dataColorFill(s);

    let hitD;
    if (s.callout && s.callout.kind === 'icon') {
      g.appendChild(this.buildDataIcon(s));
      hitD = `M0 0H${s.w}V${s.h}H0Z`;
    } else if (s.callout && s.callout.kind === 'bar') {
      const ratio = barRatio(s, this.app.getPage());
      const r = Math.min(6, s.h / 3);
      if (ratio > 0) {
        g.appendChild(el('rect', {
          x: 0, y: 0, width: Math.max(0.01, s.w * ratio), height: s.h,
          rx: r, fill: s.fill || '#5b9bd5',
        }));
      }
      g.appendChild(el('rect', {
        x: 0, y: 0, width: s.w, height: s.h, rx: r,
        fill: 'none', stroke: s.stroke || '#9aa7b4',
        'stroke-width': s.strokeWidth || 1,
        'stroke-dasharray': dashArray(s.strokeStyle, s.strokeWidth),
      }));
      hitD = `M0 0H${s.w}V${s.h}H0Z`;
    } else if (s.image) {
      g.appendChild(this.buildImage(s));
      if (s.stroke && s.stroke !== 'none' && (s.strokeWidth || 0) > 0) {
        g.appendChild(el('rect', {
          x: 0, y: 0, width: s.w, height: s.h, fill: 'none',
          stroke: s.stroke,
          'stroke-width': s.strokeWidth,
          'stroke-dasharray': dashArray(s.strokeStyle, s.strokeWidth),
        }));
      }
      hitD = `M0 0H${s.w}V${s.h}H0Z`;
    } else if (s.parts && s.parts.length) {
      // geometry came from the file: each part carries its own styling
      for (const p of s.parts) {
        let fill = p.fill === 'none' ? 'none' : (p.fill ?? 'none');
        if (dataFill && fill !== 'none') fill = dataFill;
        else if (p.fillGradient && fill !== 'none') fill = this.partGradient(p.fillGradient);
        g.appendChild(el('path', {
          d: p.d,
          fill,
          'fill-opacity': p.fillOpacity ?? 1,
          stroke: p.stroke ?? 'none',
          'stroke-width': p.strokeWidth ?? 0,
          'stroke-dasharray': dashArray(p.dash, p.strokeWidth),
          'stroke-linejoin': 'round',
          'stroke-linecap': 'round',
        }));
        if (p.bottomEdge) {
          g.appendChild(el('line', {
            x1: p.bottomEdge.x1, y1: p.bottomEdge.y1,
            x2: p.bottomEdge.x2, y2: p.bottomEdge.y2,
            stroke: p.bottomEdge.stroke,
            'stroke-width': p.bottomEdge.strokeWidth,
            'stroke-linecap': 'round',
          }));
        }
      }
      hitD = `M0 0H${s.w}V${s.h}H0Z`;
    } else {
      const d = shapePath(s.type, s.w, s.h);
      hitD = d;
      const body = el('path', {
        d,
        fill: dataFill || (s.fill === 'none' ? 'none' : s.fill),
        'fill-opacity': s.fillOpacity ?? 1,
        stroke: s.stroke,
        'stroke-width': s.strokeWidth,
        'stroke-dasharray': dashArray(s.strokeStyle, s.strokeWidth),
        'stroke-linejoin': 'round',
        'stroke-linecap': 'round',
      });
      g.appendChild(body);

      const det = shapeDetail(s.type, s.w, s.h);
      if (det) {
        g.appendChild(el('path', {
          d: det, fill: 'none', stroke: s.stroke,
          'stroke-width': Math.max(0.5, s.strokeWidth * 0.8),
          'stroke-linecap': 'round',
        }));
      }
    }

    // invisible hit target so unfilled / concave shapes still pick correctly
    g.appendChild(el('path', { d: hitD, class: 'shape-hit' }));

    // text lives in its own group so mirroring never turns it upside down
    const tg = el('g', {
      transform: `translate(${fx < 0 ? s.w : 0} ${fy < 0 ? s.h : 0}) scale(${fx} ${fy})`,
    });
    tg.appendChild(this.renderText(s));
    g.appendChild(tg);

    return g;
  }

  /** A diagonal two-stop gradient for an imported 3-D bevel. */
  partGradient(stops) {
    const id = `bevel${this.gradSeq++}`;
    const grad = el('linearGradient', { id, x1: '0', y1: '0', x2: '1', y2: '1' });
    grad.appendChild(el('stop', { offset: '0', 'stop-color': stops[0] }));
    grad.appendChild(el('stop', { offset: '1', 'stop-color': stops[1] }));
    this.partDefs.appendChild(grad);
    return `url(#${id})`;
  }

  // ------------------------------------------------------------- pictures --
  /**
   * An imported or inserted picture. ImgOffset/ImgWidth are normalised to the
   * shape box, so a 2x image offset by half the box is a crop. Brightness,
   * contrast and gamma become an SVG filter; transparency is opacity.
   */
  buildImage(s) {
    const media = this.app.doc.media && this.app.doc.media[s.image.mediaId];
    if (!media || media.unsupported || !media.src) {
      return this.imagePlaceholder(s, media);
    }
    const img = s.image;
    const g = el('g');
    // An optional shape fill sits behind transparent areas of the picture.
    if (s.fill && s.fill !== 'none') {
      g.appendChild(el('rect', {
        x: 0, y: 0, width: s.w, height: s.h,
        fill: s.fill, 'fill-opacity': s.fillOpacity ?? 1,
      }));
    }
    const x = (img.offsetX || 0) * s.w;
    const y = (img.offsetY || 0) * s.h;
    const w = Math.max(1, (img.scaleX ?? 1) * s.w);
    const h = Math.max(1, (img.scaleY ?? 1) * s.h);
    const clipped = x < -0.01 || y < -0.01 || x + w > s.w + 0.01 || y + h > s.h + 0.01;

    const node = el('image', {
      x, y, width: w, height: h,
      preserveAspectRatio: 'none',
      opacity: (img.transparency || 0) > 0
        ? String(Math.max(0, 1 - img.transparency)) : undefined,
    });
    node.setAttribute('href', media.src);
    node.setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', media.src);

    // Visio ignores brightness/contrast/gamma on indexed pictures (PNG-8,
    // GIF); transparency still applies.
    const filter = media.adjust === false ? null : this.imageFilter(img);
    if (filter) node.setAttribute('filter', filter);

    if (clipped) {
      const id = `imgClip${this.clipSeq++}`;
      const clip = el('clipPath', { id });
      clip.appendChild(el('rect', { x: 0, y: 0, width: s.w, height: s.h }));
      this.partDefs.appendChild(clip);
      node.setAttribute('clip-path', `url(#${id})`);
    }
    g.appendChild(node);
    return g;
  }

  /** Visio stores Brightness/Contrast in -1..1 and Gamma as a multiplier. */
  imageFilter(img) {
    const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
    const b = clamp(img.brightness || 0, -1, 1);
    const c = clamp(img.contrast || 0, -0.99, 0.99);
    const g = img.gamma && Math.abs(img.gamma - 1) > 1e-4
      ? clamp(img.gamma, 0.1, 10) : null;
    if (!b && !c && !g) return null;
    const id = `imgAdj${this.gradSeq++}`;
    const f = el('filter', {
      id, x: '-10%', y: '-10%', width: '120%', height: '120%',
      'color-interpolation-filters': 'sRGB',
    });
    if (b || c) {
      // Visio's contrast is piecewise: 0.5 neutral, slope 1+c below neutral
      // and 1/(1-c) above it (measured against a 5-band grey ramp). Brightness
      // enters both the offset and the slope: out = k*(in - 0.5 + B/2) + 0.5 + B/2.
      const slope = c < 0 ? 1 + c : 1 / (1 - c);
      const intercept = 0.5 * (1 - slope) + (b / 2) * (1 + slope);
      const transfer = el('feComponentTransfer');
      for (const ch of ['R', 'G', 'B']) {
        transfer.appendChild(el(`feFunc${ch}`, {
          type: 'linear', slope: slope.toFixed(4), intercept: intercept.toFixed(4),
        }));
      }
      f.appendChild(transfer);
    }
    if (g) {
      const transfer = el('feComponentTransfer');
      for (const ch of ['R', 'G', 'B']) {
        transfer.appendChild(el(`feFunc${ch}`, {
          type: 'gamma', exponent: (1 / g).toFixed(4),
        }));
      }
      f.appendChild(transfer);
    }
    this.partDefs.appendChild(f);
    return `url(#${id})`;
  }

  imagePlaceholder(s, media) {
    const g = el('g');
    g.appendChild(el('rect', {
      x: 0, y: 0, width: s.w, height: s.h,
      fill: '#f1f3f6', stroke: '#b9c0ca', 'stroke-width': 1,
      'stroke-dasharray': '6 4',
    }));
    const t = el('text', {
      x: s.w / 2, y: s.h / 2,
      'text-anchor': 'middle', 'dominant-baseline': 'middle',
      'font-family': '"Segoe UI", Calibri, Helvetica, Arial, sans-serif',
      'font-size': Math.max(10, Math.min(16, s.w / 18)),
      fill: '#6b7785',
    });
    t.textContent = media
      ? `Picture (${String(media.ext || '?').toUpperCase()} not previewed)`
      : 'Picture data unavailable';
    g.appendChild(t);
    return g;
  }

  renderText(s) {
    // A data callout derives its text from the target's Shape Data, so its
    // imported rich field run is not used for display.
    if (!s.callout && s.rich && Array.isArray(s.rich.paragraphs) && s.rich.paragraphs.length) {
      return this.renderRichText(s);
    }
    const t = el('text', {
      class: 'text-hit',
      'font-family': s.fontFamily,
      'font-size': s.fontSize,
      'font-weight': s.bold ? '700' : '400',
      'font-style': s.italic ? 'italic' : 'normal',
      'text-decoration': s.underline ? 'underline' : 'none',
      fill: s.textColor,
    });
    const label = s.callout ? calloutText(s, this.app.getPage()) : (s.label ?? '');
    if (!label) return t;

    const tb = textBoxOf(s);
    const font = fontString(s);
    const lines = wrapText(label, Math.max(10, tb.w), font);
    const lh = s.fontSize * 1.22;
    const x = s.align === 'left' ? tb.x : s.align === 'right' ? tb.x + tb.w : tb.x + tb.w / 2;
    t.setAttribute('text-anchor', s.align === 'left' ? 'start' : s.align === 'right' ? 'end' : 'middle');

    // the first line's baseline, according to the vertical alignment
    let first;
    const vAlign = s.vAlign || 'middle';
    if (vAlign === 'top') first = tb.y + s.fontSize * 0.9;
    else if (vAlign === 'bottom') first = tb.y + tb.h - (lines.length - 1) * lh - s.fontSize * 0.2;
    else first = tb.y + tb.h / 2 - (lines.length - 1) * lh / 2 + s.fontSize * 0.35;

    lines.forEach((line, i) => {
      const y = first + i * lh;
      const ts = el('tspan', { x, y });
      ts.textContent = line;
      t.appendChild(ts);
    });
    return t;
  }

  /** Value for a visio <fld> run; page/file fields follow the live document. */
  fieldText(field, shape) {
    const formula = String(field?.formula || '');
    const app = this.app;
    const prop = /(?:Sheet\.[A-Za-z0-9_]+!)?Prop\.([A-Za-z0-9_]+)/i.exec(formula);
    if (prop) {
      const row = (shape?.data || []).find((d) => String(d.name) === String(prop[1]));
      if (row) return String(row.value ?? '');
    }
    if (/PAGENUMBER\s*\(/i.test(formula)) return String(app.doc.pageIndex + 1);
    if (/PAGECOUNT\s*\(/i.test(formula)) return String(app.doc.pages.length);
    if (/FILENAME\s*\(/i.test(formula)) return String(app.doc.name || '');
    if (/NOW\s*\(/i.test(formula)) {
      const m = /FORMAT\s*\(\s*NOW\s*\(\s*\)\s*,\s*["']([^"']+)["']/i.exec(formula);
      const d = new Date();
      const pad = (n) => String(n).padStart(2, '0');
      if (m) {
        const pattern = m[1];
        const hasTime = /HH|hh|ss/.test(pattern);
        return pattern.replace(/yyyy|MMMM|MMM|MM|mm|dd|HH|hh|ss/g, (token) => {
          switch (token) {
            case 'yyyy': return String(d.getFullYear());
            case 'MMMM': return d.toLocaleString('en', { month: 'long' });
            case 'MMM': return d.toLocaleString('en', { month: 'short' });
            case 'MM': return pad(d.getMonth() + 1);
            case 'mm': return pad(hasTime ? d.getMinutes() : d.getMonth() + 1);
            case 'dd': return pad(d.getDate());
            case 'HH': case 'hh': return pad(d.getHours());
            case 'ss': return pad(d.getSeconds());
            default: return token;
          }
        });
      }
      return d.toISOString().slice(0, 10);
    }
    return field?.value || '';
  }

  /**
   * Rich text: each Visio paragraph keeps its alignment/bullet and every run
   * keeps its own font size, weight, style and colour. Field runs resolve to
   * their live value before wrapping.
   */
  renderRichText(s) {
    const base = {
      fontFamily: s.fontFamily,
      fontSize: s.fontSize || 12,
      bold: !!s.bold,
      italic: !!s.italic,
      underline: !!s.underline,
      textColor: s.textColor,
    };
    const styleOf = (style = {}) => ({ ...base, ...style });
    const tb = textBoxOf(s);
    const maxW = Math.max(10, tb.w);

    const lines = [];
    for (const p of s.rich.paragraphs) {
      const atoms = [];
      if (p.bullet) {
        const first = p.runs.find((r) => (r.text || '').length || r.field);
        atoms.push({ text: `${p.bulletStr || '•'} `, style: styleOf(first?.style) });
      }
      for (const run of p.runs || []) {
        const text = run.field ? this.fieldText(run.field, s) : (run.text || '');
        if (!text) continue;
        for (const part of text.split(/(\s+)/)) {
          if (!part) continue;
          atoms.push({ text: part, style: styleOf(run.style) });
        }
      }
      const indent = p.indent || 0;
      const wrapW = Math.max(10, maxW - indent);
      let line = [];
      let width = 0;
      for (const atom of atoms) {
        const w = measureText(atom.text, fontString(atom.style));
        if (line.length && width + w > wrapW && atom.text.trim()) {
          lines.push({ atoms: line, align: p.align || 'center', indent, width });
          line = [];
          width = 0;
        }
        if (!line.length && !atom.text.trim()) continue;
        line.push(atom);
        width += w;
      }
      if (line.length) lines.push({ atoms: line, align: p.align || 'center', indent, width });
      else lines.push({ atoms: [], align: p.align || 'center', indent, width: 0 });
    }

    const totalH = lines.reduce((n, line) => {
      const fs = Math.max(base.fontSize, ...line.atoms.map((a) => a.style.fontSize || 0));
      return n + fs * 1.22;
    }, 0);
    const vAlign = s.vAlign || 'middle';
    let y = vAlign === 'top' ? tb.y + base.fontSize * 0.9
      : vAlign === 'bottom' ? tb.y + tb.h - totalH + base.fontSize * 0.9
        : tb.y + (tb.h - totalH) / 2 + base.fontSize * 0.9;

    const t = el('text', {
      class: 'text-hit',
      'font-family': base.fontFamily,
      'font-size': base.fontSize,
      'font-weight': base.bold ? '700' : '400',
      'font-style': base.italic ? 'italic' : 'normal',
      'text-decoration': base.underline ? 'underline' : 'none',
      fill: base.textColor,
      'xml:space': 'preserve',
    });

    for (const line of lines) {
      const fs = Math.max(...line.atoms.map((a) => a.style.fontSize || 0), base.fontSize);
      const lh = fs * 1.22;
      const left = tb.x + line.indent;
      const innerW = Math.max(10, tb.w - line.indent);
      const x = line.align === 'left' ? left
        : line.align === 'right' ? left + innerW : left + innerW / 2;
      const row = el('tspan', {
        x, y,
        'text-anchor': line.align === 'left' ? 'start'
          : line.align === 'right' ? 'end' : 'middle',
      });
      if (!line.atoms.length) {
        row.textContent = '';
      } else {
        for (const atom of line.atoms) {
          const a = atom.style;
          const span = el('tspan', {
            'font-family': a.fontFamily,
            'font-size': a.fontSize,
            'font-weight': a.bold ? '700' : '400',
            'font-style': a.italic ? 'italic' : 'normal',
            'text-decoration': a.underline ? 'underline' : 'none',
            fill: a.textColor,
          });
          span.textContent = atom.text;
          row.appendChild(span);
        }
      }
      t.appendChild(row);
      y += lh;
    }
    return t;
  }

  // --------------------------------------------------------- connectors --
  buildConnector(c, shapesById) {
    const route = routeConnector(c, shapesById);
    const g = el('g');
    g.dataset.connId = c.id;

    const geom = this.routeGeometry(route);
    const common = {
      fill: 'none',
      stroke: c.stroke,
      'stroke-width': c.strokeWidth,
      'stroke-dasharray': dashArray(c.strokeStyle, c.strokeWidth),
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    };
    if (c.arrowStart !== 'none') common['marker-start'] = markerFor(c.arrowStart, c.strokeWidth);
    if (c.arrowEnd !== 'none') common['marker-end'] = markerFor(c.arrowEnd, c.strokeWidth);

    const visible = el(geom.tag, { ...common, ...geom.attrs });
    g.appendChild(visible);

    const hit = el(geom.tag, { ...geom.attrs, class: 'conn-hit', stroke: 'transparent', 'stroke-width': 14, 'stroke-dasharray': 'none' });
    if (geom.tag === 'line') hit.setAttribute('stroke-linecap', 'round');
    g.appendChild(hit);

    if (c.label) {
      const mid = routeMidpoint(route);
      const font = fontString({ fontSize: 11, fontFamily: sfont(c) });
      const w = labelWidth(c.label, font);
      const g2 = el('g', { class: 'text-hit' });
      g2.appendChild(el('rect', {
        x: mid.x - w / 2 - 3, y: mid.y - 8, width: w + 6, height: 16,
        rx: 3, fill: '#ffffff', 'fill-opacity': 0.9,
      }));
      const t = el('text', {
        x: mid.x, y: mid.y + 4, 'text-anchor': 'middle',
        'font-family': sfont(c), 'font-size': 11, fill: c.stroke,
      });
      t.textContent = c.label;
      g2.appendChild(t);
      g.appendChild(g2);
    }

    return g;
  }

  routeGeometry(route) {
    if (route.kind === 'curve') return { tag: 'path', attrs: { d: route.d } };
    if (route.kind === 'poly') {
      return { tag: 'polyline', attrs: { points: route.points.map((p) => `${p.x},${p.y}`).join(' ') } };
    }
    const [a, b] = route.points;
    return { tag: 'line', attrs: { x1: a.x, y1: a.y, x2: b.x, y2: b.y } };
  }

  // ----------------------------------------------------------- overlay --
  renderOverlay(pg, shapesById) {
    const app = this.app;
    const sel = app.state.selection;
    const zoom = app.state.view.zoom;
    const hs = 4.5 / zoom;

    const selectedShapes = pg.shapes.filter((s) => sel.has(s.id));
    const selectedConns = pg.connectors.filter((c) => sel.has(c.id));

    let groupBox = null;
    if (selectedShapes.length > 1) {
      // group bounding box
      let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
      for (const s of selectedShapes) {
        x1 = Math.min(x1, s.x); y1 = Math.min(y1, s.y);
        x2 = Math.max(x2, s.x + s.w); y2 = Math.max(y2, s.y + s.h);
      }
      groupBox = { x1, y1, x2, y2 };
      this.overlay.appendChild(el('rect', {
        x: x1, y: y1, w: x2 - x1, h: y2 - y1, class: 'sel-box',
      }));
    }

    for (const s of selectedShapes) {
      const corners = [
        { x: 0, y: 0 }, { x: s.w, y: 0 }, { x: s.w, y: s.h }, { x: 0, y: s.h },
      ].map((p) => localToWorld(s, p));
      this.overlay.appendChild(el('polygon', {
        points: corners.map((p) => `${p.x},${p.y}`).join(' '),
        class: 'sel-box',
      }));
      if (selectedShapes.length === 1) {
        for (const [name, fx, fy] of HANDLES) {
          const p = localToWorld(s, { x: s.w * fx, y: s.h * fy });
          this.overlay.appendChild(this.handleRect(p.x, p.y, hs, name));
        }
      }
    }

    // Group handles go last: a member's dashed outline shares the group
    // bounds, and if it is painted above the handle it swallows the edge
    // clicks (the north/south/west/east handles then never start a resize).
    if (groupBox) {
      for (const [name, fx, fy] of HANDLES) {
        const hx = groupBox.x1 + (groupBox.x2 - groupBox.x1) * fx;
        const hy = groupBox.y1 + (groupBox.y2 - groupBox.y1) * fy;
        this.overlay.appendChild(this.handleRect(hx, hy, hs, name));
      }
    }

    for (const c of selectedConns) {
      const route = routeConnector(c, shapesById);
      const geom = this.routeGeometry(route);
      this.overlay.appendChild(el(geom.tag, {
        ...geom.attrs, class: 'sel-box', stroke: '#2b6cb0',
      }));
      const A = route.points[0];
      const B = route.points[route.points.length - 1];
      for (const [name, p] of [['conn-start', A], ['conn-end', B]]) {
        const h = el('circle', { cx: p.x, cy: p.y, r: hs, class: 'handle', 'data-handle': name });
        this.overlay.appendChild(h);
      }
      (c.waypoints || []).forEach((w, i) => {
        const h = el('rect', {
          x: w.x - hs, y: w.y - hs, width: hs * 2, height: hs * 2,
          class: 'handle', 'data-handle': `waypoint-${i}`,
        });
        this.overlay.appendChild(h);
      });
      if (!(c.waypoints || []).length) {
        const mid = routeMidpoint(route);
        const h = el('circle', {
          cx: mid.x, cy: mid.y, r: hs * 0.8, class: 'handle-line',
          fill: '#fff', 'data-handle': 'bend',
        });
        this.overlay.appendChild(h);
      }
    }

    // marquee
    if (app.state.marquee) {
      const m = app.state.marquee;
      this.overlay.appendChild(el('rect', {
        x: m.x, y: m.y, w: m.w, h: m.h, class: 'marquee',
      }));
    }

    // glue hint while wiring
    if (app.state.connectSource && app.state.hoverShapeId) {
      const s = shapesById.get(app.state.hoverShapeId);
      if (s) {
        this.overlay.appendChild(el('rect', {
          x: s.x - 3 / this.app.state.view.zoom, y: s.y - 3 / this.app.state.view.zoom,
          w: s.w + 6 / this.app.state.view.zoom, h: s.h + 6 / this.app.state.view.zoom,
          class: 'glue-hint', rx: 2,
        }));
      }
    }

    // live preview while wiring a new connector
    if (app.state.connectPreview) {
      const src = app.state.connectPreview.from;
      const s = src.shapeId ? shapesById.get(src.shapeId) : null;
      const b = app.state.connectPreview.point;
      let a = src.point;
      if (s && b) {
        const ctr = shapeCenter(s);
        a = anchorOnShapeAt(s, bestSide(s, b.x - ctr.x, b.y - ctr.y), b);
      }
      if (a && b) {
        this.overlay.appendChild(el('line', {
          x1: a.x, y1: a.y, x2: b.x, y2: b.y,
          stroke: '#e0533d', 'stroke-width': 1.5 / zoom, 'stroke-dasharray': `${6 / zoom} ${5 / zoom}`,
        }));
      }
    }
    if (app.state.connectSource && app.state.connectSource.shapeId) {
      const s = shapesById.get(app.state.connectSource.shapeId);
      if (s) {
        this.overlay.appendChild(el('rect', {
          x: s.x - 3 / zoom, y: s.y - 3 / zoom, w: s.w + 6 / zoom, h: s.h + 6 / zoom,
          class: 'glue-hint', rx: 2,
        }));
      }
    }
  }

  handleRect(x, y, hs, name) {
    return el('rect', {
      x: x - hs, y: y - hs, width: hs * 2, height: hs * 2,
      class: 'handle', 'data-handle': name,
    });
  }
}

function sfont(c) {
  return c.fontFamily || '"Segoe UI", Calibri, Helvetica, Arial, sans-serif';
}

/** Intersection of the segment from a box centre toward `toward` with its edge. */
function boxEdgePoint(box, toward) {
  const cx = box.x + box.w / 2;
  const cy = box.y + box.h / 2;
  const dx = toward.x - cx;
  const dy = toward.y - cy;
  if (!dx && !dy) return { x: cx, y: cy };
  const tx = dx ? (box.w / 2) / Math.abs(dx) : Infinity;
  const ty = dy ? (box.h / 2) / Math.abs(dy) : Infinity;
  const t = Math.min(tx, ty);
  return { x: cx + dx * t, y: cy + dy * t };
}

function markerFor(kind, w) {
  if (kind === 'arrow') return 'url(#mArrowOpen)';
  if (kind === 'filled' || kind === 'arrow-filled') return 'url(#mArrowSolid)';
  if (kind === 'hollowTriangle') return 'url(#mHollowTriangle)';
  if (kind === 'hollowDiamond') return 'url(#mHollowDiamond)';
  if (kind === 'filledDiamond') return 'url(#mFilledDiamond)';
  return null;
}

function labelWidth(text, font) {
  const c = document.createElement('canvas').getContext('2d');
  c.font = font;
  return c.measureText(text).width;
}
