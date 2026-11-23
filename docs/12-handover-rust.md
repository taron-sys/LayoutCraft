# Handover brief: LayoutCraft → Linux independent Rust port

Status: 2026-10-28. The Windows/JavaScript implementation is feature-complete
for the five planned sprints plus the current data-graphics slices. This brief
records what is done, what should be finished before a port, what can move to
Rust, and the contracts the port must preserve.

## 1. What the current implementation proves

**LayoutCraft** is a zero-dependency browser VSDX editor. It imports Visio files,
renders and edits them, and writes both patched and fresh `.vsdx` packages. On
this machine it is validated against Microsoft Visio LTSC Professional 2021
(16.0.14332.20416).

Working areas:

- SVG canvas, selection, resize, rotate, flip, alignment, z-order, multi-page
- shape catalogue, custom/imported multi-part geometry, curves and splines
- glued connectors, straight/elbow/curved routes, connector config cells
- groups, containers, swimlane/CFF authoring and membership
- rich text runs, paragraphs, fields, Shape Data, hyperlinks, autosize
- themes, variations, quick styles (concrete-colour application)
- layered automatic re-layout
- pictures: insert/replace/crop/rotate/flip/adjust/transparency, shared media
- data graphics: text callouts, headings, horizontal numeric bars, five-level
  icon sets and three-band colour-by-value rules
- VSDX/VSTX import; VDX best effort; patch and fresh VSDX writers

Current verification:

| Suite | Result |
|---|---|
| `acceptance.mjs` matrix | 21/21, `visio-test-runs/r40` |
| `test-datagraphics.mjs` | 47/47 |
| `test-iconsets.mjs` | 41/41 |
| `test-headings.mjs` | 19/19 |
| `test-colorbyvalue.mjs` | 24/24 |
| `test-schema.mjs` | 16/16 |
| `test-images.mjs` | 54/54 |
| golden fixtures (`verify-golden-fixtures.mjs`) | 20 sources / 200 checks |
| Rust conformance runner | 273/273 fixture + visual-reference checks; 353/353 with candidates (390/390 with page comparisons) |
| fixture provenance audit | 154/154 private inventory; redistributable set replaced by self-authored fixtures (`fixtures/public/`, 44/44 checks, [doc 39](39-fixture-replacement.md)) |
| Rust model + VSDX import slice | 40/40 workspace tests; 20/20 canonical models/packages; all 447 shapes match paint, part counts and exact path data against the refreshed canonical baseline; scene/SVG/`resvg` renderer in place and 37/37 visual-golden pages pass; patch writer handles cells, plain/rich text, Shape Data, section/rows, replacement Geometry rows, shape add/remove, connectors and pictures with byte-identical untouched entries and Visio acceptance; fresh writer emits complete packages including pictures/media |
| Rust GUI shell | `layoutcraft-app` opens VSDX/VSTX (native file dialog or drag-drop), renders/navigates pages, selects shapes, moves/resizes ungrouped shapes, deletes, edits text inline, inserts rectangles, undoes/redoes, and saves a patched copy or Save-as. Grouped shapes are read-only and rotated shapes are move-only. Palette/stencil and connector editing remain next. |
| Rust headless renderer | SVG + PNG from one scene; 37/37 Visio page exports pass the tolerance (188/188 visual checks; CFF/swimlane interior fill included), image adjustment/crop pages pass after media embedding and filters |
| Visio COM validation | opens without repair; live fields, bars, icon levels, headings and fills update; Visio re-save re-imports |
| untouched patch saves | every ZIP entry byte-identical, including the picture and Shape Data corpora |

The key engineering invariants are:

1. **Untouched patch saves are byte-identical per ZIP entry.** The original
   package is the source of truth; untouched parts, masters, themes, media and
   macros are copied unchanged.
2. **Unknown data is preserved.** The model is deliberately small; package
   bytes stand in for concepts the editor does not model.
3. **Live Visio behaviour comes from formulas, not from the editor's cache.**
   Callout and heading fields use `Property.msvCalloutField`; bars use a width
   formula; icons use a level formula and per-glyph `NoShow`; colour rules use a
   target `FillForegnd` RGB formula.
4. **Provenance.** Behaviour was learned from file packages, the
   ShapeSheet and COM automation. No Visio source or binary code is copied.
   Built-in stencils were inspected only to understand the observable
   conventions; the editor's icons are its own simple glyphs.

## 2. What remains

### 2.1 Data graphics

| Item | Size | Recommendation |
|---|---|---|
| More icon styles and 3/4-level sets | Medium | Defer to Rust or do opportunistically; it is mostly glyph data and metadata. |
| Vertical/full Text/DataBar/IconSet matrix definitions | Medium | Defer; the current horizontal/5-band forms cover the core. |
| Native DataGraphic master import (read-only) | Medium/large | Out of scope for v1; imports are ordinary nested shapes. Preserve package bytes in patch mode. |
| Native DataGraphic authoring (`GraphicItems`, hidden masters) | Large | Out of scope for v1; the project's own inline representation is deliberate and Visio-validated. |
| Data recordsets | Large | Out of scope for v1; separate data-integration workstream. |

