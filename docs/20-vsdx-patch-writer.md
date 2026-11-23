# VSDX patch writer

Status: 2026-10-28. Shape cells, text, Shape Data and simple section/row cells,
with byte-preserved entries and Visio acceptance of a patched package.

## API

```rust
use layoutcraft_vsdx::{patch_page_cells, ShapeCellPatch};

let bytes = patch_page_cells(
    &package,
    0,
    &[ShapeCellPatch::new("1")
        .set("PinX", "3")
        .set("LineColor", "#123456")
        .text("Hello")
        .rich(rich_json)              // paragraphs + styled runs + fields
        .data("Row_1", "Eve")
        .row_cell("Geometry", "0", "2", "X", "2")
        .section_cell("Geometry", "0", "NoFill", "1")
        .geometry_rows("0", replacement_rows)],
)?;
```

`patch_page_cells` reads the page XML, locates each shape by its Visio `ID`,
updates or inserts the requested XML, and writes a new OPC package. Every other
part is copied byte-for-byte; tests compare all untouched entry bytes.

## Behaviour

- Direct `<Cell>` values are patched in place, preserving other attributes;
  missing cells are inserted before the first section/child list.
- `<Text>` is replaced with escaped plain text.
- `.rich(...)` writes `<cp>`/`<pp>`/`<fld>` runs plus Character, Paragraph and
  Field sections from the portable rich-text JSON.
- Shape Data rows are updated by row name, or created with the standard
  Value/Prompt/Label/Format/Type fields.
- `.geometry_rows(...)` replaces all rows of a Geometry section with supplied
  Visio rows (MoveTo/LineTo/… and arbitrary cells).
- Section-level cells (`NoFill`, `NoLine`, `NoShow`, …) and row cells by `IX`
  or `N` can be patched (geometry rows included).
- `NewShape` inserts a minimal inline rectangle with fill, line, text and a
  four-edge Geometry section; `patch_page` can combine additions, removals and
  cell patches. Removing a shape also removes page `Connect` rows that mention
  it.
- `NewConnector` inserts a 1-D Begin/End shape with geometry, line style, arrow
  and label, plus page glue rows when source/target shape IDs are supplied.
- `NewPicture` adds a media part, page relationship, Content-Type default and a
  `ForeignData` picture shape. The media part is deduplicated against the
  package's existing entries.
- Patching `V` removes a stale `F` formula so the cached value is authoritative.

## Fresh packages

`write_fresh_document(&Document)` builds a complete VSDX from the portable model:
document/pages/masters/windows/theme/docProps parts, page sheets, shapes with
inline geometry, rich text, Shape Data and connectors; the `_with_media` variant
also emits picture media parts, relationships and `ForeignData`. It round-trips
the theme, rich-text, and connector-route golden models through the Rust
importer. Visio 2021 opens a fresh theme-corpus package (`6` shapes, page
`Quick styles`, `11×9 in`), a fresh connector corpus (`5` pages, `24` shapes,
connectors 1-D) and a fresh image corpus (`6` pages, image shape `Type=4`)
without repair. See [Fresh VSDX writer](21-fresh-writer.md).

## Limits

- The patch API still does not add master/stencil-backed shapes; added
  connectors/pictures are simple inline objects.
- Visio 2021 accepts a `PinX` patch (`shapes=6`, `PinX=3`), a new inline shape
  (`shapes=7`, ID 900), a new connector (ID 901, 1-D, `BeginX=1.5`, `EndX=4`)
  and a new picture (ID 902, `Type=4`, correct size) without repair. Rich-text +
  Geometry-row patches and shape removal were also verified through Visio COM
  (`rich-geo`: 6 shapes, run size 15 pt, Geometry row 1; `removed`: 5 shapes,
  ID 1 absent).

## Tests

- patch `PinX` and verify the imported model position moves by the expected
  amount;
- verify every untouched ZIP entry is byte-identical;
- insert a missing `LineColor` cell and re-import;
- replace text with XML-sensitive characters and verify the imported label;
- patch a Geometry row `X` and verify the imported path changes;
- update existing and new Shape Data rows and verify imported values;
- add an inline shape and verify its count, text, fill, stroke and position;
- remove a shape and verify it disappears;
- add a connector with glue/label and verify connector count, endpoints and
  `Connect` rows;
- add a picture, verify media part/relationship/Content-Type, imported media and
  image shape fields;
- write rich runs/fields and replacement Geometry rows, then verify imported
  labels/styles/paths;
- fresh-write the theme, rich-text and connector-route models and re-import
  shapes, labels, geometry counts, Shape Data and connectors;
- fresh-write a picture model with media bytes and verify the emitted media
  part, relationship and image shape;
- patch and remove nested shapes (a shape containing `<Shapes>`) without
  confusing `<Shapes>` or `OriginalID` for the target shape.

Workspace tests: **40/40**.
