# VSDX save

> Historical note: the JavaScript editor described here (the `js/` folder) was removed; see [42](42-javascript-removal.md). The last commit that still has it is `c9aa852`. The Rust crates carry this work now.

## UI entry point

`app.saveVSDX` in `js/main.js` handles the toolbar VSDX button and
`Ctrl+Shift+S`.

Routing:

1. Use `app.visioSource` if it matches the current document.
2. Otherwise try `loadVisioSource(doc.visioKey)` from IndexedDB.
3. If a source still does not match, use **fresh package** mode.
4. If pages were deleted while patching, prompt to rebuild fresh with the
   deleted pages omitted.
5. Run `auditVSDX` and show a warning/confirmation for unsupported or lossy
   changes.
6. Call `exportVSDX(doc, source, name)`.
7. After a fresh build, drop the old package lineage from the in-memory document.

`docMatchesSource` requires a matching page part plus, when present, a matching
`pg.visio.docKey`/`source.key`. The key prevents a document from patching a
different VSDX that happens to also use `visio/pages/page1.xml`.

## Audit

`auditVSDX(doc, source)` returns:

| Field | Meaning |
| --- | --- |
| `ok` | Source exists (patch) or fresh mode is possible |
| `fromScratch` | Fresh build mode |
| `canAddPages` | Source has `visio/pages/pages.xml` |
| `addedShapes` | New shapes/connectors |
| `removedShapes` | Imported objects deleted |
| `addedPages` | New pages |
| `removedPages` | Imported pages deleted |
| `fontChanges` | Font-family changes on imported shapes (patch warning) |
| `customStyleChanges` | Fill/line edits on imported per-part shapes (patch warning) |
| `newPartStyleLoss` | Per-part shapes that will be flattened |
| `connectorRouteChanges` | Connector route/glue changes (written) |
| `zOrderChanges` | Imported z changes (patch warning) |
| `typeChanges` | Shape-type changes on imported shapes (patch warning) |
| `duplicateIds` | Two model objects claiming one source Visio shape (abort) |
| `missingParts` | Source package lacks an imported page part (abort) |
| `importedPages` | Fresh mode: pages with Visio provenance |
| `importedObjects` | Fresh mode: shapes/connectors with Visio IDs |
| `missingImageData` | Picture shapes with no usable `doc.media` data (abort) |

Patch checks abort on missing parts, duplicate IDs, and adding pages to a
package without `pages.xml`. Deleted pages trigger the fresh-rebuild prompt.

## Patch mode

`buildVSDX(doc, source)`:

1. Copies the source package `Map`.
2. For every imported page, parses its page XML and calls `patchPage`.
3. Serializes only pages that changed.
4. Re-zips the complete package; every untouched entry keeps its bytes.

No `document.xml`, theme or master part is rewritten unless a new page is added.
Media parts are only added when a picture is inserted or replaced; untouched
media entries are copied through byte-for-byte.

### Object deletion

`pg.visio.ids` lists the Visio IDs represented by the imported page. A model
object whose ID is absent from the current page is considered deleted.

For each deleted shape:

- collect its ID and all nested `<Shape>` descendants
- remove the `<Shape>` element
- remove every `<Connect>` whose `FromSheet` or `ToSheet` is in that set

Deleted connectors are removed the same way. The app's normal delete command
also removes connectors glued to a deleted shape.

### Shape edits

`patchShape` compares the current model against `shape.visio.orig`.

Geometry:

- `PinX/PinY/Width/Height/Angle/FlipX/FlipY` are patched only for changed fields.
- The page flip height is `pg.visio.pageH` for imported pages.
- `shape.visio.origin` is subtracted for shapes imported inside a container, so
  Pin coordinates go back into the parent space.
- `LocPin` is recomputed for size changes. Inherited half-size LocPins are
  detected from the cached value versus the original size.
- Changed geometry cells lose their `F` formula so Visio does not overwrite the
  edit on recalculation.

