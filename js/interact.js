// Pointer + keyboard interaction: selection, dragging, resizing, wiring.
import { newShape, newConnector, shapeRect } from './model.js';
import { autoSides, bestSide, endpointPoint, shapeCenter, routeConnector } from './router.js';
import { clamp, snapTo, distToSegment } from './util.js';

const MIN_SIZE = 6;

export function initInteraction(app) {
  const svg = app.svg;
  const stage = app.stage;
  const editor = app.editor;

  let drag = null;
  let spaceDown = false;
  let startSnapshot = null;
  let pendingDbl = null;

  const toWorld = (e) => {
    const r = svg.getBoundingClientRect();
    const sx = e.clientX - r.left, sy = e.clientY - r.top;
    const v = app.state.view;
    return { x: (sx - v.x) / v.zoom, y: (sy - v.y) / v.zoom };
  };

  const toScreen = (p) => {
    const v = app.state.view;
    return { x: p.x * v.zoom + v.x, y: p.y * v.zoom + v.y };
  };

  const attrOf = (target, name) => {
    let n = target;
    while (n && n.nodeType === 1) {
      if (n.dataset && n.dataset[name]) return n.dataset[name];
      n = n.parentNode;
    }
    return null;
  };

  const shapeAt = (e) => {
    const id = attrOf(e.target, 'shapeId');
    return id ? app.getPage().shapes.find((s) => s.id === id) : null;
  };

  // ------------------------------------------------------------ pointer --
  svg.addEventListener('pointerdown', onDown);
  svg.addEventListener('contextmenu', (e) => e.preventDefault());
  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('keydown', onKeyDown);
  window.addEventListener('keyup', onKeyUp);
  window.addEventListener('blur', () => { spaceDown = false; });

  function beginDrag(d) {
    drag = d;
    startSnapshot = app.history.snapshot(app.doc);
  }

  function endDrag() {
    if (!drag) return;
    const kind = drag.kind;
    drag = null;
    if (kind !== 'pan' && startSnapshot.json !== app.history.snapshot(app.doc).json) {
      app.history.pushSnapshot(startSnapshot);
    }
    startSnapshot = null;
    app.render();
    app.updateInspector();
  }

  function onDown(e) {
    if (e.button === 2) return;
    app.cancelTextEdit();
    svg.focus?.();

    const world = toWorld(e);
    const handle = attrOf(e.target, 'handle');
    const shapeId = attrOf(e.target, 'shapeId');
    const connId = attrOf(e.target, 'connId');

    if (e.button === 1 || spaceDown || app.state.tool === 'pan') {
      e.preventDefault();
      beginDrag({ kind: 'pan', sx: e.clientX, sy: e.clientY, vx: app.state.view.x, vy: app.state.view.y });
      svg.classList.add('dragging');
      return;
    }

    if (handle) {
      if (['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'].includes(handle)) {
        startResize(handle, world);
      } else if (handle === 'conn-start' || handle === 'conn-end') {
        startEndpoint(handle === 'conn-start' ? 'from' : 'to', world);
      } else if (handle === 'bend') {
        startBend(world);
      } else if (handle.startsWith('waypoint-')) {
        startWaypoint(parseInt(handle.slice(9), 10), world);
      }
      return;
    }

    if (app.state.tool === 'shape') {
      startCreate(app.state.pendingType, world);
      return;
    }

    if (app.state.tool === 'connector') {
      handleConnectClick(hitShape(world)?.id ?? shapeId, world);
      return;
    }

    // ---- select tool
    if (shapeId) {
      // A group is selected as a unit; double-click still edits the member.
      // A container frame also brings its transitive members.
      const groupIds = app.expandGroups(app.expandContainers([shapeId]));
      const sel = app.state.selection;
      if (e.shiftKey) {
        const all = groupIds.every((id) => sel.has(id));
        if (all) groupIds.forEach((id) => sel.delete(id));
        else groupIds.forEach((id) => sel.add(id));
        app.setSelection([...sel]);
      } else if (!groupIds.every((id) => sel.has(id))) {
        app.setSelection(groupIds);
      }
      pendingDbl = { id: shapeId, conn: false, dbl: isDoubleClick(shapeId, e), wx: world.x, wy: world.y };
      startMove(world);
      return;
    }

    if (connId) {
      const sel = app.state.selection;
      if (e.shiftKey) {
        if (sel.has(connId)) sel.delete(connId); else sel.add(connId);
      } else if (!sel.has(connId)) {
        app.setSelection([connId]);
      }
      pendingDbl = { id: connId, conn: true, dbl: isDoubleClick(connId, e), wx: world.x, wy: world.y };
      startConnDrag(connId, world);
      return;
    }

    // empty canvas
    if (!e.shiftKey) app.setSelection([]);
    beginDrag({ kind: 'marquee', start: world, cur: world, base: new Set(app.state.selection) });
  }

  function onMove(e) {
    if (!drag) {
      if (app.state.tool === 'connector') {
        const s = hitShape(toWorld(e));
        const id = s ? s.id : null;
        if (id !== app.state.hoverShapeId) {
          app.state.hoverShapeId = id;
          app.render();
        }
        app.state.connectPreview = app.state.connectSource
          ? { from: app.state.connectSource, point: toWorld(e) } : null;
      }
      return;
    }

    if (drag.kind === 'pan') {
      app.state.view.x = drag.vx + (e.clientX - drag.sx);
      app.state.view.y = drag.vy + (e.clientY - drag.sy);
      app.renderer.updateView();
      app.updateStatus();
      return;
    }

    const world = toWorld(e);

    // a press that turns into a drag is not a double click
    if (pendingDbl && (Math.abs(world.x - pendingDbl.wx) > 4 / app.state.view.zoom
      || Math.abs(world.y - pendingDbl.wy) > 4 / app.state.view.zoom)) {
      pendingDbl.dbl = false;
    }

    if (drag.kind === 'marquee') {
      drag.cur = world;
      const m = normRect(drag.start, world);
      app.state.marquee = m;
      const inside = new Set();
      for (const s of app.getPage().shapes) {
        if (rectsOverlap(m, shapeRect(s))) inside.add(s.id);
      }
      const map = app.shapeMap();
      for (const c of app.getPage().connectors) {
        const route = routeConnector(c, map);
        const pts = route.kind === 'curve'
          ? [route.points[0], route.points[1]]
          : route.points;
        if (pts.some((p) => p.x >= m.x && p.x <= m.x + m.w && p.y >= m.y && p.y <= m.y + m.h)) {
          inside.add(c.id);
        }
      }
      app.state.selection = new Set(app.expandGroups([...drag.base, ...inside]));
      app.render();
      app.updateInspector();
      return;
    }

    if (drag.kind === 'move') {
      let dx = world.x - drag.start.x, dy = world.y - drag.start.y;
      if (app.state.grid.snap) {
        const s0 = drag.items.find((i) => i.type === 'shape');
        if (s0) {
          dx = snapTo(s0.x + dx, app.state.grid.size) - s0.x;
          dy = snapTo(s0.y + dy, app.state.grid.size) - s0.y;
        }
      }
      for (const it of drag.items) {
        if (it.type === 'shape') {
          const s = app.getPage().shapes.find((x) => x.id === it.id);
          if (!s) continue;
          s.x = it.x + dx;
          s.y = it.y + dy;
          if (s.callout && (dx !== 0 || dy !== 0)) {
            // An actual drag detaches a callout from its target edge.
            s.callout.position = 'free';
            s.callout.autoSize = false;
          }
        } else {
          const c = app.getPage().connectors.find((x) => x.id === it.id);
          if (!c) continue;
          c.waypoints = it.waypoints.map((w) => ({ x: w.x + dx, y: w.y + dy }));
          if (it.freeFrom) c.from.point = { x: it.freeFrom.x + dx, y: it.freeFrom.y + dy };
          if (it.freeTo) c.to.point = { x: it.freeTo.x + dx, y: it.freeTo.y + dy };
        }
      }
      app.render();
      return;
    }

    if (drag.kind === 'resize') {
      applyResize(drag, world);
      app.render();
      return;
    }

    if (drag.kind === 'resize-group') {
      applyGroupResize(drag, world);
      app.render();
      return;
    }

    if (drag.kind === 'create') {
      const w = Math.abs(world.x - drag.start.x), h = Math.abs(world.y - drag.start.y);
      const s = app.getPage().shapes.find((x) => x.id === drag.id);
      if (s) {
        s.x = Math.min(drag.start.x, world.x);
        s.y = Math.min(drag.start.y, world.y);
        s.w = Math.max(MIN_SIZE, w);
        s.h = Math.max(MIN_SIZE, h);
      }
      app.render();
      return;
    }

    if (drag.kind === 'endpoint') {
      const c = app.getPage().connectors.find((x) => x.id === drag.id);
      if (!c) return;
      const target = hitShape(world);
      const end = c[drag.end];
      if (target) {
        // Face the rest of the connector, not the pointer. Dropping an end
        // near the middle of a page-sized background must not pick its far
        // side just because the pointer happens to be on that half.
        const other = c[drag.end === 'from' ? 'to' : 'from'];
        const otherPoint = endpointPoint(other, app.shapeMap());
        const ctr = shapeCenter(target);
        const ref = otherPoint || world;
        let dx = ref.x - ctr.x, dy = ref.y - ctr.y;
        if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) { dx = world.x - ctr.x; dy = world.y - ctr.y; }
        end.shapeId = target.id;
        end.side = bestSide(target, dx, dy);
        end.point = null;
        end.local = null;
      } else {
        end.shapeId = null;
        end.side = null;
        end.local = null;
        end.point = { x: world.x, y: world.y };
      }
      app.render();
      return;
    }

    if (drag.kind === 'waypoint') {
      const c = app.getPage().connectors.find((x) => x.id === drag.id);
      if (!c) return;
      const p = { x: world.x, y: world.y };
      if (app.state.grid.snap) {
        p.x = snapTo(p.x, app.state.grid.size);
        p.y = snapTo(p.y, app.state.grid.size);
      }
      c.waypoints[drag.index] = p;
      app.render();
      return;
    }

    if (drag.kind === 'conn-drag') {
      // bending a connector: project the cursor onto the route
      const c = app.getPage().connectors.find((x) => x.id === drag.id);
      if (!c) return;
      if (!c.waypoints.length) {
        const route = routeConnector(c, app.shapeMap());
        const p = nearestOnRoute(route, world);
        c.waypoints = [p];
        drag.index = 0;
        drag.kind = 'waypoint';
      }
    }
  }

  function onUp() {
    svg.classList.remove('dragging');
    if (pendingDbl && pendingDbl.dbl) {
      const target = pendingDbl.conn
        ? app.getPage().connectors.find((x) => x.id === pendingDbl.id)
        : app.getPage().shapes.find((x) => x.id === pendingDbl.id);
      if (target && !(target.callout && target.callout.kind === 'icon')) {
        startTextEdit(target, pendingDbl.conn);
      }
    }
    pendingDbl = null;
    if (drag && drag.kind === 'create') {
      const s = app.getPage().shapes.find((x) => x.id === drag.id);
      if (s && (s.w < 14 || s.h < 14)) {
        const def = app.shapeDefaults(drag.type);
        s.w = def.w; s.h = def.h;
        s.x = drag.start.x - def.w / 2;
        s.y = drag.start.y - def.h / 2;
      }
      app.setSelection([drag.id]);
      app.setTool('select');
    }
    if (drag && drag.kind === 'marquee') {
      app.state.marquee = null;
    }
    endDrag();
  }

  // ------------------------------------------------------------ actions --
  function startMove(world) {
    const items = [];
    for (const id of app.state.selection) {
      const s = app.getPage().shapes.find((x) => x.id === id);
      if (s) {
        items.push({ type: 'shape', id, x: s.x, y: s.y });
        continue;
      }
      const c = app.getPage().connectors.find((x) => x.id === id);
      if (c) {
        items.push({
          type: 'conn', id, waypoints: (c.waypoints || []).map((w) => ({ ...w })),
          freeFrom: c.from.shapeId ? null : (c.from.point ? { ...c.from.point } : null),
          freeTo: c.to.shapeId ? null : (c.to.point ? { ...c.to.point } : null),
        });
      }
    }
    beginDrag({ kind: 'move', start: world, items });
  }

  function startResize(handle, world) {
    const sel = [...app.state.selection];
    const shapes = app.getPage().shapes.filter((s) => sel.includes(s.id));
    if (!shapes.length) return;
    if (shapes.length > 1) {
      let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
      for (const s of shapes) {
        x1 = Math.min(x1, s.x); y1 = Math.min(y1, s.y);
        x2 = Math.max(x2, s.x + s.w); y2 = Math.max(y2, s.y + s.h);
      }
      beginDrag({
        kind: 'resize-group', handle, start: world,
        box: { x: x1, y: y1, w: x2 - x1, h: y2 - y1 },
        items: shapes.map((s) => ({ id: s.id, x: s.x, y: s.y, w: s.w, h: s.h })),
      });
      return;
    }
    const s = shapes[0];
    if (s.callout) s.callout.autoSize = false;
    beginDrag({
      kind: 'resize', id: s.id, handle, start: world,
      orig: { x: s.x, y: s.y, w: s.w, h: s.h, rot: s.rot || 0, flipX: s.flipX, flipY: s.flipY },
    });
  }

  function applyResize(d, world) {
    const s = app.getPage().shapes.find((x) => x.id === d.id);
    if (!s) return;
    const o = d.orig;
    const angle = (o.rot || 0) * Math.PI / 180;
    const fx = o.flipX ? -1 : 1, fy = o.flipY ? -1 : 1;
    const cx = o.x + o.w / 2, cy = o.y + o.h / 2;
    // world -> unrotated frame
    const dx = world.x - cx, dy = world.y - cy;
    const c = Math.cos(-angle), s2 = Math.sin(-angle);
    const ux = dx * c - dy * s2, uy = dx * s2 + dy * c;
    // ... -> the shape's own (possibly mirrored) local box
    const px = fx * ux + o.w / 2;
    const py = fy * uy + o.h / 2;

    const sx = ['ne', 'e', 'se'].includes(d.handle) ? 1 : ['nw', 'w', 'sw'].includes(d.handle) ? -1 : 0;
    const sy = ['se', 's', 'sw'].includes(d.handle) ? 1 : ['ne', 'n', 'nw'].includes(d.handle) ? -1 : 0;

    let nw = o.w, nh = o.h;
    let ncx = o.w / 2, ncy = o.h / 2;   // new centre in local coordinates
    // The edge opposite the grabbed handle stays fixed. Local y runs from the
    // top, so the south handle keeps the top and the north handle keeps the
    // bottom; using the wrong one makes a vertical drag jump to the far edge.
    if (sx > 0) { nw = Math.max(MIN_SIZE, px); ncx = nw / 2; }
    else if (sx < 0) { nw = Math.max(MIN_SIZE, o.w - px); ncx = o.w - nw / 2; }
    if (sy > 0) { nh = Math.max(MIN_SIZE, py); ncy = nh / 2; }
    else if (sy < 0) { nh = Math.max(MIN_SIZE, o.h - py); ncy = o.h - nh / 2; }

    // back to world space
    const cc = Math.cos(angle), ss = Math.sin(angle);
    const ox = fx * (ncx - o.w / 2), oy = fy * (ncy - o.h / 2);
    const nwx = cx + ox * cc - oy * ss;
    const nwy = cy + ox * ss + oy * cc;

    s.w = nw; s.h = nh;
    s.x = nwx - nw / 2;
    s.y = nwy - nh / 2;
  }

  function applyGroupResize(d, world) {
    const box = d.box;
    let sx = 1, sy = 1;
    const fx = (world.x - box.x) / (box.w || 1);
    const fy = (world.y - box.y) / (box.h || 1);
    if (d.handle.includes('e')) sx = fx;
    if (d.handle.includes('w')) sx = 1 - fx;
    if (d.handle.includes('s')) sy = fy;
    if (d.handle.includes('n')) sy = 1 - fy;
    sx = clamp(sx, 0.05, 20);
    sy = clamp(sy, 0.05, 20);
    if (d.handle === 'n' || d.handle === 's') sx = 1;
    if (d.handle === 'e' || d.handle === 'w') sy = 1;

    // Scale from the edge opposite the handle. Anchoring everything at the
    // top-left makes west/north drags grow in the wrong direction.
    const right = box.x + box.w;
    const bottom = box.y + box.h;
    const anchorRight = d.handle.includes('w');
    const anchorBottom = d.handle.includes('n');
    for (const it of d.items) {
      const s = app.getPage().shapes.find((x) => x.id === it.id);
      if (!s) continue;
      s.x = anchorRight
        ? right - (right - it.x) * sx
        : box.x + (it.x - box.x) * sx;
      s.y = anchorBottom
        ? bottom - (bottom - it.y) * sy
        : box.y + (it.y - box.y) * sy;
      s.w = Math.max(MIN_SIZE, it.w * sx);
      s.h = Math.max(MIN_SIZE, it.h * sy);
    }
  }

  function startEndpoint(which, world) {
    const id = [...app.state.selection].find((i) => app.getPage().connectors.some((c) => c.id === i));
    if (!id) return;
    beginDrag({ kind: 'endpoint', id, end: which, start: world });
  }

  function startBend(world) {
    const id = [...app.state.selection].find((i) => app.getPage().connectors.some((c) => c.id === i));
    if (!id) return;
    const c = app.getPage().connectors.find((x) => x.id === id);
    if (!c) return;
    c.waypoints = [{ x: world.x, y: world.y }];
    beginDrag({ kind: 'waypoint', id, index: 0, start: world });
  }

  function startWaypoint(index, world) {
    const id = [...app.state.selection].find((i) => app.getPage().connectors.some((c) => c.id === i));
    if (!id) return;
    beginDrag({ kind: 'waypoint', id, index, start: world });
  }

  function startConnDrag(connId, world) {
    const c = app.getPage().connectors.find((x) => x.id === connId);
    if (!c) return;
    // grabbing an existing waypoint?
    for (let i = 0; i < (c.waypoints || []).length; i++) {
      if (Math.hypot(c.waypoints[i].x - world.x, c.waypoints[i].y - world.y) < 8 / app.state.view.zoom) {
        beginDrag({ kind: 'waypoint', id: connId, index: i, start: world });
        return;
      }
    }
    beginDrag({ kind: 'conn-drag', id: connId, start: world });
  }

  function startCreate(type, world) {
    const s = newShape(type, world.x, world.y);
    s.w = 4; s.h = 4;
    app.getPage().shapes.push(s);
    beginDrag({ kind: 'create', id: s.id, type, start: world });
  }

  function handleConnectClick(shapeId, world) {
    if (shapeId) {
      if (!app.state.connectSource) {
        app.state.connectSource = { shapeId };
        app.render();
        app.setStatus('Now click the target shape');
        return;
      }
      const a = app.getPage().shapes.find((s) => s.id === app.state.connectSource.shapeId);
      const b = app.getPage().shapes.find((s) => s.id === shapeId);
      if (a && b && a.id !== b.id) {
        const sides = autoSides(a, b);
        const c = newConnector(
          { shapeId: a.id, side: sides.from },
          { shapeId: b.id, side: sides.to },
        );
        c.stroke = a.stroke; c.strokeWidth = a.strokeWidth; c.strokeStyle = a.strokeStyle;
        app.getPage().connectors.push(c);
        app.setSelection([c.id]);
        app.state.connectSource = null;
        app.render();
        app.updateInspector();
        return;
      }
      app.state.connectSource = { shapeId };
      app.render();
      return;
    }
    // empty space
    if (app.state.connectSource) {
      const a = app.getPage().shapes.find((s) => s.id === app.state.connectSource.shapeId);
      if (a) {
        const c = newConnector(
          { shapeId: a.id, side: bestSide(a, world.x - shapeCenter(a).x, world.y - shapeCenter(a).y) },
          { shapeId: null, side: null, point: { x: world.x, y: world.y } },
        );
        c.stroke = a.stroke;
        app.getPage().connectors.push(c);
        app.setSelection([c.id]);
      }
      app.state.connectSource = null;
      app.render();
      app.updateInspector();
      return;
    }
    app.state.connectSource = { point: { x: world.x, y: world.y } };
    app.render();
  }

  function hitShape(world) {
    // Connector glue target: prefer a small foreground shape over a large
    // background/frame that merely contains the pointer. A candidate may be
    // slightly outside (within 12 screen px), which matches how Visio snaps
    // an endpoint to a nearby shape.
    const shapes = app.getPage().shapes;
    const threshold = 12 / app.state.view.zoom;
    let best = null, bestArea = Infinity, bestDist = Infinity, bestZ = -Infinity;
    for (let i = 0; i < shapes.length; i++) {
      const s = shapes[i];
      if (s.layerHidden) continue;
      const dx = Math.max(s.x - world.x, 0, world.x - (s.x + s.w));
      const dy = Math.max(s.y - world.y, 0, world.y - (s.y + s.h));
      const dist = Math.hypot(dx, dy);
      if (dist > threshold) continue;
      const area = s.w * s.h;
      const z = s.z ?? 1000 + i;
      const better = area < bestArea - 0.01
        || (Math.abs(area - bestArea) <= 0.01
            && (dist < bestDist - 0.01
                || (Math.abs(dist - bestDist) <= 0.01 && z > bestZ)));
      if (better) { best = s; bestArea = area; bestDist = dist; bestZ = z; }
    }
    return best;
  }

  // ------------------------------------------------------- text editing --
  function startTextEdit(target, isConnector = false) {
    app.history.push(app.doc);
    const box = isConnector
      ? { x: target.from.point?.x ?? 0, y: target.from.point?.y ?? 0, w: 120, h: 24 }
      : shapeRect(target);
    const p = toScreen({ x: box.x, y: box.y });
    const w = Math.max(60, box.w * app.state.view.zoom);
    const h = Math.max(20, box.h * app.state.view.zoom);
    editor.classList.add('editing');
    editor.style.left = `${p.x}px`;
    editor.style.top = `${p.y}px`;
    editor.style.width = `${w}px`;
    editor.style.height = `${h}px`;
    editor.style.fontSize = `${(isConnector ? 11 : target.fontSize) * app.state.view.zoom}px`;
    editor.style.fontFamily = isConnector
      ? '"Segoe UI", Calibri, Helvetica, Arial, sans-serif'
      : target.fontFamily;
    editor.style.textAlign = isConnector ? 'center' : (target.align || 'center');
    editor.value = isConnector
      ? (target.label || '')
      : (target.callout ? (target.callout.label || '') : (target.label || ''));
    editor.dataset.targetId = target.id;
    editor.dataset.kind = isConnector ? 'conn' : 'shape';
    editor.focus();
    editor.select();
  }

  editor.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Escape') { editor.blur(); }
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { editor.blur(); }
  });
  editor.addEventListener('blur', () => {
    if (!editor.classList.contains('editing')) return;
    const id = editor.dataset.targetId;
    const kind = editor.dataset.kind;
    editor.classList.remove('editing');
    const val = editor.value;
    if (kind === 'conn') {
      const c = app.getPage().connectors.find((x) => x.id === id);
      if (c) c.label = val;
    } else {
      const s = app.getPage().shapes.find((x) => x.id === id);
      if (s) {
        if (s.callout) {
          // The canvas editor edits a callout's prefix; its value is derived.
          s.callout.label = val;
        } else {
          s.label = val;
          // Editing the plain label discards per-run formatting for that shape.
          delete s.rich;
          if (s.containerInfo && ['lane', 'pool', 'separator'].includes(s.containerInfo.role)) {
            s.containerInfo.heading = val;
            const rec = (app.getPage().containers || []).find((c) =>
              String(c.modelId) === String(s.id)
              || (s.visio && String(c.visioId) === String(s.visio.id)));
            if (rec) { rec.heading = val; rec.label = val; }
          }
        }
      }
    }
    app.render();
    app.updateInspector();
  });

  // ------------------------------------------------------------ keyboard --
  function onKeyDown(e) {
    const tag = (e.target.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea' || tag === 'select') return;

    if (e.code === 'Space' && !spaceDown) {
      spaceDown = true;
      svg.classList.add('mode-pan');
      e.preventDefault();
      return;
    }

    const mod = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();

    if (mod) {
      switch (k) {
        case 'z': e.preventDefault(); app.undo(); return;
        case 'y': e.preventDefault(); app.redo(); return;
        case 'a': e.preventDefault(); app.selectAll(); return;
        case 'c': app.copy(); return;
        case 'x': e.preventDefault(); app.cut(); return;
        case 'v': app.paste(); return;
        case 'd': e.preventDefault(); app.duplicate(); return;
        case 'g':
          e.preventDefault();
          if (e.shiftKey) app.ungroupSelection(); else app.groupSelection();
          return;
        case 'l':
          if (e.shiftKey) { e.preventDefault(); app.createSwimlane(); }
          return;
        case 's':
          e.preventDefault();
          if (e.shiftKey) app.saveVSDX(); else app.saveLocal();
          return;
        case 'o': e.preventDefault(); app.openFile(); return;
        case 'n': e.preventDefault(); app.newDoc(); return;
        case 'b': e.preventDefault(); app.toggleStyle('bold'); return;
        case 'i': e.preventDefault(); app.toggleStyle('italic'); return;
        case 'u': e.preventDefault(); app.toggleStyle('underline'); return;
        case '=': case '+': e.preventDefault(); app.zoomStep(1.2); return;
        case '-': e.preventDefault(); app.zoomStep(1 / 1.2); return;
        case '0': e.preventDefault(); app.fitView(); return;
        default: return;
      }
    }

    switch (e.key) {
      case 'Delete': case 'Backspace': e.preventDefault(); app.deleteSelection(); return;
      case 'Escape':
        if (app.state.connectSource) { app.state.connectSource = null; app.render(); }
        return;
      case 'Enter': {
        const first = [...app.state.selection][0];
        if (first) {
          const s = app.getPage().shapes.find((x) => x.id === first);
          if (s) startTextEdit(s);
        }
        return;
      }
      case 'ArrowUp': case 'ArrowDown': case 'ArrowLeft': case 'ArrowRight': {
        e.preventDefault();
        const step = e.shiftKey ? 10 : 1;
        const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0;
        const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0;
        app.nudge(dx, dy);
        return;
      }
    }

    switch (k) {
      case 'v': app.setTool('select'); break;
      case 'h': app.setTool('pan'); break;
      case 'c': app.setTool('connector'); break;
      case 'r': app.setTool('shape', 'rect'); break;
      case 'e': app.setTool('shape', 'ellipse'); break;
      case 'g': app.toggleGrid(); break;
      case 'l': app.toggleSnap(); break;
      case 'f': app.fitView(); break;
      default: break;
    }
  }

  function onKeyUp(e) {
    if (e.code === 'Space') {
      spaceDown = false;
      svg.classList.remove('mode-pan');
    }
  }
}