The current data-graphic schema is a callout object plus a target rule:

```text
shape.callout = {
  kind: 'text' | 'heading' | 'bar' | 'icon',
  field, label, position, gap, autoSize,
  min, max, showValue,
  iconSet, targetVisioId
}
shape.attachedTo = <target id>
shape.dataColor = {
  field, min, max, low, mid, high, baseFill
}
```

The Rust enum can start as `Text | Heading | Bar | Icon` plus a separate
`DataColor` target rule; there should be no need to reopen the item-type model
after the port.

### 2.2 Model-level fidelity (decide before the port)

These affect the document model, not just rendering, so deciding them late is
expensive:

- **Theme part / theme-index cells**: the editor currently applies concrete
  colours; it does not rewrite `theme1.xml` or full quick-style matrices. The
  Rust importer now resolves the theme palette, StyleSheet chains, quick-style
  colour indices and the active variation into concrete shape fields, and it
  imports the rich-text run/paragraph/field model. Decide whether the writer
  must preserve/rewrite theme references or only write the resolved styles.
- **Layers, page setup, backgrounds, guides, comments/review**: currently
  preserved only as untouched package bytes. If the Rust editor must edit them,
  model them before porting; otherwise declare them out of scope for v1.
- **Native data graphics and recordsets**: decided out of scope for v1 in
  [0001 — v1 scope and provenance](14-v1-scope-provenance.md); patch preserves
  them, fresh saves flatten with a warning.
- **Per-part styles**: imported multi-part shapes are flattened to one fill/line
  on save; the renderer supports per-part display. A Rust model could keep parts
  natively, but then the writer contract and tests change.
- **Rotated groups**: imports carry the transform chain; a whole-tree move is
  safe. Rotated resize and single-member moves are skipped with a warning.
  Decide whether v1 needs them.

### 2.3 Fidelity that can safely move to Rust

- non-uniform `SplineStart` knots; full obstacle avoidance and rendered line
  jumps; radial/circular/orthogonal layouts; lane-aware task layout
- EMF/WMF/TIFF rasterisation; VDX image resolution; media garbage collection;
  IndexedDB/embedded database for very large picture sets
- canvas text-selection formatting and text-level hyperlink runs
- `.vsd`/VDX bridge and BPMN helpers, only if explicitly in scope

## 3. Conditions for a sensible handover

Do these while the Visio oracle and current harness are still available:

1. **Schema versioning is in place.** `schemaVersion` is 1; `js/schema.js`
   normalises legacy documents, rejects newer schemas, and owns the migration
   table. `docs/13-document-schema.md` and `schema/*.json` describe the
   contract. Future model changes must bump the version and add a migration.
2. **Golden fixtures are in place.** `test/golden/manifest.json` covers the 20
   source examples/corpora with canonical model hashes and every untouched ZIP
   entry hash; `models/` holds the pretty canonical models. Generation is
   deterministic and verification is 200/200. This replaces the older idea of
   hand-capturing a few ad-hoc outputs.
3. **The conformance suite is portable.** `rust/layoutcraft-conformance` reads
   the same manifest without a browser, verifies sources/golden models and can
   compare candidate models/patches. See
   [Native conformance runner](15-conformance-runner.md). Keep Visio COM in a
   Windows-only reference folder.
4. **Freeze the finished data-graphic model.** Color-by-value and headings are
   done; use them (with callouts, bars and icons) as the regression baseline.
5. **Provenance is recorded** in
   [0001 — v1 scope and provenance](14-v1-scope-provenance.md): no Visio
   source/binary code, no copied stencil assets, no proprietary fonts, and
   Microsoft-authored fixtures are validation-only pending licence review.
6. **The v1 scope is decided** in the same record: import/render/edit,
   patch/fresh save, pictures, Shape Data, data graphics, groups/containers,
   connectors, resolved themes and the listed exclusions.
7. **Record the Visio build and oracle scripts.** The validation memory and
   `visio-test-harness/` contain the COM recipes, including the `Page.Export`
   async/stale-frame caveat and `ResultStr` locale trap. These are needed if a
   future Windows VM must recapture goldens.
8. **Fixture provenance is inventoried** in `test/golden/provenance.json`: the private 20-source set is validation-only
   and `review-required`, and is not distributed. The redistributable set is self-authored (`fixtures/public/`, 44/44 checks,
   [Public fixture replacement](39-fixture-replacement.md)); external distribution no longer depends on clearing the private set.
