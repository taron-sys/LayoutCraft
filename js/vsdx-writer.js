// VSDX writer.
//
// The importer resolves masters and flattens them into editable shapes. The
// writer starts from the original OPC package and patches the cells of
// objects the editor changed, removes objects that were deleted, and adds
// objects the editor created as inline Geometry shapes. Every other package
// entry is copied through untouched.
//
// Imported masters, themes, styles, images and unknown parts therefore
// survive. New shapes use inline geometry and no master, so Visio can open
// them without the original stencil.
import { download } from './util.js';
import { routeConnector, routeMidpoint, localToWorld } from './router.js';
import { shapeDef, shapePath, shapeDetail, shapeTextBox } from './shapes.js';
import { flattenPath } from './svg-path.js';
import { VISIO_DOCUMENT_TEMPLATE } from './visio-document-template.js';

const NS = 'http://schemas.microsoft.com/office/visio/2012/main';
const REL_NS = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PKG_REL_NS = 'http://schemas.openxmlformats.org/package/2006/relationships';
const CT_NS = 'http://schemas.openxmlformats.org/package/2006/content-types';
const DPI = 96;
const enc = new TextEncoder();
const dec = new TextDecoder();

const numDiff = (a, b) => Math.abs((Number(a) || 0) - (Number(b) || 0)) > 1e-6;
const strDiff = (a, b) => String(a ?? '') !== String(b ?? '');
const validColor = (v, fallback) => (v && v !== 'none' ? v : fallback);

/**
 * What can this save cover? The UI uses this to decide what to warn about
 * before writing. Objects created in LayoutCraft are written as inline shapes;
 * a few imported-only properties still cannot be represented.
 */
export function auditVSDX(doc, source) {
  if (!source || !source.package) return auditFromScratch(doc);

  const out = {
    ok: true,
    canAddPages: source.package.has('visio/pages/pages.xml'),
    addedShapes: 0,
    removedShapes: 0,
    addedPages: 0,
    removedPages: 0,
    fontChanges: 0,
    customStyleChanges: 0,
    newPartStyleLoss: 0,
    connectorRouteChanges: 0,
    zOrderChanges: 0,
    typeChanges: 0,
    duplicateIds: [],
    missingParts: [],
    authoredGroups: 0,
    rotatedGroupChanges: 0,
    containerGroupChanges: 0,
    ungroupChanges: 0,
    emptyGroups: 0,
    missingImageData: 0,
  };

  const usedParts = new Set();
  const authoredRoots = new Set();

  for (const pg of doc.pages) {
    const part = pg.visio && pg.visio.part;
    if (!part) { out.addedPages += 1; }
    else {
      usedParts.add(part);
      if (!source.package.has(part)) out.missingParts.push(part);
    }
    const pageIds = new Set();

    for (const shape of pg.shapes) {
      if (shape.image) {
        const rec = doc.media && doc.media[shape.image.mediaId];
        if (!rec || !/^data:/.test(String(rec.src || ''))) out.missingImageData += 1;
      }
      const sid = shape.visio && shape.visio.id;
      if (!sid) {
        out.addedShapes += 1;
        if (shape.parts && (shape.parts.length > 1
            || shape.parts.some((p) => p.fillGradient || p.bottomEdge))) {
          out.newPartStyleLoss += 1;
        }
        continue;
      }
      if (pageIds.has(String(sid))) out.duplicateIds.push(String(sid));
      pageIds.add(String(sid));
      const orig = shape.visio.orig;
      if (!orig) continue;
      if (strDiff(shape.fontFamily, orig.fontFamily)) out.fontChanges += 1;
      if ((shape.z ?? 0) !== (orig.z ?? 0)) out.zOrderChanges += 1;
      if (orig.type && shape.type !== orig.type) out.typeChanges += 1;
      if (orig.custom && (strDiff(shape.fill, orig.fill)
          || strDiff(shape.stroke, orig.stroke)
          || numDiff(shape.strokeWidth, orig.strokeWidth)
          || strDiff(shape.strokeStyle, orig.strokeStyle))) {
        out.customStyleChanges += 1;
      }
      const was = shape.visio.group;
      const now = shape.group;
      if (was && (!now || now.rootId !== was.rootId)) out.ungroupChanges += 1;
      if (now && now.authored) authoredRoots.add(now.rootId);
    }

    for (const conn of pg.connectors) {
      const sid = conn.visio && conn.visio.id;
      if (!sid) { out.addedShapes += 1; continue; }
      if (pageIds.has(String(sid))) out.duplicateIds.push(String(sid));
      pageIds.add(String(sid));
      if (conn.visio.orig && conn.visio.orig.route !== undefined
          && connectorRouteChanged(conn, conn.visio.orig)) {
        out.connectorRouteChanges += 1;
      }
      const was = conn.visio.group;
      const now = conn.group;
      if (was && (!now || now.rootId !== was.rootId)) out.ungroupChanges += 1;
      if (now && now.authored) authoredRoots.add(now.rootId);
    }
    for (const sourceId of pg.visio?.ids || []) {
      if (!pageIds.has(String(sourceId))) out.removedShapes += 1;
    }

    // Group frames that cannot be rewritten yet.
    const allMembers = [...pg.shapes, ...pg.connectors];
    for (const g of pg.groups || []) {
      if (!g.visioId) continue;
      const members = allMembers.filter((o) => o.group && o.group.id === String(g.visioId));
      const hasChildGroup = (pg.groups || []).some((h) => h.parentId != null
        && String(h.parentId) === String(g.visioId));
      if (!members.length && !hasChildGroup) { out.emptyGroups += 1; continue; }
      const directChanged = members.some((o) => {
        if (o.x === undefined) return false;           // connector: no frame
        const oo = o.visio && o.visio.orig;
        if (!oo) return true;
        return numDiff(o.x, oo.x) || numDiff(o.y, oo.y)
          || numDiff(o.w, oo.w) || numDiff(o.h, oo.h);
      });
      if (!directChanged && !(g.rotated && g.parentId == null)) continue;
      if (g.rotated) {
        // Only the outermost rotated group can be represented, and only as a
        // whole-tree move (its Pin cells move; children stay local). Nested
        // groups are covered by that check.
        if (g.parentId != null) continue;
        const move = rotatedRootDelta(pg, g);
        if (!move.pure) out.rotatedGroupChanges += 1;
      }
      // Container frames are patched like group frames; membership edits are
      // not represented yet (the Relationships cells are preserved as-is).
    }
  }
  out.authoredGroups = authoredRoots.size;

  for (const part of source.pageParts || []) {
    if (!usedParts.has(part)) out.removedPages += 1;
  }
  return out;
}

function auditFromScratch(doc) {
  const out = {
    ok: true,
    fromScratch: true,
    canAddPages: true,
    importedPages: 0,
    importedObjects: 0,
    addedShapes: 0,
    removedShapes: 0,
    addedPages: doc.pages.length,
    removedPages: 0,
    fontChanges: 0,
    customStyleChanges: 0,
    newPartStyleLoss: 0,
    connectorRouteChanges: 0,
    zOrderChanges: 0,
    typeChanges: 0,
    duplicateIds: [],
    missingParts: [],
    authoredGroups: 0,
    rotatedGroupChanges: 0,
    containerGroupChanges: 0,
    ungroupChanges: 0,
    emptyGroups: 0,
    containerMembershipLoss: 0,
    missingImageData: 0,
  };
  const groupRoots = new Set();
  for (const pg of doc.pages) {
    if (pg.visio?.part) out.importedPages += 1;
    for (const shape of pg.shapes) {
      out.addedShapes += 1;
      if (shape.visio) out.importedObjects += 1;
      if (shape.image) {
        const rec = doc.media && doc.media[shape.image.mediaId];
        if (!rec || !/^data:/.test(String(rec.src || ''))) out.missingImageData += 1;
      }
      if (shape.group) groupRoots.add(shape.group.rootId || shape.group.id);
      if (shape.parts && (shape.parts.length > 1
          || shape.parts.some((p) => p.fillGradient || p.bottomEdge))) {
        out.newPartStyleLoss += 1;
      }
    }
    for (const conn of pg.connectors) {
      out.addedShapes += 1;
      if (conn.visio) out.importedObjects += 1;
      if (conn.group) groupRoots.add(conn.group.rootId || conn.group.id);
    }
    if ((pg.containers || []).some((c) => !c.authored && ((c.members || []).length
        || (c.items || []).length || (c.related || []).length))) {
      out.containerMembershipLoss += 1;
    }
  }
  // A fresh package has no group parts to patch, so every group is flattened
  // to separate inline shapes.
  out.authoredGroups = groupRoots.size;
  return out;
}

