# 0001 — v1 scope and provenance

- **Status:** Accepted for engineering handover
- **Date:** 2026-10-28
- **Revised:** 2026-11-22 — fixtures replaced by the self-authored public set ([doc 39](39-fixture-replacement.md))
- **Scope:** LayoutCraft → Linux-first, independent Rust implementation
- **Related:** [Handover brief](12-handover-rust.md), [Document schema](13-document-schema.md), , [UI toolkit](17-ui-toolkit.md)

This record decides what the Rust v1 will deliver and what must not enter the
project. It is an engineering decision. Since 2026-11-22 the
distributable fixture set is self-authored and marked cleared (see
[Public fixture replacement](39-fixture-replacement.md)), so no third-party
fixture or Visio-rendered image is published; the fixture-licensing question no
longer blocks distribution.

## Decision

v1 is a Linux-first native VSDX editor plus a headless core library/CLI. It must
open Visio-authored VSDX/VSTX files, edit and render them, and write both
**patched** and **fresh** VSDX packages with the behaviour and safety
invariants described below.

The JS implementation remains the behavioural reference. The Rust port should
consume `test/golden/manifest.json` and reproduce the canonical model and
untouched-patch entry hashes without a browser. Visio 2021 on Windows is a
capture/validation oracle only; it is not a Linux runtime dependency.

## v1 scope

### In scope

| Area | v1 commitment |
|---|---|
| Import | VSDX and VSTX; page, shape, connector, group, container, master flattening, pictures, text, Shape Data, themes |
| Document | Schema v1 JSON import/export, autosave, ordered migrations, newer-schema rejection |
| Save | Patch mode preserving the source package and untouched entry bytes; fresh mode regenerating a Visio-openable package |
| Canvas | Multi-page editor, selection, move/resize/rotate/flip, z-order, undo/redo, grid/snap, pan/zoom |
| Shapes | Catalogue shapes, imported multi-part geometry, arcs/splines/NURBS/polylines, text boxes |
| Connectors | Glue, straight/right-angle/curved routing, config cells, arrowheads, labels |
| Groups/containers | Import, selection, move/resize, grouping, container/CFF authoring and membership |
| Text | Rich runs, paragraphs, bullets, fields, Shape Data, hyperlinks, autosize |
| Data graphics | Text callouts, headings, horizontal bars, five-level boxes/diamonds/circles icons, three-band colour by value |
| Pictures | PNG/JPEG/GIF/BMP/WebP/SVG insert/replace/crop/adjust/transparency; shared media parts |
| Styling | Resolved themes, variations and quick styles; Fill/Line/Text controls |
| Layout | Layered top-down/left-to-right re-layout; groups and containers as units |
| Export | VSDX, JSON, SVG, PNG |
| Headless CLI | Import/export, golden conformance, package inspection; suitable for CI |
| Security | Untrusted-file limits for ZIP/XML/image content; path traversal and entity-expansion defences |
| Validation | Linux golden/schema tests; Windows Visio acceptance for patch/fresh outputs and live data graphics |

### Explicitly out of scope for v1

- Native DataGraphic **authoring** (`GraphicItems`, hidden composite masters) and
  editable native DataGraphic item import. Native graphics are preserved in
  patch mode and flattened to ordinary shapes in fresh mode, as today.
- Data recordsets and external data
  including recordset-driven graphics.
- Comments/review, change tracking, co-authoring.
- Theme XML rewriting, full quick-style matrices/effects, background graphics.
  v1 writes resolved concrete styles, not theme references.
- Editing layers, page setup, backgrounds, guides, headers/footers.
- `.vsd` binary support and VDX bridging. VDX import is not a v1 promise.
- EMF/WMF/TIFF rasterisation. Preserve bytes and show a placeholder, as the JS
  app does.
- BPMN helpers, VBA/macros, OLE/ink editing, external formula VM.
- Per-part fill/line editing; fresh saves may flatten parts to one fill/line
  with a warning.
- Rotated-group resize and single-rotated-member edits beyond the current
  warning/skip policy.
- Full obstacle avoidance, rendered line jumps, radial/circular/orthogonal
  layout styles, RTL routing.
- Media garbage collection/media manager beyond preserving referenced parts.
- Pixel-perfect visual parity with Visio. Structural/model/package conformance
  is the v1 gate; the Visio page-PNG goldens are an optional smoke reference.

### Accepted limitations

These are known and should not be re-litigated as v1 bugs:

- Native DataGraphic and recordset documents may render differently from Visio.
- Theme-picker behaviour writes concrete colours; Visio's theme index cells are
  not rewritten.
- Imported per-part styling is flattened on fresh saves (patch preserves the
  original package bytes for untouched parts).
- Horizontal bars, five-level icon sets and three equal colour bands are the v1
  data-graphic palettes; extended matrices are deferred.
- The Visio `Shape.Text` field placeholder is not a reliable visual oracle; use
  page renders/cell formulas.

## Provenance and asset rules

1. **No Microsoft source or binary code.** Do not use decompiled, disassembled
   or extracted Visio code, copied implementation, or extracted binary
   routines. A behaviour must be justified by public file-format documentation,
   files the application generates or saves, ShapeSheet cells read through the
   published automation interface, observable COM state, or rendered pages; the
   evidence for it belongs in the provenance record. This document does not
   claim that these are the only sources ever consulted, nor a separated-team
   process ([provenance statement](../PROVENANCE.md)).