Text/style:

- changed label → plain `<Text>` with `cp`/`pp` and U+2028 line separators
- a shape with `shape.rich` keeps its source Text/Character/Paragraph/Field
  cells untouched when the label is unchanged; editing the plain label drops
  `rich` for that shape and writes one plain run
- fresh/rebuilt shapes write the rich model: Character rows per run,
  Paragraph rows for alignment/bullets/indent, Field rows with formulas/cached
  values, and `cp`/`pp`/`fld` runs in `<Text>`
- font size → Character.Size in inches (pixels / 96)
- text colour → Character.Color
- bold/italic/underline → Character.Style bits 1/2/4, preserving other bits
- horizontal alignment → Paragraph.HorzAlign
- vertical alignment → VerticalAlign
- fill → FillForegnd + FillPattern, disables FillGradientEnabled
- line → LineColor/LineWeight/LinePattern, disables LineGradientEnabled

Per-part shapes (`shape.visio.orig.custom`) skip fill/line patching because the
editable model has no per-part style UI and the renderer uses the imported parts.
The audit warns about those edits.

Pictures (`patchImage`):

- position/size/rotation/flip and label are the normal shape patch
- `Brightness`/`Contrast` are written as 0..1 cells with 0.5 neutral
  (`V = (model + 1) / 2`); `Gamma` and `Transparency` are written directly
- `ImgOffsetX`/`ImgOffsetY`/`ImgWidth`/`ImgHeight` are written with a
  `Width*N` / `Height*N` formula and a matching cached value, so the crop
  survives later resizing in Visio
- a resize refreshes the cached `ImgOffset*`/`Img*` inches even when the crop
  ratio did not change: Visio draws from the cached value inside the shape and
  does not recalculate `Width*1` on open
- replacing the picture adds a new `visio/media/imageN.*` part, a page
  relationship and, if the extension is new to the package, a
  `[Content_Types].xml` Default entry; the `<ForeignData>` `<Rel r:id>` and
  `CompressionType` are updated
- picture text blocks stay frame-relative: an imported picture keeps its
  `Txt*` formulas through a resize (the editor stores `textBoxRel` and scales
  the caption), and a new picture writes `Width*N`/`Height*N` formulas
- the old media part is left in the package (undo may still reference it); this
  is harmless but not garbage-collected
- imported media is never re-encoded: an untouched patch keeps the original
  package entry byte-for-byte

The editor renders brightness/contrast/gamma only for truecolour pictures, as
Visio does; an indexed PNG/GIF keeps the cells but Visio ignores them.

### Connector edits

`patchConnector` handles:

- label
- LineColor, LineWeight, LinePattern
- BeginArrow/EndArrow
- route/endpoint/glue changes (through `rebuildConnector` + `applyConnects`)

An unchanged connector keeps its original formulas, triggers and `<Connects>`
rows. Moving glued shapes therefore works in Visio through the preserved
source formulas.

A changed connector is rebuilt:

- PinX/PinY, Width/Height, LocPinX/LocPinY, BeginX/BeginY, EndX/EndY,
  Angle=0, FlipX=0, FlipY=0, ObjType=2
- all Geometry sections removed; one new Geometry section written from the
  current route
- old `<Connects>` rows for that sheet removed
- new glue rows generated

A route change rewrites the current route and re-applies dynamic glue. Visio
keeps `<Connects>` rows only when the glued end's X/Y cells carry the
`_WALKGLUE` formulas and trigger cells (see “New connectors” below), so
`applyConnects` restores them for every glued end it writes. The saved
appearance matches LayoutCraft; Visio may reroute the connector after a later
shape move instead of keeping the exact authored route.

### Group edits

An explicit page-level group has its own `<Shape Type="Group">` frame, while the
editor model stores absolute member boxes. `applyGroupLayout` reconciles them
before shapes are patched:

- if the whole group subtree moved by one delta, only the group's `PinX`/`PinY`
  changes; child local cells and their formulas are left untouched
