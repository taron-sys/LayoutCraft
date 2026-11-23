# File reference

Line counts and line numbers are from the handover snapshot on 2026-10-26.
Treat function names as the stable reference; line numbers will drift.

## Application runtime

| File | Lines | Responsibility |
| --- | ---: | --- |
| `index.html` | 164 | Toolbar, palette/inspector containers, SVG canvas, file input |
| `css/app.css` | 236 | All UI styling |
| `js/main.js` | 701 | App object, view state, commands, import/export routing |
| `js/model.js` | 148 | Page/shape/connector constructors, defaults, undo history |
| `js/shapes.js` | 292 | Shape catalogue: SVG paths, details, text boxes |
| `js/svg-path.js` | 241 | Pure SVG path flattener for inline Visio Geometry |
| `js/render.js` | 451 | SVG shape/connector/overlay renderer, gradients, markers |
| `js/router.js` | 229 | Glue anchoring, side selection, routing, dedupe |
| `js/interact.js` | 722 | Pointer/keyboard interaction, hit testing |
| `js/ui.js` | 493 | Palette, inspector, context menu, page tabs, toolbar |
| `js/schema.js` | ~65 | Document schema version, normalisation and migrations |
| `js/file.js` | 126 | localStorage autosave, JSON/SVG/PNG import/export |
| `js/visio.js` | 2107 | VSDX/VDX importer |
| `js/layout.js` | ~260 | Layered auto-layout: units, Tarjan SCC, barycentre ordering, component packing |
| `js/vsdx-writer.js` | 1654 | VSDX audit/patch/fresh package writer and ZIP |
| `js/vsdx-source.js` | 90 | IndexedDB package cache |
| `js/util.js` | 73 | Geometry/text utilities, download |
| `server.mjs` | 60 | Zero-dependency static file server |
| `package.json` | 13 | `start`/`dev` scripts, no dependencies |

## `js/main.js`

Important functions/commands:

| Name | Role |
| --- | --- |
| `app` | Global app object; exposed as `window.layoutcraft` for debugging/tests |
| `app.getPage` | Current page |
| `app.render` | Re-render + inspector/status + queued autosave |
| `app.setSelection` | Update selection/UI |
| `app.expandGroups` | Expand a selection to whole group roots |
| `app.expandContainers` | Expand a container frame to its transitive members |
| `app.setContainerMembership` | Add/remove regular container membership |
| `app.createContainer` | Wrap the selection in an authored inline container |
| `app.groupSelection`, `app.ungroupSelection` | Editor group create/clear |
| `app.addShape` | Add catalogue shape |
| `app.insertImageFile` | Read/convert an image, add a `doc.media` record, insert or replace a picture shape |
| `app.imagePick` | Open the picture file picker (optional replace target) |
| `app.fitImageNatural`, `app.fitImageFrame` | Natural picture size / clear crop |
| `app.resetImageAdjustments` | Reset brightness/contrast/gamma/transparency |
| `app.imageCropOf`, `app.setImageCrop` | Normalised crop helpers used by the inspector |
| `app.calloutsFor`, `app.layoutAttachments` | Attached data graphics and their target-relative auto-size/position pass before render |
| `app.addDataCallout`, `app.addDataHeading`, `app.addDataBar`, `app.addDataIcon`, `app.removeDataCallout` | Create/remove a text callout, heading, numeric bar or five-level icon set |
| `app.addDataColor`, `app.removeDataColor` | Add/replace or remove a three-band colour-by-value rule on a target |
| `app.duplicate`, `app.copy`, `app.cut`, `app.paste` | Object duplication, strips Visio provenance from copies |
| `app.undo`, `app.redo` | History restore |
| `app.addPage`, `app.deletePage`, `app.setPage` | Page management |
| `app.newDoc` | New document; clears package cache/key |
| `app.saveVSDX` | Patch-or-build VSDX routing, warnings, lineage reset |
| `app.relayout` | Layered top-down/left-to-right re-layout of the selection or page; groups move as units, containers/labels/backdrops stay |
| `app.runCommand` | Toolbar commands (`export-vsdx`, `relayout`, etc.) |
| `docMatchesSource` | Package key/page-part safety check |
| boot import handler | Imports VSDX/VDX/JSON, caches package, sets page tabs |
| `sampleDoc` | Starter network diagram |

