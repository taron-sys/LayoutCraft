# LayoutCraft

A diagram editor in the browser that opens and saves Visio-compatible .vsdx files — draw flowcharts, network diagrams and architecture
sketches on an infinite canvas. No build step, no dependencies, no framework.

```bash
npm start          # or: python3 -m http.server 8137
# open http://localhost:8137
```

Any static file server works; the app is plain ES modules, HTML and CSS.

Implementation notes, the VSDX import/save design, Windows setup, the test
matrix and the remaining Visio compatibility work are documented in
[`docs/`](docs/README.md). The JSON schema is versioned (`schemaVersion: 1`)
and pinned by a self-authored, redistributable golden set in
[`fixtures/public/`](fixtures/public/README.md) ([how it was replaced](docs/39-fixture-replacement.md));
the Visio-authored validation corpus is kept in a private companion repository. A porting brief for
a Linux independent Rust reimplementation is in
[`docs/12-handover-rust.md`](docs/12-handover-rust.md); the agreed v1 scope and
provenance rules are in
[`docs/14-v1-scope-provenance.md`](docs/14-v1-scope-provenance.md) and the
[provenance statement](PROVENANCE.md). A
Rust workspace (`layoutcraft-model`, `layoutcraft-vsdx`, and the browser-free
`layoutcraft-conformance` runner) lives in [`rust/`](rust/README.md). The v1 desktop UI choice is recorded in
[`docs/17-ui-toolkit.md`](docs/17-ui-toolkit.md). Test files made with Microsoft Visio are not part of this repository
([`docs/38-private-fixtures.md`](docs/38-private-fixtures.md)).

## What's in the box

**Canvas** — infinite, pannable, zoomable (10%–800%). Optional grid with
10 px cells, major lines every 5 cells, and snap-to-grid while dragging.

**40+ shapes** across four stencils — basic geometry, flowchart symbols
(process, decision, terminator, document, data, manual input, delay, display,
preparation, card, stored data, note), data storage (database cylinder, server,
tape, cloud, cube) and network symbols (router, switch, firewall, web, VPN).
Drag one from the palette or click it and draw on the canvas.

**Connectors** that glue to shapes and re-route themselves when shapes move.
Straight, right-angle and curved routing, open or filled arrowheads, labels,
dashed/dotted line styles. Create one with the **Connect** button (or press
`C`): click the source shape, then the target shape. Select the connector and
the inspector's Arrow start/end controls set open, filled, hollow-triangle or
diamond ends. Drag a connector to bend it — a waypoint appears and
can be dragged around. Drag an endpoint onto another shape to re-glue it; a
side-glued end slides along that side to the point nearest the other end, so a
page-sized background shape does not pull an arrow to a distant midpoint.
Endpoint drops prefer a nearby foreground shape over a large background.
The connector inspector also edits Visio's `ShapeRouteStyle` (including the
vertical-first/horizontal-first elbow variants), `ConFixedCode`,
`ConLineRouteExt`, `WalkPreference` and line-jump/`ShapePlowCode`; imported
dynamic connectors no longer collapse to straight lines, and untouched
connector formulas/glue are preserved byte-for-byte.

**Re-layout** — the Layout button (or the Arrange panel / context menu)
rearranges the selection or whole page as a layered flow, top-down or
left-to-right. Connectors between the laid-out shapes drive the layers;
cycles are collapsed, crossing count is reduced with barycentre ordering, and
disconnected pieces pack beside the main graph. Groups move as one unit.
Container/list frames, swimlane lanes and their members, annotation text,
line-only shapes and page backdrops are left where they are.

**Editing** — click to select, shift-click for multi-select, rubber-band
selection, drag to move, eight handles to resize (works on rotated and mirrored
shapes), arrow keys to nudge. Double-click a shape or a connector to type into
it. Page-level Visio groups select and move as a unit; `Ctrl+G` /
`Ctrl+Shift+G` group and ungroup editor objects (editor grouping is not yet
written as a Visio group structure). Containers/lists import their real
membership; selecting a container frame brings its contents along, while
clicking a member selects only that member. The context menu adds/removes
regular container membership and the save writes both sides' `Relationships`
cells. “Create container” wraps the current selection in an inline container
that Visio recognises without a stencil master (fresh and patched documents).