- a resize or a partial member move rewrites the group's
  `PinX`/`PinY`/`Width`/`Height` and `LocPin` (preserving the LocPin fraction),
  forces the direct members' local `PinX`/`PinY`, and scales the group text
  block
- nested groups are processed bottom-up for frames and top-down for writes, so
  the delta is not applied twice
- a rotated group supports a whole-subtree move: only its `PinX`/`PinY` change,
  with the world delta rotated into the parent's local space; children keep
  their local cells and formulas. A rotated resize or a member moved on its own
  is not representable with the editor's axis-aligned handle box and is reported
  by the audit instead of written
- an untouched import is not rewritten: a member union that differs slightly
  from the stored frame (margins, images, rotated boxes) does not trigger a
  patch

**Rust `groupedit` (rotated groups).** `GroupEditor::resize_group`, `move_member` and
`resize_member` return `ShapeCellPatch`es in inches. A rotated group is edited in its own
axes: resizing keeps the chosen anchor fixed on the page and scales members (and nested
groups) in those axes; moving a member converts the page delta into the parent's local
axes; with `refit` the ancestor frames grow (never shrink) and `LocPin`/sibling pins shift
so nothing moves on the page. Flipped shapes and non-uniform resizes of obliquely rotated
members return an error instead of a wrong write. Verified with
`visio-test-harness/probe-world-centres.ps1` against the importer.

Authored editor groups have no source group part; they are written as separate
inline shapes and the save dialog warns. Imported group ungroup/regroup,
emptied group shells and rotated groups are also warned about rather than
written.

Container frames go through the same layout path. Container/list membership is
written back from the editor's `shape.containers`/`listOf`: the writer updates
type 1/2/4/5/11 `DEPENDSON` groups on both sides, preserves unrelated
`Relationships` groups, and carries group-only refs (lanes/separators) through
untouched.

Authored **swimlane/CFF** frames are inline shapes with role-specific User rows:

- pool: `CFF Container` category, `Container` structure, `numLanes`,
  `visShowTitle`/`visShowPhase`, margins/title cells
- lane list / phase list: `CFF List` category, `List` structure,
  `msvSDListRequiredCategories` (`Swimlane` or `Phase`), `msvSDListDirection`
  and `msvSDListItemMaster=0`
- lane: `Swimlane` category, `Container` structure, `visHeadingText`,
  `SwimlaneListGUID`, `visCFFSettings`
- separator: `Phase` category, `Container` structure, `ListDirection`

The graph is the same one imported files use: pool type 1 → lists; list type 1
→ tasks and type 2 → lanes/separators; lane/separator type 5 → list; tasks type
4 → list/lane/phase. List items are written in the editor's visual order. New
container frames are inserted above the pool/list but below tasks so a newly
added lane cannot cover them; a pool title follows frame growth through its text
block. Imported CFF group lanes can be renamed (the heading child shape and
`visHeadingText` are patched), but adding/removing those group lanes is refused.

### New shapes

A model object with no `shape.visio.id` is authored. It gets an ID above the
maximum page shape ID and is written by `buildNewShape`:

- Pin/Width/Height/LocPin/Angle/Flip cells
- ObjType=1
- no `LayerMember` on imported pages; layer 0 on new pages
- fill/line cells
- text block cells and zero margins
- Character/Paragraph sections
- `Connection` rows at top/right/bottom/left
- one or more inline `Geometry` sections

A new picture (`shape.image`) is written as an inline `Type="Foreign"` shape
instead of a normal 2-D shape:

- the same box/geometry/frame/text cells, with `NoFill=0` on its rectangle
- `ResizeMode=0`, `ClippingPath=""` and the `Img*` crop cells
- optional `Brightness`/`Contrast`/`Gamma`/`Transparency`
- `<ForeignData ForeignType="Bitmap|MetaFile" CompressionType="PNG|JPEG|GIF">`
  with a `<Rel r:id="…"/>`
