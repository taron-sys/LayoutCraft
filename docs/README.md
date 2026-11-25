# LayoutCraft — handover documentation

Last updated: 2026-10-28

This folder documents the current state of LayoutCraft from the VSDX import,
round-trip save, authored-object save, fresh-package and feature-sprint work. It
is intended to be read before changing the importer or writer.

## Start here

1. [Overview and status](01-overview.md)
2. [Running on Windows](02-windows-setup.md)
3. [Architecture](03-architecture.md)
4. [Visio import](04-visio-import.md)
5. [VSDX save](05-vsdx-save.md)
6. [Testing and Visio validation](06-testing.md)
7. [Known gaps and next steps](07-known-gaps.md)
9. [File reference](09-file-reference.md)
12. [Handover brief for a Linux/Rust port](12-handover-rust.md)
13. [Document schema and golden fixtures](13-document-schema.md)
14. [v1 scope and provenance](14-v1-scope-provenance.md)
15. [Native conformance runner](15-conformance-runner.md)
17. [v1 desktop UI toolkit](17-ui-toolkit.md)
18. [Rust core: model and VSDX package](18-rust-core.md)
19. [Headless scene/SVG/raster renderer](19-headless-renderer.md)
20. [VSDX patch writer](20-vsdx-patch-writer.md)
21. [Fresh VSDX writer](21-fresh-writer.md)
44. [Document model: in-memory source of truth, undo journal, recalculation, save (design)](44-document-model.md)

## Test fixtures and distribution

- [Provenance statement](../PROVENANCE.md) — what the project claims, and what it does not
- [38 Private test fixtures](38-private-fixtures.md)
- [39 Public fixture replacement](39-fixture-replacement.md) — the redistributable
  self-authored set in `fixtures/public/`
- [41 Legal-risk audit loop](41-legal-audit-loop.md) — the repeatable audit prompt
  and the residual owner/counsel questions
- [Feature names](feature-names.md) — naming checks and the labels to ship (naming policy record, not legal advice)

## Quick start on Windows

Prerequisites: current Chrome or Edge, Node.js 18+ (or Python 3), and Microsoft
Visio 2021/365 for validation.

```powershell
cd C:\path\to\LayoutCraft
npm start
# open http://localhost:8137/
```

There is no build step and no npm dependency install. `server.mjs` is a small
static file server and now uses `fileURLToPath`, so it is Windows-safe. A
`python -m http.server 8137` also works if Python is available.

## What is currently true

- The editor is a no-dependency browser app in plain ES modules.
- `.vsdx`, `.vstx`, `.vsdm` and `.vstm` files are imported by reading the OPC
  package directly in the browser. `.vdx` is imported through the same page
  builder; `.vsd` is not supported.
- Opening a VSDX retains the original package. Saving can **patch** that
  package, preserving unknown parts, masters, images and macros.
- New shapes, connectors and pages created in LayoutCraft are written as inline
  Geometry objects without a stencil.
- Page-level Visio groups import with membership metadata and select/move as a
  unit; `Ctrl+G`/`Ctrl+Shift+G` group editor objects. Containers/lists import
  their real `Relationships` membership and selection is container-aware.
  Rotated groups import with the composed transform and save a whole-group
  move; horizontal/vertical swimlanes and CFFs can be authored with lanes,
  phases and structured lane edits. Rich text/fields, Shape Data, autosize and
  hyperlinks import/edit/save, connector route style/fixed/walk/line-jump
  config is editable, and themes/quick styles plus document default colours
  are applied. A layered auto-layout re-arranges the selection or page
  top-down/left-to-right (groups as units; containers, labels and backdrops
  stay), and the labelled Connect tool creates arrow connectors by clicking a
  source and target. Pictures import/render/crop/adjust/save, and can be
  inserted, replaced or dropped in. Shape Data can drive text callouts and
  numeric bars that follow their target and update live. Rotated resize and
- If no original package is available, or the document was created from JSON,
  a **complete new VSDX package** is generated.
- Deleting a page while patching offers to rebuild a fresh package with the
  page omitted.
- All nine files in `examples/` have been rebuilt from scratch, re-imported,
  ZIP-checked and opened in LibreOffice. They and the patched/fresh/added-page
  outputs have now also been opened in Microsoft Visio LTSC Professional 2021
  without repair dialogs; see [Testing](06-testing.md) for the matrix and the
  connector-glue defect that run found and fixed.
- The working copy was not a Git repository when this documentation was
  written. `08-phase-history.md` reconstructs the development sequence from the
  session; initialise Git on the Windows machine before further work.

## Most important handover caveat

The writer is deliberately pragmatic rather than a full Visio object model.
It round-trips a lot of real-world content, but a fresh rebuild flattens:

- imported masters into inline Geometry
- per-part fills/strokes into one fill/line per shape
- rich text is no longer flattened: runs, paragraphs, fields, Shape Data and
  hyperlinks are modelled; canvas text-selection formatting and text-level
  hyperlink runs remain editor gaps
- pictures are no longer flattened: media parts, page relationships and
  Content-Types are written; EMF/WMF/TIFF are preserved but not previewed
- Shape Data and LayoutCraft-authored text callouts, headings, data bars, icon sets
  and colour-by-value rules are written; native DataGraphic masters and comments
  are not
- macros/custom properties, OLE/ink and content that never entered the editable
  model

Patching an existing package preserves all of that as package bytes, but it
still has a small set of unsupported edits documented in
[Known gaps](07-known-gaps.md).

Further notes (research, working logs, legal and release preparation) are kept in a private repository and are not part of this one.

## Recommended first actions on Windows

1. Read [Running on Windows](02-windows-setup.md) and start the server.
2. Open each file in `examples/` and compare against its PNG thumbnail where
   one exists.
3. Save a patched copy of **MS Example.vsdx**, open it in Visio and confirm
   Visio does not show a repair dialog.
4. Save a fresh copy of a new/blank document and of **MS Example.vsdx**
   imported as JSON, then open both in Visio.
5. Work through the checklist in [Testing](06-testing.md).
6. Record every Visio repair prompt or visual mismatch in
   [Known gaps](07-known-gaps.md) before changing code.
