# Rust core: import and render path

Status: 2026-10-28. Headless import and render path of the Rust port.

## Crates

The Rust workspace now contains:

```text
rust/
  layoutcraft-model/       typed portable document model
  layoutcraft-vsdx/        safe VSDX OPC package + page metadata + model importer
  layoutcraft-render/      scene -> SVG -> resvg page raster
  layoutcraft-conformance/ browser-free golden checker
```

### `layoutcraft-model`

- `Document`, `Page`, `Shape`, `ShapePart`, `DataRow`, `Connector`,
  `Endpoint`, `Image`, `Media`, `DataGraphic` and `DataColor`.
- `SCHEMA_VERSION = 1`; `Document::validate()` rejects newer schemas and
  documents without pages.
- Unknown JSON fields are retained in per-structure `extra` maps, so a reader
  does not silently drop future fields.
- `Document::open`, `from_slice`, `from_json`, `to_json_pretty`, `counts`.
- Integration test loads all 20 `test/golden/models/*.json`, validates schema
  and matches page/shape/connector/media counts against the manifest.

### `layoutcraft-vsdx`

- `Package::open` reads an OPC ZIP into memory with per-entry and total size
  limits.
- Rejects absolute paths, `..`, `.` components and backslashes; reads ZIP
  entries without extracting them.
- XML helper rejects `DOCTYPE`/`ENTITY` declarations and parses with
  `roxmltree`.
- Relationship resolution relative to the source part, with escape protection.
- `Package::pages()` reads `visio/document.xml` / the document relationships and
  the page list, resolves each page part, and returns ID, name and part.
- `page_shapes()` parses the recursive page `Shape` tree, including group
  children, IDs, names, types, direct cells and text.
- `MasterCatalog` loads `visio/masters/masters.xml`, master relationships and
  each master's shape index.
- `resolve_page_shapes()` walks the page tree, resolves `Master`/`MasterShape`
  through the inherited master context, merges direct master cells (page cells
  override), and returns `ResolvedShape` trees with master ID/part provenance.
- `import_document()` converts the resolved shape trees into `layoutcraft-model`:
  page names, 2-D shape bounding boxes/rotation/flips, line/ellipse geometry,
  text labels, Shape Data rows, resolved Fill/Line/Character styles, and part
  paint.
