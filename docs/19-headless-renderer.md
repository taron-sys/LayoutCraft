# Headless scene, SVG and raster renderer

Status: 2026-10-28. `layoutcraft-render` is the shared headless render path for
page display, SVG export, PNG export and visual-golden comparison.

## API

```rust
use layoutcraft_render::{scene_from_document, to_svg, render_png};

let scene = scene_from_document(&document, page_index)?;
let svg = to_svg(&scene);
let png = render_png(&scene, 1.0)?;
```

`scene_from_document_with_media` accepts `media_id -> bytes` so pictures can be
embedded as data URIs. The `render_vsdx` example reads those bytes straight from
a VSDX package.

The scene is renderer-independent: page bounds/background plus ordered
`SceneItem::Shape` and `SceneItem::Connector` records. Shapes carry resolved
paint, geometry parts, image placement/adjustment and text runs/paragraphs.
Connectors carry resolved world points, waypoints, arrows and labels.

## Coordinates and bounds

- Shape geometry parts are local to the shape box; the renderer translates them
  by the imported world `x/y` and applies the shape rotation around its centre.
- Connector endpoints resolve `Endpoint.point`, or target shape +
  normalized `local` (with rotation), plus any imported waypoints.
- Page bounds are the union of shape boxes and connector points expanded by half
  the stroke width and rounded for raster output. This matches Visio
  `Page.Export` content bounds for the goldens; text boxes are included when
  they extend the content box.
- The SVG keeps an exact `viewBox` and uses integer `width`/`height`; `resvg`
  rasterizes at 1× or a requested scale.

## Rendering

- Fill/stroke/fill-opacity/stroke-width, dash presets, part-specific paint and
  rotated groups.
- Pictures: crop/scale (`ImgOffsetX/Y`, `ImgWidth/Height`), clipping to the shape
  box, transparency, and brightness/contrast/gamma filters (skipped for indexed
  media where Visio ignores them).
- Text: plain labels and rich runs (`<tspan>` per run), paragraph alignment,
  bullets, indentation, fields, text boxes, vertical alignment and font/style
  overrides. Greedy word wrapping uses `fontdb`/`ttf-parser` glyph advances
  (with a fallback metric) so long labels break at the shape's text-box width.
- Connectors: glued endpoints, waypoints, dashes, start/end arrowheads and
  labels. Connectors without imported waypoints are auto-routed: straight,
  curved (outward control points) or elbow, including the fixed-fraction
  style-3/4/5/6 variants and side-aware anchors.

The renderer tests additionally cover generated elbow routing; text wrapping
is exercised by the visual page renders and the deterministic SVG test.
- Data graphics: LayoutCraft callouts use the target's Shape Data value; data
  colour rules pick their band. Native Visio DataGraphic masters remain an
  import limitation.

## Raster and visual validation

```powershell
cargo +stable-x86_64-pc-windows-gnu run -p layoutcraft-render `
  --example render_vsdx -- ..\examples\MS Example.vsdx 0 out.png
```

Rendering the 37 visual-golden pages through `render_vsdx` and checking them
with `layoutcraft-conformance --visuals` gives **188/188 visual checks** (all
reference and comparison checks: **310/310**): all 37 pages pass the pixel
thresholds, including CFF/swimlane list interior fills. Packages without a theme part no longer apply QuickStyle
variation colours, matching Visio's neutral white/black output for such
shapes.

The UML employee page, network plan, block diagram and AWS/SIOS page also
match their Visio references after the compound master-body and page-child
stencil fixes. Master-body children merge their visible Geometry sections into
one nonzero compound fill/stroke (hollow AWS icons), and nested page children
compose their `Pin`/`LocPin` offsets up the master tree (Auto Scaling Group,
logo tiles, icon details). Container titles are emitted even when their text
box geometry is hidden.

The image adjustment page matches after media embedding and SVG filters. Line
gradients render as `stroke="url(#...)"` linear gradients, and outer
`ShapeShdw*` shadows draw the silhouette offset behind each shape with an
optional Gaussian blur; the scene bounds include the shadow so the raster
viewport does not clip it. Explicit glow effects draw a blurred wide stroke,
soft edges blur the part group, and reflections mirror the silhouette below
with a fade mask. Quick-style matrices select theme fill/line/effect styles,
including variants 100-103. `FillPattern` 2-24 emit two-colour SVG pattern
tiles, gradient directions (`FillGradientDir` 1-7 radial, 8-12 rectangular,
13 path) render as SVG radial presets or directional linear approximations,
and stop colours missing on the shape inherit from its style's gradient
section. `LineColorTrans`/`FillForegndTrans` become stroke/fill opacity, so
master background rectangles that Visio keeps invisible no longer draw
bounding boxes around stencils. Filled paths use the even-odd rule (Visio's
geometry fill), and `RelEllipticalArcTo` rows expand like their absolute
counterparts, keeping stencil glyphs ("SQL", screen/lightbulb details) intact.
`TxtAngle` becomes a per-shape `textRot` counter-rotation so rotated electrical
symbols keep upright labels; connector glue stores local coordinates in the
target's rotated frame. `Rounding` clips the filled parts to a rounded
rectangle and draws a matching rounded outline, giving UML state/container
frames their corners. Master-child text resolves themed colours through the
page-instance Character row and the theme's light text slot, and QuickStyle
line colours resolve through Visio's variant palette (including
`vt:variationClrSchemeLst`). Connectors inherit a themed `EndArrow` from the
Connector StyleSheet as a filled arrow; repeated endpoint/waypoint pairs are
deduped before drawing so elbow routes do not lose their arrowhead. Within a
shape, filled Geometry sections paint before line-only sections (Visio's
fill-then-line order), keeping detail strokes on top. Connectors without an
authored `EndArrow`
draw no arrowhead, matching Visio. Oblique shadows scale from
`ShapeShdwScaleFactor`, and top/bottom bevel
types 1-12 draw a clipped gradient ring with an inset contour for the framed
styles. Connector labels use the authored Visio text-pin transform, split on
line breaks, and are included in scene bounds. Pages with a drawing scale
(`PageScale / DrawingScale`) scale geometry, parts, text boxes and connector
points into paper space while font sizes and stroke widths stay paper sized.
Master-body children scale from master to instance and compose rotation/flip;
pictures inherited from masters resolve through the master's relationships.
Referenced background pages are composited beneath the foreground when their
drawing scale matches. The next fidelity work is native data graphics/writers, gradient
pattern codes 25-40 and full 3-D lighting/rotation; the headless path itself is
stable.

## Tests

Four renderer integration tests cover:

- scene construction with rich runs;
- deterministic SVG with geometry and styled text;
- connector glue/waypoint resolution;
- `resvg` rasterization of `MS Example` at the golden page size (745×640).

Workspace tests: **40/40**.