**Swimlanes / cross-functional flowcharts** — `Ctrl+Shift+L` (or “Create
swimlane” in the context menu) inserts a CFF pool with lanes around the current
selection, assigning each selected shape to the lane containing its centre.
Lanes can be added above/below, renamed, removed (their tasks move to a
neighbouring lane), and tasks can be moved between lanes from the inspector or
context menu. Phases can be added as separators and removed. The pool, list,
lanes, phase list and separators are inline shapes with the same User rows and
`DEPENDSON` membership graph Visio writes for its stencil masters, so
`Page.GetContainers`, `GetListMembers` and `MemberOfContainers` report them
correctly. Authoring lanes is supported for swimlanes created here; imported
Visio CFFs support moving tasks between existing lanes and adding phases.

**Formatting** — fill and line colour, line width and style, font size, bold,
italic, underline, alignment, rotation, mirror, z-order, and align/distribute
for multi-selections. Imported rich text keeps per-run size/weight/style/colour,
paragraph alignment/bullets and fields (Page, PageCount, FileName, Date, Shape
Data); the inspector edits paragraphs and runs, inserts/edits fields, manages
Shape Data and hyperlinks, and can turn on “Fit text” autosize. Fresh saves
write the runs, fields, Shape Data and hyperlink rows back. Editing the plain
label collapses a shape to one run.

**Themes** — imported theme slots, variation palettes and quick-style colours
are resolved from the package; a Theme panel offers Office/Linear/Integral/
Colorful palettes, variation selection, Selection/Page/Document scope and
subtle/moderate/intense/outline/plain quick styles. Unstyled imported shapes use
the document's default fill/line/text styles (white/dark) instead of the
editor's blue, and theme application writes concrete fill/line colours.

**Pictures** — insert a picture with the toolbar button, the context menu or by
dropping a file, and replace one from the inspector. Imported Visio pictures
render in place with their crop, rotation/flip, transparency and group
membership. PNG, JPEG, GIF, BMP, WebP and SVG preview; when inserting, formats
Visio cannot embed directly (WebP/SVG and other decodable images) are converted
to PNG, while EMF/WMF/TIFF are preserved as placeholders. The inspector previews
the image, resets it to its natural size or frame, edits a four-sided crop, and
adjusts brightness, contrast, gamma and transparency. Picture bytes are shared
by every shape that uses the same media, both in the model and in a saved
package. The toolbar now labels the **Picture** tool and the **?** button opens a
short feature check dialog that can start the connector and picture flows, start
a new diagram, or save the current document as Visio; these flows mirror the new
Rust writer’s connector/picture, rich-text, Geometry-row and fresh-package
support.

**Data graphics** — select a shape with Shape Data and add a text callout, a
filled heading, a numeric data bar, a five-level icon set (boxes, diamonds or
circles), or a three-band **color by value** rule. Graphics can sit above,
below, left or right, or freely. A callout or heading shows an optional prefix
and the live value of a selected property; a bar uses `min`/`max` to fill
proportionally and can show its value; an icon set fills one glyph per value
band and previews its level; color by value repaints the target's fill from low
to high. They update when the property changes, follow the target when it moves
or resizes, and travel with the target when copied. They are saved as linked
shapes or live cells: callout/heading text is a Visio field
(`Prop.msvCalloutField`, built from `Sheet.<target>!Prop.<row>`) with
target-relative `Pin` formulas, a bar is one shape with a filled live-width
Geometry section and a `NoFill` track outline, an icon set is an outline row
plus five filled Geometry sections whose `NoShow` formulas read a live level,
and a colour rule writes the target's `FillForegnd` as a live `RGB()` formula
over its own property. The custom `layoutcraftDG*` User rows let LayoutCraft reopen
them as editable data graphics. Visio-native DataGraphic masters still import
as ordinary shapes.

**Documents** — multiple pages, autosave to browser storage, undo/redo
(200 steps), copy/cut/paste/duplicate, export to `.json`, `.svg` or `.png`,
and a `.vsdx`/`.vstx` save that patches an opened file or builds a fresh
package, writing new objects/pages as inline shapes.

## Opening Visio files

The **Open** button (or `Ctrl+O`) reads LayoutCraft's own `.json` as well as
Microsoft Visio files:

| Format | Support |
| --- | --- |
| `.vsdx` | full — the ZIP's XML parts are read directly in the browser |
| `.vstx` / `.vsdm` / `.vstm` | full — templates and macro-enabled variants use the same package |
| `.vdx` | full — plain XML, Visio 2003 format |
| `.vsd` | **no** — the legacy binary format is proprietary and not implemented |

`.vdx` pictures are not resolved (VDX has no package media parts); a `.vsdx`
picture is imported from `visio/media/*` through its page relationships.

A `.vsdx` is a ZIP of XML parts, inflated in the browser with
`DecompressionStream`. Getting a faithful result out of one means undoing
several layers of indirection that Visio builds in:

- **Masters.** Most shapes inherit their geometry and styling from a master in
  `visio/masters/`. The importer resolves the master chain, including
  `MasterShape` references into nested master shapes. A master may be a
  *group* of parts — Visio's 3-D blocks are a bevelled rim plus a flat front
  face — so a shape can end up with several paths, each with its own fill.
- **Groups.** A page-level Visio group is traversed as a group; every member is
  imported and clicking one selects the whole group. Master-derived groups
  (icons, 3-D blocks) still become one shape with parts. Rotated groups compose
  their `Pin`/`LocPin`/`Angle` transform down the tree, so members and labels
  land at their real rotated positions; a whole-group move saves by updating
  only the group `PinX`/`PinY`.
- **Geometry formulas.** Master sub-shape coordinates are recalculated for
  the instance from their ShapeSheet formulas (`Width`, `Scratch.A1`,
  `User.AntiScale`, `Sheet.5!Width`, `Geometry1.X4`, …) instead of scaling the
  master's cached values. If a reference cannot be resolved, the cached `V`
  value is used. This keeps bevels, arrowheads and BPMN event circles correct
  at any shape size. Curves are supported too: NURBS and spline rows use their
  degree, knots and weights, circular and elliptical arcs are reconstructed
  from their ShapeSheet cells, and infinite-line rows draw across the page.
- **Units.** Cells carry a formula, a cached value, and a display unit
  (`IN`, `PT`, `CM`, …). The cached value is always in Visio's internal
  units — inches for lengths, radians for angles — so `0.1111` is 8 pt and
  `0.25` is 18 pt; the unit attribute must *not* be applied again.
- **Themes.** Colours are either hex, an index into the document's palette, or
  a formula like `THEMEGUARD(MSOTINT(THEMEVAL("accentcolor"),-50))`. The
  importer evaluates the colour subset — `THEMEGUARD`/`THEMEVAL`, `MSOTINT`,
  `TINT`, `SHADE`, `LUMDIFF`, `RGB`, `IF`, `INDEX` and arithmetic — against the
  theme's colour scheme and `vt:variationClrSchemeLst`/`vt:bkgnd` extensions.
  Fill/line style sheets and `QuickStyle*Color` cells are resolved as well
  (0–8 index the base theme, 100–106 the active variation palette). Cached
  values still win; this only fills in cells whose value is `Themed`.
- **Pictures.** A `<Shape Type="Foreign">` names a `visio/media/*` part through
  its page relationships; the bytes are decoded to a data URL and shared by
  every shape that uses that part. The frame, crop (`ImgOffset`/`ImgWidth`),
  transparency and (for truecolour images) brightness/contrast/gamma are
  imported. Visio ignores those three adjustments on indexed PNG/GIF pictures,
  and so does the editor. EMF/WMF/TIFF are kept but shown as placeholders, and
  `.vdx` images are not resolved.
- **Shape Data and data graphics.** `Property` rows import as Shape Data with
  labels, values, prompts, formats and types. Data callouts, headings, bars and
  icon sets authored here are linked shapes; callouts and headings carry a Visio
  field through an `msvCalloutField` transport row, bars carry a live Geometry
  width formula, and icons carry a live level with per-glyph `NoShow` formulas,
  all with target-relative position formulas and `layoutcraftDG*` User rows for
  re-import. Colour-by-value rules are recovered from `layoutcraftDGColor*` User
  rows and repaint the target's `FillForegnd`. Visio-native DataGraphic masters
  are flattened to ordinary shapes; their callout text still appears.
- **Text.** Font, size, colour and weight come from a shape's `Character`
  section, and the label's position from its text block and margins.
- **Z-order.** Shapes and connectors share one stack, the way Visio orders
  them — a connector is not always behind every shape.

