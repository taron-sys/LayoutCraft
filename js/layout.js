// Pure auto-layout for the Re-layout command.
//
// The page is reduced to "units": a single shape or a whole group. Edges are
// connector endpoints between units. Each connected component is laid out as
// a layered graph (cycles collapsed with Tarjan SCC), then components are
// packed into rows. The result is a Map of unit id -> absolute top-left.

/** Tarjan strongly-connected components. */
function stronglyConnected(ids, adj) {
  let counter = 0;
  const index = new Map();
  const low = new Map();
  const onStack = new Set();
  const stack = [];
  const out = [];
  const visit = (v) => {
    index.set(v, counter);
    low.set(v, counter);
    counter++;
    stack.push(v);
    onStack.add(v);
    for (const w of adj.get(v) || []) {
      if (!index.has(w)) {
        visit(w);
        low.set(v, Math.min(low.get(v), low.get(w)));
      } else if (onStack.has(w)) {
        low.set(v, Math.min(low.get(v), index.get(w)));
      }
    }
    if (low.get(v) === index.get(v)) {
      const group = [];
      let w;
      do {
        w = stack.pop();
        onStack.delete(w);
        group.push(w);
      } while (w !== v);
      out.push(group);
    }
  };
  for (const id of ids) if (!index.has(id)) visit(id);
  return out;
}

/** Lay out one connected component; returns relative positions and bounds. */
function layoutComponent(ids, adj, byId, options) {
  const horizontal = options.direction === 'horizontal';
  const nodeGap = options.nodeGap;
  const layerGap = options.layerGap;
  const inside = new Set(ids);
  const compAdj = new Map();
  for (const id of ids) {
    compAdj.set(id, [...(adj.get(id) || [])].filter((v) => inside.has(v)));
  }

  const comps = stronglyConnected(ids, compAdj);
  const sccOf = new Map();
  comps.forEach((group, i) => group.forEach((id) => sccOf.set(id, i)));
  const dag = comps.map(() => new Set());
  for (const [u, ns] of compAdj) {
    for (const v of ns) {
      if (sccOf.get(u) !== sccOf.get(v)) dag[sccOf.get(u)].add(sccOf.get(v));
    }
  }

  // Longest-path layers on the condensation DAG.
  const indeg = comps.map(() => 0);
  for (const ns of dag) for (const v of ns) indeg[v]++;
  const layerOf = comps.map(() => 0);
  const queue = [];
  comps.forEach((_, i) => { if (!indeg[i]) queue.push(i); });
  while (queue.length) {
    const u = queue.shift();
    for (const v of dag[u]) {
      layerOf[v] = Math.max(layerOf[v], layerOf[u] + 1);
      indeg[v]--;
      if (!indeg[v]) queue.push(v);
    }
  }

  const layers = [];
  comps.forEach((group, i) => {
    const li = layerOf[i];
    if (!layers[li]) layers[li] = [];
    for (const id of group) layers[li].push(id);
  });
  for (let i = 0; i < layers.length; i++) if (!layers[i]) layers[i] = [];
  const orderOf = (id) => byId.get(id)?.order ?? 0;
  for (const layer of layers) layer.sort((a, b) => orderOf(a) - orderOf(b));

  const pred = new Map(ids.map((id) => [id, []]));
  const succ = new Map(ids.map((id) => [id, []]));
  for (const [u, ns] of compAdj) {
    for (const v of ns) {
      succ.get(u).push(v);
      pred.get(v).push(u);
    }
  }
  const pos = new Map();
  const sync = () => layers.forEach((layer) => layer.forEach((id, i) => pos.set(id, i)));
  sync();

  // Barycentre sweeps to reduce crossings. Four passes is enough for small
  // diagrams and keeps the operation instant on page-sized graphs.
  for (let pass = 0; pass < 4; pass++) {
    const down = pass % 2 === 0;
    const list = down ? layers.slice(1) : layers.slice(0, -1).reverse();
    for (const layer of list) {
      const adjacent = down ? pred : succ;
      const score = new Map();
      for (const id of layer) {
        const ns = adjacent.get(id).filter((v) => pos.has(v));
        score.set(id, ns.length
          ? ns.reduce((sum, v) => sum + pos.get(v), 0) / ns.length
          : pos.get(id));
      }
      layer.sort((a, b) => (score.get(a) - score.get(b)) || (pos.get(a) - pos.get(b)));
      layer.forEach((id, i) => pos.set(id, i));
    }
  }

  const cross = (u) => (horizontal ? u.h : u.w);
  const main = (u) => (horizontal ? u.w : u.h);
  const layerCross = layers.map((layer) => Math.max(0, ...layer.map((id) => cross(byId.get(id)))));
  const layerMain = layers.map((layer) => Math.max(0, ...layer.map((id) => main(byId.get(id)))));
  const maxCross = Math.max(0, ...layerCross);

  const positions = new Map();
  let mainPos = 0;
  layers.forEach((layer, li) => {
    const sizes = layer.map((id) => cross(byId.get(id)));
    const total = sizes.reduce((a, b) => a + b, 0) + nodeGap * Math.max(0, layer.length - 1);
    let crossPos = (maxCross - total) / 2;
    layer.forEach((id, i) => {
      positions.set(id, horizontal
        ? { x: mainPos, y: crossPos }
        : { x: crossPos, y: mainPos });
      crossPos += sizes[i] + nodeGap;
    });
    mainPos += layerMain[li] + layerGap;
  });

  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const [id, p] of positions) {
    const u = byId.get(id);
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x + u.w);
    maxY = Math.max(maxY, p.y + u.h);
  }
  for (const p of positions.values()) { p.x -= minX; p.y -= minY; }
  return { positions, width: maxX - minX, height: maxY - minY };
}