## `js/visio.js`

| Name | Role |
| --- | --- |
| `readZip` | Central-directory ZIP reader; stored/deflate |
| `VisioFile.loadStyles`, `loadMasters` | Parse document style sheets, masters, theme |
| `importVisioFile` | Public extension-based entry |
| `importVSDX` | OPC/VSDX import |
| `importVDX` | Single XML/VSDX 2003 import |
| `buildPage` | Page traversal, master/container/connector resolution, model creation |
| `userValue` | Read an inherited `User.*` cell or User-section row |
| `parseRelationships` | Decode `Relationships`/`DEPENDSON` container/list graph |
| `readForeignMedia`, `imagePixelSize`, `bytesToBase64` | Picture `ForeignData` rel → package media → data URL, natural size, indexed flag |
| `VisioFile.mediaIds` | Shared media records keyed by package part; attached to `doc.media` |
| `group` traversal | Explicit `<Shape Type="Group">` recursion and container detection; stores `frame`, `labelRel`, `labelStyle` |
| `partsOf` | Custom geometry, nested master bodies, bevels, details |
| `evaluatedPath`, `pathFromRows` | Geometry rows → SVG path (linear, arcs, NURBS/splines, infinite lines) |
| `circularArcPoints`, `ellipticalArcPoints` | Reconstruct `ArcTo` and `EllipticalArcTo` rows as local-space samples |
| `nurbsCurve`, `sampleSpline`, `nurbsPoint`, `clampedKnots` | NURBSTo knot/weight assembly and rational de Boor evaluation; periodic spline support |
| `evaluatedBox` | Geometry bounds for bevel detection |
| `FormulaParser` | Compact formula subset evaluator |
| `GeometryScope` | Lazy cell/geometry reference resolver |
| `Theme` | Theme parts, variations, colour slots, quick styles |
| `styleOf`, `partStyle`, `characterStyle`, `textBlock` | Appearance extraction |
| `verticalAlign`, `paragraphAlign` | Text alignment |
| `arrowKind` | Visio arrow value → model arrow kind |
| `shapeSnapshot`, `connectorSnapshot` | Provenance snapshots for patch diffing |
| `textOf`, `labelOf`, `richTextOf` | Plain text extraction and per-run/paragraph/field parsing (`shape.rich`) |
| `characterRowStyle`, `paragraphRowStyle` | Style for a Character/Paragraph row referenced by cp/pp |
| `propertyDataOf`, `hyperlinkDataOf` | Shape Data (`Property`) and hyperlink rows |
| `connectorConfigOf` | `ShapeRouteStyle`, fixed/ext/walk/plow/glue cells |
| `Theme` (`toJSON`, `themedColor`, `quickColor`) | Theme slots/variations, role-aware `Themed`, quick-style colours |
| `toShapeLocal`, `sideTowards` | Connector glue normalization |

## `js/vsdx-writer.js`