The `<Connects>` table is read as well, so connectors arrive still glued to
their shapes, and a connector's own geometry becomes its route. Visio measures
in inches from the bottom-left of the page, so everything is converted to
pixels with a top-left origin on the way in.

UML and database **relationship** shapes are groups in the file but behave as
connectors: they are imported with their glue points and right-angle route
instead of becoming a box. Their end symbols are rendered too — dependency's
open arrow, inheritance's hollow triangle, and aggregation/composition's
hollow and filled diamonds — and the cardinality text Visio stores separately
(`1`, `*`, …) stays next to each end.

During development the drawings are checked against Visio files kept in a private companion repository
(`docs/38-private-fixtures.md`); none is part of this repository. The redistributable fixture set is
`fixtures/public/`, generated by LayoutCraft itself (`docs/39-fixture-replacement.md`).

Visio's 3-D blocks keep their bevels: the rim and front face are separate
paths, the top face is foreshortened, and the bevel is shaded from the
face's own colour with a light outline. Arrowheads on the file's own
"arrow" shapes, dashed line styles and exact glue points carry over too.

What does *not* carry over: OLE objects/ink, EMF/WMF/TIFF previews (the media is
preserved on save), true 3-D lighting beyond that bevel shading, and BPMN icons
come through only as plain paths.

## Saving back to Visio

The VSDX toolbar button saves the current document in either of two ways:

- **Patch an opened package.** A `.vsdx`, `.vstx`, `.vsdm` or `.vstm` opened in
  the session keeps its original OPC package. Only affected page XML is
  rewritten; every other entry is copied through byte-for-byte.
- **Build a new package.** A new LayoutCraft document, an imported `.json`, or a
  document whose side-cached package is gone gets a complete OPC package:
  document/styling shell, theme, empty masters part, pages, windows and
  document properties. All shapes and connectors are written as inline
  Geometry, so no stencil is required.

Patching handles:

- position, size, rotation, mirroring, labels and text styling of imported shapes
- group frames when members move or resize (an untouched group subtree is not
  rewritten; rotated groups save a whole-group move but a rotated resize is
  warned about)
- fill and line colour/width/style on imported shapes with direct geometry
- connector colour, width, style, arrowheads, labels, endpoints and routing,
  including new glue points on target shapes
- deletion of imported shapes and connectors, including their `<Connects>` rows
- new shapes, connectors and pages created in LayoutCraft
- picture resize/crop/adjustment cells; a replaced picture gets a new media
  part, page relationship and Content-Type entry
- data callouts, headings, bars and icon sets: their transport Property/field,
  `layoutcraftDG*` User rows, target-relative `Pin` formulas, a bar's filled
  live-width Geometry plus `NoFill` track, and an icon's outline plus five
  conditional filled Geometry sections; a changed property or position rewrites
  the field, level and formulas
- colour-by-value rules: `layoutcraftDGColor*` User rows and a live `FillForegnd`
  `IF(...RGB(...))` formula over the target's own property, with cached colour
  refresh when the value changes
- every untouched entry, including masters, themes, images and macros

Building fresh handles:

- every page, shape and connector, with layer 0, page size and view centre
- model z-order (connectors below shapes by default)
- page deletion by omission
- imported shapes/connectors rebuilt from the editable model; per-part shapes
  are flattened to one fill/line, which the save command warns about
- picture media parts, page image relationships and Content-Type Defaults
- data-callout, data-heading, data-bar and data-icon shapes with live
  fields/levels, transport Property rows, bar/icon Geometry and target-relative
  position formulas, plus target `FillForegnd` formulas for colour rules
- a generated drawing shell with a minimal style sheet, theme, masters part,
  page part, page relationship, Content-Type overrides, window and document
  properties

Neither path writes:

- page deletion while patching an existing package (the save command offers to rebuild a fresh copy)
- font-family, z-order or shape-type changes on imported shapes while patching
- fill/line changes on imported per-part shapes while patching; the editor's
  renderer ignores those edits too
- a plain label edit collapses rich text to one run; untouched rich runs and
  fields are preserved, and fresh builds write the imported rich model
- the original media part of a replaced picture (the new part is added and the
  old one is left in place), or a package thumbnail (preserved but not
  regenerated)
- the exact authored route of a newly glued connector: it is written and glued,
  but Visio treats it as a dynamic connector and may reroute it after a later
  shape move