/** Connected components of the undirected graph. */
function components(ids, adj) {
  const seen = new Set();
  const out = [];
  for (const id of ids) {
    if (seen.has(id)) continue;
    const group = [];
    const stack = [id];
    seen.add(id);
    while (stack.length) {
      const v = stack.pop();
      group.push(v);
      for (const w of adj.get(v) || []) {
        if (!seen.has(w)) { seen.add(w); stack.push(w); }
      }
    }
    out.push(group);
  }
  return out;
}

/**
 * Lay out units.
 *
 * @param {{id:string,x:number,y:number,w:number,h:number,order?:number}[]} units
 * @param {{from:string,to:string}[]} edges
 * @param {{direction?:'vertical'|'horizontal',nodeGap?:number,layerGap?:number,
 *          blockGap?:number,originX?:number,originY?:number,maxWidth?:number}} [options]
 * @returns {Map<string,{x:number,y:number}>}
 */
export function layoutUnits(units, edges, options = {}) {
  if (!units.length) return new Map();
  const opts = {
    direction: options.direction === 'horizontal' ? 'horizontal' : 'vertical',
    nodeGap: options.nodeGap ?? 48,
    layerGap: options.layerGap ?? 84,
    blockGap: options.blockGap ?? 64,
    originX: options.originX ?? 0,
    originY: options.originY ?? 0,
  };
  const byId = new Map(units.map((u) => [u.id, u]));
  const ids = [...byId.keys()];
  const adj = new Map(ids.map((id) => [id, new Set()]));
  const und = new Map(ids.map((id) => [id, new Set()]));
  const seen = new Set();
  for (const edge of edges) {
    const { from, to } = edge;
    if (!byId.has(from) || !byId.has(to) || from === to) continue;
    const key = `${from}\u0000${to}`;
    if (seen.has(key)) continue;
    seen.add(key);
    adj.get(from).add(to);
    und.get(from).add(to);
    und.get(to).add(from);
  }

  const blocks = components(ids, und).map((group) => {
    const block = layoutComponent(group, adj, byId, opts);
    const first = byId.get(group[0]);
    block.order = first?.order ?? 0;
    return block;
  });
  blocks.sort((a, b) => a.order - b.order);

  const totalArea = blocks.reduce((sum, b) => sum + (b.width + opts.blockGap) * (b.height + opts.blockGap), 0);
  const maxWidth = options.maxWidth ?? Math.max(720, Math.sqrt(totalArea) * 1.7);
  const place = new Map();
  let x = 0;
  let y = 0;
  let rowHeight = 0;
  for (const block of blocks) {
    if (x > 0 && x + block.width > maxWidth) {
      y += rowHeight + opts.blockGap;
      x = 0;
      rowHeight = 0;
    }
    for (const [id, p] of block.positions) {
      place.set(id, { x: p.x + x, y: p.y + y });
    }
    x += block.width + opts.blockGap;
    rowHeight = Math.max(rowHeight, block.height);
  }

  let minX = Infinity;
  let minY = Infinity;
  for (const [id, p] of place) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    void id;
  }
  for (const p of place.values()) {
    p.x += opts.originX - minX;
    p.y += opts.originY - minY;
  }
  return place;
}
