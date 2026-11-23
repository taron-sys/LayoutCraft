# Fresh VSDX writer

Status: 2026-10-28. First complete fresh-package path for the portable model.

## API

```rust
let bytes = layoutcraft_vsdx::write_fresh_document(&document)?;
std::fs::write("diagram.vsdx", bytes)?;
```

The writer creates a complete OPC package with:

- `[Content_Types].xml`, package rels, document rels, docProps;
- empty masters part with relationships;
- document settings/style sheet/document sheet;
- pages list with page relationships;
- Visio theme;
- Windows view state;
- one page part per model page, with inline shapes, text, Shape Data and
  connectors;
- model geometry parts converted from SVG M/L/H/V/C/S/Q/T/A endpoints into
  inline Geometry rows.

## Validation

- The theme, rich-text and connector-route golden models round-trip through the
  Rust reader with matching page/shape/connector counts, labels, positions,
  fills/strokes, part counts and Shape Data.
- Visio 2021 opens a fresh theme-corpus package without repair: 1 page, 6
  shapes, page `Quick styles`, 11×9 in.
- Visio 2021 opens a fresh connector-route package without repair: 5 pages,
  24 shapes and 1-D connectors.
- `write_fresh_document_with_media` emits picture media parts, page
  relationships, Content-Type defaults and `ForeignData`. Visio 2021 opens the
  fresh image package without repair: 6 pages, one `Type=4` image shape.

## Limits

- Curve commands are flattened to their endpoints rather than reconstructed as
  Bézier/arc Geometry rows; fresh output is intended as a valid first rebuild,
  not a byte-faithful master recreation.
- Pictures are emitted only when the caller supplies media bytes
  (`write_fresh_document_with_media`); patch mode should be preferred for
  imported pictures.
- Master/stencil-backed new shapes, containers/groups and native data graphics
  are not emitted.
- Output is validated by the Rust reader and Visio open, not by a full visual
  acceptance matrix.

Workspace tests: **40/40**.
