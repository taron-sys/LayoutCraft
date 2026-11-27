# Architecture

## Current architecture

LayoutCraft is a Rust core with two hosts (`AGENTS.md`, rules 8 and 9).

```text
                      .vsdx / .vssx / draw.io / Excalidraw
                                    │
   layoutcraft-vsdx  (read and write the package: import, patch writer, fresh writer, page and shape edits)
   layoutcraft-model (the document model, JSON)
                                    │
   layoutcraft-engine (the open drawing, edits, undo and redo, clipboard, stencils, exporters and importers)
   layoutcraft-render (scene, SVG, PNG)
          │                    │                      │
   layoutcraft-app      layoutcraft-wasm         layoutcraft-mcp
   native egui app      C ABI for web/engine.js  MCP server, stdio
                        and the browser shell
```

- The editable model is what is drawn; the opened package stays in the engine and is the side artifact the patch writer works on, so parts the editor does not model survive a save.
  Without a source package, or for a page that was added, the fresh writer builds the part from the model.
- Behaviour belongs in `layoutcraft-engine` and the crates below it. A host calls the engine and draws; it does not carry its own copy of a rule. The same engine call gives the same result in the desktop app, the browser shell and the MCP server.
- The engine, `layoutcraft-vsdx`, `layoutcraft-render` and `layoutcraft-model` also compile to WebAssembly: no `std::time::Instant` and no debug or timing code in them.
- `web/` is the browser shell: `shell.html`, `shell.js`, the ribbon and the dialogs, over `web/layoutcraft.wasm` through `web/engine.js`. It is the only JavaScript left in the repository
  (with `tools/dump-ribbon.mjs`, which dumps its ribbon for the native app); it goes when the egui app runs in the browser ([42](42-javascript-removal.md)).
- `rust/xtask` holds the repository tools: `cargo xtask provenance`, `notices` and `serve`.

The file-by-file map is in [09](09-file-reference.md); the desktop build and its milestones in [34](34-desktop-build.md); the MCP server in [32](32-mcp.md) and [35](35-mcp-native.md).

## The JavaScript editor (historical)

The rest of this page describes the JavaScript editor that was removed ([42](42-javascript-removal.md)); the last commit that still has it is `c9aa852`. The module names below are the files of its `js/` folder.
The development server it came with (`server.mjs`) and its `index.html` and `css/app.css` are not in the table: `cargo xtask serve` replaces the server, and the shell page is `web/shell.html`.

### Module map

| File | Responsibility |
| --- | --- |
| `main.js` | App object, document ownership, commands, import/save routing |
| `model.js` | Pages, shapes, connectors, defaults, undo history |
| `shapes.js` | Shape catalogue: SVG path generators and text boxes |
| `svg-path.js` | Pure SVG-path flattener used when authoring Visio Geometry |
| `render.js` | SVG shape/connector/overlay renderer, gradients, markers |
| `router.js` | Glue points, side selection, route generation |
| `interact.js` | Pointer and keyboard interaction |
| `ui.js` | Palette, inspector, context menu, page tabs, toolbar wiring |
| `file.js` | localStorage autosave, JSON/SVG/PNG import/export |
| `visio.js` | VSDX/VDX importer |
| `vsdx-writer.js` | VSDX audit, patch writer, fresh-package builder, ZIP writer |
| `vsdx-source.js` | IndexedDB cache for the original OPC package |
| `util.js` | Geometry, text measurement/wrapping, download helper |

The application has no framework and no runtime dependency. Modules are loaded
directly by the browser as ES modules.

### Main data flow

```text
.open VSDX ──► visio.js ──► editable document model ──► render.js / router.js
                                  │
                                  ├─ autosave JSON to localStorage
                                  ├─ original package Map in memory
                                  └─ original package copy in IndexedDB

Save VSDX ──► audit ──► matching source package?
                         ├─ yes: patch page XML + rezip original package
                         └─ no:  generate a complete package from the model
```

The editable model is the single source of truth for what is drawn. The
original Visio package is a side artifact used only by the writer.

### Document model

`model.js` defines:

```js
doc = {
  name,
  version,
  schemaVersion,        // current portable JSON schema (schema.js)
  pages: [{
    id,
    name,
    shapes: [shape...],
    connectors: [connector...],
    groups: [group...],   // optional metadata for imported/authored groups
    containers: [container...], // imported container/list records + membership
  }],
  media: { [mediaId]: { src, type, ext, name, width, height, adjust, unsupported } },
  pageIndex,
}
```

A shape contains:

```js
{
  id,                // LayoutCraft runtime ID
  type,              // catalogue key, 'custom', or 'image'
  x, y, w, h,        // world pixels, top-left origin
  rot,               // degrees
  flipX, flipY,
  label,
  fill, stroke, strokeWidth, strokeStyle,
  fontSize,          // pixels
  fontFamily,
  textColor,
  bold, italic, underline,
  align, vAlign,
  parts,             // optional imported/exported sub-paths
  textBox,           // optional local text box in pixels
  z,                 // optional stacking key
  group,             // optional { id, parentId, rootId, depth, ... }
  containers,        // optional Visio IDs of containers this object belongs to
  listOf,            // optional Visio IDs of lists this object is an item of
  containerInfo,     // authored container frame metadata (inline container)
  image,             // picture: { mediaId, offsetX, offsetY, scaleX, scaleY,
                     //   brightness, contrast, gamma, transparency, ... }
  callout,           // data graphic: { kind: 'text'|'heading'|'bar'|'icon', field,
                     //   label, position, gap, autoSize, min, max, showValue, iconSet }
  attachedTo,        // target shape id for a data callout/heading/bar/icon
  dataColor,         // colour rule: { field, min, max, low, mid, high, baseFill }
}
```

A connector contains:

```js
{
  id,
  from: { shapeId, side, point, local }, // local is normalized 0..1 in the shape box
  to:   { shapeId, side, point, local },
  route: 'straight' | 'elbow' | 'curved',
  waypoints: [{x,y}...],
  label,
  stroke, strokeWidth, strokeStyle,
  arrowStart, arrowEnd,
}
```

The model is intentionally small. It does not represent Visio masters,
formulas, ShapeSheet sections or layers. Visio files are flattened into this
model at import time. Rich text, container membership, group identity, Shape
Data/hyperlinks and pictures are modelled in the fields listed above; OLE, ink
and image formats the browser cannot decode are not previewed.

