# Visio import

> This note describes the removed JavaScript importer. The Rust core reads `.vsd` and `.vdx` as described in [47](47-legacy-formats.md).

> Historical note: the JavaScript editor described here (the `js/` folder) was removed; see [42](42-javascript-removal.md). The last commit that still has it is `c9aa852`. The Rust crates carry this work now.

## Entry points

`js/visio.js` exports `importVisioFile(file)`. It dispatches on extension:

- `.vdx` → `importVDX(text)` (single Visio 2003 XML file)
- `.vsdx`, `.vstx`, `.vsdm`, `.vstm` → `readZip(arrayBuffer)` then `importVSDX(zip)`
- everything else → error

`.vsd` is deliberately unsupported: it is a legacy binary format.

## ZIP reading

`readZip` is a small central-directory ZIP reader:

- finds the End of Central Directory (EOCD)
- reads central directory entries
- supports stored (method 0) and deflate (method 8)
- inflates deflate entries with `DecompressionStream('deflate-raw')`
- returns a `Map<path, Uint8Array>`

It does not support ZIP64, encryption, or unusual compression methods. Standard
VSDX files use deflate and work.

`VSDX` page parts are located through `visio/document.xml`, its relationships,
`visio/pages/pages.xml`, page relationships, and a sorted
`visio/pages/pageN.xml` fallback.

## Master and style resolution

`VisioFile` (in `js/visio.js`) holds:

- `masters` — parsed `visio/masters/masters.xml` and each master part
- `styles` — all `StyleSheet` elements from `document.xml`
- `theme` — parsed theme and variation colour schemes

For each page shape:

- the page shape's own cells override inherited master cells
- a cell marked `F="Inh"` is skipped only if its cached `V` is not concrete
- `MasterShape` resolves nested master body shapes
- style chains are followed through `TextStyle`, `FillStyle` and `LineStyle`
- `theme` and `QuickStyle*Color` cells supply themed/quick-style colours

The model does **not** retain master references; the final appearance is
flattened into the editable shape and `parts`. Phase A/B/C save metadata retains
the page shape ID, not a reconstructable master graph.

## Shape traversal

`buildPage` walks page `<Shape>` elements recursively for known containers.

Special handling:

- **CONNECTOR_MASTERS** and any shape with `ObjType=2` and Begin/End cells are
  imported as connectors, even when the master is a group.
- **Page-level groups** (`Type="Group"` with nested `<Shapes>` and no
  master-derived body) are traversed. Every child is imported with the group's
  origin and `group` metadata; nested groups keep `rootId`/`parentId`/`depth`.
  The group text block and text style are stored as `labelRel`/`labelStyle` so
  the editor can draw the group label behind its members.
- **Containers** are detected from `CONTAINER_MASTERS`, container categories
  (`CFF Container`, `Pool`, `Swimlane`, `Lane`, `Phase`,
  `ContainerStyleDefaults`) or `User.msvSDContainer*` rows. A bare
  `User.msvStructureType="Container"` is not enough: Azure/network icon groups
  use it but must keep their parts as one shape.
- **Inline CFF roles** are classified from the explicit Category/Structure and
  list rows: `CFF Container` → pool, `CFF List` + `Swimlane` →
  lane list, `CFF List` + `Phase` → phase list, `Swimlane` → lane, `Phase` →
  separator. A plain shape with these rows gets `containerKind`,
  `containerId` and `containerInfo` and a `page.containers` record, so
  LayoutCraft-authored swimlanes re-import as editable CFFs. Lists link to their
  pool and lanes/separators link to their list; the `Relationships` cells stay
  the writer's source of truth.
- Container bodies and titles are imported and grouped; **membership of user
  tasks is not resolved** (Visio stores it through `CONTAINERSHEETREF`/list
  formulas, not a `<Member>` tree).
- Non-container groups with a master that has a nested body are treated as one
  custom shape, using the master's body shapes as `parts` (icons, 3-D blocks).