2. **No copied stencil assets.** `DGICON_M.VSSX`, `DVCALL_M.VSSX` and similar
   Visio stencils may be inspected only to understand conventions such as
   `msvCalloutField`, `NoShow` and level formulas. Their geometry, icons,
   previews and other assets must not be copied into the product. The JS/Rust
   icon glyphs are original simple shapes.
3. **Fonts.** Do not redistribute Calibri, Segoe UI or other proprietary fonts.
   v1 may use system fonts or open-licensed fonts. Preserve VSDX font names in
   the package; document the rendering fallback and accept text-metric drift.
4. **The public fixture set is ours; the third-party set stays private.** The
   distributable golden set is `fixtures/public/`, generated by LayoutCraft from
   our own drawing operations and recorded as self-authored/cleared
   ([Public fixture replacement](39-fixture-replacement.md)). The
   Microsoft-authored example `.vsdx/.vstx` files, the probe drawings and the
   Visio-exported PNGs are validation-only: they live only in the private
   location ([Private test fixtures](38-private-fixtures.md)) and are never
   published, packaged or embedded. Do not add them, or anything derived from
   them, to the public repository, CI artefacts or releases. A manifest records
   hashes; it does not relicense a file.
5. **No Visio branding.** "Visio" is used descriptively for compatibility.
   Do not present the product as Microsoft Visio or use Microsoft marks/logos.
6. **Third-party code.** Audit every Rust crate and its transitive licences
   before release. Prefer MIT/Apache-2.0-compatible dependencies; do not add
   copyleft dependencies without an explicit decision. Produce a dependency
   inventory/SBOM.
7. **Validation output.** Visio page PNGs, COM probes and captured XML are
   engineering evidence, not shipped assets.
8. **Provenance log.** Keep a short record of external sources used for
   behavioural research and their role (file format, COM observation, public
   documentation). Do not add Microsoft binaries to the repository.

## v1 acceptance gates

1. **Golden model/package conformance.** The browser-free Rust runner
   (`rust/layoutcraft-conformance`) passes 20/20 private sources and all 200
   fixture checks (202 including the two manifest checks): source hashes,
   canonical model hashes, counts, and every untouched ZIP entry hash. It also
   verifies 37 page-PNG visual references (273/273 total) and candidate
   models/patches (353/353). Visual comparison is a smoke check, not a
   pixel-parity gate. The redistributable self-authored set
   (`fixtures/public/`, [doc 39](39-fixture-replacement.md)) passes 44/44 checks
   with no visual references.
2. **Schema contract.** Schema version 1 is read/written; version 0 migrates;
   newer versions are rejected without replacing the open document;
   import/export/autosave round-trip.
3. **Visio acceptance.** Patched and fresh outputs open in Visio 2021 without a
   repair dialog; untouched patches are entry-byte-identical; live callouts,
   headings, bars, icon levels and colour formulas update; a Visio re-save
   re-imports.
4. **Editing smoke matrix.** Move/resize/rotate, connector glue/route changes,
   group/container operations, rich text/Shape Data, picture crop/adjust,
   data-graphic edits, undo/redo, multi-page and page add/delete.
5. **Security smoke.** Reject or safely handle ZIP path traversal, oversized
   entries, XML external entities/entity expansion, malformed XML, decompression
   bombs and image-decode bombs within documented limits.
6. **Provenance checklist.** No Microsoft binaries/assets/fonts in the product;
   every fixture in the redistributable set is self-authored and marked cleared
   ([doc 39](39-fixture-replacement.md)); the third-party corpus is
   validation-only, private and not distributed; the private inventory audit
   passes (154/154). Dependency licences are inventoried.

## Port deliverables

- `layoutcraft-model` (typed model, schema v1, migrations)
- `layoutcraft-sheet` (cell/formula policy and geometry sampling)
- `layoutcraft-vsdx` (OPC/XML, patch/fresh writers, media)
- `layoutcraft-render` (resolved styles, SVG/raster, text measurement)
- `layoutcraft-app` (desktop editor) and `layoutcraft-cli` (headless conformance)
- Golden/schema test runner that reads `test/golden/manifest.json` (private
  oracle) and `fixtures/public/manifest.json` (redistributable set)
- Windows-only Visio acceptance notes kept outside the Linux build
- Scope/provenance record (this document) and licence inventory

The exact UI toolkit is deliberately not fixed here; it is an architecture
decision for the UI workstream, provided it can render the scene and provide
the editing surface in the v1 scope.

## Consequences

- The port has a bounded, testable feature set and can start without waiting
  for native DataGraphic or recordset support.
- Some Visio documents will preserve native graphics only in patch mode; fresh
  saves will flatten them with a warning.
- Theme fidelity and layer/page-setup editing remain intentionally weaker than
  Visio.
- The distributable golden set is self-authored
  ([doc 39](39-fixture-replacement.md)); the Visio-authored corpus and its
  Visio-exported PNGs are validation-only and are not distributed.
- The schema version must move with model changes; regenerate the golden
  manifest in the same change and never compare canonical models across schema
  versions.

## Open actions

1. **Resolved 2026-11-22: replaced, not cleared.** The example/template fixtures
   and Visio-exported PNGs are not part of any distributed set; the public
   golden set is generated by our own engine and is self-authored/cleared
   ([doc 39](39-fixture-replacement.md)). The private corpus keeps its
   validation role and is never published.
2. Start `layoutcraft-model`/`layoutcraft-vsdx` against the browser-free runner.
3. Tune visual thresholds once the Rust renderer can produce page PNGs.
4. Confirm target platform matrix (Linux x86_64/aarch64, Windows/macOS).