Pictures are an exception to "small": bytes live in `doc.media` as data URLs,
shared by media id. This keeps a picture used on five pages (or copied shapes)
from duplicating the bytes in memory, serialization or the saved package. The
trade-off is that autosave JSON includes the data URLs; see
[Known gaps](07-known-gaps.md#6-browserplatform-gaps).

A data graphic is a shape with `callout` metadata and `attachedTo` pointing at
another shape on the same page. A callout or heading text is derived
(`calloutText`) from the target's Shape Data; a bar's fill is derived by
`barRatio` from the same value and its min/max; an icon's one-to-five level is
derived by `iconLevel` from the same value and range. A colour rule instead
lives on the target as `shape.dataColor`; `dataColorFill` derives the target's
fill from three equal bands. `app.layoutAttachments` runs before every render
to auto-size and place non-free graphics relative to the target. No separate
data-graphics document object is needed; copying the target also copies and
re-links its graphics and its colour rule.

Page-level groups are the exception: a group is still stored as flat shapes,
but each member carries `shape.group` metadata (`id`, `parentId`, `rootId`,
`depth`), and `page.groups` records the group identities. Rendered coordinates
stay absolute; selection uses `rootId` to pick the whole group.

Container/list membership is imported from the `Relationships` graph into
`page.containers` (`visioId`, `kind`, `members`, `items`, `related`) and onto
members as `shape.containers`/`listOf` (a shape can belong to several)
(`docs/04-visio-import.md`). Editing and authoring containers is not implemented.

### Coordinate systems and units

This is the source of several subtle bugs; read it before touching geometry.

- Visio stores lengths internally in **inches**, angles in **radians**, with a
  **bottom-left** page origin and Y increasing **up**.
- LayoutCraft stores world positions in **pixels at 96 DPI**, with a **top-left**
  origin and Y increasing **down**.
- Import conversion:
  - `xPx = xIn * 96`
  - `yPx = (pageH - yIn) * 96`
  - rotation: `rotDegrees = -VisioAngleRadians * 180 / PI`
- Write conversion reverses both operations, using the page's stored flip
  height (`pg.visio.pageH` for imported pages, generated page height for new
  pages).
- The importer uses Visio's cached `V` cell value, not the display unit `U`.
  `0.1111` is 8 pt and `0.25` is 18 pt because the cached value is already in
  internal inches.
- LayoutCraft `fontSize` is in pixels, but Visio `Character.Size` is in inches, so
  the writer uses `fontSize / 96`.
- LayoutCraft `strokeWidth` is in pixels, but Visio `LineWeight` is in inches, so
  the writer uses `strokeWidth / 96`.
- Visio `Angle` is written as `-rot * PI / 180`.

### Imported object provenance

The importer attaches source metadata to the editable model:

```js
pg.visio = {
  part: 'visio/pages/page1.xml',
  pageId: '8',
  pageH: 8.5,            // flip height used at import
  ids: ['1','2',...],    // imported shape/connector IDs represented by this page
  docKey: 'v...',        // set by main.js when the package is cached
}
```

A shape may contain:

```js
shape.visio = {
  id: '6',               // Visio page shape ID
  origin: {x, y},        // parent-space origin in inches; nonzero for container children
  orig: { ... },         // snapshot of editable fields at import for diffing
}
```

A connector may contain:

```js
conn.visio = {
  id: '24',
  orig: {
    label, stroke, strokeWidth, strokeStyle,
    arrowStart, arrowEnd, route, waypoints, from, to,
  },
}
```

`orig` is what makes patch-on-save possible without touching unchanged cells.
Do not rename or drop these fields casually; `vsdx-writer.js` compares
against them.

### Original package lifecycle

When a VSDX/VSTX/VDM/VSTM is opened:

1. `visio.js` reads the ZIP into a `Map<path, Uint8Array>`.
2. `importVisioFile` returns the document and attaches the package as a
   non-enumerable `_visioPackage` property and the file name as
   `_visioFileName`.
3. `main.js` copies it into `app.visioSource`:
   ```js
   { package, name, pageParts, key }
   ```
   and assigns `doc.visioKey` and `pg.visio.docKey`.
4. `vsdx-source.js` stores the package in IndexedDB under that key.
5. On reload, boot reads the saved document and restores the package by
   `visioKey`.

The package is deliberately non-enumerable and excluded from JSON/undo so
undoredo snapshots remain small and serialisable. Shape/page provenance is
enumerable and survives undo/redo.

### Renderer

`render.js` draws every item from the document on every change. There is no
incremental scene graph.

- Imported custom shapes use `shape.parts`: one or more SVG sub-paths, each with
  its own fill/stroke. This is how 3-D blocks, bevels and groups appear.
- Picture shapes (`shape.image`) render an `<image>` from `doc.media`; crop uses
  a generated `clipPath`, transparency uses `opacity`, and brightness/contrast/
  gamma use a generated `feComponentTransfer` filter (skipped for indexed
  pictures, matching Visio). Unsupported media renders a labelled placeholder.
- Data callouts render as rounded boxes with their live text plus a leader
  (`buildCalloutLeader`) to the target; headings use the same box without a
  leader; a data bar renders a filled bar over a track outline using the derived
  ratio; a data icon renders five outlined boxes, diamonds or circles with the
  first `iconLevel` glyphs filled. A colour rule overrides the target's fill
  (including the visible parts of an imported multi-part shape). Text, ratios,
  levels and colours are derived from Shape Data, never stored separately.
- Catalogue shapes use `shapePath`, `shapeDetail` and `shapeTextBox` from
  `shapes.js`.
- Connectors are routed by `routeConnector` on every render. Imported connector
  waypoints are used as the polyline vertices.
- Selection handles, the marquee, glue hints and connector previews live in the
  overlay layer.
- Dash arrays and arrow markers are generated from the small line/arrow enums.

### Router and glue

`router.js`:

- `anchorOnShapeAt(shape, side, toward)` slides a side-glued endpoint along that
  side toward the other endpoint. This prevents page-sized background shapes
  from pulling connectors to a distant midpoint.
- `bestSide` chooses a side facing the other object.
- `endpointPoint` uses exact local glue when imported (`end.local`), otherwise
  slides along the chosen side.
- `dedupePoints` removes duplicate consecutive route points. This is essential:
  a zero-length final segment makes SVG marker orientation undefined and
  produces sideways arrowheads.
- `routeConnector` produces line, polyline or cubic curve geometry.

`hitShape` in `interact.js` prefers the smallest foreground shape under the
pointer, then distance/z. This keeps large background shapes from stealing
clicks or connector drops.

### Undo, autosave and page state

- `History` stores JSON snapshots of the whole document, limit 200. The `media`
  map is kept by reference rather than serialised into every snapshot: picture
  records are immutable and only ever added, so a shared map preserves undo
  without duplicating megabytes of data URLs per step.
- Autosave writes `app.doc` to localStorage 700 ms after a render/change;
  `schema.js` normalises legacy data and rejects newer schemas on load/import.
- Pages have only an ID, name, shapes and connectors. Page size/view is not in
  the editable model; the writer derives it for fresh pages and preserves the
  source page part for imported pages.
- Page deletion while patching an existing package is not a partial XML edit;
  `app.saveVSDX` offers a fresh rebuild instead.

### What the architecture deliberately does not do

- No ShapeSheet object model in the browser.
- No full Visio formula engine: a small evaluator covers the common arithmetic and the cached values cover the rest.
- No master library bundled with the app.
- No OLE/ink import or preview of EMF/WMF/TIFF (their bytes are preserved).
- No layer/page-setup editor.
- No per-character text model (rich text is paragraph/run based).

The writer compensates where practical with inline Geometry, flattening, and
byte-preserving patch mode. See [Known gaps](07-known-gaps.md).