9. **The desktop UI toolkit is decided** in
   [0003 — v1 desktop UI toolkit](17-ui-toolkit.md): `eframe`/`egui` for the
   shell and a shared SVG/reSVG raster path for canvas, export and visual
   goldens.

## 4. Suggested Rust architecture

A workspace with separable crates keeps the port testable without a UI:

```text
layoutcraft-model      // pages, shapes, connectors, groups, media, data graphics,
                    //   rich text, Shape Data, JSON schema/versioning
layoutcraft-sheet      // ShapeSheet cell/formula parsing, cached-value policy,
                    //   optional formula evaluation; geometry sampling
layoutcraft-vsdx       // OPC zip, XML DOM/streaming, rels/content types,
                    //   patch writer, fresh writer, media parts
layoutcraft-render     // resolved styles → SVG or raster; text measurement;
                    //   selection/overlay-agnostic scene
layoutcraft-app        // UI + commands + undo/redo (egui/iced/slint or WASM web)
```

Suggested crates (verify current licenses and versions at port time): `zip`,
`quick-xml` and/or `roxmltree`, `serde`/`serde_json`, `image`, `resvg`/`usvg` or
`tiny-skia`, `fontdb`, `clap` for a conversion/validation CLI.

Design rules:

- **Model first, UI second.** The current JS model is a good starting point; the
  Rust model should be typed enums/structs with explicit optional fields, not a
  bag of strings.
- **Keep patch and fresh writers separate.** Patch mode must copy original
  package bytes and touch only the parts it means to change. Fresh mode may
  regenerate everything and must still open in Visio.
- **Deterministic output.** Canonical part order, stable XML attribute order,
  fixed timestamps in generated metadata, and a manifest of entry hashes make
  byte-level regression tests possible. Archive-level bytes need not match, but
  entry contents should.
- **Formula policy.** The current importer mostly reads cached values and
  samples geometry; the writer emits live formulas for Visio. Decide whether v1
  evaluates formulas or only emits/caches them. Do not let a half formula engine
  block import.
- **Text measurement.** Auto-size and page-PNG goldens depend on fonts. Bundle a
  known font set or accept documented tolerance from the start.
- **No COM on Linux.** Keep Visio as a capture tool, never a runtime dependency.
  Linux CI tests files and models, not a Visio UI.

## 5. Recommended port sequence

1. `layoutcraft-model` + JSON import/export + golden model tests.
2. `layoutcraft-vsdx` reader: OPC/XML/rels/content types, page/master/theme/media,
   cached cells, geometry sampling.
3. `layoutcraft-render`: resolved styles, shapes, connectors, rich text, pictures,
   callouts/bars/icons; page PNG/SVG comparison against goldens.
4. Patch writer: preserve untouched entries and assert per-entry hashes; then
   implement shape/text/connector/picture/Shape Data diffs.
5. Fresh writer: all pages, masters/style shell, media parts, relationships.
6. Editing UI and undo/redo.
7. Native data graphics/recordsets are out of v1 scope; revisit after the core port.

## 6. Risks and traps

- **Byte identity is not free.** ZIP compression, XML namespace/attribute order
  and self-closing tags can all change entry bytes. The current suite compares
  entry *contents*; preserve that contract rather than archive hashes.
- **Live fields vs cached text.** Visio can show a field placeholder in
  `Shape.Text` while the page renders the value. Tests must use page PNGs or
  cell formulas, not `Shape.Text`.
- **String Shape Data needs `U='STR'`.** Without it Visio reads 0 and fields are
  blank. This is easy to regress in a new writer.
- **Visio's `ResultStr` is locale-formatted.** `4,0000` in this environment
  becomes 40000 under a naive PowerShell cast; use `ResultIU` for numeric cells.
- **`Page.Export` can return stale frames** if several exports run in one
  automation session; re-open the saved file in a fresh Visio instance for
  final-state images.
- **Native DataGraphic masters are not ordinary shapes.** Importing them as
  flattened shapes is a declared limitation; do not assume the inline
  representation is a byte-compatible master.
- **Font and locale differences** will change text layout and auto-size across
  Windows/Linux. Capture goldens with explicit fonts and document tolerance.
- **Schema and goldens move together.** Bump `schemaVersion` and add a
  migration when the model changes, then regenerate `test/golden/`; never compare
  canonical models across different schema versions.

## 7. Suggested next three moves

1. Keep the visual goldens and the Rust canonical model baseline in
   sync; synchronise the JavaScript model generator if it is to serve as a
   second cross-check again.
2. Replace or clear the validation-only fixtures before external distribution.
3. Continue the native shell (`layoutcraft-app`) from the viewer slice: palette,
   editing, writer integration, then text/IME. Keep native
   data-graphics/recordsets out of v1 unless scope changes.

After those, the port is a reimplementation of a stable contract rather than a
moving target.