- Rotated page-level groups are handled with the full transform chain: each
  group's `Pin`/`LocPin`/`Angle` composes onto its parent, so children import at
  their real page positions and carry the cumulative rotation. A group's text
  block is mapped through the same transform and the renderer draws the rotated
  label. `page.visio.rotatedGroups` still records the rotated group IDs; saving
  supports a whole-group move but not a rotated resize (see
  [Save](05-vsdx-save.md#group-edits)).
- Ordinary page shapes are imported as catalogue shapes or custom shapes.
- Imported `z` is assigned from source order; connectors and shapes share the
  page stack.

## Containers and lists

Container/list shapes are identified from `User.msvSDContainer*` /
`msvSDListItemMaster` metadata (a bare `msvStructureType="Container"` is not
enough — see the Azure icon-group false positive). Membership is serialised in
each shape's `Relationships` cell:

```text
container: SUM(DEPENDSON(1,Sheet.7!SheetRef(),Sheet.6!SheetRef()))
member:    SUM(DEPENDSON(4,Sheet.1!SheetRef()))
```

`layoutcraft-vsdx/src/containers.rs` decodes types 1 (container→members), 2 (list→items),
4 (member→containers), 5 (list item→list) and 11 (list↔related list); either end of a membership is enough, and a
reference to a shape the page does not have is left out. The page gets `containers: [{ visioId, kind, members, items,
related }]` (`kind` is `list` for a record with items); members get `shape.containers` / `shape.listOf` (connectors
`containers`). A shape can belong to several containers (a CFF task is in the pool list, a lane and a phase).

Deleting a container or list deletes what it holds, and copying one copies what it holds, the copies naming each other;
a deleted shape's references leave the other shapes' cells ([45](45-containers.md)). Moving a container does not carry its
members yet, and moving a shape in or out does not change membership.

## Geometry

`partsOf` builds the drawable representation:

- A shape with its own geometry is drawn from its evaluated rows.
- A shape with a master body can produce one part per nested master sub-shape.
- 3-D masters detect bevel cells and foreshorten the top face; a front face is
  shaded and a bottom lip is added.
- Page geometry merges master geometry rows with page overrides.
- `NoFill`, `NoLine` and `NoShow` flags are honored where evaluated.

Supported row types:

| Row | Handling |
| --- | --- |
| `MoveTo`, `LineTo` | Exact |
| `RelMoveTo`, `RelLineTo` | Exact relative mapping |
| `ArcTo` | Exact circular arc from the bow (`A`, sagitta) |
| `Ellipse` | Centre plus edge points converted to two arcs |
| `EllipticalArcTo` | Ellipse solved from start/end/control point plus `C` angle and `D` axis ratio, then sampled |
| `NURBSTo` | De Boor evaluation of the `E` NURBS formula, including degree, knot values, weights and `xType`/`yType` scaling; row `A`–`D` supply the surrounding knots/weights |
| `PolylineTo` | Endpoint plus the interior `POLYLINE(...)` vertices, with percentage/local coordinates |
| `SplineStart`/`SplineKnot`/`SplineEnd` | Uniform periodic splines (closed loops) from the stored knot sequence; other knot layouts fall back to a clamped uniform spline |
| `InfiniteLine` | Drawn as a long segment through the two points; the section is left unclosed |

Curves are sampled to fine polylines so the renderer, hit testing and SVG
export share one path representation. NURBSTo is what Visio's Freeform/spline
tool writes; the older SplineStart rows are supported for files that contain
them.

The importer is designed for rendering, not for reconstructing a ShapeSheet.
Unsupported geometry falls back to cached values or is approximated.

## Units

`inches(cell)` returns the cell's cached `V` directly. This is intentional:

- Visio's cached values are always internal units (`IN` lengths, `DA` radians)
- the `U` attribute is only for display
- applying `U` again caused the historical 72x/25.4x scaling bugs

## Formula evaluation

`FormulaParser` is a compact recursive-descent evaluator for the formula subset
used by geometry and colour cells. It is not a Visio formula VM.

Builtins/cases include:

`abs`, `acos`, `and`, `asin`, `atan`, `atan2`, `ceiling`, `cellisthemed`,
`cos`, `deg`, `exp`, `floor`, `guard`, `if`, `index`, `int`, `isthemed`, `log`,
`lumdiff`, `max`, `min`, `mod`, `msotint`, `not`, `or`, `power`, `rad`, `rgb`,
`round`, `setatref`, `setatrefexpr`, `setf`, `shade`, `sign`, `sin`, `sqrt`,
`strsame`, `tan`, `theme`, `themeguard`, `themeval`, `tint`.

`GeometryScope` lazily resolves references used by master geometry:

- `Width`, `Height`, `PinX`, `PinY`
- `Scratch.A1` (formula 1-based → XML row IX 0-based)
- `User.*`, `Property.*`, `Prop.*`, `Controls.*`
- `Sheet.N!Cell`
- `GeometryN.Xm`

If a reference cannot be resolved, the cached `V` is used.

A full formula engine is deliberately not part of the importer: the cached `V`
values that Visio stores with each cell are used instead.

## Theme and quick styles

Theme loading parses `visio/theme/theme1.xml` and related theme parts, including:

- colour scheme
- background colour
- variation colour scheme list (`vt:variationClrSchemeLst`)
- explicit fill scoring to choose the active variation palette

The importer maps:

- `THEMEVAL`/`Theme` slots such as `AccentColor`, `AccentColor2..6`,
  `VariantColor1..7`, `BackgroundColor`, `FillColor`, `FillColor2`,
  `ConnectorColor`, `TextColor`, `Dark`, `Light`
- `MSOTINT`, `TINT`, `SHADE`, `LUMDIFF`, `RGB`, numeric arithmetic
- `QuickStyleFillColor` and `QuickStyleLineColor`: 0–8 base theme colours,
  100–106 active variation slots

Large container-like shapes get a light tint (85%) of a quick colour rather
than the raw accent, which keeps swimlanes from becoming solid colour blocks.
Explicit cached fill/stroke values take priority over quick styles.

## Styling

- `styleOf` collects fill, stroke, stroke width and dash from the shape, master
  and style chains.
- `partStyle` applies per-geometry `NoFill`/`NoLine`.
- `characterStyle` combines the shape Character row, master row and text style
  rows for font name, size, colour, bold/italic/underline and font.
- `characterRowStyle` does the same for any Character row referenced by a
  `<cp>` run.
- `paragraphAlign` reads horizontal alignment from Paragraph row 0;
  `paragraphRowStyle` reads alignment, bullet/string and indent for a `<pp>`.
- `verticalAlign` reads `VerticalAlign`.
- `textBlock` resolves the text box position/size.

The editable model keeps the shape's base style plus an optional rich-text
model; ordinary single-run labels still use only `label`/style fields.

## Text

`textOf` produces the plain label:

- strips `\r`
- converts Visio U+2028/U+2029 line separators to `\n`
- normalizes non-breaking spaces
- collapses spaces/tabs
- trims

`richTextOf` parses the `<Text>` children into `shape.rich.paragraphs[]`:

- `<cp IX>` selects a Character row (font size/name, bold/italic/underline,
  colour)
- `<pp IX>` starts a paragraph using that Paragraph row (alignment, bullet,
  bullet string, indent)
- `<fld IX>` becomes a field run with its `Field` row formula, format and
  cached value
- plain text nodes become runs; CRLF/U+2028 split paragraphs

Rich text is retained when present (multiple runs/paragraphs, fields, bullets).
On save, an unchanged label keeps the source runs; editing the plain label
collapses that shape to one run. Fresh/rebuilt shapes write the stored runs,
paragraphs and fields back.

## Connectors

Connector import:

- reads `BeginX/BeginY/EndX/EndY`
- reads `<Connects>` to find glued sheets
- uses exact local glue when the cached endpoint maps inside the target
- reads the connector's own Geometry rows as route waypoints
- skips `Del="1"` and empty rows
- imports line style, width, colour, label and arrow cells
- maps UML arrows: 12 dependency, 14 inheritance, 22 aggregation,
  38 composition; 0 none; 1/2 open; 4 filled default
- maps LinePattern: 0 none, 2 dashed, 3 dotted, 23 fine dash

UML/database relationship masters are groups but import as connectors via
`ObjType=2`/known names. Their `1`/`*` multiplicity text shapes remain separate
editable shapes.

Some connector route changes in Visio may not preserve dynamic rerouting after a
patch save; see [VSDX save](05-vsdx-save.md).

## Shape Data and data graphics

Visio Shape Data is `Section N='Property'` with one `Row N='Row_N'` per property.
`Value`, `Prompt`, `Label`, `Format`, `SortKey`, `Type`, `Invisible`,
`Verify`, `DataLinked`, `LangID` and `Calendar` are imported into
`shape.data`. String values carry `U='STR'`; the cached `V` is what is read.

A LayoutCraft-authored data callout is an ordinary inline shape with a small
amount of transport metadata:

```xml
<Section N='Property'>
  <Row N='msvCalloutField'>
    <Cell N='Value' V='Alice' U='STR' F='Sheet.1!Prop.Row_1'/>
  </Row>
</Section>
<Section N='Field'>
  <Row IX='0'><Cell N='Value' V='Alice' U='STR' F='Prop.msvCalloutField'/></Row>
</Section>
<Text>Owner: <fld IX='0'>Alice</fld></Text>
<Section N='User'>
  <Row N='layoutcraftDGTarget'><Cell N='Value' V='1'/></Row>
  <Row N='layoutcraftDGField'><Cell N='Value' V='Row_1' U='STR'/></Row>
  <Row N='layoutcraftDGLabel'><Cell N='Value' V='Owner:' U='STR'/></Row>
  <Row N='layoutcraftDGPosition'><Cell N='Value' V='top' U='STR'/></Row>
</Section>
```

The `msvCalloutField` Property row is the bridge: Visio resolves a field from
a Property row, while a field whose formula is a direct cross-sheet property
reference is displayed blank. Non-free callouts also carry `PinX`/`PinY`
formulas such as `Sheet.1!PinX+(Sheet.1!Width*0.5-Sheet.1!LocPinX)` plus the
placement offset, so they follow the target in Visio.

The importer reads the `layoutcraftDG*` rows and restores `shape.callout` and
`shape.attachedTo` (resolved after every shape has its Visio ID). The callout's
own `msvCalloutField` row is hidden from the Shape Data UI. Visio's built-in
DataGraphic masters are hidden masters of `MasterType='2'`/COM type 5 with
`GraphicItems`; imported documents expose their callouts as ordinary nested
shapes, not as editable callouts in LayoutCraft.

A **data bar** is one inline shape with two Geometry sections:

- `Geometry IX='0'`: the filled body, `NoLine=1`; the right-edge `X` cells carry
  `F='Width*IF(<max>=<min>,0,MIN(1,MAX(0,(Sheet.<target>!Prop.<row>-<min>)/(<max>-<min>))))'`,
  so Visio fills the bar to the live percentage.
- `Geometry IX='1'`: the track outline, `NoFill=1`, using the shape's line
  colour/width.

The same `Property.msvCalloutField`, `Field` and text transport is used for the
value label (suppressed with `User.layoutcraftDGShowValue=0`). User rows
`layoutcraftDGBar`, `layoutcraftDGMin`, `layoutcraftDGMax` and `layoutcraftDGShowValue`
restore the kind, scale and label state; the importer takes the bar fill from
Geometry 0 and the track stroke from the last Geometry part.

A **data icon** is one inline shape with six Geometry sections:

- `Geometry IX='0'`: all five glyph outlines, `NoFill=1`, line colour/width
  from the shape.
- `Geometry IX='1'`…`'5'`: one filled glyph each, `NoLine=1`, with
  `NoShow='IF(User.layoutcraftDGLevel>=N,FALSE,TRUE)'`.
- `User.layoutcraftDGLevel` caches the current 1–5 level and carries a live
  formula over the target's property, e.g.
  `MAX(1,MIN(5,INT(((Sheet.1!Prop.Row_2-0)/(2000-0))*5)+1))`. Changing the
  property therefore hides/shows the filled glyphs in Visio.
- `layoutcraftDGIcon`, `layoutcraftDGIconSet`, `layoutcraftDGMin`, `layoutcraftDGMax` and
  the other `layoutcraftDG*` rows restore the kind, style and range. The importer
  takes the fill from the first filled part and the stroke from the outline
  part; `HideText=1` keeps the icon shape text-free.

A **data heading** is the same field-run callout transport with
`layoutcraftDGKind='heading'`. It has a filled box and shows the live value (and
optional prefix), but no leader line. `Heading 1`–`3` in Visio's `DVCALL`
stencil works the same way at runtime: a filled callout with a field, border
and fill options. LayoutCraft writes the heading as a normal text-field callout
with its own fill/line/font cells, so Visio renders and updates it live.

A **colour-by-value rule** lives on the target shape, not on a separate
callout. The importer restores it from User rows and the live fill formula:

```xml
<Cell N='FillForegnd' V='#ffeb84'
      F='IF((Prop.Row_2-0)/(2000-0)&lt;0.3333333333,RGB(248,105,107),
          IF((Prop.Row_2-0)/(2000-0)&lt;0.6666666667,RGB(255,235,132),
             RGB(99,190,123)))'/>
<Section N='User'>
  <Row N='layoutcraftDGColor'><Cell N='Value' V='1'/></Row>
  <Row N='layoutcraftDGColorField'><Cell N='Value' V='Row_2' U='STR'/></Row>
  <Row N='layoutcraftDGColorMin'><Cell N='Value' V='0'/></Row>
  <Row N='layoutcraftDGColorMax'><Cell N='Value' V='2000'/></Row>
  <Row N='layoutcraftDGColorLow'><Cell N='Value' V='#f8696b' U='STR'/></Row>
  <Row N='layoutcraftDGColorMid'><Cell N='Value' V='#ffeb84' U='STR'/></Row>
  <Row N='layoutcraftDGColorHigh'><Cell N='Value' V='#63be7b' U='STR'/></Row>
  <Row N='layoutcraftDGColorBase'><Cell N='Value' V='#ffffff' U='STR'/></Row>
</Section>
```

The formula is three equal bands from min to max. The cached `FillForegnd` is
the current band; the importer keeps it as the shape fill but derives the
visible fill from the rule. `layoutcraftDGColorBase` restores the pre-rule fill
when the rule is removed.

## Pictures and media

A Visio picture is a `<Shape Type="Foreign">` whose `<ForeignData>` names a
package media part through a relationship:

```xml
<Shape ID="1" Type="Foreign" ...>
  <Cell N="ImgOffsetX" V="0" F="ImgWidth*0"/>
  <Cell N="ImgWidth" V="5.6667" F="Width*1"/>
  ...
  <ForeignData ForeignType="Bitmap" CompressionType="PNG">
    <Rel r:id="rId1"/>
  </ForeignData>
</Shape>
```

The importer:

- resolves the page's rels part (`visio/pages/_rels/pageN.xml.rels`) and reads
  `visio/media/*` from the ZIP; a picture inherited from a master resolves
  through the master part's rels
- decodes the bytes to a data URL and shares one `doc.media` record between
  every shape that uses the same part (imported Visio packages already share a
  part, for example one PNG used on five pages)
- reads the natural pixel size from the PNG/JPEG/GIF/BMP header
- imports the picture as `shape.type = 'image'` with
  `shape.image = { mediaId, offsetX, offsetY, scaleX, scaleY, brightness,
  contrast, gamma, transparency, unsupported }`
- normalises `ImgOffsetX/ImgOffsetY/ImgWidth/ImgHeight` by the shape box, so a
  crop stays tied to the frame when the shape is resized in the editor
- uses Visio's own semantics for the adjustment cells; see
  
- keeps the text block, label, Character style, rotation, flip and group
  membership like any other shape

Supported previews are PNG, JPEG, GIF, BMP, WebP and SVG. EMF, WMF and TIFF are
kept in `doc.media` (so patch saves and fresh rebuilds preserve them) but the
editor shows a placeholder; Visio may still display the original in a patched
file.

**Transparency in 32-bit BMP pictures.** The BMP format calls the fourth byte of an uncompressed 32-bit picture with the
40-byte header unused, yet a user's `.vsdx` held such pictures with transparency in it (byte counts only: clear pixels were
black, edge pixels partial). Ignoring it drew a black background. The PNG served to renderers now treats the byte as alpha
unless it is 0 in every pixel or 255 in every pixel. If any colour is above its alpha, the colours are straight; otherwise
they are taken as premultiplied and scaled back. Some pictures of that file fitted each case. The package part is not changed.
Limits: a very dark straight-alpha picture looks premultiplied, so its soft edges come out too light; bitmaps inside EMF and
WMF pictures are not covered. This has **not been compared with Visio**.

**Indexed-colour quirk.** Visio 2021 ignores Brightness/Contrast/Gamma on
palette-indexed pictures (PNG colour type 3 and GIF). Transparency still
applies. The importer records `media.adjust = false` and the renderer skips the
filter, so the editor matches Visio; the inspector disables those three
controls with an explanation.

`.vdx` images are not resolved: VDX has no package media parts, and the VDX
path passes no rels. `.vsd` remains unsupported. OLE objects and ink are not
imported.

## VDX differences

`.vdx` is parsed from one XML document. The importer uses inline page shapes and
styles. Master resolution is much more limited than for `.vsdx`; no master
parts are loaded from `ctx.masters`. Treat `.vdx` as best-effort. It can still
be rebuilt into a fresh `.vsdx` from the imported model.

## Known import gaps

- `.vsd` binary
- EMF/WMF/TIFF pictures are preserved but not previewed; VDX images are not
  resolved
- OLE objects and ink
- full formula evaluation
- rich text is imported/rendered/saved, but hyperlinks in text, shape-data
  fields, autosize and some field formats are not modelled
- containers/groups: explicit groups are flat with group metadata, container
  membership is imported/editable, and inline containers plus horizontal/
  vertical CFFs can be authored; imported group lanes cannot be added/removed
- page setup UI, background editing, print settings, a layers pane (layers
  themselves import: see "Layers and guides" below)
- comments and review
- Visio-native DataGraphic masters: instances are recognised (master `User.msvCalloutType`
  = Text Callout / Heading / Data Bar / Icon Set) and tagged `nativeDataGraphic`
  (kind, master, target shape ID and field parsed from `Sheet.N!Prop.X`, label,
  cached value). They keep their cached geometry and are not editable callouts;
  only LayoutCraft-authored data graphics are managed as callouts, bars or icons.
  The JS importer also keeps native instances nested in master-composited groups.
  `visio/data` recordsets/connections are not modelled
- VBA/custom XML
- full theme/style fidelity for exotic templates

For a prioritised list see [Known gaps](07-known-gaps.md).

## Layers and guides

The PageSheet `Layer` section (one row per layer, `IX` = layer index) imports to
`page.layers` as `{index, name, visible, print, locked}`. A shape's `LayerMember`
cell (`"0;2"`) imports to `shape.layers`. Visio's rule, checked against
`Page.Export` on `test/corpus/layers-guides.vsdx`: a shape is drawn when it has no
layers or at least one of its layers is visible; a shape on a visible and a hidden
layer stays drawn. Shapes that fail the rule get `layerHidden`, are skipped by
both renderers and by connector hit-testing, and members of a group on a hidden
layer inherit that. Unknown layer indices count as visible.

`Print = 0` only affects printing/export (Visio's `Page.Export` omits those shapes
but they are visible on screen), so it is recorded but not applied. Locked layers
are recorded; the editor does not enforce the lock yet. Connectors are not
filtered by layer.

`Type="Guide"` shapes import to `page.guides` as `{visioId, xIn, yIn, angle}`
(inches, Visio's y-up page space). They are not drawn, no longer appear as stray
empty shapes, and a patch save leaves them in the package untouched.

## Group identity

Groups that are flattened into world-space member shapes (page groups and
containers) are listed in `page.groups` as `{visioId, parentId, rootId, depth,
kind, name, members, frame}`. `members` are the Visio IDs of the shapes directly
inside, `parentId` is the enclosing group (null at top level) and `kind` is
`group` or `container`. `frame` is the group's own oriented box in the same
convention as a shape (`x`/`y` top-left of the unrotated box around its world
centre, `w`/`h`, `rot` in degrees), so a rotated group keeps its true frame;
`bounds` is the axis-aligned box of that frame. A group with no usable
`Width`/`Height` falls back to the union of its descendants' boxes with `rot` 0.
Tests check that every member's rotated corners lie inside the group's oriented
frame and bounds. Master-backed groups are drawn as one shape and have no entry.

## EMF pictures

Visio stores pictures from other tools as EMF (`ForeignType='EnhMetaFile'`). The Rust
importer converts them to SVG (`layoutcraft_vsdx::emf`), so the media record becomes
`image/svg+xml` and every renderer draws them as an ordinary image. Consumers get the
renderable bytes from `media_bytes(package, record)`; the package part stays the
original EMF, so patch saves keep it byte-for-byte.

Handled: polygons, polylines, Béziers (16/32-bit), poly-polygons, lines, rectangles,
round rectangles, ellipses, paths (fill, stroke, clip), pens, brushes, world
transforms, window/viewport and fixed map modes, clip rectangles/regions, DC
save/restore and simple text. Skipped, never an error: bitmap records
(`STRETCHDIBITS`/`BITBLT`), EMF+ records, arcs/pies/chords, raster operations, hatch
brushes (drawn solid), palettes. Parsing is bounds-checked and a malformed record is
skipped.

GDI draws a pen thinner than one output pixel exactly one pixel wide, and Visio's
page export shows it: Supermicro's server panels come out dense and near black because
they are built from thousands of 1-unit outlines. The converter reproduces this by
flooring stroke width to one pixel at the picture's display width, which the importer
records on the media record (`displayWidth`, the first shape's width).

`HideText` hides a master-backed group's own label (cached value, even when the
formula reads an action row); nested `Type="Shape"` children are left alone because
Visio still shows the text their parent group carries.