- a media part, page relationship and Content-Type Default (`createMediaWriter`)

Media bytes come from `doc.media[mediaId].src` (a data URL). Identical data URLs
are written once; a shape copy reuses the same media part. Missing media data
aborts the save (`missingImageData` in the audit) rather than writing a broken
picture.

### Data callouts, headings, bars and icons

A data graphic is a normal inline shape plus a transport property and User
rows (see [Import](04-visio-import.md#shape-data-and-data-graphics)):

- `Property.msvCalloutField`: cached value and `F='Sheet.<target>!Prop.<row>'`
- `Field` row 0: `F='Prop.msvCalloutField'`, cached value; the `<Text>` has the
  optional prefix followed by a `<fld>` run
- `layoutcraftDGTarget/DGKind/DGField/DGLabel/DGPosition/DGAuto` User rows for
  re-import; a bar adds `DGBar/DGMin/DGMax/DGShowValue`, and an icon adds
  `DGIcon/DGIconSet/DGMin/DGMax/DGLevel`
- non-free graphics with a page-level, unrotated target get `PinX`/`PinY`
  formulas referencing the target's Pin, Width/Height and LocPin plus the gap;
  grouped/container/rotated targets fall back to static Pin values
- a bar writes two Geometry sections (see import): Geometry 0 is the filled
  body with `NoLine=1` and `Width*percent` formulas on its right edge;
  Geometry 1 is the `NoFill=1` track outline. Its Width/Height changes rebuild
  the bar geometry on save; changing min/max, the field or position rebuilds it
  via `patchCallout`.
- an icon writes six Geometry sections: one `NoFill=1` outline row with all five
  glyphs, then five `NoLine=1` filled rows whose `NoShow` formulas read
  `User.layoutcraftDGLevel`. The level row caches 1–5 and carries a formula over
  the target's property, so Visio changes the icon live. Min/max, style, field
  or position changes rebuild the geometry; a target-value-only change updates
  the cached level and `NoShow` values.
- a heading is the same text-field callout with `DGKind='heading'`, a filled
  box and no leader; its normal fill/line/character cells carry the style.
- a colour rule is written on the target, not on a separate shape:
  `layoutcraftDGColor`/`Field`/`Min`/`Max`/`Low`/`Mid`/`High`/`Base` User rows and
  `FillForegnd = IF(<band test>, RGB(...), ...)`. The cached fill is the current
  band and is refreshed when the value changes; metadata changes rewrite the
  rows/formula. The generic fill diff is skipped while a rule is active so an
  edit cannot overwrite the formula.

On patch, `patchCallout` rewrites the transport property, field text, User rows
and bar Geometry only when the metadata changed; the target's value is refreshed
in the cached cells on every save, and `patchShape` keeps the Pin formulas
(updates only the cached V) for a graphic that follows its target. Free or
unlinked graphics use static Pin cells. The target's Shape Data Value is written
with `U='STR'` for string properties; without it Visio reads the value as 0 and
the field is blank. Visio preserves the `layoutcraftDG*` User rows when it re-saves
the file, so a Visio round-trip re-imports as an editable callout, heading,
bar, icon or colour rule.

Geometry sources:

- catalogue shape → `shapePath(type, w, h)`
- optional detail → `shapeDetail(type, w, h)` in a `NoFill=1` Geometry section
- copied/imported per-part shape → all `shape.parts[].d` paths combined into
  one geometry with the front fill/line (lossy; warning)

SVG path data is flattened by `js/svg-path.js` into line segments. Moves, lines,
H/V, cubic/quadratic curves, arcs and close commands are supported. Rounded
corners and ellipses become fine polylines.

### New connectors

`buildNewConnector` writes a 1-D shape:

- Pin/Width/Height/LocPin from route endpoints
- BeginX/BeginY/EndX/EndY
- ObjType=2, ConFixedCode=3, GlueType=2, DynFeedback=2
- line style, arrows, FillPattern=0
- label, Character/Paragraph/Txt cells
- Geometry section with NoFill=1
- dynamic glue cells for each glued end: the end's X/Y cells get
  `_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)` (or the End equivalent),
  and `BegTrigger`/`EndTrigger` get `_XFTRIGGER(Sheet.<target>!EventXFMod)`
- a free end keeps its static value; when the other end is glued, its trigger
  references the connector itself, matching what Visio writes for a one-end
  glued connector

Visio silently ignores `<Connects>` rows for a connector whose endpoint cells
have plain values. Before this was found, authored connectors opened as loose
lines in Visio 2021: `Shape.Connects` was empty and endpoints did not follow a
moved shape. Writing `_WALKGLUE` on **both** X and Y of each glued end (plus the
trigger cells) is what makes the glue take effect; a master is not required.

New connectors are inserted at the bottom of the page shape stack, because the
editor draws connectors with default z = -1 behind shapes. New shapes are
appended in model order.

Route geometry:

- straight/elbow/waypoint routes use `routeConnector(...).points`
- curved routes use the flattened cubic from `route.d`
- local geometry is page points minus the begin point

Glue:

- `applyConnects` writes `<Connect>` rows for every new/rewritten connector end
  whose model end has a `shapeId`.
- `resolveGlue` checks the target's `Connection` rows. If a row is within 1 px
  of the endpoint, it is reused.
- If the target has a page-level Connection section and no close row, it adds a
  row at the exact endpoint, including points just outside the shape box.
- If the target has no page-level Connection section, it falls back to
  `ToCell='PinX' ToPart='3'` rather than overwriting an inherited master row.
- `FromCell` is BeginX/EndX; `FromPart` is 9/12.
- `ToCell` is `Connections.Xn`/`ToPart=100+ix`, or PinX/3.

### New pages

`makePackageEditor` adds pages to an existing package:

- page part `visio/pages/pageN.xml`
- page `<Page>` entry in `pages.xml`
- page relationship in `pages.xml.rels`
- Content-Type override for the page part
- empty page relationships part
- minimal PageSheet: size, scale, drawing scale, locks, shadow, print,
  layer 0 named `LayoutCraft`
- page size derived from the document default (first source page) and content
- page ID above the existing maximum, relation ID above the existing maximum
- page name uniquified against existing names

The page XML contains `<Shapes>` and `<Connects>`, with all new objects written
inline. Pages are appended in model order.

`docProps/app.xml` and `windows.xml` are not updated for pages added while
patching. Visio generally copes, but this is a known minor gap.

### Page deletion while patching

The importer metadata identifies imported page parts, but removing them from an
OPC package touches:

- `pages.xml`
- page relationships
- page parts and page rels
- Content-Type overrides
- windows
- `document.xml` TopPage
- possible background-page references

The writer does not implement that partial removal. Instead, `app.saveVSDX`
prompts to build a fresh package from the current model with the page omitted.
The prompt reports imported pages/objects and per-part flattening.

### Z-order while patching

New shapes are appended; new connectors are inserted first. Existing shape z is
preserved. `app.reorder` changes imported z values, which the patch writer does
not rewrite, so the audit warns about z-order changes.

## Fresh package mode

`buildNewVSDX(doc)` in `js/vsdx-writer.js`.

Triggered when:

- no source package is available
- the document was imported from JSON
- the document came from a `.vdx` (no OPC package)
- the source package cannot be found/restored
- the user accepts the page-deletion fresh rebuild

### Generated package tree

```text
[Content_Types].xml
_rels/.rels
docProps/core.xml
docProps/app.xml
visio/document.xml
visio/_rels/document.xml.rels
visio/masters/masters.xml
visio/masters/_rels/masters.xml.rels
visio/pages/pages.xml
visio/pages/_rels/pages.xml.rels
visio/pages/page1.xml
visio/pages/_rels/page1.xml.rels
...
visio/windows.xml
visio/theme/theme1.xml
visio/media/image1.png        # only when the model has pictures
```

Each additional page gets a page part, a page rels part, a `Page` entry, a
pages relationship and a Content-Type override. Picture media parts get a page
relationship and a Content-Type Default for their extension.

### Generated shell

- `document.xml`: DocumentSettings with TopPage 0, default styles 0, a minimal
  StyleSheet 0 with line/fill/text/Character/Paragraph defaults, empty Colors,
  Calibri/Segoe UI FaceNames, and a DocumentSheet.
- `theme1.xml`: generic DrawingML theme with Office-like colours/fonts and
  three-entry fill/line/effect/background lists. No Visio `vt:schemeID` or
  variation extensions.
- `masters.xml`: empty masters part plus empty rels.
- `windows.xml`: one Drawing window for page 0, centred on the first page.
- `core.xml`: title from the document name, creator `LayoutCraft`, current dates.
- `app.xml`: application `LayoutCraft`, Pages count, Masters 0, page titles.
- `pages.xml`: pages start at ID 0, ViewScale -1, view centre from page size.
- PageSheet: size, scale, drawing scale, locks, shadow, print orientation,
  PaperKind 1, layer 0 named `LayoutCraft`.

Page IDs and shapes:

- page IDs 0, 1, 2, … in document order
- page part numbers 1, 2, 3, …
- page relationship IDs rId1, rId2, …
- shape/connector IDs start at 1 per page
- `sortZ: true` reorders page `<Shape>` children by model z:
  - connectors default z = -1
  - shapes default z = 1000 + index
  - explicit stored z values are honored

### Fresh page sizing

`newPageDims` starts from the package default (11×8.5 in) and grows to:

- maximum shape right/bottom
- connector endpoints/waypoints
- plus 1 inch
- rounded up to 0.25 inch

Page height is used for the Visio Y flip and is written to PageHeight. View
centre is width/2, height/2.

### Fresh imported content

If the document contains imported metadata (JSON/VDX/source-missing), every
shape is rebuilt from the editable model:

- geometry/outline preserved from `parts` or catalogue path
- one fill/line per shape
- plain text
- static connector routes
- current z-order
- page names and order
- page sizes derived from content

The UI confirms this and reports imported page/object counts plus per-part shape
counts when applicable.

### Fresh lineage reset

After a successful fresh save, `app.saveVSDX` removes:

- `app.visioSource`
- `doc.visioKey`
- every `pg.visio`
- every `shape.visio`
- every `connector.visio`

and calls `saveLocal`. The saved file becomes the canonical package. Undo
snapshots made before this point still contain old provenance and can restore
the old package from IndexedDB if its key still exists.

## Units and value mappings

| LayoutCraft | Visio |
| --- | --- |
| world pixel | inch × 96 |
| top-left Y | flip with page height |
| angle degrees | radians, negated |
| fontSize px | Character.Size inches = px / 96 |
| strokeWidth px | LineWeight inches = px / 96 |
| fill/stroke hex | FillForegnd/LineColor hex |
| `none` fill | FillPattern 0 |
| line style solid | LinePattern 1 |
| dashed | 2 |
| dotted | 3 |
| dashFine | 23 |
| no line / width 0 | LinePattern 0 |
| `none` / `arrow` | Begin/EndArrow 0 / 1 |
| `filled` | 4 |
| `hollowTriangle` | 14 |
| `hollowDiamond` | 22 |
| `filledDiamond` | 38 |
| picture brightness/contrast −1..1 | cell 0..1, 0.5 neutral: `(v + 1) / 2` |
| picture crop offset/scale | `Width*N`/`Height*N` formulas and cached inches |
| picture transparency 0..1 | `Transparency` 0..1 |

`validColor()` replaces `"none"` with a valid fallback (`#000000` for lines,
normal text colour for text) while LinePattern 0 hides the line. This fixed a
LibreOffice failure where `LineColor="none"` made the whole VSDX unloadable.

## ZIP writer

`zipBlob(entries)`:

- iterates in Map insertion order
- computes CRC32 for each entry
- tries `CompressionStream('deflate-raw')`
- stores uncompressed if compression is unavailable or not smaller
- writes local headers, central directory and EOCD
- sets UTF-8 name flags
- uses current DOS time/date
- returns a Blob with `application/vnd.ms-visio.drawing`

It does not preserve ZIP comment/timestamps/permissions and does not write
ZIP64. This is fine for ordinary VSDX sizes.

## Output naming

`vsdxFileName(doc, source)`:

- with a source: `<doc.name>-edited.<source extension>`
- without: `<doc.name>.vsdx`

The base name is sanitized for Windows-forbidden characters. The source
extension preserves `.vstx/.vsdm/.vstm` for patched copies. Fresh builds always
use `.vsdx`.

## Current verification of the writer

- All examples: untouched patch reports 0 pages patched; entry contents were spot-checked byte-identical
- MS Example: move/resize/rotate/flip/text/fill/line/font/alignment/deletion
- UML: relationship connector deletion, direct-geometry fill/line/text
- BPMN: nested container origins, multi-page, new pages
- new authored shapes: catalogue geometry, detail paths, copied parts, rotation,
  flips, text, styles
- new authored connectors: straight/elbow/curved, labels, arrows, glue to
  imported and new shapes
- existing connector route rewrite and Connects rewrite
- fresh package: sample, blank, multi-page, JSON-imported model, page deletion
- fresh packages re-import and open in LibreOffice
- `.vstx` with media: patch mode preserves every media hash
- pictures: import, untouched byte-identical patch, resize/crop/adjust patch,
  image replacement and new media parts/rels/content types, JSON fresh rebuild
  with media, and re-import of every output
- data callouts, headings, bars, icons and colour rules: patched and fresh
  saves, target-relative formulas, live field/level/fill updates in Visio, Visio
  re-save preserving the metadata and re-import

Validated on Windows in Microsoft Visio LTSC Professional 2021 16.0.14332.20416:
untouched patch saves were byte-identical, all outputs opened with no repair
dialog, edited shape/connector values read back exactly, authored connectors
were glued and followed moved shapes, and a Visio save re-imported cleanly. The
picture corpus opens as `Type=4` (Foreign) shapes with the expected `ImgWidth`,
`Transparency` and page positions; both the patched and fresh copies of the
resized, cropped, replaced and new pictures render in Visio. The run found the
connector-glue defect described above. See
[Testing](06-testing.md) for the matrix and [Known gaps](07-known-gaps.md) for
the remaining fresh-shell differences.

## Per-part style edits

An imported master-backed shape is drawn from its master's child shapes, each
emitted as `parts` carrying `source {masterShapeId, pageShapeId, role}`
(`fill`, `stroke` or `tint`). Visio stores each master child as a nested
`<Shape MasterShape='N'>` on the page, so a style edit is a cell write on that
child, which `part_style_patch(part, PartStyle)` builds:

- `fill` part: `FillForegnd`;
- `stroke` part: `LineColor` and/or `LineWeight` (model px / 96);
- `tint` parts are derived from the fill and are rejected; a part with no page
  child or no `source` is rejected rather than guessed.

Setting a cell replaces the inherited `F='Inh'` formula with a concrete value, the
same local override Visio writes. Verified against Visio 16.0.14332: a Rust patch
of the container body (`FillForegnd` on child 2) and of a laptop's body part (empty
child 28) render in Visio exactly as Visio's own edit does, and the rest of the
page is unchanged. Visio recolours a sub-shape whose fill is a `THEMEGUARD`
formula by storing the new value on the child, so editing through COM and through
the patch writer give the same file shape.

The patch writer also expands a self-closing `<Shape .../>` before inserting
cells. Empty master-instance overrides are written that way; cells used to land
outside the element (a silent drop that broke any patch of such a shape).