// ------------------------------------------------------------- helpers ---
// The canvas is re-rendered after every gesture, which replaces the DOM node
// under the cursor and breaks the browser's own double-click detection — so
// we track clicks ourselves.
let lastClick = { id: null, t: 0, x: 0, y: 0 };

function isDoubleClick(id, e) {
  const now = performance.now();
  const same = lastClick.id === id
    && now - lastClick.t < 450
    && Math.abs(e.clientX - lastClick.x) < 6
    && Math.abs(e.clientY - lastClick.y) < 6;
  lastClick = { id, t: now, x: e.clientX, y: e.clientY };
  return same;
}

function normRect(a, b) {
  return {
    x: Math.min(a.x, b.x), y: Math.min(a.y, b.y),
    w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y),
  };
}

function rectsOverlap(a, b) {
  return !(a.x + a.w < b.x || b.x + b.w < a.x || a.y + a.h < b.y || b.y + b.h < a.y);
}

function nearestOnRoute(route, p) {
  const pts = route.points;
  if (!pts || pts.length < 2) return { x: p.x, y: p.y };
  let best = { x: p.x, y: p.y }, bestD = Infinity;
  for (let i = 0; i < pts.length - 1; i++) {
    const a = pts[i], b = pts[i + 1];
    const dx = b.x - a.x, dy = b.y - a.y;
    const len2 = dx * dx + dy * dy;
    let t = len2 ? ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2 : 0;
    t = clamp(t, 0, 1);
    const q = { x: a.x + dx * t, y: a.y + dy * t };
    const d = distToSegment(p, a, b);
    if (d < bestD) { bestD = d; best = q; }
  }
  return best;
}
