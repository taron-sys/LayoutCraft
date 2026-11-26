# Overview and current status

> The JavaScript editor this overview describes was removed ([42](42-javascript-removal.md); its last commit is `c9aa852`). The product is the Rust core with a native app and a browser shell; the feature lists below record what the editor did.

## What LayoutCraft is

LayoutCraft is a diagram editor that can come as a Webapp or independent Desktop app.
It has its own JSON document model and exports JSON/SVG/PNG, but most of the recent 
work has gone into Microsoft Visio interoperability.

The application consists of:

- a catalogue of basic, flowchart, data-storage and network shapes
- a pannable/zoomable SVG canvas
- selection, resize, rotate, mirror, text editing and formatting
- glued connectors with straight, right-angle and curved routes
- pictures: insert/replace/crop/adjust imported and authored images
- Shape Data with text callouts, headings, numeric bars, five-level icon sets and colour-by-value rules bound to a shape's properties
- multiple pages, autosave to localStorage, undo/redo
- a VSDX/VDX importer
- a VSDX writer with two modes:
  - **patch** an opened `.vsdx/.vstx/.vsdm/.vstm`
  - **build** a complete new package from the editable document

Run it locally:

```sh
cd rust
cargo run --release -p layoutcraft-app   # the desktop app
cargo xtask serve                        # the browser shell: http://127.0.0.1:8137/web/shell.html
```

No Node.js is needed (the browser shell also needs `web/layoutcraft.wasm`, see the [docs index](README.md)).

## Phase status

| Phase | Scope | Status |
| --- | --- | --- |
| A | Retain original package; patch imported shapes/connectors; preserve unknown package entries; delete imported objects | Done |
| B | Write new shapes, connectors and pages; write connector routes/endpoints/glue | Done |
| C | Build a complete VSDX from scratch; fresh fallback when no package is available; page deletion through rebuild | Done |
| D | Real-Visio acceptance, harness, repair-dialog detection and round-trip fixes | Done |
| E | Feature/fidelity sprints: groups/containers, swimlane/CFF authoring, rich text/fields/Shape Data/autosize/links, connector routing/config, themes, auto-layout, pictures/media, data graphics | Sprints 1–4 plus spline/NURBS/arc/infinite-line geometry, layered auto-layout, pictures, text/data graphics (callouts, headings, bars, icons, colour by value) complete; remaining backlog below |

Phase D is a suggested name for the remaining work. The phases are project
labels, not formal Visio terminology.

## Loading status

Supported input:

| Format | Support |
| --- | --- |
| `.vsdx` | Direct OPC/XML import in the browser |
| `.vstx` | Same package format as `.vsdx` |
| `.vsdm` | Same package format; macros are not interpreted |
| `.vstm` | Same package format; macros are not interpreted |
| `.vdx`, `.vsx`, `.vtx` | Visio 2003 XML, converted to a new `.vsdx` package ([47](47-legacy-formats.md)) |
| `.vsd`, `.vss`, `.vst` | Binary format, file versions 6 and 11 (Visio 2000-2010), converted to a new `.vsdx` package; version 5 and older are refused ([47](47-legacy-formats.md)) |

The importer resolves masters, nested master bodies, style sheets, theme
colours, quick styles, geometry rows (including NURBS, splines, elliptical and
circular arcs and infinite lines) and connector glue. See
[Visio import](04-visio-import.md) for the exact coverage.

Known import gaps include OLE/ink and EMF/WMF/TIFF previews, VDX images,
layers/page setup/backgrounds, comments and Visio-native DataGraphic masters,
full formula evaluation and complete group/container semantics. Pictures
(PNG/JPEG/GIF/BMP/WebP/SVG) and Shape Data with text data callouts, bars and
icon sets and colour-by-value rules are imported, rendered, edited and saved.

## Saving status

Save routing of the JavaScript editor (`app.saveVSDX` in its `main.js`); the Rust core keeps the two modes, `writer.rs` (patch) and `fresh.rs` (build), in `layoutcraft-vsdx`:

1. If a matching original package is retained, **patch** it.
2. If a cached package can be restored by the document's `visioKey`, use it.
3. Otherwise **build a fresh package** from the model.
4. If a page was deleted while patching, ask to rebuild a fresh package with
   that page omitted.

Patch mode:

- updates only changed page parts
- preserves every other package entry byte-for-byte
- writes new objects as inline Geometry
- deletes objects and their `<Connects>` rows
- updates connector endpoints/geometry/glue when a route is changed
- adds pages with page parts, `pages.xml` entries, relationships, content types,
  page sheets, layers, sizes and page rels
- patches picture boxes, crop and adjustment cells; a replaced picture adds a
  media part, page relationship and Content-Type entry
- patches Shape Data and writes data callouts, headings, bars and icon sets
  (transport property, field/level, bar/icon Geometry, User rows and
  target-relative position formulas) and colour rules (`FillForegnd` formulas)

Fresh mode:

- generates the whole OPC package
- generates document styles/theme/windows/properties
- writes every page/shape/connector inline
- writes picture media parts, page image relationships and Content-Types
- writes Shape Data and data-callout/heading/bar/icon shapes with live fields,
  bar fills and conditional icon geometry, plus target fill formulas for colour
  rules
- uses page IDs from 0
- preserves model z-order
- resets the in-app document to a fresh lineage after a successful save

See [VSDX save](05-vsdx-save.md) for details.

## Output naming and lineage

- Patched copy: `<name>-edited.<same extension>`, e.g.
  `MS Example-edited.vsdx`, `tf33958593-edited.vstx`.
- Fresh package: `<name>.vsdx`.
- After a fresh build, the current document drops its old `visioKey`, page
  provenance, and per-object Visio IDs. Subsequent saves rebuild from the
  editable model rather than trying to patch the old package. Undo snapshots
  made before the fresh build still contain the old metadata.

## Verified on Windows

See [Testing](06-testing.md) for the full matrix and the latest run. Highlights:

- all nine files in `examples/` and the Visio-authored `test/corpus/` files
  import
- untouched patched saves report `patchedPages: 0`, and every package entry is
  byte-identical to the source (examples, picture corpus and Shape Data corpus)
- edits to imported shapes/connectors, pictures, crops, adjustments, Shape Data
  and data graphics round-trip through re-import
- new shapes/connectors/pages, picture media parts and page relationships
  round-trip
- fresh rebuilds of all nine examples, the picture corpus and the data-graphics corpus re-import
- patched and fresh outputs open in Visio 2021 without repair dialogs; the
  deliberately broken-package control does produce one
- JSON documents carry `schemaVersion: 1`; legacy documents migrate, newer
  schemas are refused, and exported/autosaved JSON is the portable interchange
- the 20-source golden manifest (`test/golden/manifest.json`) pins canonical
  model hashes and every untouched ZIP entry hash; verification is 200/200
- the browser-free Rust runner (`rust/layoutcraft-conformance`) verifies the
  fixture set and candidate models/patches: 273/273 and 353/353 checks
- IndexedDB persistence and save-after-reload work

Remaining verification and fidelity gaps are listed in
[Known gaps](07-known-gaps.md).

## Browser/runtime requirements

Use current Chrome or Edge. The importer requires:

- `DecompressionStream('deflate-raw')` for ZIP entries
- `DOMParser` / `XMLSerializer`
- `Blob`, `File`, `URL`
- IndexedDB for the side package cache
- localStorage for autosave

The writer prefers `CompressionStream('deflate-raw')`; if unavailable it stores
ZIP entries uncompressed. The importer itself still needs
`DecompressionStream`, so a browser without it cannot open standard deflated
VSDX packages.

Serve the app over HTTP. Opening `index.html` from `file://` is not supported
because the app is built from ES modules.

## Repository status

- No framework, bundler or runtime dependency.
- The JavaScript editor and its `package.json` were removed ([42](42-javascript-removal.md)).
- `test/MS Example-edited.vsdx` is currently a generated output artifact, not a
  test suite.
- The working directory was not a Git repository as of 2026-10-26. Initialise
  source control before Phase D work.
- `docs/` is documentation only and is not loaded by the application.