| Name | Role |
| --- | --- |
| `auditVSDX(doc, source)` | Patch/fresh audit, warnings, safety |
| `buildVSDX(doc, source)` | Patch original package; add objects/pages |
| `buildNewVSDX(doc)` | Generate a complete new package |
| `exportVSDX(doc, source, fileName)` | Build + download |
| `vsdxFileName` | Naming and sanitization |
| `patchPage` | Patch/remove/add on one page; sortZ for fresh pages |
| `patchShape` | Geometry/text/style diff and cell patch |
| `patchConnector` | Style/label/arrow diff |
| `setText`, `setRichText` | Plain text and Character/Paragraph/Field run writing |
| `setPropertyData`, `setHyperlinks`, `applyTextFit` | Shape Data, hyperlink and autosize cells |
| `setConnectorConfig` | Dynamic-connector route/config cells |
| `buildNewShape` | Inline 2-D Geometry shape |
| `buildNewConnector` | Inline 1-D connector |
| `patchImage`, `writeForeignShape` | Picture cells, crop formulas, `ForeignData`/`Rel`, replacement |
| `createMediaWriter`, `dataUrlBytes`, `mediaExtOf` | Add media parts, page image relationships and Content-Type Defaults |
| `setCalloutText`, `writeCalloutProperty`, `writeCalloutUserRows` | Data-callout field run, transport Property row and metadata |
| `writeDataBarGeometry`, `barRatioOf`, `barPercentFormula` | Bar track/fill Geometry and live percentage formula |
| `writeDataIconGeometry`, `iconLevelOf`, `iconLevelFormula` | Outline/filled icon Geometry, live level formula and cached level |
| `writeDataColor`, `patchDataColor`, `dataColorFormula` | Target fill formula, colour metadata and cached fill refresh |
| `applyCalloutPin`, `patchCallout`, `refreshCalloutFieldValue` | Target-relative Pin formulas, metadata/bar/icon Geometry patch and cached value/level refresh |
| `rebuildConnector` | Rewrite existing connector route/box/geometry |
| `connectorGeometry` | Route points, page/local conversion, label midpoint |
| `addGeometrySection` | Visio Geometry rows from local sub-paths |
| `addConnectionSection` | Four side connection points |
| `applyConnects` | Rewrite `<Connect>` rows |
| `applyGroupLayout` | Reconcile group/container frames with member boxes |
| `applyMemberships` | Reconcile container/list membership edits |
| `writeRelationships` | Merge managed `DEPENDSON` groups in a `Relationships` cell |
| `addContainerUserRows` | Container User rows/units for an authored inline container |
| `resolveGlue` | Reuse/add exact connection point, fallback PinX |
| `reorderPageShapes` | Fresh-page z-order |
| `makePackageEditor` | Add pages to existing package: Page, rel, Content-Type, rels |
| `newPageDims` | Content-aware page size |
| `contentTypesTemplate`, `documentTemplate`, `themeTemplate`, `windowsTemplate`, `appTemplate`, `coreTemplate` | Fresh package shell (document template from `visio-document-template.js`) |
| `pagesRelsTemplate` | Empty relationships part for pages/masters/page rels |
| `validColor` | Reject `"none"` as colour |
| `linePattern` | Model line style → Visio index |
| `ARROW_VALUE` | Model arrow → Visio index |
| `crc32`, `deflateRaw`, `zipBlob` | OPC ZIP writer |

## `js/vsdx-source.js`

| Name | Role |
| --- | --- |
| `persistVisioSource(key, source)` | Store package entries in IndexedDB |
| `loadVisioSource(key)` | Restore package/source metadata |
| `deleteVisioSource(key)` | Remove a cached package |

DB: `layoutcraft-vsdx`, store `packages`, keyPath `key`.

## `js/model.js`

- DEFAULTS: fill, stroke, stroke width/style, font, colours, bold/italic/
  underline, alignment, vertical alignment.
- `newDoc`, `newPage`, `newShape`, `newCustomShape`, `newConnector`.
- Pages carry `groups` and `containers`; shapes/connectors may carry a `group`
  record and `containers`/`listOf` Visio IDs.
- `doc.media` is a map of shared picture records (`src` data URL, `type`, `ext`,
  natural `width`/`height`, `adjust`, `unsupported`).
- A picture shape has `type = 'image'` and `image = { mediaId, offsetX, offsetY,
  scaleX, scaleY, brightness, contrast, gamma, transparency, unsupported }`;
  offsets/scales are normalised to the shape box.
- A data graphic has `type = 'dataCallout' | 'dataBar' | 'dataIcon'`,
  `attachedTo = <target shape id>` and `callout = { kind: 'text' | 'heading' |
  'bar' | 'icon', field, label, position, gap, autoSize, min, max, showValue,
  iconSet, targetVisioId }`. `dataRowValue`, `calloutTarget` and `calloutText`
  derive the live text; `barRatio` and `iconLevel` derive a bar's fill or an
  icon's 1–5 level.