/** Build the complete edited package, without downloading it. */
export async function buildVSDX(doc, source) {
  if (!source || !source.package) throw new Error('No original VSDX package is available');

  // A shallow copy is enough: entry byte arrays are never mutated in place.
  const entries = new Map(source.package);
  const media = createMediaWriter(entries);
  const pictures = doc.media || {};
  let patchedPages = 0;
  let newPages = 0;
  const needsPages = doc.pages.some((pg) => !pg.visio?.part);
  const pkg = needsPages ? makePackageEditor(entries) : null;

  for (const pg of doc.pages) {
    const part = pg.visio && pg.visio.part;
    if (part) {
      const raw = entries.get(part);
      if (!raw) continue;
      const xml = new DOMParser().parseFromString(dec.decode(raw), 'application/xml');
      if (xml.querySelector('parsererror')) throw new Error(`Could not parse ${part}`);
      if (patchPage(xml, pg, { pageH: pg.visio.pageH, media, pagePart: part, pictures })) {
        entries.set(part, enc.encode(serializeXml(xml)));
        patchedPages += 1;
      }
      continue;
    }

    if (!pkg) throw new Error('This package stores pages inline; adding pages is not supported yet.');
    const dims = newPageDims(pg, pkg);
    const partPath = pkg.addPage(pg.name, dims.width, dims.height);
    // Give the page an empty rels part first: the media writer reads it when a
    // picture is added and rewrites it with the image relationships.
    entries.set(partPath.replace(/^visio\/pages\//, 'visio/pages/_rels/') + '.rels',
      enc.encode(pagesRelsTemplate()));
    const xml = new DOMParser().parseFromString(
      `<PageContents xmlns="${NS}" xmlns:r="${REL_NS}" xml:space="preserve"><Shapes></Shapes><Connects></Connects></PageContents>`,
      'application/xml',
    );
    patchPage(xml, pg, {
      pageH: dims.height, allNew: true, sortZ: true,
      media, pagePart: partPath, pictures,
    });
    entries.set(partPath, enc.encode(serializeXml(xml)));
    newPages += 1;
  }

  if (pkg) pkg.flush(entries);
  media.flush();

  return { blob: await zipBlob(entries), patchedPages, newPages };
}

export async function exportVSDX(doc, source, fileName) {
  const result = source && source.package
    ? await buildVSDX(doc, source)
    : await buildNewVSDX(doc);
  download(fileName, result.blob);
  return result;
}

/** Build a complete package for a document that never opened a VSDX. */
export async function buildNewVSDX(doc) {
  const pages = doc.pages.length
    ? doc.pages
    : [{ name: 'Page-1', shapes: [], connectors: [] }];
  const names = pages.map((p, i) => p.name || `Page-${i + 1}`);
  const firstDims = newPageDims(pages[0], { defaultWidth: 11, defaultHeight: 8.5 });

  const entries = new Map();
  entries.set('[Content_Types].xml', enc.encode(contentTypesTemplate()));
  entries.set('_rels/.rels', enc.encode(rootRelsTemplate()));
  entries.set('docProps/core.xml', enc.encode(coreTemplate(doc.name)));
  entries.set('docProps/app.xml', enc.encode(appTemplate(names)));
  entries.set('visio/document.xml', enc.encode(documentTemplate()));
  entries.set('visio/_rels/document.xml.rels', enc.encode(documentRelsTemplate()));
  entries.set('visio/masters/masters.xml', enc.encode(mastersTemplate()));
  entries.set('visio/masters/_rels/masters.xml.rels', enc.encode(pagesRelsTemplate()));
  entries.set('visio/pages/pages.xml', enc.encode(pagesTemplate()));
  entries.set('visio/pages/_rels/pages.xml.rels', enc.encode(pagesRelsTemplate()));
  entries.set('visio/windows.xml', enc.encode(windowsTemplate(firstDims)));
  entries.set('visio/theme/theme1.xml', enc.encode(themeTemplate()));

  const pkg = makePackageEditor(entries);
  const media = createMediaWriter(entries);
  const pictures = doc.media || {};
  let newPages = 0;
  let newObjects = 0;
  for (let i = 0; i < pages.length; i++) {
    const pg = pages[i];
    const dims = newPageDims(pg, pkg);
    const part = pkg.addPage(pg.name || `Page-${i + 1}`, dims.width, dims.height);
    entries.set(part.replace(/^visio\/pages\//, 'visio/pages/_rels/') + '.rels',
      enc.encode(pagesRelsTemplate()));
    const xml = new DOMParser().parseFromString(
      `<PageContents xmlns="${NS}" xmlns:r="${REL_NS}" xml:space="preserve"><Shapes></Shapes><Connects></Connects></PageContents>`,
      'application/xml',
    );
    patchPage(xml, pg, {
      pageH: dims.height, allNew: true, sortZ: true,
      media, pagePart: part, pictures,
    });
    entries.set(part, enc.encode(serializeXml(xml)));
    newPages += 1;
    newObjects += pg.shapes.length + pg.connectors.length;
  }
  pkg.flush(entries);
  media.flush();
  return { blob: await zipBlob(entries), patchedPages: 0, newPages, newObjects };
}

/** File name for a saved copy, keeping the source package type. */
export function vsdxFileName(doc, source) {
  const src = (source && source.name) || '';
  const ext = (/\.(vsdx|vstx|vsdm|vstm)$/i.exec(src) || [null, 'vsdx'])[1];
  const base = String(doc.name || 'diagram')
    .replace(/[\\/:*?"<>|]+/g, '-').trim() || 'diagram';
  return source ? `${base}-edited.${ext}` : `${base}.vsdx`;
}

// ---------------------------------------------------------------- patching --

/**
 * Is a rotated group's edit a pure whole-subtree move?
 *
 * Rotated groups keep the group's Angle and their children in local space.
 * Moving the whole tree is representable by changing only the group's
 * PinX/PinY; a resize or a member moved on its own is not (the editor's
 * handle box is axis-aligned, while Visio scales in the group's rotated
 * frame), so those are reported by the audit instead of guessed.
 */
function rotatedRootDelta(pg, root) {
  const all = pg.shapes.filter((s) => s.group
    && String(s.group.rootId) === String(root.rootId) && s.visio);
  if (!all.length) return { pure: true, changed: false, dx: 0, dy: 0 };
  let dx = null, dy = null;
  for (const s of all) {
    const o = s.visio.orig;
    if (!o) return { pure: false, changed: true };
    if (Math.abs(s.w - o.w) > 1e-4 || Math.abs(s.h - o.h) > 1e-4
        || Math.abs((s.rot || 0) - (o.rot || 0)) > 1e-4
        || !!s.flipX !== !!o.flipX || !!s.flipY !== !!o.flipY) {
      return { pure: false, changed: true };
    }
    const ddx = s.x - o.x, ddy = s.y - o.y;
    if (dx === null) { dx = ddx; dy = ddy; }
    else if (Math.abs(ddx - dx) > 1e-4 || Math.abs(ddy - dy) > 1e-4) {
      return { pure: false, changed: true };
    }
  }
  return {
    pure: true,
    changed: Math.abs(dx) > 1e-4 || Math.abs(dy) > 1e-4,
    dx, dy,
  };
}

/**
 * Rename imported CFF group frames (lanes, separators, pool titles). Inline
 * shapes go through patchContainerRows instead; group lanes have no model
 * shape, so their heading lives only in the group element.
 */
function applyGroupContainerRows(pg, byId) {
  let changed = false;
  for (const g of pg.groups || []) {
    if (!g.visioId || !g.role) continue;
    if (!['pool', 'lane', 'separator'].includes(g.role)) continue;
    const el = byId.get(String(g.visioId));
    if (!el) continue;
    const heading = g.heading ?? g.label ?? '';
    if (heading === (g.origHeading ?? '')) continue;
    setUserRow(el, 'visHeadingText', heading, 'STR');
    changed = true;
  }
  return changed;
}

/**
 * Recompute imported group frames from their members.
 *
 * The editor stores absolute world positions, while a Visio group stores a
 * frame plus children in its local space. Moving a group is normally just a
 * frame move; resizing or moving one member also needs the frame and the
 * children's local coordinates rewritten. Rotated groups only support a pure
 * whole-tree move here; other rotated edits are reported by the audit.
 *
 * Returns per-shape layout overrides for patchShape plus whether any group
 * cell changed.
 */
function applyGroupLayout(pg, pageH, byId) {
  const allGroups = (pg.groups || []).filter((g) => g.visioId
    && (g.kind === 'group' || g.kind === 'container')
    && g.frame && Number.isFinite(g.frame.x) && Number.isFinite(g.frame.y)
    && byId.has(String(g.visioId)));
  const rotatedRoots = allGroups.filter((g) => g.rotated && g.parentId == null);
  const groups = allGroups.filter((g) => !g.rotated);
  const overrides = new Map();
  const skipConnectors = new Set();
  if (!groups.length && !rotatedRoots.length) return { changed: false, overrides, skipConnectors };

  let changed = false;

  // Rotated roots first: a pure move rewrites the group Pin in the parent's
  // local space and leaves every child's cells untouched. Even an unsupported
  // rotated edit suppresses the members' layout cells, because their old
  // parent-origin translation would put them in the wrong place.
  for (const g of rotatedRoots) {
    const vid = String(g.visioId);
    const rootsMembers = pg.shapes.filter((s) => s.group
      && String(s.group.rootId) === String(g.rootId) && s.visio);
    const d = rotatedRootDelta(pg, g);
    if (d.pure && d.changed) {
      const el = byId.get(vid);
      if (el) {
        // World pixels -> the group's parent local inches. The editor's y axis
        // points down; Visio's is up. A rotated ancestor (only possible when
        // this group is not top-level) rotates the delta as well.
        const a = g.parentAngle || 0;
        const wx = d.dx / DPI, wy = -d.dy / DPI;
        const lx = Math.cos(a) * wx + Math.sin(a) * wy;
        const ly = -Math.sin(a) * wx + Math.cos(a) * wy;
        setCell(el, 'PinX', fmt(cellNumber(el, 'PinX', 0) + lx));
        setCell(el, 'PinY', fmt(cellNumber(el, 'PinY', 0) + ly));
        changed = true;
      }
    }
    for (const s of rootsMembers) overrides.set(s.id, { skip: true });
    for (const c of pg.connectors) {
      if (c.visio && c.group && String(c.group.rootId) === String(g.rootId)) {
        skipConnectors.add(String(c.visio.id));
      }
    }
  }

  const near = (a, b) => Math.abs(a - b) < 1e-4;
  const shapeBox = (s) => {
    const x1 = s.x / DPI;
    const y1 = pageH - (s.y + s.h) / DPI;
    const x2 = (s.x + s.w) / DPI;
    const y2 = pageH - s.y / DPI;
    return { x1, y1, x2, y2, w: x2 - x1, h: y2 - y1 };
  };
  const origBox = (s) => {
    if (!s.visio || !s.visio.orig) return null;
    const o = s.visio.orig;
    const x1 = o.x / DPI;
    const y1 = pageH - (o.y + o.h) / DPI;
    const x2 = (o.x + o.w) / DPI;
    const y2 = pageH - o.y / DPI;
    return { x1, y1, x2, y2, w: x2 - x1, h: y2 - y1 };
  };

  const byParent = new Map();
  for (const g of groups) {
    const key = g.parentId == null ? '' : String(g.parentId);
    if (!byParent.has(key)) byParent.set(key, []);
    byParent.get(key).push(g);
  }

  // Unions, innermost first so a parent can use a child group's new frame.
  const frames = new Map();
  const bottomUp = [...groups].sort((a, b) => (b.depth || 0) - (a.depth || 0));
  for (const g of bottomUp) {
    const vid = String(g.visioId);
    const direct = pg.shapes.filter((s) => s.group && s.group.id === vid && s.visio);
    const childGroups = byParent.get(vid) || [];
    let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
    const grow = (b) => {
      x1 = Math.min(x1, b.x1); y1 = Math.min(y1, b.y1);
      x2 = Math.max(x2, b.x2); y2 = Math.max(y2, b.y2);
    };
    for (const s of direct) grow(shapeBox(s));
    for (const h of childGroups) {
      const f = frames.get(String(h.visioId));
      if (f) grow(f);
    }
    if (!Number.isFinite(x1)) continue;
    frames.set(vid, {
      x1, y1, x2, y2,
      w: Math.max(0.01, x2 - x1),
      h: Math.max(0.01, y2 - y1),
    });
  }

  const skipIds = new Set();
  const skipGroups = new Set();
  const topDown = [...groups].sort((a, b) => (a.depth || 0) - (b.depth || 0));

  for (const g of topDown) {
    const vid = String(g.visioId);
    if (skipGroups.has(vid)) continue;
    const nf = frames.get(vid);
    const old = g.frame;
    if (!nf || !old) continue;
    const direct = pg.shapes.filter((s) => s.group && s.group.id === vid && s.visio);
    const childGroups = byParent.get(vid) || [];

    // A top-level group whose whole subtree moved by one delta is a frame
    // move: patch the group transform and leave every child local coordinate
    // alone. Nested groups are covered by the ancestor, so do not also move
    // them (that would apply the delta twice).
    if (g.parentId == null) {
      const all = pg.shapes.filter((s) => s.group && s.group.rootId === g.rootId && s.visio);
      const dx = nf.x1 - old.x;
      const dy = nf.y1 - old.y;
      let pure = all.length > 0 && near(nf.w, old.w) && near(nf.h, old.h);
      if (pure) {
        for (const s of all) {
          const ob = origBox(s), nb = shapeBox(s);
          if (!ob || !near(nb.x1 - ob.x1, dx) || !near(nb.y1 - ob.y1, dy)
              || !near(nb.w, ob.w) || !near(nb.h, ob.h)) { pure = false; break; }
        }
      }
      if (pure && (Math.abs(dx) > 1e-9 || Math.abs(dy) > 1e-9)) {
        const el = byId.get(vid);
        if (el) {
          setCell(el, 'PinX', fmt(cellNumber(el, 'PinX', old.x) + dx));
          setCell(el, 'PinY', fmt(cellNumber(el, 'PinY', old.y) + dy));
          changed = true;
        }
        for (const s of all) skipIds.add(s.id);
        for (const h of groups) {
          if (String(h.rootId) === String(g.rootId)) skipGroups.add(String(h.visioId));
        }
        continue;
      }
    }

    const memberChanged = direct.some((s) => {
      const ob = origBox(s), nb = shapeBox(s);
      return !ob || !near(nb.x1, ob.x1) || !near(nb.y1, ob.y1)
        || !near(nb.w, ob.w) || !near(nb.h, ob.h);
    });
    const childChanged = childGroups.some((h) => {
      const f = frames.get(String(h.visioId));
      return !f || !h.frame || !near(f.x1, h.frame.x) || !near(f.y1, h.frame.y)
        || !near(f.w, h.frame.w) || !near(f.h, h.frame.h);
    });
    // An untouched import can have a member union that differs slightly from
    // the stored group frame (margins, images, rotated boxes). Only rewrite
    // the frame when a member or child group actually changed.
    if (!memberChanged && !childChanged) continue;
    const el = byId.get(vid);
    if (!el) continue;

    const oldW = cellNumber(el, 'Width', old.w || nf.w);
    const oldH = cellNumber(el, 'Height', old.h || nf.h);
    const locX0 = cellNumber(el, 'LocPinX', oldW / 2);
    const locY0 = cellNumber(el, 'LocPinY', oldH / 2);
    const fx = oldW > 0 ? locX0 / oldW : 0.5;
    const fy = oldH > 0 ? locY0 / oldH : 0.5;
    const newLocX = nf.w * fx;
    const newLocY = nf.h * fy;
    const parentFrame = g.parentId != null ? frames.get(String(g.parentId)) : null;
    const pOx = parentFrame ? parentFrame.x1 : 0;
    const pOy = parentFrame ? parentFrame.y1 : 0;

    setCell(el, 'Width', fmt(nf.w));
    setCell(el, 'Height', fmt(nf.h));
    setCell(el, 'LocPinX', fmt(newLocX));
    setCell(el, 'LocPinY', fmt(newLocY));
    setCell(el, 'PinX', fmt(nf.x1 - pOx + newLocX));
    setCell(el, 'PinY', fmt(nf.y1 - pOy + newLocY));
    if (!near(nf.w, oldW) && cellOf(el, 'TxtPinX')) {
      setCellV(el, 'TxtPinX', fmt(cellNumber(el, 'TxtPinX', 0) * (nf.w / Math.max(oldW, 1e-9))));
    }
    if (!near(nf.h, oldH) && cellOf(el, 'TxtPinY')) {
      setCellV(el, 'TxtPinY', fmt(cellNumber(el, 'TxtPinY', 0) * (nf.h / Math.max(oldH, 1e-9))));
    }
    for (const s of direct) overrides.set(s.id, { ox: nf.x1, oy: nf.y1, force: true });
    changed = true;
  }

  for (const id of skipIds) overrides.set(id, { skip: true });
  return { changed, overrides, skipConnectors };
}

function sameIdSet(a, b) {
  const x = [...new Set((a || []).map(String))].sort();
  const y = [...new Set((b || []).map(String))].sort();
  return x.length === y.length && x.every((v, i) => v === y[i]);
}

/** All DEPENDSON calls in a Relationships cell, including empty groups. */
function parseRelationshipGroups(cell) {
  const f = cell ? (cell.getAttribute('F') || '') : '';
  const out = [];
  const re = /DEPENDSON\s*\(\s*(\d+)\s*((?:,\s*Sheet\.[A-Za-z0-9_]+!SheetRef\s*\(\s*\))*)\s*\)/gi;
  let m;
  while ((m = re.exec(f))) {
    out.push({
      type: Number(m[1]),
      refs: [...m[2].matchAll(/Sheet\.([A-Za-z0-9_]+)!SheetRef/g)].map((x) => String(x[1])),
    });
  }
  return out;
}

/**
 * Replace only the managed DEPENDSON type groups; other relationships on the
 * shape (callouts and future types) are preserved. Removing every group drops
 * the cell, which is how a shape with no relationships starts.
 */
function writeRelationships(el, managed) {
  const cell = cellOf(el, 'Relationships');
  const managedTypes = new Set(Object.keys(managed).map(Number));
  const groups = parseRelationshipGroups(cell).filter((g) => !managedTypes.has(g.type));
  for (const [type, refs] of Object.entries(managed)) {
    if (refs && refs.length) {
      groups.push({ type: Number(type), refs: [...new Set(refs.map(String))] });
    }
  }
  if (!groups.length) {
    if (cell) cell.remove();
    return true;
  }
  groups.sort((a, b) => a.type - b.type);
  const formula = `SUM(${groups.map((g) => `DEPENDSON(${g.type}${g.refs
    .map((r) => `,Sheet.${r}!SheetRef()`).join('')})`).join(',')})`;
  setCellWithFormula(el, 'Relationships', 0, formula);
  return true;
}

/**
 * Reconcile the container/list membership graph after editor edits.
 *
 * Group frames (lanes, separators) have no model shape of their own, so their
 * membership refs are carried over from the import snapshot. Model objects are
 * derived from shape.containers/listOf, which also covers new and deleted
 * members. Untouched membership is not rewritten.
 */
function applyMemberships(pg, ctx) {
  const containers = pg.containers || [];
  if (!containers.length) return false;
  const objects = [...pg.shapes, ...pg.connectors];
  const groupIds = new Set((pg.groups || []).map((g) => String(g.visioId)));
  const visioId = (o) => ctx.modelToVisio.get(o.id)
    || (o.visio && o.visio.id) || null;
  const elementOf = (o) => {
    if (o.visio && o.visio.id) return ctx.byId.get(String(o.visio.id));
    if (o.w !== undefined) return ctx.newShapeSpecs.get(o.id)?.el || null;
    return ctx.connectorEls.get(o.id) || null;
  };

  // Only containers with a real element are writable. An authored container
  // is a new inline shape resolved through modelToVisio/newShapeSpecs.
  const resolved = [];
  for (const c of containers) {
    const key = String(c.visioId || c.modelId || '');
    if (!key) continue;
    const vid = c.visioId ? String(c.visioId)
      : (c.modelId ? ctx.modelToVisio.get(c.modelId) : null);
    const el = c.visioId ? ctx.byId.get(String(c.visioId))
      : (c.modelId ? ctx.newShapeSpecs.get(c.modelId)?.el : null);
    if (vid && el) resolved.push({ c, key, vid: String(vid), el });
  }
  if (!resolved.length) return false;

  const refMap = new Map();
  for (const r of resolved) {
    refMap.set(r.key, r.vid);
    if (r.c.visioId) refMap.set(String(r.c.visioId), r.vid);
    if (r.c.modelId) refMap.set(String(r.c.modelId), r.vid);
  }
  const resolve = (id) => refMap.get(String(id)) || null;
  const dedupe = (ids) => [...new Set(ids.map(String))];

  const desired = new Map();
  for (const r of resolved) {
    const orig = r.c.orig || { members: r.c.members || [], items: r.c.items || [], related: r.c.related || [] };
    desired.set(r.vid, {
      // Group-frame refs (lanes/separators) have no model object; keep them.
      members: dedupe((orig.members || []).filter((x) => groupIds.has(String(x)))),
      items: dedupe((orig.items || []).filter((x) => groupIds.has(String(x)))),
      related: dedupe((orig.related || []).map(resolve).filter(Boolean)),
    });
  }
  const add = (rec, key, id) => {
    if (!rec) return;
    const s = String(id);
    if (!rec[key].includes(s)) rec[key].push(s);
  };
  for (const o of objects) {
    const vid = visioId(o);
    if (!vid) continue;
    for (const cid of o.containers || []) add(desired.get(resolve(cid)), 'members', vid);
    for (const lid of o.listOf || []) add(desired.get(resolve(lid)), 'items', vid);
  }
  // List item order drives lane/phase order in Visio. Prefer the editor's
  // record order, then any items discovered only through object membership.
  for (const r of resolved) {
    const d = desired.get(r.vid);
    if (!d) continue;
    const ordered = dedupe((r.c.items || []).map(resolve).filter(Boolean));
    d.items = dedupe([...ordered, ...d.items]);
  }

  let changed = false;
  // Container/list side: type 1 members, type 2 items, type 11 related lists.
  for (const r of resolved) {
    const d = desired.get(r.vid);
    const orig = r.c.orig || { members: r.c.members || [], items: r.c.items || [], related: r.c.related || [] };
    const managed = {};
    const origMembers = dedupe((orig.members || []).map(String));
    const origItems = dedupe((orig.items || []).map(String));
    const origRelated = dedupe((orig.related || []).map(String));
    if (!sameIdSet(d.members, origMembers)) managed[1] = d.members;
    if (r.c.kind === 'list') {
      if (!sameIdSet(d.items, origItems)) managed[2] = d.items;
      if (!sameIdSet(d.related, dedupe(origRelated.map(resolve).filter(Boolean)))) {
        managed[11] = d.related;
      }
    }
    if (Object.keys(managed).length && writeRelationships(r.el, managed)) changed = true;
  }

  // Member side: type 4 containers, type 5 list items. Resolve model-level
  // authored-container ids to the new sheet ids; drop refs we cannot write.
  for (const o of objects) {
    const el = elementOf(o);
    if (!el) continue;
    const nowC = dedupe((o.containers || []).map(resolve).filter(Boolean));
    const nowL = dedupe((o.listOf || []).map(resolve).filter(Boolean));
    const origC = dedupe(((o.visio && o.visio.containers) || []).map(String));
    const origL = dedupe(((o.visio && o.visio.listOf) || []).map(String));
    const managed = {};
    if (!sameIdSet(nowC, origC)) managed[4] = nowC;
    if (!sameIdSet(nowL, origL)) managed[5] = nowL;
    if (Object.keys(managed).length && writeRelationships(el, managed)) changed = true;
  }
  return changed;
}
function patchPage(doc, pg, {
  pageH = 8.5, allNew = false, sortZ = false,
  media = null, pagePart = null, pictures = null,
} = {}) {
  let changed = false;
  const root = doc.documentElement;
  const shapesById = new Map(pg.shapes.map((s) => [s.id, s]));
  const shapesRoot = ensureShapesRoot(doc, root);
  const byId = new Map();
  for (const el of all(root, 'Shape')) {
    const id = el.getAttribute('ID');
    if (id !== null) byId.set(String(id), el);
  }

  let maxId = 0;
  for (const id of byId.keys()) {
    const n = parseInt(id, 10);
    if (Number.isFinite(n)) maxId = Math.max(maxId, n);
  }
  const nextId = () => String(++maxId);
  const modelToVisio = new Map();

  let groupLayout = null;
  let skipConnectorRoutes = null;
  if (!allNew && pg.groups && pg.groups.length) {
    groupLayout = applyGroupLayout(pg, pageH, byId);
    skipConnectorRoutes = groupLayout.skipConnectors;
    if (groupLayout.changed) changed = true;
    if (applyGroupContainerRows(pg, byId)) changed = true;
  }

  if (!allNew) {
    // Remove shapes/connectors deleted in the editor, plus their Connects.
    const keep = new Set();
    for (const o of [...pg.shapes, ...pg.connectors]) {
      if (o.visio && o.visio.id) keep.add(String(o.visio.id));
    }
    const removed = new Set();
    for (const sourceId of pg.visio?.ids || []) {
      const id = String(sourceId);
      if (keep.has(id)) continue;
      const el = byId.get(id);
      if (!el) continue;
      removed.add(id);
      for (const sub of all(el, 'Shape')) {
        const subId = sub.getAttribute('ID');
        if (subId !== null) removed.add(String(subId));
      }
      if (el.parentNode) el.parentNode.removeChild(el);
      changed = true;
    }
    if (removed.size) {
      for (const c of [...all(root, 'Connect')]) {
        if (removed.has(String(c.getAttribute('FromSheet')))
            || removed.has(String(c.getAttribute('ToSheet')))) {
          if (c.parentNode) c.parentNode.removeChild(c);
          changed = true;
        }
      }
    }

    for (const shape of pg.shapes) {
      if (shape.visio && shape.visio.id) {
        modelToVisio.set(shape.id, String(shape.visio.id));
        const el = byId.get(String(shape.visio.id));
        if (el && patchShape(el, shape, pg,
          groupLayout ? groupLayout.overrides.get(shape.id) : null,
          { media, pagePart, pictures, pg, modelToVisio })) changed = true;
      }
    }
    for (const conn of pg.connectors) {
      if (conn.visio && conn.visio.id) {
        modelToVisio.set(conn.id, String(conn.visio.id));
        const el = byId.get(String(conn.visio.id));
        if (el && patchConnector(el, conn)) changed = true;
      }
    }
  }

  // New 2-D shapes get inline Geometry so they do not need a master.
  // Container/list/lane/separator frames must sit below tasks. In patch mode,
  // find the last existing background shape and insert after it; a new lane
  // then lands above the pool/list but below tasks instead of covering them.
  const BG_ROLES = new Set(['pool', 'list', 'phase-list', 'lane', 'separator', 'container']);
  const bgIds = new Set();
  for (const s of pg.shapes) {
    if (s.containerInfo && BG_ROLES.has(s.containerInfo.role)) {
      const id = s.visio?.id ?? modelToVisio.get(s.id);
      if (id) bgIds.add(String(id));
    }
  }
  for (const g of pg.groups || []) {
    if (g.visioId && BG_ROLES.has(g.role)) bgIds.add(String(g.visioId));
  }
  let bgAnchor = null;
  for (const child of shapesRoot.children) {
    if (child.localName === 'Shape' && bgIds.has(child.getAttribute('ID'))) bgAnchor = child;
  }

  const newShapeSpecs = new Map();
  for (const shape of pg.shapes) {
    if (!allNew && shape.visio && shape.visio.id) continue;
    const id = nextId();
    const el = buildNewShape(doc, shape, id, pageH, allNew, {
      media, pagePart, pictures, pg, modelToVisio,
    });
    const structural = shape.containerInfo && BG_ROLES.has(shape.containerInfo.role);
    if (structural && !sortZ) {
      if (bgAnchor && bgAnchor.parentNode === shapesRoot) {
        shapesRoot.insertBefore(el, bgAnchor.nextSibling);
      } else {
        shapesRoot.insertBefore(el, shapesRoot.firstChild);
      }
      bgAnchor = el;
    } else {
      shapesRoot.appendChild(el);
    }
    modelToVisio.set(shape.id, id);
    newShapeSpecs.set(shape.id, { id, w: shape.w, h: shape.h, el });
    changed = true;
  }

  const rewriteConnects = new Set();
  const connectorEls = new Map();
  for (const conn of pg.connectors) {
    if (!allNew && conn.visio && conn.visio.id) {
      if (conn.visio.orig && conn.visio.orig.route !== undefined
          && connectorRouteChanged(conn, conn.visio.orig)
          && !(skipConnectorRoutes && skipConnectorRoutes.has(String(conn.visio.id)))) {
        const id = String(conn.visio.id);
        const el = byId.get(id);
        if (el) {
          rebuildConnector(el, conn, { pageH, shapesById });
          rewriteConnects.add(id);
          connectorEls.set(id, el);
          changed = true;
        }
      }
      continue;
    }
    const id = nextId();
    const el = buildNewConnector(doc, conn, id, { pageH, shapesById, layerMember: allNew });
    // The editor draws connectors below shapes (z = -1 default), so put new
    // connectors at the bottom of the page's shape stack too.
    shapesRoot.insertBefore(el, shapesRoot.firstChild);
    modelToVisio.set(conn.id, id);
    rewriteConnects.add(id);
    connectorEls.set(id, el);
    changed = true;
  }

  if (rewriteConnects.size) {
    if (applyConnects(doc, root, pg, {
      pageH, shapesById, modelToVisio, newShapeSpecs, byId, rewriteIds: rewriteConnects,
      connectorEls,
    })) changed = true;
  }

  if (pg.containers && pg.containers.length
      && (!allNew || pg.containers.some((c) => c.authored || c.modelId))) {
    if (applyMemberships(pg, { byId, modelToVisio, newShapeSpecs, connectorEls })) changed = true;
  }

  if (sortZ) reorderPageShapes(shapesRoot, pg, modelToVisio);
  return changed;
}

function reorderPageShapes(shapesRoot, pg, modelToVisio) {
  const byId = new Map();
  for (const el of [...shapesRoot.children]) {
    if (el.localName !== 'Shape') continue;
    const id = el.getAttribute('ID');
    if (id !== null) byId.set(String(id), el);
  }
  const order = [];
  pg.shapes.forEach((s, i) => {
    const id = modelToVisio.get(s.id);
    if (id && byId.has(String(id))) order.push({ z: s.z ?? 1000 + i, id: String(id) });
  });
  pg.connectors.forEach((c) => {
    const id = modelToVisio.get(c.id);
    if (id && byId.has(String(id))) order.push({ z: c.z ?? -1, id: String(id) });
  });
  order.sort((a, b) => a.z - b.z);
  for (const item of order) shapesRoot.appendChild(byId.get(item.id));
}

function patchShape(el, shape, pg, layout = null, ctx = null) {
  const o = shape.visio && shape.visio.orig;
  if (!o) return false;
  let changed = false;

  const wIn = shape.w / DPI;
  const hIn = shape.h / DPI;
  const locX = locPin(el, 'LocPinX', 'Width', wIn, o.w / DPI);
  const locY = locPin(el, 'LocPinY', 'Height', hIn, o.h / DPI);
  const pageH = (pg.visio && pg.visio.pageH) || 0;
  // Nested group members store PinX/PinY in their parent's local space. The
  // importer's world box plus that parent origin gives the original space;
  // group layout can override the origin after a frame resize/move.
  const ox = layout && Number.isFinite(layout.ox)
    ? layout.ox : ((shape.visio.origin && shape.visio.origin.x) || 0);
  const oy = layout && Number.isFinite(layout.oy)
    ? layout.oy : ((shape.visio.origin && shape.visio.origin.y) || 0);
  const skipLayout = !!(layout && layout.skip);
  const forceLayout = !!(layout && layout.force);
  // A data callout that follows its target keeps Visio's Pin formulas; only
  // its cached value is refreshed. Free or unlinked callouts are static.
  const calloutInfo = shape.callout ? calloutCtx(shape, ctx) : null;
  const pinFollows = !!(shape.callout && (shape.callout.position || 'top') !== 'free'
    && calloutInfo && calloutFollowsTarget(calloutInfo.target, calloutInfo.targetVisioId));
  const setPin = pinFollows ? setCellV : setCell;

  if (!skipLayout && (forceLayout || numDiff(shape.x, o.x) || numDiff(shape.w, o.w))) {
    setPin(el, 'PinX', fmt(shape.x / DPI - ox + locX));
    changed = true;
  }
  if (!skipLayout && (forceLayout || numDiff(shape.y, o.y) || numDiff(shape.h, o.h))) {
    setPin(el, 'PinY', fmt(pageH - (shape.y + shape.h) / DPI + locY - oy));
    changed = true;
  }
  if (!skipLayout && numDiff(shape.w, o.w)) {
    setCell(el, 'Width', fmt(wIn));
    setCellV(el, 'LocPinX', fmt(locX));
    changed = true;
  }
  if (!skipLayout && numDiff(shape.h, o.h)) {
    setCell(el, 'Height', fmt(hIn));
    setCellV(el, 'LocPinY', fmt(locY));
    changed = true;
  }
  // A container/pool title is anchored to the frame; when the frame grows the
  // text block must follow it, or the pool title slides behind the first lane.
  // Pictures are different: their Txt* cells are already frame-relative
  // formulas (Width*1, PinY=0), so leave them for Visio to recalculate.
  if (!shape.image && !skipLayout && shape.textBox
      && (numDiff(shape.w, o.w) || numDiff(shape.h, o.h))) {
    const tb = shape.textBox;
    const tw = Math.max(0.01, tb.w / DPI);
    const th = Math.max(0.01, tb.h / DPI);
    setCellV(el, 'TxtWidth', fmt(tw));
    setCellV(el, 'TxtHeight', fmt(th));
    setCell(el, 'TxtPinX', fmt((tb.x + tb.w / 2) / DPI));
    setCell(el, 'TxtPinY', fmt(hIn - (tb.y + tb.h / 2) / DPI));
    setCellV(el, 'TxtLocPinX', fmt(tw / 2));
    setCellV(el, 'TxtLocPinY', fmt(th / 2));
    changed = true;
  }
  if (numDiff(shape.rot || 0, o.rot || 0)) {
    setCell(el, 'Angle', fmt(-(shape.rot || 0) * Math.PI / 180));
    changed = true;
  }
  if (!!shape.flipX !== !!o.flipX) { setCell(el, 'FlipX', shape.flipX ? 1 : 0); changed = true; }
  if (!!shape.flipY !== !!o.flipY) { setCell(el, 'FlipY', shape.flipY ? 1 : 0); changed = true; }
  if (shape.image && patchImage(el, shape, o, ctx)) changed = true;

  if (!shape.callout && strDiff(shape.label || '', o.label || '')) {
    if (shape.rich?.paragraphs?.length && richPlain(shape.rich) === (shape.label || '')) {
      setRichText(el, shape);
    } else {
      setText(el, shape.label || '');
    }
    changed = true;
  }
  if (patchContainerRows(el, shape)) changed = true;
  if (JSON.stringify(shape.data || []) !== JSON.stringify(shape.visio?.origData || [])) {
    setPropertyData(el, shape);
    changed = true;
  }
  if (JSON.stringify(shape.hyperlinks || []) !== JSON.stringify(shape.visio?.origLinks || [])) {
    setHyperlinks(el, shape);
    changed = true;
  }
  if (applyTextFit(el, shape, shape.visio?.origTextFit || null)) changed = true;

  if (strDiff(shape.align || 'center', o.align || 'center')) {
    setRowCell(el, 'Paragraph', '0', 'HorzAlign', alignNumber(shape.align));
    changed = true;
  }
  if (strDiff(shape.vAlign || 'middle', o.vAlign || 'middle')) {
    setCell(el, 'VerticalAlign', vAlignNumber(shape.vAlign));
    changed = true;
  }
  if (numDiff(shape.fontSize, o.fontSize)) {
    setRowCell(el, 'Character', '0', 'Size', fmt(shape.fontSize / DPI));
    changed = true;
  }
  if (strDiff(shape.textColor, o.textColor)) {
    setRowCell(el, 'Character', '0', 'Color', validColor(shape.textColor, '#1b2733'));
    changed = true;
  }
  if (!!shape.bold !== !!o.bold
      || !!shape.italic !== !!o.italic
      || !!shape.underline !== !!o.underline) {
    setCharacterStyle(el, (shape.bold ? 1 : 0) | (shape.italic ? 2 : 0) | (shape.underline ? 4 : 0));
    changed = true;
  }

  // Per-part geometry carries its own fill/stroke; a top-level fill edit
  // cannot be mapped onto it (and the editor's renderer ignores it too).
  // A colour rule owns FillForegnd, so skip the generic fill diff while active.
  if (!o.custom && !shape.dataColor) {
    if (strDiff(shape.fill, o.fill)) {
      if (!shape.fill || shape.fill === 'none') {
        setCell(el, 'FillPattern', 0);
      } else {
        setCell(el, 'FillForegnd', shape.fill);
        setCell(el, 'FillPattern', 1);
      }
      setCell(el, 'FillGradientEnabled', 0);
      changed = true;
    }
    const strokeChanged = strDiff(shape.stroke, o.stroke);
    const widthChanged = numDiff(shape.strokeWidth, o.strokeWidth);
    const styleChanged = strDiff(shape.strokeStyle, o.strokeStyle);
    if (strokeChanged) {
      setCell(el, 'LineColor', validColor(shape.stroke, '#000000'));
      setCell(el, 'LineGradientEnabled', 0);
      changed = true;
    }
    if (widthChanged) {
      setCell(el, 'LineWeight', fmt((shape.strokeWidth || 0) / DPI));
      changed = true;
    }
    if (widthChanged || styleChanged || strokeChanged) {
      patchLinePattern(el, shape, widthChanged, styleChanged, strokeChanged);
      changed = true;
    }
  }

  if (shape.callout && (shape.callout.kind === 'bar' || shape.callout.kind === 'icon')
      && (numDiff(shape.w, o.w) || numDiff(shape.h, o.h))) {
    if (shape.callout.kind === 'bar') {
      writeDataBarGeometry(el.ownerDocument, el, shape, shape.w, shape.h, ctx);
    } else {
      writeDataIconGeometry(el.ownerDocument, el, shape, shape.w, shape.h, ctx);
    }
    changed = true;
  }
  if (patchDataColor(el, shape, o)) changed = true;
  if (shape.callout && patchCallout(el, shape, o, pg, ctx, wIn, hIn)) changed = true;

  return changed;
}

// -------------------------------------------------------------- pictures --
function imageState(img = {}) {
  return {
    mediaId: img.mediaId ?? null,
    scaleX: img.scaleX ?? 1,
    scaleY: img.scaleY ?? 1,
    offsetX: img.offsetX ?? 0,
    offsetY: img.offsetY ?? 0,
    brightness: img.brightness ?? 0,
    contrast: img.contrast ?? 0,
    gamma: img.gamma ?? 1,
    transparency: img.transparency ?? 0,
  };
}

/** ForeignType/CompressionType for a media record, as Visio writes them. */
function applyForeignType(fd, rec) {
  const ext = mediaExtOf(rec);
  fd.setAttribute('ForeignType', ext === 'emf' || ext === 'wmf' ? 'MetaFile' : 'Bitmap');
  if (ext === 'png') fd.setAttribute('CompressionType', 'PNG');
  else if (ext === 'gif') fd.setAttribute('CompressionType', 'GIF');
  else if (ext === 'jpeg') fd.setAttribute('CompressionType', 'JPEG');
  else fd.removeAttribute('CompressionType');
}

function setForeignRel(el, rid) {
  let fd = direct(el, 'ForeignData');
  if (!fd) {
    fd = el.ownerDocument.createElementNS(NS, 'ForeignData');
    fd.setAttribute('ForeignType', 'Bitmap');
    el.insertBefore(fd, direct(el, 'Text'));
  }
  let rel = [...fd.children].find((n) => n.localName === 'Rel');
  if (!rel) {
    rel = el.ownerDocument.createElementNS(NS, 'Rel');
    fd.appendChild(rel);
  }
  rel.setAttributeNS(REL_NS, 'r:id', rid);
}

/**
 * Patch an imported picture. The normal layout/text/fill cells are handled by
 * patchShape; this adds the picture's own cells and, when the image was
 * replaced, a new media part plus a ForeignData relationship.
 */
function patchImage(el, shape, o, ctx) {
  const now = imageState(shape.image);
  const before = imageState(o.image || {});
  before.wIn = Math.max(1e-6, (o.w ?? shape.w) / DPI);
  before.hIn = Math.max(1e-6, (o.h ?? shape.h) / DPI);
  let changed = false;

  if (ctx && ctx.media && ctx.pagePart && now.mediaId !== before.mediaId) {
    const rec = ctx.pictures && ctx.pictures[now.mediaId];
    const rid = rec ? ctx.media.add(ctx.pagePart, rec) : null;
    if (rid) {
      setForeignRel(el, rid);
      const fd = direct(el, 'ForeignData');
      if (fd) applyForeignType(fd, rec);
      changed = true;
    }
  }

  const wIn = Math.max(1e-6, shape.w / DPI);
  const hIn = Math.max(1e-6, shape.h / DPI);
  const cells = [
    ['offsetX', 'ImgOffsetX', wIn, 'Width', false],
    ['offsetY', 'ImgOffsetY', hIn, 'Height', false],
    ['scaleX', 'ImgWidth', wIn, 'Width', false],
    ['scaleY', 'ImgHeight', hIn, 'Height', false],
    ['brightness', 'Brightness', 1, null, true],
    ['contrast', 'Contrast', 1, null, true],
    ['gamma', 'Gamma', 1, null, false],
    ['transparency', 'Transparency', 1, null, false],
  ];
  for (const [key, cell, mul, ref, halfUnit] of cells) {
    if (!numDiff(now[key], before[key])) continue;
    // Brightness/Contrast are 0..1 cells with 0.5 neutral; the model stores
    // the adjustment as -1..1.
    const value = halfUnit ? (now[key] + 1) / 2 : now[key];
    if (ref) {
      setCellWithFormula(el, cell, fmt(now[key] * mul), `${ref}*${fmt(now[key])}`);
    } else {
      setCell(el, cell, fmt(value));
    }
    changed = true;
  }
  // A picture resized in the editor keeps ImgWidth/ImgHeight formulas such as
  // Width*1, but Visio draws from the cached V inside the shape. Refresh the
  // cached inches so the new frame shows the picture at the editor's scale. A
  // master-derived picture may have no direct Img* cells at all; then create
  // them on the instance when the new geometry differs from what it inherited.
  for (const [key, cell, mul, ref, beforeAbs] of [
    ['offsetX', 'ImgOffsetX', wIn, 'Width', before.offsetX * before.wIn],
    ['offsetY', 'ImgOffsetY', hIn, 'Height', before.offsetY * before.hIn],
    ['scaleX', 'ImgWidth', wIn, 'Width', before.scaleX * before.wIn],
    ['scaleY', 'ImgHeight', hIn, 'Height', before.scaleY * before.hIn],
  ]) {
    const target = now[key] * mul;
    const c = cellOf(el, cell);
    if (!c) {
      if (Math.abs(target - beforeAbs) <= 1e-4) continue;
      setCellWithFormula(el, cell, fmt(target), `${ref}*${fmt(now[key])}`);
      changed = true;
      continue;
    }
    const v = parseFloat(c.getAttribute('V'));
    if (Number.isFinite(v) && Math.abs(v - target) <= 1e-4) continue;
    setCellV(el, cell, fmt(target));
    changed = true;
  }
  return changed;
}

/** A new picture is an inline Type="Foreign" shape with no master. */
function writeForeignShape(doc, el, shape, wIn, hIn, ctx) {
  const img = imageState(shape.image);
  el.setAttribute('Type', 'Foreign');
  const fd = doc.createElementNS(NS, 'ForeignData');
  const rec = ctx && ctx.pictures && ctx.pictures[img.mediaId];
  if (rec) applyForeignType(fd, rec);
  else fd.setAttribute('ForeignType', shape.image.foreignType || 'Bitmap');
  const rid = rec && ctx.media && ctx.pagePart
    ? ctx.media.add(ctx.pagePart, rec) : null;
  if (rid) {
    const rel = doc.createElementNS(NS, 'Rel');
    rel.setAttributeNS(REL_NS, 'r:id', rid);
    fd.appendChild(rel);
  }
  el.insertBefore(fd, direct(el, 'Text'));

  // The picture's own geometry is a plain frame, not an unfilled path: Visio
  // keeps NoFill=0 on its pictures so the glyph itself is pickable.
  const geom = [...el.children].find(
    (n) => n.localName === 'Section' && n.getAttribute('N') === 'Geometry');
  if (geom) {
    const noFill = [...geom.children].find(
      (n) => n.localName === 'Cell' && n.getAttribute('N') === 'NoFill');
    if (noFill) noFill.setAttribute('V', '0');
  }

  setCell(el, 'ResizeMode', 0);
  setCellWithFormula(el, 'ClippingPath', '', '""');
  setCellWithFormula(el, 'ImgOffsetX', fmt(img.offsetX * wIn), `Width*${fmt(img.offsetX)}`);
  setCellWithFormula(el, 'ImgOffsetY', fmt(img.offsetY * hIn), `Height*${fmt(img.offsetY)}`);
  setCellWithFormula(el, 'ImgWidth', fmt(img.scaleX * wIn), `Width*${fmt(img.scaleX)}`);
  setCellWithFormula(el, 'ImgHeight', fmt(img.scaleY * hIn), `Height*${fmt(img.scaleY)}`);
  if (img.brightness) setCell(el, 'Brightness', fmt((img.brightness + 1) / 2));
  if (img.contrast) setCell(el, 'Contrast', fmt((img.contrast + 1) / 2));
  if (numDiff(img.gamma, 1)) setCell(el, 'Gamma', fmt(img.gamma));
  if (img.transparency) setCell(el, 'Transparency', fmt(img.transparency));
}

// ---------------------------------------------------------- data callouts --
function calloutCtx(shape, ctx) {
  const pg = ctx && ctx.pg;
  const target = pg && pg.shapes.find((s) => s.id === shape.attachedTo) || null;
  const targetVisioId = (ctx && ctx.modelToVisio && ctx.modelToVisio.get(shape.attachedTo))
    || (target && target.visio && target.visio.id) || null;
  return { pg, target, targetVisioId };
}

function calloutValueOf(target, field) {
  const row = target && (target.data || []).find((d) => String(d.name) === String(field));
  return row ? String(row.value ?? '') : '';
}

function barRatioOf(shape, target) {
  const raw = calloutValueOf(target, shape.callout.field)
    .replace(/[^0-9.+-]/g, '').trim();
  const value = parseFloat(raw);
  if (!Number.isFinite(value)) return 0;
  let min = Number(shape.callout.min);
  let max = Number(shape.callout.max);
  if (!Number.isFinite(min)) min = 0;
  if (!Number.isFinite(max)) max = 100;
  if (max === min) return value > max ? 1 : 0;
  const t = (value - min) / (max - min);
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

function barPercentFormula(shape, targetVisioId) {
  const min = Number.isFinite(Number(shape.callout.min)) ? Number(shape.callout.min) : 0;
  const max = Number.isFinite(Number(shape.callout.max)) ? Number(shape.callout.max) : 100;
  if (max === min) return '0';
  const T = `Sheet.${targetVisioId}`;
  return `IF(${fmt(max)}=${fmt(min)},0,MIN(1,MAX(0,(${T}!Prop.${shape.callout.field}-${fmt(min)})/(${fmt(max)}-${fmt(min)}))))`;
}

/** 1..5, the same five equal bands the editor renders. */
function iconLevelOf(shape, target) {
  const raw = calloutValueOf(target, shape.callout.field)
    .replace(/[^0-9.+-]/g, '').trim();
  const value = parseFloat(raw);
  if (!Number.isFinite(value)) return 1;
  let min = Number(shape.callout.min);
  let max = Number(shape.callout.max);
  if (!Number.isFinite(min)) min = 0;
  if (!Number.isFinite(max)) max = 100;
  if (max === min) return value > max ? 5 : 1;
  const t = (value - min) / (max - min);
  const clamped = t < 0 ? 0 : t > 1 ? 1 : t;
  return Math.max(1, Math.min(5, Math.floor(clamped * 5) + 1));
}

function iconLevelFormula(shape, targetVisioId) {
  if (!targetVisioId || !shape.callout.field) return null;
  const min = Number.isFinite(Number(shape.callout.min)) ? Number(shape.callout.min) : 0;
  const max = Number.isFinite(Number(shape.callout.max)) ? Number(shape.callout.max) : 100;
  const T = `Sheet.${targetVisioId}`;
  if (max === min) return `IF(${T}!Prop.${shape.callout.field}>${fmt(max)},5,1)`;
  return `MAX(1,MIN(5,INT(((${T}!Prop.${shape.callout.field}-${fmt(min)})`
    + `/(${fmt(max)}-${fmt(min)}))*5)+1))`;
}

// -------------------------------------------------------- colour by value --
function rgbFormula(color) {
  let hex = String(color || '').replace('#', '');
  if (hex.length === 3) hex = hex.split('').map((c) => c + c).join('');
  if (!/^[0-9a-f]{6}$/i.test(hex)) hex = 'ffffff';
  const n = parseInt(hex, 16);
  return `RGB(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255})`;
}

function dataColorColorOf(shape) {
  const raw = calloutValueOf(shape, shape.dataColor.field)
    .replace(/[^0-9.+-]/g, '').trim();
  const value = parseFloat(raw);
  if (!Number.isFinite(value)) return null;
  let min = Number(shape.dataColor.min);
  let max = Number(shape.dataColor.max);
  if (!Number.isFinite(min)) min = 0;
  if (!Number.isFinite(max)) max = 100;
  const low = shape.dataColor.low || '#f8696b';
  const mid = shape.dataColor.mid || '#ffeb84';
  const high = shape.dataColor.high || '#63be7b';
  if (max === min) return value >= max ? high : low;
  const t = (value - min) / (max - min);
  return t < 1 / 3 ? low : t < 2 / 3 ? mid : high;
}

function dataColorFormula(shape) {
  const field = shape.dataColor.field;
  const min = Number.isFinite(Number(shape.dataColor.min)) ? Number(shape.dataColor.min) : 0;
  const max = Number.isFinite(Number(shape.dataColor.max)) ? Number(shape.dataColor.max) : 100;
  const low = rgbFormula(shape.dataColor.low || '#f8696b');
  const mid = rgbFormula(shape.dataColor.mid || '#ffeb84');
  const high = rgbFormula(shape.dataColor.high || '#63be7b');
  const P = `Prop.${field}`;
  if (max === min) return `IF(${P}>=${fmt(max)},${high},${low})`;
  const span = `(${P}-${fmt(min)})/(${fmt(max)}-${fmt(min)})`;
  return `IF(${span}<0.3333333333,${low},IF(${span}<0.6666666667,${mid},${high}))`;
}

function removeDataColorRows(el) {
  const sec = directSection(el, 'User');
  if (!sec) return;
  for (const row of [...sec.children]) {
    const name = row.getAttribute('N') || '';
    if (name.startsWith('layoutcraftDGColor')) sec.removeChild(row);
  }
}

function writeDataColor(el, shape) {
  removeDataColorRows(el);
  addUserRow(el, 'layoutcraftDGColor', '1');
  addUserRow(el, 'layoutcraftDGColorField', shape.dataColor.field || '', 'STR');
  addUserRow(el, 'layoutcraftDGColorMin',
    Number.isFinite(Number(shape.dataColor.min)) ? shape.dataColor.min : 0);
  addUserRow(el, 'layoutcraftDGColorMax',
    Number.isFinite(Number(shape.dataColor.max)) ? shape.dataColor.max : 100);
  addUserRow(el, 'layoutcraftDGColorLow', shape.dataColor.low || '#f8696b', 'STR');
  addUserRow(el, 'layoutcraftDGColorMid', shape.dataColor.mid || '#ffeb84', 'STR');
  addUserRow(el, 'layoutcraftDGColorHigh', shape.dataColor.high || '#63be7b', 'STR');
  if (shape.dataColor.baseFill) {
    addUserRow(el, 'layoutcraftDGColorBase', shape.dataColor.baseFill, 'STR');
  }
  const color = dataColorColorOf(shape) || shape.fill || '#dbe9f6';
  setCellWithFormula(el, 'FillForegnd', color, dataColorFormula(shape));
  setCell(el, 'FillPattern', 1);
  setCell(el, 'FillGradientEnabled', 0);
}

function patchDataColor(el, shape, o) {
  const before = o.dataColor || null;
  if (!shape.dataColor) {
    if (!before) return false;
    removeDataColorRows(el);
    if (!shape.fill || shape.fill === 'none') {
      setCell(el, 'FillPattern', 0);
    } else {
      setCell(el, 'FillForegnd', shape.fill);
      setCell(el, 'FillPattern', 1);
    }
    setCell(el, 'FillGradientEnabled', 0);
    return true;
  }
  if (JSON.stringify(shape.dataColor) !== JSON.stringify(before)) {
    writeDataColor(el, shape);
    return true;
  }
  const cell = cellOf(el, 'FillForegnd');
  if (!cell || !String(cell.getAttribute('F') || '').includes(`Prop.${shape.dataColor.field}`)) {
    writeDataColor(el, shape);
    return true;
  }
  const color = dataColorColorOf(shape) || shape.fill || '#dbe9f6';
  if (String(cell.getAttribute('V') ?? '') !== color) {
    setCellV(el, 'FillForegnd', color);
    return true;
  }
  return false;
}

function removeGeometrySections(el) {
  for (const n of [...el.children]) {
    if (n.localName === 'Section' && n.getAttribute('N') === 'Geometry') el.removeChild(n);
  }
}

/**
 * A data bar is one shape with two Geometry sections: a filled body whose
 * right edge is a live formula over the target's Prop row, and a NoFill track
 * outline. `Width` is the full track, so the formula is Width*percent.
 */
function writeDataBarGeometry(doc, el, shape, wPx, hPx, ctx) {
  removeGeometrySections(el);
  const { target, targetVisioId } = calloutCtx(shape, ctx);
  const ratio = barRatioOf(shape, target);
  const barPx = Math.max(0.5, wPx * ratio);
  const bodyPath = `M0 0H${fmt(barPx)}V${fmt(hPx)}H0Z`;
  addGeometrySection(doc, el, 0, localSubpaths(bodyPath, wPx, hPx), false);
  const body = [...el.children].find(
    (n) => n.localName === 'Section' && n.getAttribute('N') === 'Geometry'
      && n.getAttribute('IX') === '0');
  if (body) {
    const noLine = [...body.children].find(
      (n) => n.localName === 'Cell' && n.getAttribute('N') === 'NoLine');
    if (noLine) noLine.setAttribute('V', '1');
    if (targetVisioId && shape.callout.field) {
      const pct = barPercentFormula(shape, targetVisioId);
      for (const c of body.getElementsByTagName('*')) {
        if (c.localName !== 'Cell' || c.getAttribute('N') !== 'X') continue;
        if (parseFloat(c.getAttribute('V')) > 1e-4) c.setAttribute('F', `Width*${pct}`);
      }
    }
  }
  const track = shapeDef('dataBar').path(wPx, hPx);
  addGeometrySection(doc, el, 1, localSubpaths(track, wPx, hPx), true);
}

/** One glyph of an icon row, in local pixel coordinates. */
function iconGlyphPath(set, x, y, c) {
  if (set === 'circles') {
    const n = 32;
    let d = '';
    for (let i = 0; i < n; i++) {
      const a = -Math.PI / 2 + (i * Math.PI * 2) / n;
      const px = x + c / 2 + Math.cos(a) * c / 2;
      const py = y + c / 2 + Math.sin(a) * c / 2;
      d += (i === 0 ? 'M' : 'L') + `${px.toFixed(3)} ${py.toFixed(3)}`;
    }
    return d + 'Z';
  }
  if (set === 'diamonds') {
    return `M${x + c / 2} ${y}L${x + c} ${y + c / 2}`
      + `L${x + c / 2} ${y + c}L${x} ${y + c / 2}Z`;
  }
  return `M${x} ${y}H${x + c}V${y + c}H${x}Z`;
}

function setGeometryCell(sec, name, value, formula = null) {
  if (!sec) return;
  let c = null;
  for (const n of sec.children) {
    if (n.localName === 'Cell' && n.getAttribute('N') === name) { c = n; break; }
  }
  if (!c) {
    c = sec.ownerDocument.createElementNS(sec.namespaceURI || NS, 'Cell');
    c.setAttribute('N', name);
    sec.insertBefore(c, sec.firstChild);
  }
  c.setAttribute('V', String(value));
  if (formula) c.setAttribute('F', formula);
  else c.removeAttribute('F');
}

/**
 * A five-level icon set is one inline shape: a NoFill outline row plus five
 * filled sections whose NoShow cells depend on a live level formula. Visio
 * therefore changes the icon when the target's property changes, exactly as
 * the editor's `iconLevel()` does.
 */
function writeDataIconGeometry(doc, el, shape, wPx, hPx, ctx) {
  removeGeometrySections(el);
  const { target, targetVisioId } = calloutCtx(shape, ctx);
  const level = iconLevelOf(shape, target);
  const set = shape.callout.iconSet || 'boxes';
  const pad = Math.max(0.5, Math.min(3, hPx * 0.06));
  const gap = Math.max(1, Math.min(4, hPx * 0.16));
  const cell = Math.max(1, Math.min(hPx - 2 * pad, (wPx - 2 * pad - 4 * gap) / 5));
  const total = cell * 5 + gap * 4;
  const x0 = (wPx - total) / 2;
  const y0 = (hPx - cell) / 2;
  const cellX = (i) => x0 + i * (cell + gap);

  let outline = '';
  for (let i = 0; i < 5; i++) outline += iconGlyphPath(set, cellX(i), y0, cell);
  const outSec = addGeometrySection(doc, el, 0,
    localSubpaths(outline, wPx, hPx), true);
  setGeometryCell(outSec, 'NoLine', 0);

  for (let i = 1; i <= 5; i++) {
    const sec = addGeometrySection(doc, el, i,
      localSubpaths(iconGlyphPath(set, cellX(i - 1), y0, cell), wPx, hPx), false);
    setGeometryCell(sec, 'NoLine', 1);
    setGeometryCell(sec, 'NoShow', level >= i ? 0 : 1,
      targetVisioId ? `IF(User.layoutcraftDGLevel>=${i},FALSE,TRUE)` : null);
  }

  setCell(el, 'HideText', 1);
  const row = setUserRow(el, 'layoutcraftDGLevel', level);
  const cellEl = [...row.children].find(
    (c) => c.localName === 'Cell' && c.getAttribute('N') === 'Value');
  const formula = iconLevelFormula(shape, targetVisioId);
  if (cellEl) {
    if (formula) cellEl.setAttribute('F', formula);
    else cellEl.removeAttribute('F');
  }
}

/**
 * A data callout's text is a field run against the target's Prop row. The
 * cached value makes it readable before Visio recalculates; the formula keeps
 * it live when the property changes.
 */
function setCalloutText(el, shape, ctx) {
  if (shape.callout.kind === 'icon') {
    setText(el, '');
    return;
  }
  const { target, targetVisioId } = calloutCtx(shape, ctx);
  const field = shape.callout.field || '';
  const value = calloutValueOf(target, field);
  const prefix = shape.callout.label || '';
  const runs = [];
  if (shape.callout.kind === 'bar' && shape.callout.showValue === false) {
    if (prefix) runs.push({ text: prefix });
  } else {
    if (prefix) runs.push({ text: `${prefix} ` });
    if (targetVisioId && field) {
      // Visio resolves a field from a Property row; a direct cross-sheet
      // Prop reference in the field is shown as "?". The row itself carries the
      // cross-sheet formula and the field simply reads Prop.msvCalloutField.
      runs.push({
        field: {
          formula: 'Prop.msvCalloutField',
          value,
          format: 'FIELDPICTURE(0)',
        },
      });
    } else {
      runs.push({ text: value });
    }
  }
  const rich = { paragraphs: [{ align: shape.align || 'center', runs }] };
  setRichText(el, { ...shape, rich });
}

function addOrGetRow(sec, name) {
  for (const row of sec.children) {
    if (row.localName === 'Row' && row.getAttribute('N') === name) return row;
  }
  const row = sec.ownerDocument.createElementNS(sec.namespaceURI || NS, 'Row');
  row.setAttribute('N', name);
  sec.appendChild(row);
  return row;
}

/** The callout's own Property row carries Sheet.<target>!Prop.<row>. */
function writeCalloutProperty(el, shape, target, targetVisioId) {
  const field = shape.callout.field || '';
  const value = calloutValueOf(target, field);
  let sec = directSection(el, 'Property');
  if (!sec) sec = ensureSection(el, 'Property');
  const row = addOrGetRow(sec, 'msvCalloutField');
  setRowCellV(row, 'Value', value);
  const valueCell = [...row.children].find(
    (c) => c.localName === 'Cell' && c.getAttribute('N') === 'Value');
  if (valueCell) {
    valueCell.setAttribute('U', 'STR');
    if (targetVisioId && field) {
      valueCell.setAttribute('F', `Sheet.${targetVisioId}!Prop.${field}`);
    } else {
      valueCell.removeAttribute('F');
    }
  }
  setRowCellV(row, 'Label', '');
  setRowCellV(row, 'SortKey', '');
  setRowCellV(row, 'Type', '0');
  setRowCellV(row, 'Invisible', '0');
  setRowCellV(row, 'Verify', '0');
  setRowCellV(row, 'DataLinked', '0');
  setRowCellV(row, 'LangID', 'en-GB');
  setRowCellV(row, 'Calendar', '0');
}

function removeCalloutUserRows(el) {
  const sec = directSection(el, 'User');
  if (!sec) return;
  for (const row of [...sec.children]) {
    const name = row.getAttribute('N') || '';
    if (name.startsWith('layoutcraftDG')) sec.removeChild(row);
  }
}

function writeCalloutUserRows(el, shape, target, targetVisioId) {
  removeCalloutUserRows(el);
  const kind = shape.callout.kind === 'bar' ? 'bar'
    : shape.callout.kind === 'icon' ? 'icon'
      : shape.callout.kind === 'heading' ? 'heading' : 'text';
  addUserRow(el, 'layoutcraftDG', '1');
  addUserRow(el, 'layoutcraftDGKind', kind, 'STR');
  if (targetVisioId) addUserRow(el, 'layoutcraftDGTarget', targetVisioId);
  addUserRow(el, 'layoutcraftDGField', shape.callout.field || '', 'STR');
  addUserRow(el, 'layoutcraftDGLabel', shape.callout.label || '', 'STR');
  addUserRow(el, 'layoutcraftDGPosition', shape.callout.position || 'top', 'STR');
  addUserRow(el, 'layoutcraftDGAuto', shape.callout.autoSize === false ? '0' : '1');
  if (shape.callout.kind === 'bar' || shape.callout.kind === 'icon') {
    addUserRow(el, 'layoutcraftDGMin',
      Number.isFinite(Number(shape.callout.min)) ? shape.callout.min : 0);
    addUserRow(el, 'layoutcraftDGMax',
      Number.isFinite(Number(shape.callout.max)) ? shape.callout.max : 100);
  }
  if (shape.callout.kind === 'bar') {
    addUserRow(el, 'layoutcraftDGBar', '1');
    addUserRow(el, 'layoutcraftDGShowValue', shape.callout.showValue === false ? '0' : '1');
  } else if (shape.callout.kind === 'icon') {
    addUserRow(el, 'layoutcraftDGIcon', '1');
    addUserRow(el, 'layoutcraftDGIconSet', shape.callout.iconSet || 'boxes', 'STR');
    addUserRow(el, 'layoutcraftDGLevel', iconLevelOf(shape, target));
    const formula = iconLevelFormula(shape, targetVisioId);
    if (formula) {
      const sec = directSection(el, 'User');
      const row = sec && [...sec.children].find(
        (r) => r.localName === 'Row' && r.getAttribute('N') === 'layoutcraftDGLevel');
      const value = row && [...row.children].find(
        (c) => c.localName === 'Cell' && c.getAttribute('N') === 'Value');
      if (value) value.setAttribute('F', formula);
    }
  }
}

function calloutFollowsTarget(target, targetVisioId) {
  if (!target || !targetVisioId) return false;
  const origin = target.visio && target.visio.origin;
  if (target.group
      || (origin && (Math.abs(origin.x) > 1e-6 || Math.abs(origin.y) > 1e-6))
      || Math.abs(target.rot || 0) > 1e-6) return false;
  return true;
}

/**
 * A non-free callout follows its target in Visio through Pin formulas.
 * Targets in a group/container or rotated groups fall back to static cells.
 */
function applyCalloutPin(el, shape, target, targetVisioId, wIn, hIn, pageH) {
  if (!target || !targetVisioId) return;
  const pos = shape.callout.position || 'top';
  const pinX = (shape.x + shape.w / 2) / DPI;
  const pinY = pageH - (shape.y + shape.h / 2) / DPI;
  if (pos === 'free' || !calloutFollowsTarget(target, targetVisioId)) {
    // Static cells: remove any previous formula so the editor's position wins.
    setCell(el, 'PinX', fmt(pinX));
    setCell(el, 'PinY', fmt(pinY));
    return;
  }
  const T = `Sheet.${targetVisioId}`;
  const cx = `${T}!PinX+(${T}!Width*0.5-${T}!LocPinX)`;
  const cy = `${T}!PinY+(${T}!Height*0.5-${T}!LocPinY)`;
  const gap = fmt((shape.callout.gap ?? 8) / DPI);
  let fx, fy;
  if (pos === 'top') { fx = cx; fy = `${cy}+${T}!Height*0.5+${gap}+Height*0.5`; }
  else if (pos === 'bottom') { fx = cx; fy = `${cy}-${T}!Height*0.5-${gap}-Height*0.5`; }
  else if (pos === 'left') { fx = `${cx}-${T}!Width*0.5-${gap}-Width*0.5`; fy = cy; }
  else { fx = `${cx}+${T}!Width*0.5+${gap}+Width*0.5`; fy = cy; }
  setCellWithFormula(el, 'PinX', fmt(pinX), fx);
  setCellWithFormula(el, 'PinY', fmt(pinY), fy);
  setCellWithFormula(el, 'LocPinX', fmt(wIn / 2), 'Width*0.5');
  setCellWithFormula(el, 'LocPinY', fmt(hIn / 2), 'Height*0.5');
}

function writeCalloutMeta(doc, el, shape, wIn, hIn, pageH, ctx) {
  const { target, targetVisioId } = calloutCtx(shape, ctx);
  writeCalloutUserRows(el, shape, target, targetVisioId);
  writeCalloutProperty(el, shape, target, targetVisioId);
  applyCalloutPin(el, shape, target, targetVisioId, wIn, hIn, pageH);
}

function refreshCalloutFieldValue(el, shape, ctx) {
  const { target } = calloutCtx(shape, ctx);
  if (!target) return false;
  const value = calloutValueOf(target, shape.callout.field);
  let changed = false;
  const propSec = directSection(el, 'Property');
  if (propSec) {
    const prow = [...propSec.children].find(
      (n) => n.localName === 'Row' && n.getAttribute('N') === 'msvCalloutField');
    const pcell = prow && [...prow.children].find(
      (c) => c.localName === 'Cell' && c.getAttribute('N') === 'Value');
    if (pcell && String(pcell.getAttribute('V') ?? '') !== value) {
      pcell.setAttribute('V', value);
      changed = true;
    }
  }
  const sec = directSection(el, 'Field');
  if (sec) {
    const row = [...sec.children].find((n) => n.localName === 'Row');
    const cell = row && [...row.children].find(
      (c) => c.localName === 'Cell' && c.getAttribute('N') === 'Value');
    if (cell && String(cell.getAttribute('V') ?? '') !== value) {
      cell.setAttribute('V', value);
      changed = true;
    }
    const t = direct(el, 'Text');
    if (changed && t) {
      for (const fld of [...t.children]) {
        if (fld.localName === 'fld') { fld.textContent = value; break; }
      }
    }
  }
  if (shape.callout.kind === 'icon') {
    const level = iconLevelOf(shape, target);
    const userSec = directSection(el, 'User');
    const levelRow = userSec && [...userSec.children].find(
      (n) => n.localName === 'Row' && n.getAttribute('N') === 'layoutcraftDGLevel');
    const levelCell = levelRow && [...levelRow.children].find(
      (c) => c.localName === 'Cell' && c.getAttribute('N') === 'Value');
    if (levelCell && Number(levelCell.getAttribute('V')) !== level) {
      levelCell.setAttribute('V', String(level));
      changed = true;
    }
    for (const sec of el.children) {
      if (sec.localName !== 'Section' || sec.getAttribute('N') !== 'Geometry') continue;
      const ix = Number(sec.getAttribute('IX'));
      if (!(ix >= 1)) continue;
      const cell = [...sec.children].find(
        (c) => c.localName === 'Cell' && c.getAttribute('N') === 'NoShow');
      if (!cell) continue;
      const want = level >= ix ? 0 : 1;
      if (Number(cell.getAttribute('V')) !== want) {
        cell.setAttribute('V', String(want));
        changed = true;
      }
    }
  }
  return changed;
}

function patchCallout(el, shape, o, pg, ctx, wIn, hIn) {
  const now = shape.callout;
  const before = o.callout || null;
  const attachedChanged = String(shape.attachedTo || '') !== String(o.attachedTo || '');
  if (!attachedChanged && JSON.stringify(now) === JSON.stringify(before)) {
    return refreshCalloutFieldValue(el, shape, ctx);
  }
  const { target, targetVisioId } = calloutCtx(shape, ctx);
  const pageH = (pg.visio && pg.visio.pageH) || 0;
  writeCalloutUserRows(el, shape, target, targetVisioId);
  writeCalloutProperty(el, shape, target, targetVisioId);
  setCalloutText(el, shape, ctx);
  if (shape.callout.kind === 'bar') {
    writeDataBarGeometry(el.ownerDocument, el, shape, wIn * DPI, hIn * DPI, ctx);
  } else if (shape.callout.kind === 'icon') {
    writeDataIconGeometry(el.ownerDocument, el, shape, wIn * DPI, hIn * DPI, ctx);
  }
  applyCalloutPin(el, shape, target, targetVisioId, wIn, hIn, pageH);
  return true;
}

function patchConnector(el, conn) {  const o = conn.visio && conn.visio.orig;
  if (!o) return false;
  let changed = false;

  if (strDiff(conn.label || '', o.label || '')) {
    setText(el, conn.label || '');
    changed = true;
  }
  const strokeChanged = strDiff(conn.stroke, o.stroke);
  const widthChanged = numDiff(conn.strokeWidth, o.strokeWidth);
  const styleChanged = strDiff(conn.strokeStyle, o.strokeStyle);
  if (strokeChanged) {
    setCell(el, 'LineColor', validColor(conn.stroke, '#000000'));
    setCell(el, 'LineGradientEnabled', 0);
    changed = true;
  }
  if (widthChanged) {
    setCell(el, 'LineWeight', fmt((conn.strokeWidth || 0) / DPI));
    changed = true;
  }
  if (widthChanged || styleChanged || strokeChanged) {
    patchLinePattern(el, conn, widthChanged, styleChanged, strokeChanged);
    changed = true;
  }
  if (strDiff(conn.arrowStart || 'none', o.arrowStart || 'none')) {
    setArrow(el, 'start', conn.arrowStart || 'none');
    changed = true;
  }
  if (strDiff(conn.arrowEnd || 'none', o.arrowEnd || 'none')) {
    setArrow(el, 'end', conn.arrowEnd || 'none');
    changed = true;
  }
  if (setConnectorConfig(el, conn, o, false)) changed = true;
  return changed;
}

function addUserRow(el, name, value, unit = null) {
  const d = el.ownerDocument;
  let sec = directSection(el, 'User');
  if (!sec) {
    sec = d.createElementNS(NS, 'Section');
    sec.setAttribute('N', 'User');
    el.appendChild(sec);
  }
  const row = d.createElementNS(NS, 'Row');
  row.setAttribute('N', name);
  const c = d.createElementNS(NS, 'Cell');
  c.setAttribute('N', 'Value');
  c.setAttribute('V', String(value));
  if (unit) c.setAttribute('U', unit);
  row.appendChild(c);
  sec.appendChild(row);
}

/** Set one User row on an existing shape, creating the row/section if needed. */
function setUserRow(el, name, value, unit = null) {
  const d = el.ownerDocument;
  let sec = directSection(el, 'User');
  if (!sec) {
    sec = d.createElementNS(NS, 'Section');
    sec.setAttribute('N', 'User');
    el.appendChild(sec);
  }
  let row = null;
  for (const r of sec.children) {
    if (r.localName === 'Row' && r.getAttribute('N') === name) { row = r; break; }
  }
  if (!row) {
    row = d.createElementNS(NS, 'Row');
    row.setAttribute('N', name);
    sec.appendChild(row);
  }
  let c = null;
  for (const x of row.children) {
    if (x.localName === 'Cell' && x.getAttribute('N') === 'Value') { c = x; break; }
  }
  if (!c) {
    c = d.createElementNS(NS, 'Cell');
    c.setAttribute('N', 'Value');
    row.insertBefore(c, row.firstChild);
  }
  c.setAttribute('V', String(value));
  c.removeAttribute('F');
  if (unit) c.setAttribute('U', unit);
  return row;
}

/**
 * Update the container/list cells that the editor can change after import:
 * lane headings, lane count, list direction and CFF orientation. Untouched
 * values are left exactly as the source wrote them.
 */
function patchContainerRows(el, shape) {
  const info = shape.containerInfo;
  if (!info) return false;
  const o = shape.visio?.origContainer;
  let changed = false;
  const heading = info.heading ?? shape.label ?? '';
  if (!o || heading !== (o.heading || '')) {
    setUserRow(el, 'visHeadingText', heading, 'STR');
    changed = true;
  }
  if (!o || info.numLanes !== o.numLanes) {
    setUserRow(el, 'numLanes', info.numLanes ?? 0);
    if (info.role === 'lane') {
      setUserRow(el, 'visCFFSettings',
        `/stg1=1, /stg2=1, /stg3=1, /stg4=${info.numLanes ?? 0}, /stg5=0.2500, /stg6=0`, 'STR');
    }
    changed = true;
  }
  if (info.kind === 'list' && (!o || info.listDirection !== o.listDirection)) {
    setUserRow(el, 'msvSDListDirection', info.listDirection ?? 0);
    changed = true;
  }
  if (info.role === 'separator' && (!o || info.listDirection !== o.listDirection)) {
    setUserRow(el, 'ListDirection', info.listDirection ?? 0);
    changed = true;
  }
  const vertical = info.direction === 'vertical' ? 1 : 0;
  if (!o || vertical !== (o.direction === 'vertical' ? 1 : 0)) {
    setUserRow(el, 'CFFVertical', vertical);
    changed = true;
  }
  return changed;
}

const CFF_LIST_GUID = '{0DDACF8F-D74C-4EDD-9757-5521A8DA1E73}';
const CFF_LANE_CATEGORIES = 'CFF Container;CFF List;Swimlane;Phase;NonCFF';

/**
 * Minimal container metadata accepted by Visio without a stencil master.
 * A plain inline rectangle with these User rows is reported by
 * Page.GetContainers and accepts ContainerProperties.AddMember.
 * The U="STR" units are required: without them Visio reads the
 * msvStructureType string as empty and ignores the container.
 *
 * CFF roles add the category and list rows Visio's container engine uses to
 * classify a pool, a lane list, a lane, a phase list and a phase separator.
 * All of these work on inline shapes; no stencil master is required.
 */
function addContainerUserRows(el, shape) {
  const info = shape.containerInfo || {};
  const role = info.role || 'container';
  const heading = info.heading ?? shape.label ?? '';
  const vertical = info.direction === 'vertical' ? 1 : 0;
  const laneDirection = info.listDirection ?? (vertical ? 0 : 2);
  const phaseDirection = info.phaseDirection ?? (vertical ? 2 : 0);
  const numLanes = info.numLanes ?? 0;
  const listGuid = info.listGuid || CFF_LIST_GUID;
  let rows;

  if (role === 'pool') {
    rows = [
      ['msvShapeCategories', 'CFF Container;DoNotContain', 'STR'],
      ['msvStructureType', 'Container', 'STR'],
      ['msvSDContainerRequiredCategories', 'CFF List', 'STR'],
      ['msvSDContainerExcludedCategories', '', 'STR'],
      ['msvSDContainerMargin', '0'],
      ['msvSDContainerResize', '2'],
      ['msvSDContainerNoHighlight', '1'],
      ['msvSDContainerNoRibbon', '1'],
      ['msvSDContainerStyle', '1'],
      ['msvSDContainerStyleCount', '6'],
      ['msvSDContainerLocked', '0'],
      ['visDirection', '0'],
      ['HeadingSide', '0'],
      ['RTL', '0'],
      ['visRotateLabel', '0'],
      ['visHeadingText', heading, 'STR'],
      ['visCFFStyle', '1'],
      ['CFFVertical', String(vertical)],
      ['numLanes', String(numLanes)],
      ['msvSDMembersOnHiddenLayer', '1'],
      ['visCFFHeadingPos', '2'],
      ['HeadingSpan', '0'],
      ['HeadingAlign', '0'],
      ['visHeadingHeight', '0.5', 'IN'],
      ['visShowTitle', '1'],
      ['visShowPhase', '1'],
      ['visMargins', '0.25'],
      ['visAutoExpand', '0'],
      ['visVersion', '15'],
    ];
  } else if (role === 'list' || role === 'phase-list') {
    const phase = role === 'phase-list';
    rows = [
      ['msvShapeCategories', 'CFF List;DoNotContain', 'STR'],
      ['msvStructureType', 'List', 'STR'],
      ...(phase ? [['msvSDContainerRequiredCategories', 'Phase', 'STR']] : []),
      ['msvSDListRequiredCategories', phase ? 'Phase' : 'Swimlane', 'STR'],
      ['msvSDContainerExcludedCategories', CFF_LANE_CATEGORIES, 'STR'],
      ['msvSDContainerMargin', '0'],
      ['msvSDContainerStyle', '1'],
      ['msvSDContainerLocked', '0'],
      ['msvSDContainerNoHighlight', '1'],
      ['msvSDContainerNoRibbon', '1'],
      ['msvSDContainerResize', '0'],
      ['msvSDListSpacing', '0'],
      ['msvSDListDirection', String(phase ? phaseDirection : laneDirection)],
      ['msvSDListAlignment', '0'],
      ['msvSDListItemMaster', '0'],
      ['msvSDListNoReorder', '0'],
      ['msvLayoutIncludeSubshapes', '0'],
      ['msvSDMembersOnHiddenLayer', '1'],
      ['visHeadingHeight', phase ? '0.25' : '0.5', 'IN'],
      ['visRotateLabel', '0'],
      ['visVersion', '15'],
    ];
  } else if (role === 'lane') {
    rows = [
      ['msvShapeCategories', 'Swimlane;DoNotContain', 'STR'],
      ['msvStructureType', 'Container', 'STR'],
      ['msvSDContainerMargin', '0.25', 'IN'],
      ['msvSDContainerResize', '0'],
      ['msvSDContainerNoHighlight', '0'],
      ['msvSDContainerNoRibbon', '1'],
      ['msvSDContainerStyle', '1'],
      ['msvSDContainerStyleCount', '6'],
      ['msvSDContainerRequiredCategories', '0', 'STR'],
      ['msvSDContainerExcludedCategories', CFF_LANE_CATEGORIES, 'STR'],
      ['visDirection', '1'],
      ['HeadingSide', '1'],
      ['RTL', '0'],
      ['visRotateLabel', '0'],
      ['visHeadingText', heading, 'STR'],
      ['visCFFStyle', '1'],
      ['msvSDContainerLocked', '0'],
      ['msvLayoutIncludeSubshapes', '0'],
      ['msvSDMembersOnHiddenLayer', '1'],
      ['SwimlaneListGUID', listGuid, 'GUID'],
      ['visCFFSettings', `/stg1=1, /stg2=1, /stg3=1, /stg4=${numLanes}, /stg5=0.2500, /stg6=0`, 'STR'],
      ['visCFFSingleLaneShowAll', '1'],
      ['LineWeight', '0.0173611', 'PT'],
      ['visVersion', '15'],
    ];
  } else if (role === 'separator') {
    rows = [
      ['msvShapeCategories', 'Phase;DoNotContain', 'STR'],
      ['msvStructureType', 'Container', 'STR'],
      ['msvSDContainerExcludedCategories', CFF_LANE_CATEGORIES, 'STR'],
      ['msvSDContainerMargin', '0.25', 'IN'],
      ['msvSDContainerResize', '0'],
      ['msvSDContainerNoHighlight', '1'],
      ['msvSDContainerNoRibbon', '1'],
      ['msvSDContainerStyle', '1'],
      ['msvSDContainerLocked', '0'],
      ['msvLayoutIncludeSubshapes', '0'],
      ['msvSDMembersOnHiddenLayer', '1'],
      ['ListDirection', String(phaseDirection)],
      ['visShowPhase', '1'],
      ['visHeadingText', heading, 'STR'],
      ['visCFFStyle', '1'],
      ['LineWeight', '0.0173611', 'PT'],
      ['RTL', '0'],
      ['CFFVertical', String(vertical)],
      ['visVersion', '15'],
    ];
  } else {
    rows = [
      ['msvShapeCategories', 'ContainerStyleDefaults', 'STR'],
      ['msvStructureType', 'Container', 'STR'],
      ['msvSDContainerStyle', '1'],
      ['msvSDContainerMargin', '0.1181102362204724', 'MM'],
      ['msvSDContainerResize', '1'],
      ['msvSDContainerLocked', '0'],
      ['msvSDContainerNoHighlight', '0'],
      ['msvSDContainerNoRibbon', '0'],
      ['msvSDContainerRequiredCategories', '0'],
      ['msvSDContainerExcludedCategories', 'DoNotContain', 'STR'],
      ['msvSDHeadingStyle', '1'],
      ['msvSDHeadingStyleCount', '4'],
      ['msvSDContainerStyleCount', '14'],
      ['visVersion', '15'],
    ];
  }
  for (const [name, value, unit] of rows) addUserRow(el, name, value, unit);
}

// ------------------------------------------------------------- authoring --

function ensureShapesRoot(doc, root) {
  let shapes = direct(root, 'Shapes');
  if (!shapes) {
    shapes = doc.createElementNS(NS, 'Shapes');
    root.insertBefore(shapes, root.firstChild);
  }
  return shapes;
}

function ensureConnectsRoot(doc, root) {
  let connects = direct(root, 'Connects');
  if (!connects) {
    connects = doc.createElementNS(NS, 'Connects');
    root.appendChild(connects);
  }
  return connects;
}

function directSection(el, name) {
  for (const c of el.children) {
    if (c.localName === 'Section' && c.getAttribute('N') === name) return c;
  }
  return null;
}

function buildNewShape(doc, shape, id, pageH, layerMember = true, ctx = null) {
  const el = doc.createElementNS(NS, 'Shape');
  el.setAttribute('ID', id);

  const wPx = Math.max(1, shape.w);
  const hPx = Math.max(1, shape.h);
  const wIn = wPx / DPI;
  const hIn = hPx / DPI;
  const left = shape.x / DPI;
  const bottom = pageH - (shape.y + hPx) / DPI;

  setCell(el, 'PinX', fmt(left + wIn / 2));
  setCell(el, 'PinY', fmt(bottom + hIn / 2));
  setCell(el, 'Width', fmt(wIn));
  setCell(el, 'Height', fmt(hIn));
  setCell(el, 'LocPinX', fmt(wIn / 2));
  setCell(el, 'LocPinY', fmt(hIn / 2));
  setCell(el, 'Angle', fmt(-(shape.rot || 0) * Math.PI / 180));
  setCell(el, 'FlipX', shape.flipX ? 1 : 0);
  setCell(el, 'FlipY', shape.flipY ? 1 : 0);
  setCell(el, 'ObjType', 1);
  if (layerMember) setCell(el, 'LayerMember', 0);

  setCell(el, 'FillGradientEnabled', 0);
  setCell(el, 'LineGradientEnabled', 0);
  if (!shape.fill || shape.fill === 'none') {
    setCell(el, 'FillPattern', 0);
  } else {
    setCell(el, 'FillForegnd', shape.fill);
    setCell(el, 'FillPattern', 1);
  }
  setCell(el, 'LineColor', validColor(shape.stroke, '#000000'));
  setCell(el, 'LineWeight', fmt((shape.strokeWidth || 0) / DPI));
  setCell(el, 'LinePattern', linePattern(shape));

  if (shape.containerInfo) addContainerUserRows(el, shape);

  const wPx0 = Math.max(1, shape.w);
  const hPx0 = Math.max(1, shape.h);
  const tb = shape.textBoxRel ? {
    x: shape.textBoxRel.x * wPx0, y: shape.textBoxRel.y * hPx0,
    w: shape.textBoxRel.w * wPx0, h: shape.textBoxRel.h * hPx0,
  } : (shape.textBox || textBoxFor(shape));
  const tw = Math.max(0.01, tb.w / DPI);
  const th = Math.max(0.01, tb.h / DPI);
  if (shape.image) {
    // Picture text blocks are frame-relative in Visio, so the caption follows
    // a later resize.
    setCellWithFormula(el, 'TxtWidth', fmt(tw), `Width*${fmt(tb.w / wPx0)}`);
    setCellWithFormula(el, 'TxtHeight', fmt(th), `Height*${fmt(tb.h / hPx0)}`);
    setCellWithFormula(el, 'TxtPinX', fmt((tb.x + tb.w / 2) / DPI),
      `Width*${fmt((tb.x + tb.w / 2) / wPx0)}`);
    setCellWithFormula(el, 'TxtPinY', fmt(hIn - (tb.y + tb.h / 2) / DPI),
      `Height*${fmt(1 - (tb.y + tb.h / 2) / hPx0)}`);
    setCellWithFormula(el, 'TxtLocPinX', fmt(tw / 2), 'TxtWidth*0.5');
    setCellWithFormula(el, 'TxtLocPinY', fmt(th / 2), 'TxtHeight*0.5');
  } else {
    setCell(el, 'TxtWidth', fmt(tw));
    setCell(el, 'TxtHeight', fmt(th));
    setCell(el, 'TxtPinX', fmt((tb.x + tb.w / 2) / DPI));
    setCell(el, 'TxtPinY', fmt(hIn - (tb.y + tb.h / 2) / DPI));
    setCell(el, 'TxtLocPinX', fmt(tw / 2));
    setCell(el, 'TxtLocPinY', fmt(th / 2));
  }
  setCell(el, 'TxtAngle', 0);
  setCell(el, 'LeftMargin', 0);
  setCell(el, 'RightMargin', 0);
  setCell(el, 'TopMargin', 0);
  setCell(el, 'BottomMargin', 0);
  setCell(el, 'VerticalAlign', vAlignNumber(shape.vAlign));
  applyTextFit(el, shape, null);

  patchTextStyle(el, shape, firstFont(shape.fontFamily));
  addConnectionSection(el, wIn, hIn);

  const parts = shape.parts || [];
  if (shape.callout && shape.callout.kind === 'bar') {
    writeDataBarGeometry(doc, el, shape, wPx, hPx, ctx);
  } else if (shape.callout && shape.callout.kind === 'icon') {
    writeDataIconGeometry(doc, el, shape, wPx, hPx, ctx);
  } else if (parts.length) {
    // Copied/imported per-part appearance is flattened into one inline
    // geometry; Visio cannot carry a different fill per Geometry section.
    const subpaths = [];
    for (const part of parts) {
      subpaths.push(...localSubpaths(part.d, wPx, hPx));
    }
    addGeometrySection(doc, el, 0, subpaths, !shape.fill || shape.fill === 'none');
  } else {
    const def = shapeDef(shape.type);
    addGeometrySection(doc, el, 0,
      localSubpaths(def.path(wPx, hPx), wPx, hPx),
      !shape.fill || shape.fill === 'none');
    const detail = shapeDetail(shape.type, wPx, hPx);
    if (detail) addGeometrySection(doc, el, 1, localSubpaths(detail, wPx, hPx), true);
  }

  if (shape.callout) setCalloutText(el, shape, ctx);
  else if (shape.rich && shape.rich.paragraphs?.length) setRichText(el, shape);
  else if (shape.label) setText(el, shape.label);
  if (shape.data?.length) setPropertyData(el, shape);
  if (shape.dataColor) writeDataColor(el, shape);
  if (shape.hyperlinks?.length) setHyperlinks(el, shape);
  if (shape.image) writeForeignShape(doc, el, shape, wIn, hIn, ctx);
  if (shape.callout) writeCalloutMeta(doc, el, shape, wIn, hIn, pageH, ctx);
  return el;
}

/** ShapeRouteStyle for the editor's route, when the model has no explicit value. */
function routeStyleFor(conn) {
  if (conn.routeStyle !== undefined) return conn.routeStyle;
  if (conn.route === 'straight') return 2;
  if (conn.route === 'elbow') return 1;
  return undefined;                       // curved has no native inline style
}

/**
 * Write/patch dynamic-connector configuration cells. `force` is used for a
 * new connector; patch mode only writes values that changed, so untouched
 * imported connectors keep their source formulas and cached values.
 */
function setConnectorConfig(el, conn, orig = null, force = false) {
  let changed = false;
  const setNum = (name, value, before) => {
    if (value !== undefined && value !== null) {
      if (force || before === undefined || Number(value) !== Number(before)) {
        setCell(el, name, fmt(value));
        changed = true;
      }
      return;
    }
    if (!force && before === undefined) return;
    const c = cellOf(el, name);
    if (c) { c.remove(); changed = true; }
  };
  let style = conn.routeStyle;
  if (style === undefined && (force || conn.route !== (orig?.route || conn.route))) {
    style = routeStyleFor(conn);
  }
  setNum('ShapeRouteStyle', style, orig?.routeStyle);
  setNum('ConFixedCode', conn.conFixedCode ?? (force ? 3 : undefined), orig?.conFixedCode);
  setNum('ConLineRouteExt', conn.lineRouteExt, orig?.lineRouteExt);
  setNum('WalkPreference', conn.walkPreference, orig?.walkPreference);
  setNum('ShapePlowCode', conn.plowCode, orig?.plowCode);
  setNum('GlueType', conn.glueType ?? (force ? 2 : undefined), orig?.glueType);
  setNum('DynFeedback', conn.dynFeedback ?? (force ? 2 : undefined), orig?.dynFeedback);
  return changed;
}

function buildNewConnector(doc, conn, id, ctx) {
  const el = doc.createElementNS(NS, 'Shape');
  el.setAttribute('ID', id);
  const geom = connectorGeometry(conn, ctx);

  setCell(el, 'PinX', fmt((geom.begin.x + geom.end.x) / 2));
  setCell(el, 'PinY', fmt((geom.begin.y + geom.end.y) / 2));
  setCell(el, 'Width', fmt(geom.dx));
  setCell(el, 'Height', fmt(geom.dy));
  setCell(el, 'LocPinX', fmt(geom.dx / 2));
  setCell(el, 'LocPinY', fmt(geom.dy / 2));
  setCell(el, 'BeginX', fmt(geom.begin.x));
  setCell(el, 'BeginY', fmt(geom.begin.y));
  setCell(el, 'EndX', fmt(geom.end.x));
  setCell(el, 'EndY', fmt(geom.end.y));
  setCell(el, 'Angle', 0);
  setCell(el, 'FlipX', 0);
  setCell(el, 'FlipY', 0);
  setCell(el, 'ObjType', 2);
  if (ctx.layerMember !== false) setCell(el, 'LayerMember', 0);
  setConnectorConfig(el, conn, null, true);

  setCell(el, 'FillPattern', 0);
  setCell(el, 'LineColor', validColor(conn.stroke, '#000000'));
  setCell(el, 'LineWeight', fmt((conn.strokeWidth || 0) / DPI));
  setCell(el, 'LinePattern', linePattern(conn));
  setCell(el, 'LineGradientEnabled', 0);
  setCell(el, 'BeginArrow', ARROW_VALUE[conn.arrowStart || 'none'] ?? 0);
  setCell(el, 'EndArrow', ARROW_VALUE[conn.arrowEnd || 'none'] ?? 0);

  const tw = 1;
  const th = 0.25;
  setCell(el, 'TxtWidth', fmt(tw));
  setCell(el, 'TxtHeight', fmt(th));
  setCell(el, 'TxtPinX', fmt(geom.mid.x));
  setCell(el, 'TxtPinY', fmt(geom.mid.y));
  setCell(el, 'TxtLocPinX', fmt(tw / 2));
  setCell(el, 'TxtLocPinY', fmt(th / 2));
  setCell(el, 'TxtAngle', 0);
  setCell(el, 'LeftMargin', 0);
  setCell(el, 'RightMargin', 0);
  setCell(el, 'TopMargin', 0);
  setCell(el, 'BottomMargin', 0);
  setCell(el, 'VerticalAlign', 1);

  patchTextStyle(el, {
    fontSize: 11,
    textColor: validColor(conn.stroke, '#1b2733'),
    bold: false, italic: false, underline: false,
    align: 'center',
    fontFamily: 'Calibri',
  }, 'Calibri');
  addGeometrySection(doc, el, 0, [{ points: geom.local, closed: false }], true);
  if (conn.label) setText(el, conn.label);
  return el;
}

function rebuildConnector(el, conn, ctx) {
  const geom = connectorGeometry(conn, ctx);
  setCell(el, 'PinX', fmt((geom.begin.x + geom.end.x) / 2));
  setCell(el, 'PinY', fmt((geom.begin.y + geom.end.y) / 2));
  setCell(el, 'Width', fmt(geom.dx));
  setCell(el, 'Height', fmt(geom.dy));
  setCell(el, 'LocPinX', fmt(geom.dx / 2));
  setCell(el, 'LocPinY', fmt(geom.dy / 2));
  setCell(el, 'BeginX', fmt(geom.begin.x));
  setCell(el, 'BeginY', fmt(geom.begin.y));
  setCell(el, 'EndX', fmt(geom.end.x));
  setCell(el, 'EndY', fmt(geom.end.y));
  setCell(el, 'Angle', 0);
  setCell(el, 'FlipX', 0);
  setCell(el, 'FlipY', 0);
  setCell(el, 'ObjType', 2);
  setConnectorConfig(el, conn, conn.visio?.orig || null, false);

  for (const c of [...el.children]) {
    if (c.localName === 'Section' && c.getAttribute('N') === 'Geometry') el.removeChild(c);
  }
  addGeometrySection(el.ownerDocument, el, 0,
    [{ points: geom.local, closed: false }], true);

  if (conn.label) {
    const tw = cellNumber(el, 'TxtWidth', 1) || 1;
    const th = cellNumber(el, 'TxtHeight', 0.25) || 0.25;
    setCell(el, 'TxtWidth', fmt(tw));
    setCell(el, 'TxtHeight', fmt(th));
    setCell(el, 'TxtPinX', fmt(geom.mid.x));
    setCell(el, 'TxtPinY', fmt(geom.mid.y));
    setCell(el, 'TxtLocPinX', fmt(tw / 2));
    setCell(el, 'TxtLocPinY', fmt(th / 2));
  }
  return true;
}

function connectorGeometry(conn, ctx) {
  const route = routeConnector(conn, ctx.shapesById);
  let world = route.points || [];
  if (route.kind === 'curve' && route.d) {
    const flat = flattenPath(route.d, 16);
    if (flat.length === 1) world = flat[0].points;
  }
  if (world.length < 2) world = route.points || [];
  if (world.length < 2) {
    const a = conn.from?.point || { x: 0, y: 0 };
    const b = conn.to?.point || a;
    world = [a, b];
  }
  const pageH = ctx.pageH;
  const page = world.map((p) => ({ x: p.x / DPI, y: pageH - p.y / DPI }));
  const begin = page[0];
  const end = page[page.length - 1];
  const dx = end.x - begin.x;
  const dy = end.y - begin.y;
  const local = page.map((p) => ({ x: p.x - begin.x, y: p.y - begin.y }));
  const midWorld = routeMidpoint(route);
  const midPage = { x: midWorld.x / DPI, y: pageH - midWorld.y / DPI };
  return {
    route, begin, end, dx, dy, local,
    mid: { x: midPage.x - begin.x, y: midPage.y - begin.y },
  };
}

function localSubpaths(d, wPx, hPx) {
  return flattenPath(d).map((sub) => ({
    closed: sub.closed,
    points: sub.points.map((p) => ({ x: p.x / DPI, y: (hPx - p.y) / DPI })),
  }));
}

function addGeometrySection(doc, shapeEl, ix, subpaths, noFill) {
  const sec = doc.createElementNS(NS, 'Section');
  sec.setAttribute('N', 'Geometry');
  sec.setAttribute('IX', String(ix));
  for (const [name, value] of [
    ['NoFill', noFill ? 1 : 0],
    ['NoLine', 0],
    ['NoShow', 0],
    ['NoSnap', 0],
    ['NoQuickDrag', 0],
  ]) {
    const c = doc.createElementNS(NS, 'Cell');
    c.setAttribute('N', name);
    c.setAttribute('V', String(value));
    sec.appendChild(c);
  }
  let rowIx = 1;
  for (const sub of subpaths) {
    if (!sub.points.length) continue;
    const move = doc.createElementNS(NS, 'Row');
    move.setAttribute('T', 'MoveTo');
    move.setAttribute('IX', String(rowIx++));
    appendPointCells(doc, move, sub.points[0]);
    sec.appendChild(move);
    for (let i = 1; i < sub.points.length; i++) {
      const row = doc.createElementNS(NS, 'Row');
      row.setAttribute('T', 'LineTo');
      row.setAttribute('IX', String(rowIx++));
      appendPointCells(doc, row, sub.points[i]);
      sec.appendChild(row);
    }
    if (sub.closed) {
      const first = sub.points[0];
      const last = sub.points[sub.points.length - 1];
      if (Math.hypot(first.x - last.x, first.y - last.y) > 1e-6) {
        const row = doc.createElementNS(NS, 'Row');
        row.setAttribute('T', 'LineTo');
        row.setAttribute('IX', String(rowIx++));
        appendPointCells(doc, row, first);
        sec.appendChild(row);
      }
    }
  }
  const before = [...shapeEl.children].find(
    (n) => n.localName === 'Text' || n.localName === 'Shapes'
      || n.localName === 'ForeignData' || /^Data\d+$/.test(n.localName));
  shapeEl.insertBefore(sec, before || null);
  return sec;
}

function appendPointCells(doc, row, p) {
  for (const [name, value] of [['X', p.x], ['Y', p.y]]) {
    const c = doc.createElementNS(NS, 'Cell');
    c.setAttribute('N', name);
    c.setAttribute('V', fmt(value));
    row.appendChild(c);
  }
}

function addConnectionSection(shapeEl, wIn, hIn) {
  const doc = shapeEl.ownerDocument;
  const sec = doc.createElementNS(NS, 'Section');
  sec.setAttribute('N', 'Connection');
  const pts = [
    { x: wIn / 2, y: hIn }, // n
    { x: wIn, y: hIn / 2 }, // e
    { x: wIn / 2, y: 0 },   // s
    { x: 0, y: hIn / 2 },   // w
  ];
  pts.forEach((p, ix) => {
    const row = doc.createElementNS(NS, 'Row');
    row.setAttribute('T', 'Connection');
    row.setAttribute('IX', String(ix));
    for (const [name, value] of [['X', p.x], ['Y', p.y], ['DirX', 0], ['DirY', 0], ['Type', 0]]) {
      const c = doc.createElementNS(NS, 'Cell');
      c.setAttribute('N', name);
      c.setAttribute('V', fmt(value));
      row.appendChild(c);
    }
    sec.appendChild(row);
  });
  const before = [...shapeEl.children].find(
    (n) => n.localName === 'Text' || n.localName === 'Shapes' || n.localName === 'ForeignData');
  shapeEl.insertBefore(sec, before || null);
}

function patchTextStyle(el, shape, fontName) {
  setRowCell(el, 'Character', '0', 'Font', fontName);
  setRowCell(el, 'Character', '0', 'ComplexScriptFont', fontName);
  setRowCell(el, 'Character', '0', 'Color', validColor(shape.textColor, '#1b2733'));
  setRowCell(el, 'Character', '0', 'Size', fmt((shape.fontSize || 12) / DPI));
  setCharacterStyle(el, (shape.bold ? 1 : 0) | (shape.italic ? 2 : 0) | (shape.underline ? 4 : 0));
  setRowCell(el, 'Paragraph', '0', 'HorzAlign', alignNumber(shape.align));
}

/** Visio's autosize text-block formulas. */
function applyTextFit(el, shape, was = null) {
  const tf = shape.textFit;
  const changed = JSON.stringify(tf || null) !== JSON.stringify(was || null);
  if (!changed) return false;
  if (tf && (!cellOf(el, 'TxtWidth') || !cellOf(el, 'TxtHeight'))) {
    const tb = shape.textBox || { x: 0, y: 0, w: shape.w, h: shape.h };
    const tw = Math.max(0.01, tb.w / DPI);
    const th = Math.max(0.01, tb.h / DPI);
    setCellV(el, 'TxtWidth', fmt(tw));
    setCellV(el, 'TxtHeight', fmt(th));
    if (!cellOf(el, 'TxtPinX')) setCell(el, 'TxtPinX', fmt((tb.x + tb.w / 2) / DPI));
    if (!cellOf(el, 'TxtPinY')) setCell(el, 'TxtPinY', fmt(shape.h / DPI - (tb.y + tb.h / 2) / DPI));
    setCellV(el, 'TxtLocPinX', fmt(tw / 2));
    setCellV(el, 'TxtLocPinY', fmt(th / 2));
  }
  const widthF = tf?.width
    ? (tf.height ? 'TEXTWIDTH(TheText,TxtHeight)' : 'TEXTWIDTH(TheText)') : null;
  const heightF = tf?.height ? 'TEXTHEIGHT(TheText,TxtWidth)' : null;
  for (const [name, formula] of [['TxtWidth', widthF], ['TxtHeight', heightF]]) {
    const c = cellOf(el, name);
    if (!c) continue;
    if (formula) c.setAttribute('F', formula);
    else c.removeAttribute('F');
  }
  for (const [name, formula] of [['TxtLocPinX', tf?.width ? 'TxtWidth*0.5' : null],
    ['TxtLocPinY', tf?.height ? 'TxtHeight*0.5' : null]]) {
    const c = cellOf(el, name);
    if (!c) continue;
    if (formula) c.setAttribute('F', formula);
    else c.removeAttribute('F');
  }
  return true;
}

function textBoxFor(shape) {
  try {
    return shapeTextBox(shape.type, Math.max(1, shape.w), Math.max(1, shape.h));
  } catch (err) {
    return { x: 0, y: 0, w: shape.w, h: shape.h };
  }
}

function firstFont(f) {
  const s = String(f || '');
  const quoted = /^\s*"([^"]+)"/.exec(s);
  if (quoted && quoted[1] !== 'none') return quoted[1];
  const first = s.split(',')[0].replace(/["']/g, '').trim();
  return first && first !== 'none' ? first : 'Calibri';
}

function applyConnects(doc, root, pg, ctx) {
  const connects = ensureConnectsRoot(doc, root);
  for (const c of [...all(connects, 'Connect')]) {
    if (ctx.rewriteIds.has(String(c.getAttribute('FromSheet')))) connects.removeChild(c);
  }

  for (const conn of pg.connectors) {
    const sheetId = ctx.modelToVisio.get(conn.id);
    if (!sheetId || !ctx.rewriteIds.has(String(sheetId))) continue;
    const geom = connectorGeometry(conn, ctx);
    const startWorld = {
      x: geom.begin.x * DPI,
      y: (ctx.pageH - geom.begin.y) * DPI,
    };
    const endWorld = {
      x: geom.end.x * DPI,
      y: (ctx.pageH - geom.end.y) * DPI,
    };
    const a = resolveGlue(conn.from, startWorld, ctx);
    const b = resolveGlue(conn.to, endWorld, ctx);
    if (a) addConnect(doc, connects, sheetId, 'BeginX', '9', a);
    if (b) addConnect(doc, connects, sheetId, 'EndX', '12', b);
    const el = ctx.connectorEls && ctx.connectorEls.get(String(sheetId));
    if (el) applyConnectorGlueCells(el, sheetId, a, b);
  }
  return true;
}

/**
 * A 1-D shape with a <Connect> row is only treated as glued by Visio when the
 * glued end's X/Y cells carry the dynamic-glue formula. With plain values the
 * connector opens as a loose line and its endpoints do not follow the target.
 * A glued end uses _WALKGLUE for both coordinates; an unglued end keeps its
 * static value and its trigger references the connector itself.
 */
function applyConnectorGlueCells(el, sheetId, a, b) {
  const self = String(sheetId);
  const beginFormula = '_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)';
  const endFormula = '_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)';

  for (const name of ['BeginX', 'BeginY']) {
    if (a) setCellFormula(el, name, beginFormula);
    else clearCellFormula(el, name);
  }
  for (const name of ['EndX', 'EndY']) {
    if (b) setCellFormula(el, name, endFormula);
    else clearCellFormula(el, name);
  }

  if (a || b) {
    setCellWithFormula(el, 'BegTrigger', 2,
      `_XFTRIGGER(Sheet.${a ? a.sheetId : self}!EventXFMod)`);
    setCellWithFormula(el, 'EndTrigger', 2,
      `_XFTRIGGER(Sheet.${b ? b.sheetId : self}!EventXFMod)`);
  } else {
    // Both ends are free points: drop any trigger left over from a previous
    // dynamic state so the connector stays a plain line.
    removeCell(el, 'BegTrigger');
    removeCell(el, 'EndTrigger');
  }
}

function addConnect(doc, connects, fromSheet, fromCell, fromPart, target) {
  const c = doc.createElementNS(NS, 'Connect');
  c.setAttribute('FromSheet', String(fromSheet));
  c.setAttribute('FromCell', fromCell);
  c.setAttribute('FromPart', fromPart);
  c.setAttribute('ToSheet', String(target.sheetId));
  c.setAttribute('ToCell', target.toCell);
  c.setAttribute('ToPart', String(target.toPart));
  connects.appendChild(c);
}

function resolveGlue(end, endpointWorld, ctx) {
  if (!end || !end.shapeId) return null;
  const sheetId = ctx.modelToVisio.get(end.shapeId);
  const target = ctx.shapesById.get(end.shapeId);
  if (!sheetId || !target) return null;

  const added = ctx.newShapeSpecs.get(end.shapeId);
  const el = added ? added.el : ctx.byId.get(String(sheetId));
  const wIn = added ? target.w / DPI
    : cellNumber(el, 'Width', target.w / DPI) || (target.w / DPI);
  const hIn = added ? target.h / DPI
    : cellNumber(el, 'Height', target.h / DPI) || (target.h / DPI);

  const candidates = [];
  const sec = el ? directSection(el, 'Connection') : null;
  if (sec) {
    let j = 0;
    for (const row of sec.children) {
      if (row.localName !== 'Row') continue;
      const ixAttr = parseInt(row.getAttribute('IX'), 10);
      const ix = Number.isFinite(ixAttr) ? ixAttr : j;
      j += 1;
      const x = rowCellNumber(row, 'X');
      const y = rowCellNumber(row, 'Y');
      if (!Number.isFinite(x) || !Number.isFinite(y) || !wIn || !hIn) continue;
      const local = {
        x: (x / wIn) * target.w,
        y: (1 - y / hIn) * target.h,
      };
      candidates.push({ world: localToWorld(target, local), ix });
    }
  }

  let best = null;
  let bestD = Infinity;
  for (const c of candidates) {
    const d = Math.hypot(c.world.x - endpointWorld.x, c.world.y - endpointWorld.y);
    if (d < bestD) { bestD = d; best = c; }
  }
  if (best && bestD <= 1) {
    return { sheetId, toCell: `Connections.X${best.ix + 1}`, toPart: 100 + best.ix };
  }

  // Without a page-level Connection section we would be overwriting the
  // master's inherited X1 point by adding row 0. Keep Visio's dynamic glue
  // instead; the cached Begin/End cells remain at the authored endpoint.
  if (!sec) return { sheetId, toCell: 'PinX', toPart: 3 };

  // Preserve the exact attachment instead of snapping to a side midpoint:
  // add a connection point at the endpoint's local position.
  if (el && wIn > 0 && hIn > 0) {
    const local = worldToLocal(target, endpointWorld);
    if (!Number.isFinite(local.x) || !Number.isFinite(local.y)) {
      return { sheetId, toCell: 'PinX', toPart: 3 };
    }
    // Keep the exact point, including points just outside the shape box:
    // imported 3-D masters often attach at a rim outside the front face.
    const X = (local.x / target.w) * wIn;
    const Y = (1 - local.y / target.h) * hIn;
    const targetSec = sec || ensureConnectionSection(el);
    let maxIx = -1;
    for (const row of targetSec.children) {
      if (row.localName !== 'Row') continue;
      const ix = parseInt(row.getAttribute('IX'), 10);
      if (Number.isFinite(ix)) maxIx = Math.max(maxIx, ix);
    }
    const ix = maxIx + 1;
    const row = el.ownerDocument.createElementNS(NS, 'Row');
    row.setAttribute('T', 'Connection');
    row.setAttribute('IX', String(ix));
    for (const [name, value] of [['X', X], ['Y', Y], ['DirX', 0], ['DirY', 0], ['Type', 0]]) {
      const c = el.ownerDocument.createElementNS(NS, 'Cell');
      c.setAttribute('N', name);
      c.setAttribute('V', fmt(value));
      row.appendChild(c);
    }
    targetSec.appendChild(row);
    return { sheetId, toCell: `Connections.X${ix + 1}`, toPart: 100 + ix };
  }

  return { sheetId, toCell: 'PinX', toPart: 3 };
}

function ensureConnectionSection(shapeEl) {
  let sec = directSection(shapeEl, 'Connection');
  if (sec) return sec;
  const doc = shapeEl.ownerDocument;
  sec = doc.createElementNS(NS, 'Section');
  sec.setAttribute('N', 'Connection');
  const before = [...shapeEl.children].find(
    (n) => n.localName === 'Text' || n.localName === 'Shapes'
      || n.localName === 'ForeignData' || /^Data\d+$/.test(n.localName));
  shapeEl.insertBefore(sec, before || null);
  return sec;
}

function worldToLocal(s, p) {
  const cx = s.x + s.w / 2, cy = s.y + s.h / 2;
  const dx = p.x - cx, dy = p.y - cy;
  const a = -((s.rot || 0) * Math.PI) / 180;
  const rx = dx * Math.cos(a) - dy * Math.sin(a);
  const ry = dx * Math.sin(a) + dy * Math.cos(a);
  const fx = s.flipX ? -1 : 1, fy = s.flipY ? -1 : 1;
  return { x: rx / fx + s.w / 2, y: ry / fy + s.h / 2 };
}

function rowCellNumber(row, name) {
  for (const c of row.children) {
    if (c.localName === 'Cell' && c.getAttribute('N') === name) {
      const v = parseFloat(c.getAttribute('V'));
      return Number.isFinite(v) ? v : NaN;
    }
  }
  return NaN;
}

function cellNumber(el, name, fallback = 0) {
  const c = el ? cellOf(el, name) : null;
  if (!c) return fallback;
  const v = parseFloat(c.getAttribute('V'));
  return Number.isFinite(v) ? v : fallback;
}

// ----------------------------------------------------------- package/pages --


// --------------------------------------------------------- blank document --

function contentTypesTemplate() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="${CT_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/visio/document.xml" ContentType="application/vnd.ms-visio.drawing.main+xml"/><Override PartName="/visio/masters/masters.xml" ContentType="application/vnd.ms-visio.masters+xml"/><Override PartName="/visio/pages/pages.xml" ContentType="application/vnd.ms-visio.pages+xml"/><Override PartName="/visio/windows.xml" ContentType="application/vnd.ms-visio.windows+xml"/><Override PartName="/visio/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>`;
}

function rootRelsTemplate() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="${PKG_REL_NS}"><Relationship Id="rId1" Type="http://schemas.microsoft.com/visio/2010/relationships/document" Target="visio/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>`;
}

function documentRelsTemplate() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="${PKG_REL_NS}"><Relationship Id="rId1" Type="http://schemas.microsoft.com/visio/2010/relationships/masters" Target="masters/masters.xml"/><Relationship Id="rId2" Type="http://schemas.microsoft.com/visio/2010/relationships/pages" Target="pages/pages.xml"/><Relationship Id="rId3" Type="http://schemas.microsoft.com/visio/2010/relationships/windows" Target="windows.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/></Relationships>`;
}

function mastersTemplate() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Masters xmlns="${NS}" xml:space="preserve"></Masters>`;
}

function pagesTemplate() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Pages xmlns="${NS}" xmlns:r="${REL_NS}" xml:space="preserve"></Pages>`;
}

function pagesRelsTemplate() {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="${PKG_REL_NS}"></Relationships>`;
}

function documentTemplate() {
  return VISIO_DOCUMENT_TEMPLATE;
}
function windowsTemplate(dims) {
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Windows ClientWidth="1920" ClientHeight="900" xmlns="${NS}" xmlns:r="${REL_NS}" xml:space="preserve"><Window ID="0" WindowType="Drawing" WindowState="1073741824" WindowLeft="-8" WindowTop="-31" WindowWidth="1936" WindowHeight="939" ContainerType="Page" Page="0" ViewScale="-1" ViewCenterX="${fmt(dims.width / 2)}" ViewCenterY="${fmt(dims.height / 2)}"><ShowRulers>1</ShowRulers><ShowGrid>0</ShowGrid><ShowPageBreaks>1</ShowPageBreaks><ShowGuides>1</ShowGuides><ShowConnectionPoints>0</ShowConnectionPoints><GlueSettings>9</GlueSettings><SnapSettings>65847</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled><TabSplitterPos>0.5</TabSplitterPos></Window></Windows>`;
}

function themeTemplate() {
  const ln = '<a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>';
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="LayoutCraft"><a:themeElements><a:clrScheme name="LayoutCraft"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="LayoutCraft"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="LayoutCraft"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst>${ln}${ln}${ln}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>`;
}

function appTemplate(names) {
  const n = names.length;
  const titles = names.map((v) => `<vt:lpstr>${xmlEscape(v)}</vt:lpstr>`).join('');
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Template></Template><Application>LayoutCraft</Application><ScaleCrop>false</ScaleCrop><HeadingPairs><vt:vector size="4" baseType="variant"><vt:variant><vt:lpstr>Pages</vt:lpstr></vt:variant><vt:variant><vt:i4>${n}</vt:i4></vt:variant><vt:variant><vt:lpstr>Masters</vt:lpstr></vt:variant><vt:variant><vt:i4>0</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts><vt:vector size="${n}" baseType="lpstr">${titles}</vt:vector></TitlesOfParts><Company></Company><LinksUpToDate>false</LinksUpToDate><SharedDoc>false</SharedDoc><HyperlinksChanged>false</HyperlinksChanged><AppVersion>1.0</AppVersion></Properties>`;
}

function coreTemplate(title) {
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>${xmlEscape(title || '')}</dc:title><dc:creator>LayoutCraft</dc:creator><cp:lastModifiedBy>LayoutCraft</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF">${now}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">${now}</dcterms:modified></cp:coreProperties>`;
}

function xmlEscape(value) {
  return String(value ?? '').replace(/[&<>"']/g, (c) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' }[c]
  ));
}

// --------------------------------------------------------------- pictures --
const EXT_MIME = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif',
  bmp: 'image/bmp', webp: 'image/webp', svg: 'image/svg+xml',
  tif: 'image/tiff', tiff: 'image/tiff', emf: 'image/x-emf', wmf: 'image/x-wmf',
};

function dataUrlBytes(src) {
  const m = /^data:([^;,]*)(;base64)?,(.*)$/s.exec(String(src || ''));
  if (!m) return null;
  if (m[2]) {
    const bin = atob(m[3]);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  return enc.encode(decodeURIComponent(m[3]));
}

function mediaExtOf(rec) {
  const ext = String(rec?.ext || '').toLowerCase();
  if (ext) return ext === 'jpg' ? 'jpeg' : ext;
  const type = String(rec?.type || '');
  for (const [mime, e] of Object.entries(EXT_MIME)) {
    if (mime === type) return e === 'jpg' ? 'jpeg' : e;
  }
  return 'png';
}

/**
 * Writes new picture parts into an OPC package.
 *
 * Pictures are shared by data URL, so a shape copy does not copy the bytes.
 * Each page part gets an image relationship; [Content_Types].xml gets a
 * Default entry for the extension. Existing entries are read lazily, so this
 * can run beside makePackageEditor without their Content_Types edits fighting.
 */
function createMediaWriter(entries) {
  const added = new Map();          // media part -> Uint8Array
  const bySrc = new Map();          // data URL -> media part
  const pageStates = new Map();     // page part -> { path, doc, nextRel, changed }
  const ctDefaults = new Map();     // extension -> mime
  const used = new Set(entries.keys());
  let nextImageNo = 1;
  for (const key of used) {
    const m = /^visio\/media\/image(\d+)\./i.exec(key);
    if (m) nextImageNo = Math.max(nextImageNo, parseInt(m[1], 10) + 1);
  }

  const relsPathFor = (part) => {
    const cut = part.lastIndexOf('/');
    return cut >= 0
      ? `${part.slice(0, cut)}/_rels/${part.slice(cut + 1)}.rels`
      : `_rels/${part}.rels`;
  };

  function pageState(part) {
    if (pageStates.has(part)) return pageStates.get(part);
    const path = relsPathFor(part);
    const raw = entries.get(path);
    const doc = raw ? parsePart(raw, path)
      : new DOMParser().parseFromString(
        `<Relationships xmlns="${PKG_REL_NS}"></Relationships>`, 'application/xml');
    let nextRel = 0;
    for (const r of all(doc, 'Relationship')) {
      const m = /^rId(\d+)$/.exec(r.getAttribute('Id') || '');
      if (m) nextRel = Math.max(nextRel, parseInt(m[1], 10));
    }
    const st = { path, doc, nextRel, changed: !raw };
    pageStates.set(part, st);
    return st;
  }

  function ensurePart(rec) {
    if (!rec || !rec.src) return null;
    if (bySrc.has(rec.src)) return bySrc.get(rec.src);
    const bytes = dataUrlBytes(rec.src);
    if (!bytes) return null;
    const ext = mediaExtOf(rec);
    let path;
    do { path = `visio/media/image${nextImageNo++}.${ext}`; } while (used.has(path));
    used.add(path);
    added.set(path, bytes);
    bySrc.set(rec.src, path);
    ctDefaults.set(ext, EXT_MIME[ext] || rec.type || 'application/octet-stream');
    return path;
  }

  return {
    add(pagePart, rec) {
      if (!pagePart) return null;
      const path = ensurePart(rec);
      if (!path) return null;
      const st = pageState(pagePart);
      const base = path.split('/').pop();
      for (const r of all(st.doc, 'Relationship')) {
        const target = String(r.getAttribute('Target') || '').split('/').pop();
        if (target === base) return r.getAttribute('Id');
      }
      const rid = `rId${++st.nextRel}`;
      const rel = st.doc.createElementNS(PKG_REL_NS, 'Relationship');
      rel.setAttribute('Id', rid);
      rel.setAttribute('Type', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/image');
      rel.setAttribute('Target', `../media/${base}`);
      st.doc.documentElement.appendChild(rel);
      st.changed = true;
      return rid;
    },
    flush() {
      for (const [path, bytes] of added) entries.set(path, bytes);
      for (const st of pageStates.values()) {
        if (st.changed) entries.set(st.path, enc.encode(serializeXml(st.doc)));
      }
      if (!ctDefaults.size) return;
      const ctPath = '[Content_Types].xml';
      const raw = entries.get(ctPath);
      if (!raw) return;
      const ct = parsePart(raw, ctPath);
      const have = new Set();
      for (const n of ct.getElementsByTagName('*')) {
        if (n.localName === 'Default') have.add(n.getAttribute('Extension') || '');
      }
      let changed = false;
      for (const [ext, mime] of ctDefaults) {
        if (have.has(ext)) continue;
        const d = ct.createElementNS(CT_NS, 'Default');
        d.setAttribute('Extension', ext);
        d.setAttribute('ContentType', mime);
        ct.documentElement.appendChild(d);
        changed = true;
      }
      if (changed) entries.set(ctPath, enc.encode(serializeXml(ct)));
    },
  };
}

function makePackageEditor(entries) {
  const pagesPart = 'visio/pages/pages.xml';
  const pagesRelsPart = 'visio/pages/_rels/pages.xml.rels';
  const ctPart = '[Content_Types].xml';
  const pagesRaw = entries.get(pagesPart);
  if (!pagesRaw) throw new Error('This VSDX stores its pages inline; adding pages is not supported yet.');
  const ctRaw = entries.get(ctPart);
  if (!ctRaw) throw new Error('The package has no [Content_Types].xml.');

  const pagesDoc = parsePart(pagesRaw, pagesPart);
  const relsRaw = entries.get(pagesRelsPart);
  const pagesRels = relsRaw
    ? parsePart(relsRaw, pagesRelsPart)
    : new DOMParser().parseFromString(
      `<Relationships xmlns="${PKG_REL_NS}"></Relationships>`, 'application/xml');
  const ctDoc = parsePart(ctRaw, ctPart);

  const pageIds = [];
  const names = new Set();
  for (const p of all(pagesDoc, 'Page')) {
    const id = parseInt(p.getAttribute('ID'), 10);
    if (Number.isFinite(id)) pageIds.push(id);
    if (p.getAttribute('NameU')) names.add(p.getAttribute('NameU'));
    if (p.getAttribute('Name')) names.add(p.getAttribute('Name'));
  }
  let pageId = pageIds.length ? Math.max(...pageIds) : -1;
  let relId = 0;
  for (const r of all(pagesRels, 'Relationship')) {
    const m = /^rId(\d+)$/.exec(r.getAttribute('Id') || '');
    if (m) relId = Math.max(relId, parseInt(m[1], 10));
  }
  let pageNo = 0;
  for (const key of entries.keys()) {
    const m = /^visio\/pages\/page(\d+)\.xml$/i.exec(key);
    if (m) pageNo = Math.max(pageNo, parseInt(m[1], 10));
  }

  let defW = 11;
  let defH = 8.5;
  const firstPage = all(pagesDoc, 'Page')[0];
  if (firstPage) {
    const sheet = direct(firstPage, 'PageSheet');
    if (sheet) {
      const w = cellNumber(sheet, 'PageWidth', 11);
      const h = cellNumber(sheet, 'PageHeight', 8.5);
      if (w > 0) defW = w;
      if (h > 0) defH = h;
    }
  }

  let pagesChanged = false;
  let relsChanged = false;
  let ctChanged = false;

  return {
    defaultWidth: defW,
    defaultHeight: defH,
    addPage(name, width, height) {
      let part;
      do { pageNo += 1; part = `visio/pages/page${pageNo}.xml`; } while (entries.has(part));
      pageId += 1;
      relId += 1;
      const rid = `rId${relId}`;
      let pageName = String(name || `Page-${pageId}`).trim() || `Page-${pageId}`;
      if (names.has(pageName)) {
        let n = 2;
        while (names.has(`${pageName} (${n})`)) n += 1;
        pageName = `${pageName} (${n})`;
      }
      names.add(pageName);

      addPageElement(pagesDoc, {
        id: String(pageId), name: pageName, rid, width, height,
      });
      pagesChanged = true;

      addRel(pagesRels, rid, part.replace(/^visio\/pages\//, ''));
      relsChanged = true;

      addContentType(ctDoc, `/${part}`);
      ctChanged = true;
      return part;
    },
    flush(out) {
      if (pagesChanged) out.set(pagesPart, enc.encode(serializeXml(pagesDoc)));
      if (relsChanged) out.set(pagesRelsPart, enc.encode(serializeXml(pagesRels)));
      if (ctChanged) out.set(ctPart, enc.encode(serializeXml(ctDoc)));
    },
  };
}

function addPageElement(doc, { id, name, rid, width, height }) {
  const root = doc.documentElement;
  const page = doc.createElementNS(NS, 'Page');
  page.setAttribute('ID', id);
  page.setAttribute('NameU', name);
  page.setAttribute('IsCustomNameU', '1');
  page.setAttribute('Name', name);
  page.setAttribute('IsCustomName', '1');
  page.setAttribute('ViewScale', '-1');
  page.setAttribute('ViewCenterX', fmt(width / 2));
  page.setAttribute('ViewCenterY', fmt(height / 2));

  const sheet = doc.createElementNS(NS, 'PageSheet');
  sheet.setAttribute('LineStyle', '0');
  sheet.setAttribute('FillStyle', '0');
  sheet.setAttribute('TextStyle', '0');
  for (const [n, v, u] of [
    ['PageWidth', width, 'IN'],
    ['PageHeight', height, 'IN'],
    ['PageScale', 1, 'IN'],
    ['DrawingScale', 1, 'IN_F'],
    ['DrawingSizeType', 0, null],
    ['DrawingScaleType', 0, null],
    ['InhibitSnap', 0, null],
    ['PageLockReplace', 0, 'BOOL'],
    ['PageLockDuplicate', 0, 'BOOL'],
    ['UIVisibility', 0, null],
    ['ShdwType', 0, null],
    ['ShdwObliqueAngle', 0, null],
    ['ShdwScaleFactor', 1, null],
    ['DrawingResizeType', 1, null],
    ['PageShapeSplit', 1, null],
    ['PrintPageOrientation', width > height ? 2 : 1, null],
    ['PaperKind', 1, null],
  ]) {
    const c = doc.createElementNS(NS, 'Cell');
    c.setAttribute('N', n);
    c.setAttribute('V', fmt(v));
    if (u) c.setAttribute('U', u);
    sheet.appendChild(c);
  }

  const layer = doc.createElementNS(NS, 'Section');
  layer.setAttribute('N', 'Layer');
  const row = doc.createElementNS(NS, 'Row');
  row.setAttribute('IX', '0');
  for (const [n, v] of [
    ['Name', 'LayoutCraft'], ['Color', '255'], ['Status', '0'], ['Visible', '1'],
    ['Print', '1'], ['Active', '0'], ['Lock', '0'], ['Snap', '1'], ['Glue', '1'],
    ['NameUniv', 'LayoutCraft'], ['ColorTrans', '0'],
  ]) {
    const c = doc.createElementNS(NS, 'Cell');
    c.setAttribute('N', n);
    c.setAttribute('V', String(v));
    row.appendChild(c);
  }
  layer.appendChild(row);
  sheet.appendChild(layer);
  page.appendChild(sheet);

  const rel = doc.createElementNS(NS, 'Rel');
  rel.setAttributeNS(REL_NS, 'r:id', rid);
  page.appendChild(rel);

  root.appendChild(page);
}

function addRel(doc, id, target) {
  const root = doc.documentElement;
  const rel = doc.createElementNS(PKG_REL_NS, 'Relationship');
  rel.setAttribute('Id', id);
  rel.setAttribute('Type', 'http://schemas.microsoft.com/visio/2010/relationships/page');
  rel.setAttribute('Target', target);
  root.appendChild(rel);
}

function addContentType(doc, partName) {
  const root = doc.documentElement;
  for (const o of all(doc, 'Override')) {
    if (o.getAttribute('PartName') === partName) return;
  }
  const o = doc.createElementNS(CT_NS, 'Override');
  o.setAttribute('PartName', partName);
  o.setAttribute('ContentType', 'application/vnd.ms-visio.page+xml');
  root.appendChild(o);
}

function newPageDims(pg, pkg) {
  let maxX = 0;
  let maxY = 0;
  for (const s of pg.shapes) {
    maxX = Math.max(maxX, s.x + s.w);
    maxY = Math.max(maxY, s.y + s.h);
  }
  for (const c of pg.connectors) {
    for (const p of [...(c.waypoints || []), c.from?.point, c.to?.point]) {
      if (p) { maxX = Math.max(maxX, p.x); maxY = Math.max(maxY, p.y); }
    }
  }
  const q = (v) => Math.ceil(v * 4) / 4;
  return {
    width: q(Math.max(pkg.defaultWidth, maxX / DPI + 1)),
    height: q(Math.max(pkg.defaultHeight, maxY / DPI + 1)),
  };
}

function parsePart(raw, part) {
  const text = dec.decode(raw);
  const doc = new DOMParser().parseFromString(text, 'application/xml');
  if (doc.querySelector('parsererror')) throw new Error(`Could not parse ${part}`);
  return doc;
}

// --------------------------------------------------------------- XML cells --

function all(el, localName) {
  const out = [];
  const nodes = el.getElementsByTagName('*');
  for (const n of nodes) if (n.localName === localName) out.push(n);
  return out;
}

function direct(el, localName) {
  for (const c of el.children) if (c.localName === localName) return c;
  return null;
}

function cellOf(el, name) {
  for (const c of el.children) {
    if (c.localName === 'Cell' && c.getAttribute('N') === name) return c;
  }
  return null;
}

function ensureCell(el, name) {
  let c = cellOf(el, name);
  if (c) return c;
  const d = el.ownerDocument;
  c = d.createElementNS(el.namespaceURI || NS, 'Cell');
  c.setAttribute('N', name);
  const firstBlock = [...el.children].find(
    (n) => n.localName === 'Section' || n.localName === 'Text'
      || n.localName === 'Shapes' || n.localName === 'ForeignData'
      || /^Data\d+$/.test(n.localName));
  el.insertBefore(c, firstBlock || null);
  return c;
}

function setCell(el, name, value) {
  const c = ensureCell(el, name);
  c.setAttribute('V', String(value));
  // A changed value must win over an inherited/themed formula, otherwise
  // Visio recalculates the old formula when it opens the file.
  c.removeAttribute('F');
  return c;
}

/** Keep the cached value but make the cell formula-driven. */
function setCellFormula(el, name, formula) {
  const c = ensureCell(el, name);
  c.setAttribute('F', formula);
  return c;
}

function clearCellFormula(el, name) {
  const c = cellOf(el, name);
  if (c) c.removeAttribute('F');
}

function setCellWithFormula(el, name, value, formula) {
  const c = ensureCell(el, name);
  c.setAttribute('V', String(value));
  c.setAttribute('F', formula);
  return c;
}

function removeCell(el, name) {
  const c = cellOf(el, name);
  if (c) c.remove();
}

/** Update the cached value but leave an inherited formula in place. */
function setCellV(el, name, value) {
  const c = ensureCell(el, name);
  c.setAttribute('V', String(value));
  return c;
}


function ensureSection(el, name) {
  let sec = null;
  for (const c of el.children) {
    if (c.localName === 'Section' && c.getAttribute('N') === name) { sec = c; break; }
  }
  if (sec) return sec;
  const d = el.ownerDocument;
  sec = d.createElementNS(el.namespaceURI || NS, 'Section');
  sec.setAttribute('N', name);
  const firstBlock = [...el.children].find(
    (n) => n.localName === 'Text' || n.localName === 'Shapes'
      || n.localName === 'ForeignData' || /^Data\d+$/.test(n.localName));
  el.insertBefore(sec, firstBlock || null);
  return sec;
}

function ensureRow(sec, ix) {
  for (const c of sec.children) {
    if (c.localName === 'Row' && (c.getAttribute('IX') || '0') === String(ix)) return c;
  }
  const d = sec.ownerDocument;
  const row = d.createElementNS(sec.namespaceURI || NS, 'Row');
  row.setAttribute('IX', String(ix));
  sec.appendChild(row);
  return row;
}

function setRowCell(el, sectionName, ix, name, value) {
  const sec = ensureSection(el, sectionName);
  const row = ensureRow(sec, ix);
  let c = cellOf(row, name);
  if (!c) {
    c = row.ownerDocument.createElementNS(row.namespaceURI || NS, 'Cell');
    c.setAttribute('N', name);
    row.appendChild(c);
  }
  c.setAttribute('V', String(value));
  c.removeAttribute('F');
}

const richPlain = (rich) => (rich?.paragraphs || [])
  .map((p) => (p.runs || []).map((r) => (r.field ? (r.field.value ?? r.text ?? '') : (r.text || ''))).join(''))
  .join('\n');

function setText(el, label) {  let t = direct(el, 'Text');
  if (!t) {
    t = el.ownerDocument.createElementNS(el.namespaceURI || NS, 'Text');
    const before = [...el.children].find(
      (n) => n.localName === 'Shapes' || n.localName === 'ForeignData'
        || /^Data\d+$/.test(n.localName));
    el.insertBefore(t, before || null);
  }
  while (t.firstChild) t.removeChild(t.firstChild);
  const d = el.ownerDocument;
  const ns = el.namespaceURI || NS;
  const cp = d.createElementNS(ns, 'cp');
  cp.setAttribute('IX', '0');
  const pp = d.createElementNS(ns, 'pp');
  pp.setAttribute('IX', '0');
  t.appendChild(cp);
  t.appendChild(pp);
  const value = String(label)
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, '')
    .replace(/\r/g, '')
    .replace(/\n/g, '\u2028');
  if (value) t.appendChild(d.createTextNode(value));
}

/** Ensure a row exists in a named section and return it. */
function ensureSectionRow(el, sectionName, ix) {
  const sec = ensureSection(el, sectionName);
  for (const row of sec.children) {
    if (row.localName === 'Row' && String(row.getAttribute('IX') || '0') === String(ix)) return row;
  }
  const d = el.ownerDocument;
  const row = d.createElementNS(el.namespaceURI || NS, 'Row');
  row.setAttribute('IX', String(ix));
  sec.appendChild(row);
  return row;
}

function setRowCellV(row, name, value, unit = null) {
  const d = row.ownerDocument;
  let c = null;
  for (const x of row.children) {
    if (x.localName === 'Cell' && x.getAttribute('N') === name) { c = x; break; }
  }
  if (!c) {
    c = d.createElementNS(row.namespaceURI || NS, 'Cell');
    c.setAttribute('N', name);
    row.appendChild(c);
  }
  c.setAttribute('V', String(value));
  if (unit) c.setAttribute('U', unit);
  return c;
}

/** Write hyperlink rows (Section N='Hyperlink'). */
function setHyperlinks(el, shape) {
  const rows = shape.hyperlinks || [];
  const old = directSection(el, 'Hyperlink');
  if (old) old.remove();
  if (!rows.length) return;
  const sec = ensureSection(el, 'Hyperlink');
  for (const h of rows) {
    const row = el.ownerDocument.createElementNS(el.namespaceURI || NS, 'Row');
    row.setAttribute('N', String(h.name || 'Link_1'));
    sec.appendChild(row);
    setRowCellV(row, 'Description', h.description ?? '');
    setRowCellV(row, 'Address', h.address ?? '');
    setRowCellV(row, 'SubAddress', h.subAddress ?? '');
    setRowCellV(row, 'ExtraInfo', h.extraInfo ?? '');
    setRowCellV(row, 'Frame', h.frame ?? '');
    setRowCellV(row, 'NewWindow', h.newWindow ?? 0);
    setRowCellV(row, 'Default', h.default ?? 0);
    setRowCellV(row, 'Invisible', h.invisible ?? 0);
    setRowCellV(row, 'SortKey', h.sortKey ?? '');
  }
}

/** Write the shape's Shape Data rows (Section N='Property'). */
function setPropertyData(el, shape) {
  const rows = shape.data || [];
  const old = directSection(el, 'Property');
  if (old) old.remove();
  if (!rows.length) return;
  const sec = ensureSection(el, 'Property');
  for (const d of rows) {
    const dEl = el.ownerDocument;
    const row = dEl.createElementNS(el.namespaceURI || NS, 'Row');
    row.setAttribute('N', String(d.name || 'Row_1'));
    sec.appendChild(row);
    const valueCell = setRowCellV(row, 'Value', d.value ?? '');
    // Visio types Shape Data values from U='STR'; without it a text value is
    // read back as 0 and a text field bound to it shows no value.
    if (Number(d.type ?? 0) === 0) valueCell.setAttribute('U', 'STR');
    setRowCellV(row, 'Prompt', d.prompt ?? '');
    setRowCellV(row, 'Label', d.label ?? d.name ?? '');
    setRowCellV(row, 'Format', d.format ?? '');
    setRowCellV(row, 'SortKey', '');
    setRowCellV(row, 'Type', d.type ?? 0);
    setRowCellV(row, 'Invisible', d.invisible ?? 0);
    setRowCellV(row, 'Verify', 0);
    setRowCellV(row, 'DataLinked', 0);
    setRowCellV(row, 'LangID', 'en-GB');
    setRowCellV(row, 'Calendar', 0);
  }
}

/**
 * Write a rich-text model: Character rows for run styles, Paragraph rows for
 * alignment/bullets, Field rows for field formulas, and a Text element whose
 * cp/pp/fld runs reference them. Used when building a new shape from an
 * imported/copied rich label.
 */
function setRichText(el, shape) {
  const d = el.ownerDocument;
  const ns = el.namespaceURI || NS;
  for (const name of ['Character', 'Paragraph', 'Field']) {
    const sec = directSection(el, name);
    if (sec) sec.remove();
  }
  let t = direct(el, 'Text');
  if (!t) {
    t = d.createElementNS(ns, 'Text');
    const before = [...el.children].find(
      (n) => n.localName === 'Shapes' || n.localName === 'ForeignData'
        || /^Data\d+$/.test(n.localName));
    el.insertBefore(t, before || null);
  }
  while (t.firstChild) t.removeChild(t.firstChild);

  const base = {
    fontFamily: shape.fontFamily || 'Segoe UI',
    fontSize: shape.fontSize || 12,
    bold: !!shape.bold,
    italic: !!shape.italic,
    underline: !!shape.underline,
    textColor: shape.textColor || '#1b2733',
  };
  const charRows = new Map();
  const charIndex = (style = {}) => {
    const st = { ...base, ...style };
    const key = [st.fontFamily, st.fontSize, st.bold, st.italic, st.underline, st.textColor].join('|');
    if (charRows.has(key)) return charRows.get(key);
    const ix = charRows.size;
    charRows.set(key, ix);
    const row = ensureSectionRow(el, 'Character', ix);
    setRowCellV(row, 'Font', st.fontFamily);
    setRowCellV(row, 'ComplexScriptFont', st.fontFamily);
    setRowCellV(row, 'Color', validColor(st.textColor, '#1b2733'));
    setRowCellV(row, 'Style', (st.bold ? 1 : 0) | (st.italic ? 2 : 0) | (st.underline ? 4 : 0));
    setRowCellV(row, 'Size', fmt((st.fontSize || 12) / DPI));
    setRowCellV(row, 'LangID', 'en-GB');
    return ix;
  };

  const paragraphs = shape.rich.paragraphs || [];
  let fieldIx = 0;
  let currentCp = null;
  paragraphs.forEach((p, i) => {
    if (i > 0) t.appendChild(d.createTextNode('\r\n'));
    const firstRun = (p.runs || []).find((r) => (r.text || '').length || r.field);
    const firstStyle = firstRun?.style || {};
    const firstCp = charIndex(firstStyle);
    t.appendChild(d.createElementNS(ns, 'cp'));
    t.lastChild.setAttribute('IX', String(firstCp));
    t.appendChild(d.createElementNS(ns, 'pp'));
    t.lastChild.setAttribute('IX', String(i));
    if (i === 0) {
      t.appendChild(d.createElementNS(ns, 'tp'));
      t.lastChild.setAttribute('IX', '0');
    }
    currentCp = firstCp;

    const prow = ensureSectionRow(el, 'Paragraph', i);
    setRowCellV(prow, 'IndFirst', 0);
    setRowCellV(prow, 'IndLeft', fmt((p.indent || 0) / DPI));
    setRowCellV(prow, 'IndRight', 0);
    setRowCellV(prow, 'SpLine', -1.2);
    setRowCellV(prow, 'SpBefore', 0);
    setRowCellV(prow, 'SpAfter', 0);
    setRowCellV(prow, 'HorzAlign', alignNumber(p.align || shape.align || 'center'));
    setRowCellV(prow, 'Bullet', p.bullet ? 1 : 0);
    setRowCellV(prow, 'BulletStr', p.bullet ? (p.bulletStr || '\u2022') : '');

    for (const run of p.runs || []) {
      if (!run.text && !run.field) continue;
      const cpIx = charIndex(run.style || {});
      if (cpIx !== currentCp) {
        t.appendChild(d.createElementNS(ns, 'cp'));
        t.lastChild.setAttribute('IX', String(cpIx));
        currentCp = cpIx;
      }
      if (run.field) {
        const ix = fieldIx++;
        const fld = d.createElementNS(ns, 'fld');
        fld.setAttribute('IX', String(ix));
        const prop = /(?:Sheet\.[A-Za-z0-9_]+!)?Prop\.([A-Za-z0-9_]+)/i.exec(run.field.formula || '');
        const dataRow = prop ? (shape.data || []).find((x) => String(x.name) === String(prop[1])) : null;
        const cached = dataRow ? String(dataRow.value ?? '') : String(run.field.value ?? run.text ?? '');
        fld.textContent = cached;
        t.appendChild(fld);
        const frow = ensureSectionRow(el, 'Field', ix);
        setRowCellV(frow, 'Value', cached);
        setRowCellV(frow, 'Format', '');
        setRowCellV(frow, 'Type', 0);
        setRowCellV(frow, 'UICat', 0);
        setRowCellV(frow, 'UICod', 255);
        setRowCellV(frow, 'UIFmt', 0);
        setRowCellV(frow, 'Calendar', 0);
        setRowCellV(frow, 'ObjectKind', 0);
        const valueCell = [...frow.children].find((c) => c.getAttribute('N') === 'Value');
        if (valueCell && run.field.formula) {
          valueCell.setAttribute('F', run.field.formula);
          valueCell.setAttribute('V', cached);
          valueCell.setAttribute('U', 'STR');
        }
        const fmtCell = [...frow.children].find((c) => c.getAttribute('N') === 'Format');
        if (fmtCell) {
          fmtCell.setAttribute('V', 'esc(0)');
          fmtCell.setAttribute('U', 'STR');
          if (run.field.format) fmtCell.setAttribute('F', run.field.format);
        }
      } else {
        const value = String(run.text)
          .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, '')
          .replace(/\r/g, '')
          .replace(/\n/g, '\u2028');
        t.appendChild(d.createTextNode(value));
      }
    }
  });
}

function locPin(shapeEl, cellName, sizeCell, newSize, oldSize) {
  const c = cellOf(shapeEl, cellName);
  if (!c) return newSize / 2;
  const f = (c.getAttribute('F') || '').replace(/\s+/g, '');
  const half = new RegExp(`(${sizeCell}\\*0?\\.5|0?\\.5\\*${sizeCell})`);
  if (half.test(f)) return newSize / 2;
  if (f === sizeCell) return newSize;
  const v = parseFloat(c.getAttribute('V'));
  if (!Number.isFinite(v)) return newSize / 2;
  // LocPin is normally inherited from the master as Width/2 or Height/2, so
  // the page shape only carries the cached old value. Follow the size change
  // instead of leaving the pin at half the old width.
  if (Math.abs(v - oldSize / 2) < 1e-4) return newSize / 2;
  if (Math.abs(v - oldSize) < 1e-4) return newSize;
  return v;
}

function alignNumber(align) {
  return align === 'left' ? 0 : align === 'right' ? 2 : 1;
}

function vAlignNumber(v) {
  return v === 'top' ? 0 : v === 'bottom' ? 2 : 1;
}

function linePattern(o) {
  if (!o.stroke || o.stroke === 'none' || !o.strokeWidth) return 0;
  if (o.strokeStyle === 'dashed') return 2;
  if (o.strokeStyle === 'dotted') return 3;
  if (o.strokeStyle === 'dashFine') return 23;
  return 1;
}

/** Keep an existing custom pattern when only the line width/colour changed. */
function patchLinePattern(el, o, widthChanged, styleChanged, strokeChanged) {
  if (!o.stroke || o.stroke === 'none' || !o.strokeWidth) {
    setCell(el, 'LinePattern', 0);
    return;
  }
  if (styleChanged) {
    setCell(el, 'LinePattern', linePattern(o));
    return;
  }
  const current = cellOf(el, 'LinePattern');
  const old = current ? parseInt(current.getAttribute('V'), 10) : NaN;
  if ((widthChanged || strokeChanged) && (!Number.isFinite(old) || old === 0)) {
    setCell(el, 'LinePattern', linePattern(o));
  }
}

function setCharacterStyle(el, bits) {
  const sec = ensureSection(el, 'Character');
  const row = ensureRow(sec, '0');
  let c = cellOf(row, 'Style');
  let old = 0;
  if (c) {
    const v = parseInt(c.getAttribute('V'), 10);
    if (Number.isFinite(v)) old = v;
  }
  if (!c) {
    c = row.ownerDocument.createElementNS(row.namespaceURI || NS, 'Cell');
    c.setAttribute('N', 'Style');
    row.appendChild(c);
  }
  c.setAttribute('V', String((old & ~0x7) | bits));
  c.removeAttribute('F');
}

function endSnap(end = {}) {
  return {
    shapeId: end.shapeId || null,
    side: end.side || null,
    point: end.point ? `${end.point.x.toFixed(3)},${end.point.y.toFixed(3)}` : null,
    local: end.local ? `${end.local.x.toFixed(6)},${end.local.y.toFixed(6)}` : null,
  };
}

function connectorRouteChanged(conn, orig) {
  const now = {
    route: conn.route || 'straight',
    routeStyle: conn.routeStyle ?? null,
    waypoints: (conn.waypoints || []).map((p) => `${p.x.toFixed(3)},${p.y.toFixed(3)}`),
    from: endSnap(conn.from),
    to: endSnap(conn.to),
  };
  const then = {
    route: orig.route || 'straight',
    routeStyle: orig.routeStyle ?? null,
    waypoints: (orig.waypoints || []).map((p) => `${p.x.toFixed(3)},${p.y.toFixed(3)}`),
    from: endSnap(orig.from),
    to: endSnap(orig.to),
  };
  return JSON.stringify(now) !== JSON.stringify(then);
}

const ARROW_VALUE = {
  none: 0,
  arrow: 1,
  filled: 4,
  hollowTriangle: 14,
  hollowDiamond: 22,
  filledDiamond: 38,
};

function setArrow(el, side, kind) {
  const names = side === 'start' ? ['LineBeginArrow', 'BeginArrow'] : ['LineEndArrow', 'EndArrow'];
  let c = null;
  for (const n of names) { c = cellOf(el, n); if (c) break; }
  if (!c) c = ensureCell(el, side === 'start' ? 'BeginArrow' : 'EndArrow');
  c.setAttribute('V', String(ARROW_VALUE[kind] ?? 0));
  c.removeAttribute('F');
}

function fmt(n) {
  const v = Number(n);
  if (!Number.isFinite(v)) return '0';
  return String(Math.round(v * 1e6) / 1e6);
}

function serializeXml(xml) {
  const text = new XMLSerializer().serializeToString(xml);
  return /^<\?xml/i.test(text)
    ? text
    : `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n${text}`;
}

// ------------------------------------------------------------------- ZIP ---

let CRC_TABLE = null;

function crc32(data) {
  if (!CRC_TABLE) {
    CRC_TABLE = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1);
      CRC_TABLE[n] = c >>> 0;
    }
  }
  let crc = 0xffffffff;
  for (let i = 0; i < data.length; i++) crc = CRC_TABLE[(crc ^ data[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

async function deflateRaw(data) {
  if (typeof CompressionStream === 'undefined') return null;
  try {
    const stream = new Blob([data]).stream().pipeThrough(new CompressionStream('deflate-raw'));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  } catch (err) {
    return null;
  }
}

async function zipBlob(entries) {
  const parts = [];
  const central = [];
  let offset = 0;
  const now = new Date();
  const dosTime = ((now.getHours() << 11) | (now.getMinutes() << 5) | (now.getSeconds() >> 1)) & 0xffff;
  const dosDate = (((now.getFullYear() - 1980) << 9) | ((now.getMonth() + 1) << 5) | now.getDate()) & 0xffff;

  for (const [name, rawValue] of entries) {
    const raw = rawValue instanceof Uint8Array ? rawValue : new Uint8Array(rawValue);
    const nameBytes = enc.encode(name);
    const crc = crc32(raw);
    const compressed = await deflateRaw(raw);
    const useDeflate = compressed && compressed.length < raw.length;
    const body = useDeflate ? compressed : raw;
    const method = useDeflate ? 8 : 0;

    const local = new Uint8Array(30 + nameBytes.length);
    const lv = new DataView(local.buffer);
    lv.setUint32(0, 0x04034b50, true);
    lv.setUint16(4, 20, true);
    lv.setUint16(6, 0x0800, true);       // UTF-8 names
    lv.setUint16(8, method, true);
    lv.setUint16(10, dosTime, true);
    lv.setUint16(12, dosDate, true);
    lv.setUint32(14, crc, true);
    lv.setUint32(18, body.length, true);
    lv.setUint32(22, raw.length, true);
    lv.setUint16(26, nameBytes.length, true);
    lv.setUint16(28, 0, true);
    local.set(nameBytes, 30);
    parts.push(local, body);

    const cen = new Uint8Array(46 + nameBytes.length);
    const cv = new DataView(cen.buffer);
    cv.setUint32(0, 0x02014b50, true);
    cv.setUint16(4, 20, true);
    cv.setUint16(6, 20, true);
    cv.setUint16(8, 0x0800, true);
    cv.setUint16(10, method, true);
    cv.setUint16(12, dosTime, true);
    cv.setUint16(14, dosDate, true);
    cv.setUint32(16, crc, true);
    cv.setUint32(20, body.length, true);
    cv.setUint32(24, raw.length, true);
    cv.setUint16(28, nameBytes.length, true);
    cv.setUint16(30, 0, true);
    cv.setUint16(32, 0, true);
    cv.setUint16(34, 0, true);
    cv.setUint16(36, 0, true);
    cv.setUint32(38, 0, true);
    cv.setUint32(42, offset, true);
    cen.set(nameBytes, 46);
    central.push(cen);

    offset += local.length + body.length;
  }

  const centralSize = central.reduce((n, b) => n + b.length, 0);
  const eocd = new Uint8Array(22);
  const ev = new DataView(eocd.buffer);
  ev.setUint32(0, 0x06054b50, true);
  ev.setUint16(4, 0, true);
  ev.setUint16(6, 0, true);
  ev.setUint16(8, entries.size, true);
  ev.setUint16(10, entries.size, true);
  ev.setUint32(12, centralSize, true);
  ev.setUint32(16, offset, true);
  ev.setUint16(20, 0, true);

  return new Blob([...parts, ...central, eocd], { type: 'application/vnd.ms-visio.drawing' });
}