- Visio-only content that was never in the editable model

Curves in new inline Geometry are written as fine polylines. The save command
warns about the remaining flattening and lets you cancel.

The original package is cached in IndexedDB so patching still works after a
browser reload; `.vdx` files remain load-only for patching but can be rebuilt
into a new `.vsdx` from the imported model. Adding pages to a patched package
requires a `visio/pages/pages.xml` part; the usual modern VSDX/VSTX has one.

A patched copy is named `<name>-edited.<same extension>`. A newly built package
is named `<name>.vsdx`, and the in-app document becomes a fresh lineage so
later saves rebuild from the model rather than repeatedly warning about a
package that no longer matches.

Validated on Windows against Microsoft Visio LTSC Professional 2021
(16.0.14332.20416): all nine example files, the patched copies, the fresh
builds, the new-page/new-object cases, the picture corpus (untouched, resized,
cropped, adjusted, replaced and newly inserted, in patched and fresh packages),
the Shape Data corpus with linked data callouts, headings, bars, icon sets and
colour rules, and the page-deletion rebuild open without a repair or corrupt-file
dialog; untouched patch saves reproduce every package entry byte-for-byte; and
Visio can save a patched copy that re-imports into LayoutCraft unchanged. The JSON
schema is versioned and pinned by the private 20-source golden manifest
(200/200 validation checks) and by the self-authored public set in
`fixtures/public/` (44/44 checks, `docs/39-fixture-replacement.md`). See
[Testing](docs/06-testing.md) for the matrix and the remaining
differences from a Visio-authored blank package.

## Keyboard

| Key | Action | Key | Action |
| --- | --- | --- | --- |
| `V` | select | `Ctrl+Z` / `Ctrl+Y` | undo / redo |
| `H` | pan | `Ctrl+C` / `X` / `V` | copy / cut / paste |
| `C` | connector | `Ctrl+D` | duplicate |
| `R` / `E` | rectangle / ellipse | `Ctrl+A` | select all |
| `Space` (hold) | pan | `Ctrl+S` / `Ctrl+O` | save / open |
| `Del` | delete | `Ctrl+0` / `F` | fit to window |
| `G` | toggle grid | `Ctrl+=` / `Ctrl+-` | zoom |
| `L` | toggle snap | `Ctrl+B` / `I` / `U` | bold / italic / underline |
| arrows | nudge (shift = 10 px) | `Enter` | edit label |
| `Ctrl+Shift+S` | save VSDX copy | `Ctrl+Shift+L` | create swimlane |

## How it's put together

```
index.html      markup + toolbar
css/app.css     all styling
js/main.js      app object: document, view state, every command
js/model.js     document/shape/connector data + undo history
js/shapes.js    shape catalogue: path generators + text box geometry
js/visio.js     VSDX/VDX importer: masters, themes, geometry, pictures
js/layout.js    layered auto-layout (units, SCC, barycentre, packing)
js/vsdx-writer.js  VSDX save: patch or build package, inline shapes/pages/media, ZIP
js/vsdx-source.js  IndexedDB cache for the original OPC package
js/svg-path.js  SVG path flattener for authored Visio Geometry
js/router.js    glue points, side selection, connector routing
js/render.js    SVG renderer (shapes, connectors, pictures, selection overlay)
js/interact.js  pointer & keyboard interaction
js/ui.js        palette, inspector, context menu, page tabs
js/file.js      autosave, import/export (JSON, SVG, PNG)
js/util.js      geometry and text helpers
```

Coordinates are plain pixels in "world" space; the view holds a pan offset and a
zoom factor that the scene `<g>` is transformed by. Everything is re-rendered
from the document on each change, so the document is the single source of truth
and undo is just a JSON snapshot.

## Licence and trademarks

LayoutCraft is licensed under either of the MIT licence (`LICENSE-MIT`) or the Apache License 2.0 (`LICENSE-APACHE`), at your option.

LayoutCraft is an independent open-source project. It is not affiliated with, authorised or endorsed by Microsoft Corporation.
Microsoft and Visio are trademarks of Microsoft Corporation; they are used here only to say which file format LayoutCraft is
compatible with. The project rules forbid including Microsoft code, stencils or other assets (see `docs/14-v1-scope-provenance.md`).