- A colour rule lives on its target as `dataColor = { field, min, max, low,
  mid, high, baseFill }`; `dataColorFill` derives the visible fill.
- `History` JSON snapshots, limit 200. `snapshot()` stores `{json, media}` and
  excludes `doc.media` from the JSON; the media map is shared by reference
  (records are immutable and only added) so undo does not duplicate picture
  data URLs 200 times.
- `page`, `findShape`, `findConnector`, `shapeById`, `shapeRect`,
  `connectorsFor`.

## `js/render.js`

- Canvas SVG layers, grid pattern, scene transform.
- `buildShape`: parts/catalogue rendering, hit path, text group.
| `buildImage`, `imageFilter`, `imagePlaceholder`: picture crop/clip,
  transparency, sRGB brightness/contrast/gamma and unsupported-format fallback.
- `buildCalloutLeader`, `boxEdgePoint`: leader from a text data callout to its target.
- `buildDataIcon`: five outlined boxes, diamonds or circles with the first
  `iconLevel` glyphs filled. `dataColorFill` overrides a target's fill when a
  colour rule is active.
- `renderText` derives a callout's label from `calloutText()` instead of
  `shape.label`; imported rich field runs are ignored for display. A data bar is
  drawn from `barRatio()` as a fill over a track outline; an icon set is drawn by
  `buildDataIcon` from `iconLevel()`. Headings use the callout box without a
  leader; colour rules replace the target fill via `dataColorFill()`.
- `groupFrames`, `buildGroupLabel`: live group frame union and group-label
  rendering behind members.
- `partGradient`: two-stop bevel gradient.
- `renderText`: wrapping, alignment, vertical alignment.
- `renderRichText`, `fieldText`: styled runs, paragraphs/bullets and live
  page/file/date/Shape Data field values.
- `buildConnector`: route geometry, markers, label background/text.
- `routeGeometry`: path/polyline/line.
- `renderOverlay`: selection, handles, marquee, glue hint, connector preview.
- `dashArray`: dashed/dotted/dashFine.
- `buildDefs`: arrow markers, hollow/filled UML markers.
- `markerFor`: model arrow → marker URL.

## `js/router.js`

- `SIDES`, `sideVector`, `sideLocalPoint`.
- `localToWorld`, `sideVectorFor`, `anchorOnShape`, `anchorOnShapeAt`.
- `shapeCenter`, `bestSide`, `autoSides`.
- `endpointPoint`, `endDirection`.
- `dedupePoints` and `poly`.
- `routeConnector`, `routeMidpoint`.

## `js/shapes.js`

Catalogue keys:

Basic: `rect`, `roundRect`, `ellipse`, `circle`, `diamond`, `triangle`,
`hexagon`, `pentagon`, `parallelogram`, `trapezoid`, `star5`, `cross`,
`blockArrow`, `chevron`, `orShape`, `andShape`, `callout`.

Flowchart: `flowProcess`, `flowDecision`, `flowTerminator`, `flowDocument`,
`flowData`, `flowManualInput`, `flowDelay`, `flowDisplay`, `flowPreparation`,
`flowCard`, `flowStoredData`, `flowDirectData`, `note`.

Data storage: `database`, `server`, `tape`, `cloud`, `cube`.

Network: `router`, `switch`, `firewall`, `web`, `vpn`.

Exports: `SHAPES`, `SHAPE_GROUPS`, `shapeDef`, `shapePath`, `shapeDetail`,
`shapeTextBox`.

## `js/svg-path.js`

- `flattenPath(d, steps=12)` → sub-paths of points; supports M/m, L/l, H/h,
  V/v, C/c, S/s, Q/q, T/t, A/a, Z/z.
- `pathBounds(subpaths)`.

Used for new Visio geometry and connector curves.

## `js/file.js` and `js/util.js`

- `saveLocal`, `loadLocal`, `exportJSON`, `importJSON`; JSON paths normalise
  through `normalizeDocument`/`ensureDocumentVersion`
