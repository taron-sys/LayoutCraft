# Known gaps and recommended next steps

This is the working backlog as of 2026-10-27. It separates what is known from
what is merely unverified. Real-Visio acceptance is now done for the current
corpus; the remaining items are fidelity and feature work. For a feature-by-
feature coverage matrix (with per-engine status codes), see
Visio feature parity tracker (kept in the private notes).

## 1. Visio repair/compatibility

**Status:** verified on 2026-10-26 with Microsoft Visio LTSC Professional 2021
16.0.14332.20416.

The full matrix from [Testing](06-testing.md) was opened through Visio's normal
file-open path: all nine examples, the untouched and edited patch copies, the
fresh builds, the JSON rebuild, the page-deletion rebuild, the added-page case
and the authored-shape/connector cases. None produced a repair, corrupt-file or
safe-mode dialog. A deliberately broken package was used as a control and did
produce “Visio cannot open the file or a component in the file because it is
corrupt” (1400015), so a silent success is a real result.

The run found and fixed one real defect: Visio ignored `<Connects>` for
authored/re-routed connectors whose endpoint cells were plain values, so they
opened as loose lines. Both X/Y cells of a glued end now carry `_WALKGLUE` plus
`_XFTRIGGER` trigger cells; Visio then reports the glue and endpoints follow a
moved shape. See [Save](05-vsdx-save.md#new-connectors).

Remaining differences from a Visio-authored blank package (none caused a
repair dialog; all are optional hardening):

| Difference | Effect | Possible follow-up |
| --- | --- | --- |
| Fresh package has no `docProps/thumbnail.emf` or `custom.xml` | No preview thumbnail/custom properties | Generate/attach if desired |
| Fresh package writes a masters part, page rels and `theme1.xml` that Visio's blank omits | Accepted; extra parts are harmless | Keep or trim |
| Fresh package now embeds Visio's standard StyleSheets 0,1,2,3,4,6 and defaults 3/3/3/4 | Matches the Visio blank shell; required for container membership to load | — |
| Fresh blank page is 11×8.5 in; Visio on this metric machine defaulted to A4 portrait | Blank page size/orientation differs | Pick a default by locale, or match Visio's A4 |
| `app.xml` says `Application=LayoutCraft`, `AppVersion=1.0` | Metadata only | Optionally mimic Visio |
| Added pages while patching leave `app.xml`/`windows.xml` stale | Visio still shows the page; other consumers may see stale counts/tabs | Update on patch |

Do not assume a file is good because LibreOffice and the in-app importer accept
it — authored connectors passed both while Visio treated them as unglued. Open
the output in Visio after writer changes.

## 2. Import fidelity

| Gap | Impact | Notes |
| --- | --- | --- |
| No full formula evaluator | Some inherited geometry/colour may fall back to cached values | cached `V` is usually enough |
| Pictures | PNG/JPEG/GIF/BMP/WebP/SVG preview; crop, rotation/flip, transparency and truecolour brightness/contrast/gamma; grouped pictures; media inherited from masters resolves through the master relationship table; one shared media record per part | EMF vector pictures convert to SVG and render (Rust only); EMF bitmaps/EMF+/arcs, WMF and TIFF bytes are preserved but shown as placeholders; VDX images are not resolved; Visio's indexed-colour adjustment quirk is mirrored |
| No OLE/ink | Embedded objects and ink are absent | Patch mode preserves untouched package bytes; fresh rebuild cannot |
| Rich text runs/fields | Imported, rendered, edited and saved: per-run font/size/style/colour, paragraph alignment/bullets, Page/PageCount/FileName/Date and Shape Data fields, run-level inspector, field insertion/formula editing, autosize and shape hyperlinks | Text-level hyperlink runs, some field formats and autosize reflow in the editor are not modelled |
| Spline/NURBS/arc/infinite geometry | Imported and rendered: `NURBSTo` (degree, knot values, weights, `xType`/`yType`), periodic `SplineStart` rows, `PolylineTo` interior vertices, `ArcTo` bow, `EllipticalArcTo` and `RelEllipticalArcTo` ellipse reconstruction, relative Bézier rows and `InfiniteLine`. Zero-length `MoveTo` rows continue the current contour (Visio stencils use them to split outlines), and fills use Visio's even-odd rule so keyhole contours (electrical resistor bodies) keep their holes | Non-uniform `SplineStart` knot layouts are approximated; curves are sampled polylines |
| Custom-path rounding | `Rounding` imports as a pixel radius; filled parts are clipped to a rounded rectangle and a matching rounded outline closes the corners, so UML state/container frames render rounded | Per-corner/radius variations and rounding of non-rectangular geometry are approximated by the shape's bounding rectangle |
| Stencil text colour | Master-child text resolves its page-instance Character row first, then the merged/master row; bare `Themed` colours use the theme's light text slot, so UML state titles are white and behaviour text keeps its green | Character colour formulas (`IF`/`LUM`/`LUMDIFF`/`THEMEVAL`/`User.*`) are evaluated by `colorexpr.rs`; formulas outside that subset still fall back to the cached value |
| Auto-layout is layered only | Re-layout handles flow/tree-like graphs top-down or left-to-right, groups as units, and leaves containers/labels/backdrops alone | Visio's radial/circular/orthogonal LayoutEngine styles and lane-aware task layout are not modelled; unconnected shapes stay put on a connected page, and rotated shapes use their unrotated box |
| Groups/containers flattened | No group identity; some nesting is approximated. Master-body children scale from master to instance and compose rotation/flip; master-backed groups expose picture children; centre-based add-in (`ImageNode`) geometry is offset into its box | Container masters are traversed; group masters become parts; no general group identity |
| Drawing-scale pages | `PageScale / DrawingScale` is stored on the page and applied to geometry; text keeps paper size, so architectural plans render at page size. Referenced background pages are composited beneath the foreground when their scale matches | Drawing-scale edge cases (rotated/anisotropic, mismatched background scale) are approximate |
| Layers/page setup/backgrounds | Layers (visibility, print, lock, membership) and guides import; shapes on hidden layers are not drawn; page setup/backgrounds are not represented | No layer pane/editing, lock not enforced, `print=0` not applied on screen, connectors not layer-filtered; imported page XML preserves everything when patched, fresh rebuild loses layers |
| Data graphics / comments | Shape Data, hyperlinks, fields, LayoutCraft-authored text callouts, headings, horizontal numeric bars, five-level icon sets and colour-by-value rules are modelled; native Text/Bubble callout child groups flatten and display current Shape Data values | Native DataGraphic instances are tagged (`nativeDataGraphic`) but not instantiated or rebound, recordsets are not modelled, and comments are not modelled |
| Fill/line gradients and effects | Master/part `FillGradient` and `LineGradient` stops, opacity, angle and direction (`FillGradientDir`: 0 linear, 1-7 radial, 8-12 rectangular, 13 path) import into `ShapePart.fillGradient`/`stroke_gradient` and render as SVG linear/radial gradients. Stops without an explicit colour inherit from the matching row of the FillStyle/LineStyle gradient section (Visio's network and 3-D stencils keep colours on the style). `LineColorTrans`/`FillForegndTrans` drive stroke/fill opacity, so ImageNode master backgrounds stay invisible. `FillPattern` 2-24 render as two-colour SVG pattern tiles. Outer/oblique `ShapeShdw*` shadows, explicit `Glow*`, `SoftEdgesSize` and `Reflection*` effects import and render; top/bottom bevel types 1-12 render as a clipped gradient ring. Theme matrices use linear-light DrawingML tint/shade. | Rectangular gradients are approximated by a linear fade along the preset direction; exact 3-D lighting/material and RotationX/Y/Z, inner-shadow page-default nuances (this Visio build renders it as an offset shadow), gradient-angle edge cases |
| Theme/quick-style subset | Base colours, active variation palettes, quick-style colour indices, the theme's `fmtScheme` fill/line/effect style lists, quick-style matrices 1-6 and variant matrices 100-103, line width/dash and effect-matrix outer shadows, plus five quick-style presets; theme presets/UI apply concrete colours | Theme XML/index cells are not rewritten; pattern fills and bevel/3-D effects are not modelled |
| Document default style | `DocumentSettings` Fill/Line/Text defaults and role-aware bare-`THEMEVAL()`/`Themed` resolution are applied; unstyled shapes now render white fill, dark line/text | Quick-style matrices that theme a default cell to a variation colour beyond the supported indices |
| VDX master support limited | `.vdx` may lose master-derived styling | Best-effort; fresh rebuild is possible |
| `.vsd` binary | Unsupported | Out of scope unless a separate binary parser is added |

## 3. Patch-save limitations

| Gap | Current behaviour | Recommended work |
| --- | --- | --- |
| Page deletion | Prompts to rebuild fresh; partial XML deletion not implemented | Implement page removal from pages.xml, rels, content types, windows, TopPage |
| Imported z-order changes | Warned, not written | Reorder top-level page `<Shape>` children safely, respecting groups/containers |
| Imported shape-type changes | Warned, not written | Needs geometry/style replacement or source-element rebuild |
| Imported font-family changes | Warned, not written | Write `Character.Font`; requires font name/index mapping and style chain handling |
| Per-part fill/line edits on imported shapes | Rust: `part_style_patch` writes them back to the source sub-shape (Visio-verified); the frozen JS editor still ignores them and warns | Per-part style UI; creating a missing page-level child; edits on shapes with several geometry sections share the shape's one fill |
| Per-run text edits | Run text/style, paragraph alignment/bullet, fields, Shape Data and autosize are editable in the inspector; unchanged rich text is preserved byte-for-byte; canvas label editing collapses to one run | Add text-selection formatting on the canvas and text-level hyperlink runs |
| Pictures | Position/size/rotation/flip, crop, transparency, brightness/contrast/gamma and replacement are written; new media parts/rels/Content-Types are added | `Img*` cells are written with `Width*N` formulas; replacement leaves the old part in the package (not garbage-collected); a missing image record aborts the save |
| Data graphics | Transport `msvCalloutField` Property row, `Field` run and cached value, `layoutcraftDG*` User rows, target-relative `Pin` formulas, a data bar's two Geometry sections with a live width formula, a data icon's outline plus five conditional filled sections with a live level formula, a filled heading callout, and a target `FillForegnd` formula for colour by value; metadata changes rewrite the graphic | Grouped/container/rotated targets use static positions; bars are horizontal only; icon styles are boxes/diamonds/circles with five equal bands; colour rules use three equal bands; native DataGraphic masters and recordsets are not authored; Visio's field placeholder appears in `Shape.Text` but renders correctly |
| Dynamic connector route changes | Route, config cells and glue rewritten; `_WALKGLUE`/triggers restored; route styles 1–8 and fixed/walk/plow/ext config round-trip | Visio may reroute the connector after a later shape move (dynamic by design); obstacle avoidance and rendered line jumps are approximate |
| Group/container moves | Explicit unrotated groups and container frames rewrite their frame and text block; a pure move changes only the group PinX/PinY and leaves child formulas intact. Rotated groups import with their full transform chain and a whole-tree move patches only the group PinX/PinY | Rotated resize or a single rotated member moved on its own is skipped and warned |
| Container membership edits | Add/remove writes both sides' `Relationships` cells; unrelated `DEPENDSON` groups and group-only refs are preserved | Copy/paste of a whole CFF is not remapped |
| Authored containers/swimlanes | Basic containers and full horizontal/vertical CFFs can be authored inline (pool, lane list, lanes, phase list, separators); lanes can be added/removed/renamed and tasks moved between them in fresh and patched packages | Adding/removing lanes in an imported Visio CFF (group lanes) is not supported; imported phase separators cannot be removed |
| Authored group structure | Editor-only grouping; flattened to separate inline shapes with a warning | Write nested `<Shapes>` if authored groups become a saved feature |
| Rotated groups | Flagged on import; children use translation-only positions; save warns | Compose/invert the group transform chain |
| Ungroup/delete imported group shell | Ungroup is refused for imported groups; deleting all members warns that the shell remains | Remove the group subtree when its members are removed |
| Thumbnail | Preserved but not regenerated | Generate/update docProps thumbnail if desired |
| app.xml/windows for added pages | Not updated while patching | Update Pages/TitlesOfParts and optional window entries |
| Patch validation | No Visio/schema validation | Add Visio-opened round-trip tests |

## 4. Fresh-build limitations

Fresh builds are a model rebuild, not a copy of the original package.

| Gap | Impact |
| --- | --- |
| Masters dropped | Empty masters part; all shapes are inline |
| Per-part styles flattened | One fill/line per shape; warning |
| Rich text flattened | Plain labels; imported runs/fields are rebuilt only when the model has `shape.rich` |
| Macros/custom props dropped | Not in the editable model |
| EMF/WMF/TIFF pictures not previewable | Media bytes are written, but the in-app canvas and Visio preview depend on the format; EMF/WMF are written as `MetaFile` |
| Comments and native data graphics dropped | Shape Data, callouts, headings, bars, icons and colour rules are written; comments, recordset graphics and native DataGraphic masters are not in the model |
| Layers/backgrounds/page setup dropped | Generated layer 0 and basic PageSheet only |
| Dynamic connectors static | Current route only |
| Theme/style shell minimal | Generic DrawingML theme and embedded Visio StyleSheets; theme-picker colours are written as concrete shape cells |
| Page backgrounds lost | White/default page background |
| Page IDs renumbered | Fresh documents start at 0; page order preserved |
| Imported page sizes not preserved | Derived from content/default, not original PageSheet |
| Visio compatibility verified for the current corpus | Re-run the matrix after writer changes |

The fresh path is appropriate for new documents or intentional model rebuilds.
It should not be used casually on an important source VSDX when patch mode is
available.

## 5. Model/UI gaps

- Grouping/ungrouping exists for editor groups (`Ctrl+G` / `Ctrl+Shift+G`,
  context menu). Imported Visio group structure is **not** written back yet:
  moving members patches their cells, but the source group's frame/label and a
  fully authored group `<Shapes>` tree are not updated.
- Imported page-level groups select/move/resize as a unit. Explicit unrotated
  groups write their frame and text block on save (a pure move changes only the
  group `PinX`/`PinY`). Rotated groups import with the composed
  `Pin`/`LocPin`/`Angle` transform and rotated labels; a whole-group move saves
  by changing only the group `PinX`/`PinY`, while a rotated resize or a single
  member moved on its own is warned and not written. Group labels are imported
  with a relative text box/style and rendered in the editor behind the members.
- Containers and lists import their real membership from the `Relationships`
  graph (`page.containers`, `shape.containers`/`listOf`). Selecting a container
  frame brings its transitive members (pool → lists → lanes → tasks) so a drag
  moves them together; clicking a member selects only that member. Container
  frames are patched like group frames. Adding/removing regular membership is
  written back (context menu), and basic containers plus full horizontal or
  vertical CFF/swimlane structures can be authored from a selection in a fresh
  or patched document. Lanes and phase separators are structured list items:
  lanes can be added above/below, renamed and removed (moving their tasks to a
  neighbouring lane), tasks can be moved between lanes, and phases can be added
  or removed. Adding/removing lanes in an **imported** Visio CFF whose lanes are
  group shapes is refused; those files still support moving tasks between
  existing lanes and adding phases.
- Deleting every member of an imported group leaves the source group `<Shape>`
  in the package; the save dialog warns (“emptied group shell(s)”).
- No layers, page setup, backgrounds, guides or print settings UI.
- Pictures can be inserted, replaced, cropped, adjusted and grouped. There is no
  media manager or garbage collection: replacing an image leaves the old record
  in `doc.media` (undo may need it) and the old package part on a patch save.
  EMF/WMF/TIFF are not previewed; VDX images are not resolved; OLE/ink are not
  supported. Inserted WebP/SVG pictures are converted to PNG. Copy/paste carries
  picture bytes, including into a new document; editor groups are still
  flattened on save.
- No per-part style editing.
- Rich text has a run-level inspector, field insertion, Shape Data and autosize;
  text-selection formatting on the canvas and text-level hyperlink runs are not
  implemented.
- Shape Data, hyperlinks, fields, autosize, run formatting, text callouts,
  headings, data bars, icon sets and colour rules have an inspector; comments,
  native DataGraphic masters and recordset-driven data graphics do not.
- The connector inspector exposes route style, fixed code, line-route extent,
  walk preference and plow/jump code; the editor router does not yet use
  WalkPreference or draw line jumps.
- No page reordering/renaming UI.
- No preview for EMF/WMF/TIFF pictures or raster icons beyond imported vector
  parts.
- z-order is only partially represented; connectors default to behind shapes.
- Connector routing is recomputed every render from the editor route/config;
  straight/right-angle/curved plus Visio style variants 3–6 are supported.
  WalkPreference, line jumps, RTL and route scale are stored but not used by
  the editor router, and there is no obstacle avoidance.
- The importer stores single-style text per shape.

## 6. Browser/platform gaps

- Chrome/Edge are the supported targets.
- Static server uses `fileURLToPath` now, but should be run once on Windows to
  confirm.
- ZIP reader does not support ZIP64/encryption.
- ZIP writer does not preserve ZIP timestamps/comments/permissions.
- IndexedDB side cache is per browser/origin and cleared with site data.
- Old source packages are not garbage-collected when a fresh build resets the
  lineage; they remain as orphaned IndexedDB records unless site data is cleared.
- Autosave is a single localStorage entry. Picture data URLs count toward the
  browser's storage quota; a document with several large photos can exceed it.
  A failed autosave keeps the previous copy and shows a status warning; VSDX
  save and JSON export are unaffected. Storing the document/media in IndexedDB
  would remove this limit.
- Pictures are held in memory as data URLs; undo snapshots share the media map
  by reference instead of duplicating it, but the map keeps every image for the
  life of the document (no garbage collection after replacement/deletion) and a
  very large picture set still consumes memory.
- The app has no build step; keep it that way unless there is a strong reason.
- No service worker/offline install.

## 7. Verification and engineering gaps

- The Windows Playwright/Visio harness exists but is not committed; the app
  still has no test runner.
- No Git repository at handover time.
- `test/MS Example-edited.vsdx` is an output artifact, not a suite. The
  Visio-authored group/container corpus is in `test/corpus/`.
- No schema validation (XSD) against MS-VSDX.
- No real-Visio corpus round-trip in CI.
- No test for very large documents/memory.
- No accessibility review of the canvas/toolbar.
- No error surfacing of Visio repair causes inside the app.

## Recommended Phase D order

### D1 — Visio validation and test harness (done for this corpus)

- Windows Playwright + Visio checkpoint process exists outside the app
  (see [Testing](06-testing.md#harness-used)).
- Patch/fresh/round-trip matrix recorded; repair dialogs captured with a
  broken-package control.
- ZIP/XML checks are local scripts; they are not part of the zero-dependency
  app.
- **Next:** commit or script the harness paths/env vars, and re-run the matrix
  after writer changes. Add an XSD/schema check if one is available.

### D2 — Close high-value patch gaps

1. Patch-page deletion (avoid fresh rebuild for one page).
2. Update `docProps/app.xml`/`windows.xml` for added pages.
3. Imported z-order reordering for top-level shapes.
4. Imported font-family write support.
5. Per-run text preservation for label edits.

### D3 — Rich text and per-part styling

- Extend the model with text runs/paragraphs. **Done.**
- Add per-part style editing.
- Map part style changes back to source sub-shapes or regenerate them.
- Imported media handling. **Done:** pictures import/render/crop/adjust/save
  with shared media parts; remaining work is an EMF/WMF/TIFF rasteriser, VDX
  images and media garbage collection.
- Data graphics. **Done:** Shape Data bound callouts, headings, horizontal
  numeric bars, five-level icon sets (boxes/diamonds/circles) and three-band
  colour-by-value rules with live values/levels/fills, target-relative
  positions and save/import. Remaining data graphics: more icon styles,
  vertical bar matrices, native DataGraphic masters and data recordsets.

### D4 — Dynamic connectors and containers

- Preserve/write connector formulas and triggers.
- Retain group/container identity.
- Support moving/resizing group members and containers in Visio.
- Consider page backgrounds/layers/page setup.

### D5 — Fresh-package hardening

- Compare generated shell against a Visio-created blank file. **Done:** the
  differences are listed in section 1; none blocks opening.
- Decide whether to add the standard style sheets, thumbnail/custom props and
  locale-aware page default (see section 1).
- Preserve page dimensions/layers when rebuilding with a source package
  available.
- Optional media export from the model. **Done:** pictures write media parts,
  page rels and Content-Types in fresh packages. Master export remains open.
- Prune unused `doc.media` records on save/export and add a picture/media
  manager; the current model keeps replaced images for undo.

### D6 — Out of scope unless explicitly approved

- `.vsd` binary parser.
- Full Visio formula VM port.
- Full Visio object/automation model.
- 3-D rendering beyond the current bevel approximation.
- OLE/ink/VBA execution.

## Risk notes for a new developer

- **Do not "clean up" provenance fields** (`pg.visio`, `shape.visio`,
  `conn.visio`, `doc.visioKey`). They drive patch and lineage safety.
- **Do not apply unit attributes twice.** Cached `V` values are internal units.
- **Do not replace the current route formulas blindly.** Unchanged connectors
  intentionally keep their source formulas for dynamic behavior.
- **Do not write `LineColor="none"` or `Color="none"`.** Use a valid colour plus
  LinePattern 0. This previously made the whole file fail in LibreOffice.
- **Be careful with page IDs and `TopPage`.** Fresh pages start at 0 and the
  generated `TopPage`/window must match.
- **Preserve package entries byte-for-byte in patch mode.** Regenerating
  document/theme/master parts is the fastest way to lose fidelity.
- **Do not write `<Connects>` without `_WALKGLUE`.** A connector with glue rows
  but plain endpoint values opens as a loose line in Visio. Write
  `_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)` on both X and Y of each
  glued end, plus the `BegTrigger`/`EndTrigger` `_XFTRIGGER` cells.
- **Test both paths after every change:** patch an existing package and build a
  fresh package.
- **A LibreOffice success is not a Visio success**, and a successful re-import
  is not either — the connector-glue defect passed both. Repeat the acceptance
  checklist on Windows.


**EMF+ (2026-11-01).** Metafiles that carry EMF+ records are played through `emf/plus.rs` when every drawing record is supported (solid brushes/pens, paths, rectangles, ellipses, polygons, transforms, clips, bitmaps). Gradient/hatch brushes, dashed or custom-cap pens, strings and curves fall back to the GDI records. Visio's linear gradient angle is clockwise on screen (0 = left to right, 90 = top to bottom).
