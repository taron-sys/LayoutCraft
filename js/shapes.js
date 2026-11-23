// Shape catalogue: each entry knows how to draw itself in local coordinates
// (0,0 .. w,h) and where its text block sits (fractions of the box).

const r2 = (n) => Math.round(n * 100) / 100;

function rr(w, h, r) {
  r = Math.min(r, w / 2, h / 2);
  return `M${r2(r)} 0H${r2(w - r)}A${r2(r)} ${r2(r)} 0 0 1 ${r2(w)} ${r2(r)}` +
         `V${r2(h - r)}A${r2(r)} ${r2(r)} 0 0 1 ${r2(w - r)} ${r2(h)}H${r2(r)}` +
         `A${r2(r)} ${r2(r)} 0 0 1 0 ${r2(h - r)}V${r2(r)}A${r2(r)} ${r2(r)} 0 0 1 ${r2(r)} 0Z`;
}

function star(w, h, points = 5) {
  const cx = w / 2, cy = h / 2, R = Math.min(w, h) / 2, r = R * 0.382;
  let d = '';
  for (let i = 0; i < points * 2; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / points;
    const rad = i % 2 === 0 ? R : r;
    d += (i === 0 ? 'M' : 'L') + r2(cx + Math.cos(a) * rad) + ' ' + r2(cy + Math.sin(a) * rad);
  }
  return d + 'Z';
}

function ellipse(w, h) {
  return `M0 ${r2(h / 2)}A${r2(w / 2)} ${r2(h / 2)} 0 1 0 ${r2(w)} ${r2(h / 2)}` +
         `A${r2(w / 2)} ${r2(h / 2)} 0 1 0 0 ${r2(h / 2)}Z`;
}