- `pageBounds`
- `buildSVG`, `exportSVG`, `exportPNG`
- `download` accepts string or Blob
- geometry helpers (`clamp`, `dist`, `rot`, `distToSegment`), text measurement/
  wrapping, font stack.

## `test/`

- `test/MS Example-edited.vsdx` — a saved VSDX output artifact from manual
  testing. It is not a test runner and can be moved/removed when a real test
  fixture set is introduced.
- `test/corpus/images.vsdx` — Visio-authored picture corpus (six pages, five
  media parts); see [Testing](06-testing.md#picture-corpus).
- `test/corpus/datagraphics.vsdx` — Visio-authored Shape Data corpus (two pages,
  four cards) used by the data-graphics, icon-set, heading and colour suites.
- `test/corpus/line-gradient-shadow-2.vsdx` — Visio-authored rectangle with a
  patched red→blue `LineGradient` and an outer blurred `ShapeShdw*` shadow;
  used by the gradient/shadow import and SVG tests.
- `test/corpus/quickstyle-matrices.vsdx` — 18 shapes covering QuickStyle matrix
  themes 1–6 for fill, line and effects.
- `test/corpus/quickstyle-variants.vsdx` — QuickStyle variant matrices 100–103.
- `test/corpus/effects-cells.vsdx` — explicit glow, soft-edge, reflection and
  shadow cells.
- `test/corpus/fill-patterns.vsdx` — FillPattern 2–24 corpus.
- `test/corpus/shadow-types.vsdx` — simple/oblique/page-default shadow corpus.
- `test/corpus/bevel-types.vsdx` — BevelTopType 1–12 corpus.
- `test/golden/manifest.json` — 20-source canonical-model and untouched-patch
  entry-hash manifest; `models/` holds the pretty canonical models.
- `test/golden/visual-manifest.json` + `visuals/` — 37 Visio page-PNG visual
  references with hashes/dimensions; validation-only. See
  [Document schema and golden fixtures](13-document-schema.md).
- `test/golden/provenance.json` — per-fixture origin, review status and
  redistribution policy; audited by `verify-fixture-provenance.mjs`.
- `rust/layoutcraft-model` — typed schema-v1 document model (20/20 canonical
  golden models).
- `rust/layoutcraft-vsdx` — safe OPC/XML/relationship reader, page metadata,
  shape/master trees, `import_document()` and the patch/fresh writers;
  `vsdx-inspect` CLI.
- `rust/layoutcraft-render` — shared scene, deterministic SVG, `resvg` raster,
  picture/text/connector rendering and visual-golden checks.
- `rust/layoutcraft-app` — native `eframe`/`egui` viewer shell (open, page
  navigation, pan/zoom, shape selection and inspector).
- `rust/layoutcraft-conformance` — browser-free fixture/model/patch/visual checker
  (see [Native conformance runner](15-conformance-runner.md)).
- `rust/layoutcraft-fixtures` — generates `fixtures/public/`, the redistributable
  self-authored golden set ([Public fixture replacement](39-fixture-replacement.md)).

## `fixtures/public/`

The self-authored, redistributable golden set: generated sources, canonical
models, manifest and provenance. See `fixtures/public/README.md` and
[Public fixture replacement](39-fixture-replacement.md).

## `examples/`

Private, third-party validation corpus (not in this repository; see
[Private test fixtures](38-private-fixtures.md)). The redistributable set is
`fixtures/public/`.

## `docs/`

Documentation only; not loaded by the app. This folder can travel with the
repository to Windows.

## Quick file-to-task map

| Task | Start here |
| --- | --- |
| Rendering/import appearance | `js/visio.js`, `js/render.js`, `js/shapes.js` |
| Connector routing/hit testing | `js/router.js`, `js/interact.js` |
| Save patch/fresh | `js/vsdx-writer.js`, `js/main.js` |
| Original package cache | `js/vsdx-source.js` |
| UI/inspector | `js/ui.js`, `css/app.css`, `index.html` |
| Windows startup | `server.mjs`, `docs/02-windows-setup.md` |
| Visio validation | `docs/06-testing.md` |
| Remaining work | `docs/07-known-gaps.md` |