- Style resolution loads `visio/document.xml` StyleSheets and the document
  defaults, follows `FillStyle`/`LineStyle`/`TextStyle` chains, and applies the
  cached concrete cells or the role-aware `Themed` fallback. A theme palette
  reader handles Office fallback colours, document `ColorEntry` indices,
  `clrScheme` slots, `vt:bkgnd`, Visio variation palettes and the active
  variation selected from explicit fills. QuickStyle colour indices (including
  the active variant's 100–106 range) and a small colour-formula subset
  (`THEMEVAL`, `THEMEGUARD`, `MSOTINT`, `RGB`, `GUARD`) are resolved; large
  style-matrix fills get the light tint the browser importer applies.
- Text style resolution follows the shape Character row, the master Character
  row and the TextStyle chain, including the theme-font tint for inherited
  `Themed` colours. Font, size, colour, bold/italic/underline and the model
  defaults are populated.
- Rich text is imported from the ordered `<Text>` children: `<cp>`/`<pp>`
  switches, styled runs, line breaks, `<fld>` fields with formula/value/format,
  paragraph align/bullets/indent, and the text box from `Txt*`/margin cells.
  `textFit` is set for `TEXTWIDTH`/`TEXTHEIGHT` boxes. The audit across all 20
  golden models matches 264 shapes (139 rich), including align/vAlign and text
  boxes.
- Master and page sections merge row by row: section cells are overridden by
  the page, rows are matched by IX (geometry, Character, Paragraph, Field) or
  name (Property, User) and their cells are overridden individually. Geometry
  rows sort by IX. `NoFill`/`NoLine` follow page-over-master, while `NoShow`
  follows the page's own geometry when present and is ignored for master-only
  geometry (Visio's cached master flag describes another instance).
- `geometry.rs` ports the cached Geometry row semantics used by the browser
  importer: absolute/relative MoveTo and LineTo, `ArcTo`, `Ellipse`,
  `EllipticalArcTo`, `NURBSTo` with its formula control points, SplineStart/
  SplineKnot/SplineEnd with clamped/periodic B-spline sampling, `InfiniteLine`
  and `PolylineTo` control-point formulas. Paths use ECMAScript-compatible
  2-decimal formatting, page-space vs master-space orientation, and
  fill-dependent closure. The exact-path audit checks all 447 golden shapes.
- `masterbody.rs` resolves a compound master body: each nested master shape's
  Geometry sections are evaluated against the instance (including
  `Sheet.<id>!` references, Scratch/User rows and `Geometry<n>.<cell>`
  references), bevel masters foreshorten the top face, and child fill/line
  cells resolve through the page child and style chain. Direct page children
  contribute their concrete stencil sections and paint overrides, with their
  page-space Y axis flipped into the master body's top-down space; nested
  bodies fall back to master descendants. Every visible Geometry section is
  rendered, not just the primary one, so stencil details (monitor screens,
  keyboards, arrow lines) and decorative 1-D arrow shapes are preserved. An
  explicit master group draws its body rather than the master's own frame; a
  body whose children are all hidden contributes no parts.
- Group members are flattened recursively through composed page-space
  transforms, including rotated groups; shapes get world boxes/rotation and
  root/immediate group metadata. Page/master cell values are carried through.
- Connectors are imported from 1-D shapes + the page `<Connects>` table:
  route style/route, Begin/End glue, target-relative local/side, geometric
  waypoints, labels, route/config cells and z order.
- Pictures are imported from `ForeignData`/page relationships: deduplicated
  media records (MIME, extension, dimensions, adjustment support, SHA-256,
  bytes), crop offsets/scales, brightness/contrast/gamma/transparency and the
  image shape type. Indexed PNG/GIF adjustment cells are ignored as Visio does.
- LayoutCraft-authored data graphics are restored from `User` rows: colour rules,
  callouts/headings/bars/icons, icon sets, levels, min/max, target Visio ID and
  a stable `shape:visio:<id>` attachment; `msvCalloutField` is not exposed as
  Shape Data. Native Visio DataGraphic masters remain flattened shapes.
- `patch_page_cells` is the patch-writer slice: it patches direct ShapeSheet
  cells, plain text, Shape Data rows and simple section/row cells on a page
  shape and writes a new OPC package while copying every other entry
  byte-for-byte. It can also add inline rectangle shapes, connectors and
  pictures (media part, rel, Content-Type and `ForeignData`) and remove shapes
  with their `Connect` rows. Patched/new-shape/connector/picture files open in
  Visio 2021 without repair. Shape lookup matches attribute boundaries, so
  `OriginalID` never collides and shapes containing nested `<Shapes>` can be
  patched or removed. `.rich(...)` writes run/paragraph/field sections,
  `.geometry_rows(...)` replaces Geometry rows, and `write_fresh_document`
  generates a complete new package with document/pages/masters/windows/theme/
  docProps and inline shape/connector geometry. See
  [VSDX patch writer](20-vsdx-patch-writer.md) and
  [Fresh VSDX writer](21-fresh-writer.md).
- `vsdx-inspect` CLI prints entry/page/shape/master counts.
- Integration tests open all 20 golden sources (package/page/shape IDs and
  master cells), and compare the simple corpora, nested/rotated groups,
  connector routes, pictures, rich text, theme/quick-style paints and written
  data-graphic metadata against canonical goldens or known-good generated
  fixtures. All-golden audits cover 447 shapes for fill/stroke/width and part
  count, 264 shapes (139 rich) for rich text, alignment, text boxes, labels and
  text-fit flags, and 447 shapes for exact path data.

## Commands

```powershell
cd LayoutCraft\rust
cargo +stable-x86_64-pc-windows-gnu test --workspace
cargo +stable-x86_64-pc-windows-gnu run -q -p layoutcraft-vsdx --bin vsdx-inspect -- ..\examples\MS Example.vsdx
```

On Linux, use plain `cargo`.

Current results:

- workspace tests: **40/40** (4 conformance, 4 model, 28 vsdx, 4 render)
- model golden fixtures: **20/20** parse and count-match
- VSDX packages: **20/20** open, match page metadata/entry counts and expose
  every canonical shape ID; master-derived shapes resolve with cells
- import slice: 3 simple corpora match geometry/labels/Shape Data, unrotated and
  rotated nested groups match world boxes/rotation/labels/parts/group metadata,
  connectors match routes/glue/waypoints/config, pictures match media/image
  fields, rich text matches 264 shapes across all 20 models, theme/quick-style
  shapes match fill/stroke/font fields, all 447 shapes match
  fill/stroke/width and part counts, all 447 shapes match exact path data, and
  written data-colour/callout metadata round-trips
- `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings` pass

## Not implemented yet

- Native Visio DataGraphic masters and recordsets. LayoutCraft-authored
  callouts/headings/bars/icons/colour rules import as their inline model;
  native masters still flatten to ordinary nested shapes. Text **layout**
  (line wrapping, measurement, autosize) is also renderer work; the model now
  carries the runs, paragraphs, fields, text boxes and fit flags.
- Connector endpoint side at exact box corners is float-ambiguous and the test
  compares local geometry there instead of the side label.
- Master-derived page groups resolve their compound master body. Geometry and
  paint now match all 447 golden shapes exactly; the refreshed baseline was
  generated from this importer after the stencil/theme fixes, while the Visio
  page PNGs remain the external visual oracle. Native Visio DataGraphic
  masters/recordsets remain out of scope.
- The writer covers direct cells, plain/rich text, Shape Data rows, section/row
  cells, replacement Geometry rows, inline shape insertion/removal, connectors,
  pictures and fresh packages including fresh picture/media parts. Visio 2021
  accepts rich-text and Geometry-row patches, shape removal and fresh image
  packages. Remaining writer gaps are master/stencil-backed new shapes and
  curved Geometry reconstruction in fresh output.
- `layoutcraft-app` native shell: an `eframe`/`egui` window that opens a
  VSDX/VSTX from a command-line path, toolbar path or drag-and-drop, rasterizes
  the selected page through `layoutcraft-render`, and provides pan/zoom,
  fit/actual-size, page navigation, shape hit-testing and an inspector summary.
  Dragging moves shapes, corner handles resize them, and the edits are patched
  into the package, re-imported for current geometry/text, and kept in an
  undo/redo byte-snapshot stack. Native Windows Open/Save-as dialogs, Delete,
double-click/Ctrl+E text editing and `+ Rectangle` insertion are wired to the
patch writer. `Save copy` writes a sibling
  `-edited` file; unsaved changes guard Reload/open. Grouped shapes are
  read-only and rotated shapes are move-only. Palette/connector editing and
  grouped-shape editing remain pending.

The scene → SVG → `resvg` raster path is in `layoutcraft-render`; see
[Headless renderer](19-headless-renderer.md). Text now wraps with font-backed
glyph advances and connectors auto-route when the package carries no waypoints.
Rendering all 37 visual-golden pages passes (188/188 visual checks); CFF/
swimlane/list interior fills are modelled. The former theme-quickstyles mismatch is fixed: a package without a
theme part leaves shapes carrying QuickStyle matrices/colours at Visio's
neutral white/black default. Fill and line gradients import and render as SVG
linear gradients, and outer shapes' `ShapeShdw*` drop shadows (offset, blur,
colour, opacity) render behind the geometry. Quick-style fill/line/effect
matrices map through the theme lists (including variant matrices 100-103) with
linear-light DrawingML tint/shade; explicit `GlowSize`, `SoftEdgesSize` and
`ReflectionSize/Dist/Blur/Trans` render as blur, feather and mirrored fade.
`FillPattern` 2-24 render as SVG pattern tiles, oblique shadows use the
`ShapeShdwScaleFactor`, and top/bottom bevel types 1-12 render as a clipped
gradient ring. Exact 3-D lighting/material, `RotationX/Y/Z` and gradient
pattern codes 25-40 remain. Drawing-scale pages scale geometry while text keeps
paper units; master-body children compose rotation/flip and scale into the
instance; master-backed groups expose picture children, and media inherited
from masters resolves via the master relationships. Background-only pages are
skipped, and centre-based `ImageNode` add-in geometry is offset into its box.
Connectors resolve their line through the connector scheme even when they carry
the Normal style, use authored text-pin positions for their labels, and render
multi-line labels; labels outside a filled shape use the theme text colour. The Rust GUI shell supports open/save dialogs,
move/resize, delete, text editing and simple shape insertion; palette and
connector editing are the next GUI work.