/** @type {Record<string, any>} */
export const SHAPES = {
  /* ---------------------------------------------------------- basic --- */
  rect: {
    name: 'Rectangle', group: 'Basic Shapes', w: 120, h: 70,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H0Z`,
  },
  // Not in the palette: a Shape Data callout created by the data-graphics UI.
  dataCallout: {
    name: 'Data callout', w: 150, h: 30,
    path: (w, h) => rr(w, h, Math.min(8, h / 3)),
    text: { x: 0.5, y: 0.5, w: 0.94, h: 0.84 },
  },
  // Not in the palette: a numeric data bar created by the data-graphics UI.
  dataBar: {
    name: 'Data bar', w: 160, h: 18,
    path: (w, h) => rr(w, h, Math.min(6, h / 3)),
    text: { x: 0.5, y: 0.5, w: 0.9, h: 0.8 },
  },
  // Not in the palette: a five-level icon set created by the data-graphics UI.
  dataIcon: {
    name: 'Data icon', w: 88, h: 16,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H0Z`,
  },
  // Not in the palette: inserted as a bitmap/Foreign shape by the image tool.
  image: {
    name: 'Picture', w: 320, h: 240,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H0Z`,
  },

  roundRect: {
    name: 'Rounded Rectangle', group: 'Basic Shapes', w: 120, h: 70,
    path: (w, h) => rr(w, h, Math.min(18, h * 0.28)),
  },
  ellipse: {
    name: 'Ellipse', group: 'Basic Shapes', w: 120, h: 70,
    path: (w, h) => ellipse(w, h),
  },
  circle: {
    name: 'Circle', group: 'Basic Shapes', w: 80, h: 80,
    path: (w, h) => ellipse(w, h),
  },
  diamond: {
    name: 'Diamond', group: 'Basic Shapes', w: 110, h: 80,
    path: (w, h) => `M${r2(w / 2)} 0L${r2(w)} ${r2(h / 2)}L${r2(w / 2)} ${r2(h)}L0 ${r2(h / 2)}Z`,
  },
  triangle: {
    name: 'Triangle', group: 'Basic Shapes', w: 110, h: 80,
    path: (w, h) => `M${r2(w / 2)} 0L${r2(w)} ${r2(h)}L0 ${r2(h)}Z`,
    text: { x: 0.5, y: 0.68, w: 0.8, h: 0.3 },
  },
  hexagon: {
    name: 'Hexagon', group: 'Basic Shapes', w: 120, h: 80,
    path: (w, h) => `M${r2(w * 0.25)} 0H${r2(w * 0.75)}L${r2(w)} ${r2(h / 2)}L${r2(w * 0.75)} ${r2(h)}H${r2(w * 0.25)}L0 ${r2(h / 2)}Z`,
  },
  pentagon: {
    name: 'Pentagon', group: 'Basic Shapes', w: 110, h: 90,
    path: (w, h) => {
      const k = 0.382;
      return `M${r2(w / 2)} 0L${r2(w)} ${r2(h * k)}L${r2(w * 0.809)} ${r2(h)}H${r2(w * 0.191)}L0 ${r2(h * k)}Z`;
    },
  },
  parallelogram: {
    name: 'Parallelogram', group: 'Basic Shapes', w: 130, h: 70,
    path: (w, h) => `M${r2(w * 0.15)} 0H${r2(w)}L${r2(w * 0.85)} ${r2(h)}H0Z`,
  },
  trapezoid: {
    name: 'Trapezoid', group: 'Basic Shapes', w: 120, h: 70,
    path: (w, h) => `M${r2(w * 0.15)} 0H${r2(w * 0.85)}L${r2(w)} ${r2(h)}H0Z`,
  },
  star5: {
    name: 'Star', group: 'Basic Shapes', w: 100, h: 100,
    path: (w, h) => star(w, h),
  },
  cross: {
    name: 'Cross', group: 'Basic Shapes', w: 90, h: 90,
    path: (w, h) => {
      const a = w * 0.32, b = h * 0.32;
      return `M${r2(a)} 0H${r2(w - a)}V${r2(b)}H${r2(w)}V${r2(h - b)}H${r2(w - a)}V${r2(h)}H${r2(a)}` +
             `V${r2(h - b)}H0V${r2(b)}H${r2(a)}Z`;
    },
  },
  blockArrow: {
    name: 'Block Arrow', group: 'Basic Shapes', w: 140, h: 80,
    path: (w, h) => `M0 ${r2(h * 0.28)}H${r2(w * 0.6)}L${r2(w)} ${r2(h / 2)}L${r2(w * 0.6)} ${r2(h * 0.72)}H0Z`,
  },
  chevron: {
    name: 'Chevron', group: 'Basic Shapes', w: 130, h: 70,
    path: (w, h) => `M0 0H${r2(w * 0.85)}L${r2(w)} ${r2(h / 2)}L${r2(w * 0.85)} ${r2(h)}H0Z`,
  },
  orShape: {
    name: 'Or', group: 'Basic Shapes', w: 110, h: 70,
    path: (w, h) => `M0 0H${r2(w)}C${r2(w * 0.78)} ${r2(h * 0.5)} ${r2(w * 0.78)} ${r2(h * 0.5)} ${r2(w)} ${r2(h)}H0C${r2(w * 0.22)} ${r2(h * 0.5)} ${r2(w * 0.22)} ${r2(h * 0.5)} 0 0Z`,
  },
  andShape: {
    name: 'And', group: 'Basic Shapes', w: 110, h: 70,
    path: (w, h) => `M0 0H${r2(w)}L${r2(w / 2)} ${r2(h / 2)}L${r2(w)} ${r2(h)}H0L${r2(w / 2)} ${r2(h / 2)}Z`,
  },
  callout: {
    name: 'Callout', group: 'Basic Shapes', w: 140, h: 90,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h * 0.75)}H${r2(w * 0.42)}L${r2(w * 0.3)} ${r2(h)}L${r2(w * 0.3)} ${r2(h * 0.75)}H0Z`,
    text: { x: 0.5, y: 0.36, w: 0.85, h: 0.6 },
  },

  /* ------------------------------------------------------- flowchart --- */
  flowProcess: {
    name: 'Process', group: 'Flowchart', w: 120, h: 70,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H0Z`,
  },
  flowDecision: {
    name: 'Decision', group: 'Flowchart', w: 110, h: 80,
    path: (w, h) => `M${r2(w / 2)} 0L${r2(w)} ${r2(h / 2)}L${r2(w / 2)} ${r2(h)}L0 ${r2(h / 2)}Z`,
  },
  flowTerminator: {
    name: 'Terminator', group: 'Flowchart', w: 130, h: 60,
    path: (w, h) => rr(w, h, h / 2),
  },
  flowDocument: {
    name: 'Document', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h * 0.82)}Q${r2(w / 2)} ${r2(h * 1.02)} 0 ${r2(h * 0.82)}Z`,
    text: { x: 0.5, y: 0.42, w: 0.85, h: 0.7 },
  },
  flowData: {
    name: 'Data', group: 'Flowchart', w: 130, h: 70,
    path: (w, h) => `M0 0H${r2(w)}L${r2(w * 0.85)} ${r2(h)}H0Z`,
  },
  flowManualInput: {
    name: 'Manual Input', group: 'Flowchart', w: 120, h: 70,
    path: (w, h) => `M${r2(w * 0.22)} 0H${r2(w)}V${r2(h)}H0Z`,
  },
  flowDelay: {
    name: 'Delay', group: 'Flowchart', w: 120, h: 70,
    path: (w, h) => `M0 0H${r2(w * 0.82)}A${r2(w * 0.18)} ${r2(h / 2)} 0 0 1 ${r2(w * 0.82)} ${r2(h)}H0Z`,
  },
  flowDisplay: {
    name: 'Display', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M0 0H${r2(w)}A${r2(w * 0.35)} ${r2(h / 2)} 0 0 1 ${r2(w)} ${r2(h)}H0Z`,
    text: { x: 0.42, y: 0.5, w: 0.75, h: 0.85 },
  },
  flowPreparation: {
    name: 'Preparation', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M${r2(w * 0.2)} 0H${r2(w * 0.8)}L${r2(w)} ${r2(h / 2)}L${r2(w * 0.8)} ${r2(h)}H${r2(w * 0.2)}L0 ${r2(h / 2)}Z`,
  },
  flowCard: {
    name: 'Card', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M0 0H${r2(w * 0.78)}L${r2(w)} ${r2(h * 0.22)}V${r2(h)}H0Z`,
  },
  flowStoredData: {
    name: 'Stored Data', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H${r2(w * 0.2)}L0 ${r2(h * 0.5)}Z`,
  },
  flowDirectData: {
    name: 'Direct Access', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H0Z`,
    detail: (w, h) => `M0 ${r2(h * 0.5)}H${r2(w)}`,
  },
  note: {
    name: 'Note', group: 'Flowchart', w: 120, h: 80,
    path: (w, h) => `M0 0H${r2(w * 0.78)}L${r2(w)} ${r2(h * 0.22)}V${r2(h)}H0Z`,
    detail: (w, h) => `M${r2(w * 0.78)} 0V${r2(h * 0.22)}H${r2(w)}`,
  },

  /* ------------------------------------------------------ data store --- */
  database: {
    name: 'Database', group: 'Data Storage', w: 100, h: 110,
    path: (w, h) => {
      const ry = h * 0.16;
      return `M0 ${r2(ry)}A${r2(w / 2)} ${r2(ry)} 0 0 1 ${r2(w)} ${r2(ry)}` +
             `V${r2(h - ry)}A${r2(w / 2)} ${r2(ry)} 0 0 1 0 ${r2(h - ry)}Z`;
    },
    detail: (w, h) => `M0 ${r2(h * 0.16)}A${r2(w / 2)} ${r2(h * 0.16)} 0 0 0 ${r2(w)} ${r2(h * 0.16)}`,
    text: { x: 0.5, y: 0.56, w: 0.8, h: 0.7 },
  },
  server: {
    name: 'Server', group: 'Data Storage', w: 100, h: 110,
    path: (w, h) => rr(w, h, 4),
    detail: (w, h) => `M0 ${r2(h / 3)}H${r2(w)}M0 ${r2((h * 2) / 3)}H${r2(w)}`,
    text: { x: 0.5, y: 0.5, w: 0.8, h: 0.6 },
  },
  tape: {
    name: 'Tape', group: 'Data Storage', w: 120, h: 70,
    path: (w, h) => ellipse(w, h),
    detail: (w, h) => `M${r2(w * 0.2)} ${r2(h / 2)}H${r2(w * 0.8)}`,
    text: { x: 0.5, y: 0.5, w: 0.7, h: 0.4 },
  },
  cloud: {
    name: 'Cloud', group: 'Data Storage', w: 150, h: 95,
    path: (w, h) => {
      const sx = w / 140, sy = h / 90;
      const p = (x, y) => `${r2(x * sx)} ${r2(y * sy)}`;
      return `M${p(32, 74)}` +
             `C${p(12, 74)} ${p(4, 56)} ${p(18, 44)}` +
             `C${p(6, 30)} ${p(20, 12)} ${p(38, 18)}` +
             `C${p(46, 2)} ${p(76, 0)} ${p(86, 16)}` +
             `C${p(104, 6)} ${p(126, 20)} ${p(118, 38)}` +
             `C${p(140, 48)} ${p(130, 74)} ${p(108, 74)}Z`;
    },
    text: { x: 0.5, y: 0.5, w: 0.7, h: 0.55 },
  },
  cube: {
    name: 'Cube', group: 'Data Storage', w: 100, h: 100,
    path: (w, h) => `M${r2(w / 2)} 0L${r2(w)} ${r2(h * 0.25)}V${r2(h * 0.75)}L${r2(w / 2)} ${r2(h)}L0 ${r2(h * 0.75)}V${r2(h * 0.25)}Z`,
    detail: (w, h) => `M${r2(w / 2)} 0V${r2(h)}M0 ${r2(h * 0.25)}L${r2(w / 2)} ${r2(h * 0.5)}L${r2(w)} ${r2(h * 0.25)}`,
  },

  /* -------------------------------------------------------- network --- */
  router: {
    name: 'Router', group: 'Network', w: 100, h: 100,
    path: (w, h) => ellipse(w, h),
    text: { x: 0.5, y: 0.5, w: 0.6, h: 0.4 },
  },
  switch: {
    name: 'Switch', group: 'Network', w: 130, h: 70,
    path: (w, h) => rr(w, h, 4),
    detail: (w, h) => `M${r2(w * 0.15)} ${r2(h / 2)}L${r2(w * 0.4)} ${r2(h * 0.3)}L${r2(w * 0.65)} ${r2(h * 0.7)}L${r2(w * 0.9)} ${r2(h * 0.5)}`,
    text: { x: 0.5, y: 0.5, w: 0.7, h: 0.4 },
  },
  firewall: {
    name: 'Firewall', group: 'Network', w: 110, h: 100,
    path: (w, h) => `M0 0H${r2(w)}V${r2(h)}H0Z`,
    detail: (w, h) => {
      const bh = h / 4, bw = w / 2;
      let d = '';
      for (let row = 0; row < 4; row++) {
        const y = row * bh;
        d += `M0 ${r2(y)}H${r2(w)}`;
        for (let i = 0; i < 2; i++) {
          const x = row % 2 === 0 ? i * bw : i * bw + bw / 2;
          d += `M${r2(x)} ${r2(y)}V${r2(y + bh)}`;
        }
      }
      return d;
    },
  },
  web: {
    name: 'Web', group: 'Network', w: 110, h: 100,
    path: (w, h) => ellipse(w, h),
    detail: (w, h) => `M0 ${r2(h / 2)}H${r2(w)}M${r2(w / 2)} 0V${r2(h)}` +
      `M${r2(w * 0.17)} ${r2(h * 0.25)}H${r2(w * 0.83)}M${r2(w * 0.17)} ${r2(h * 0.75)}H${r2(w * 0.83)}`,
  },
  vpn: {
    name: 'VPN', group: 'Network', w: 110, h: 100,
    path: (w, h) => ellipse(w, h),
    detail: (w, h) => `M${r2(w * 0.2)} ${r2(h * 0.65)}L${r2(w * 0.5)} ${r2(h * 0.3)}L${r2(w * 0.8)} ${r2(h * 0.65)}`,
  },
};

export const SHAPE_GROUPS = [
  'Basic Shapes',
  'Flowchart',
  'Data Storage',
  'Network',
];

export const shapeDef = (type) => SHAPES[type] || SHAPES.rect;

export function shapePath(type, w, h) {
  return shapeDef(type).path(Math.max(1, w), Math.max(1, h));
}

export function shapeDetail(type, w, h) {
  const def = shapeDef(type);
  return def.detail ? def.detail(Math.max(1, w), Math.max(1, h)) : null;
}

export function shapeTextBox(type, w, h) {
  const def = shapeDef(type);
  const t = def.text || { x: 0.5, y: 0.5, w: 0.9, h: 0.8 };
  return {
    x: (w - w * t.w) / 2 + (t.x - 0.5) * w,
    y: (h - h * t.h) / 2 + (t.y - 0.5) * h,
    w: w * t.w,
    h: h * t.h,
  };
}

/** Small preview markup for the palette. */
export function shapePreview(type, size = 30) {
  const def = shapeDef(type);
  const w = size, h = size * 0.78;
  const d = def.path(w, h);
  const det = def.detail ? def.detail(w, h) : null;
  return `<svg viewBox="-1 -1 ${w + 2} ${h + 2}" xmlns="http://www.w3.org/2000/svg">
    <path d="${d}" fill="#cfe0f2" stroke="#7ba3c9" stroke-width="1.2" stroke-linejoin="round"/>
    ${det ? `<path d="${det}" fill="none" stroke="#7ba3c9" stroke-width="1.1"/>` : ''}
  </svg>`;
}
