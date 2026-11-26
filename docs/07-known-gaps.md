# Known gaps and recommended next steps

This is the working backlog, reviewed on 2026-11-26 against `main` at 0a45c7c; the rows closed by #54 and #56 to #59 were updated afterwards. The previous version (2026-10-27) was written
for the JavaScript editor, which has since been removed ([42](42-javascript-removal.md)); items that only described that
editor were dropped (they remain in the Git history). Each item says how far it can be trusted:

* **Code**: confirmed on 2026-11-26 by reading or searching the Rust and browser sources. It was not necessarily tried.
* **Measured**: seen by running the program on sample files.
* **Unverified**: carried over from the previous backlog and **not checked** against the current code.

For what works, see [Features](48-features.md).

## 1. Visio repair/compatibility

**Unverified for the current code.** The last recorded acceptance run (2026-10-26, Microsoft Visio LTSC Professional 2021
16.0.14332.20416) opened the test matrix of [Testing](06-testing.md) without a repair, corrupt-file or safe-mode dialog; a
deliberately broken control file did produce one. That run predates the move to Rust.

It found one defect worth remembering: Visio ignored `<Connects>` rows when the connector's end cells held plain values. See
[Save](05-vsdx-save.md#new-connectors).

Differences from a Visio-made blank drawing noted then (none caused a dialog; all **unverified** now):

| Difference | Effect |
| --- | --- |
| A new package has no `docProps/thumbnail.emf` or `custom.xml` | No preview thumbnail or custom properties |
| A new package has a masters part, page relationships and `theme1.xml` that Visio's blank omits | Accepted as extra parts |
| A new blank page is 11 x 8.5 in; Visio on a metric machine defaulted to A4 | Different default size |
| `app.xml` names LayoutCraft as the application | Metadata only |

## 2. Import fidelity

| Gap | Status | Notes |
| --- | --- | --- |
| No formula evaluation during import | Code | `layoutcraft-formula` exists, but no other crate depends on it. Import relies on cached values; `colorexpr.rs` evaluates a subset of colour formulas |
| TIFF pictures are not previewed | Code | `media.rs`. EMF and WMF are converted to SVG; a metafile that cannot be converted is not previewed |
| Embedded OLE objects and ink are not shown or edited | Unverified | Package parts that are not edited are kept when saving |
| Old binary drawings lose formulas; version 5 and older are refused | Code | [47](47-legacy-formats.md). 414 of the local samples are version 5 (measured) |
| Connector glue that a binary drawing only inherits from a master is not restored | Measured | About 0.9 % of the connections in the paired samples ([47](47-legacy-formats.md)) |
| Most zipped stencils cannot be opened as drawings: all 12 `.vssx` and 10 of 12 `.vssm` samples tried | Measured | Stencils are meant to be opened in the Shapes panel |
| Rich text: text-level hyperlink runs, some field formats | Unverified | |
| Non-uniform `SplineStart` knot layouts are approximated; curves are sampled | Unverified | |
| `Rounding` on non-rectangular geometry is approximated by the bounding rectangle | Unverified | |
| Drawing-scale edge cases (rotated, anisotropic, background pages at another scale) | Unverified | |
| Data graphics made by Visio are not instantiated or rebound; no recordsets | Unverified | |
| Rectangular gradients, 3-D lighting and rotation, some shadow defaults are approximate | Unverified | |
| Parts of themes and quick styles (pattern fills, bevel and 3-D effects) are not modelled | Unverified | |

**EMF+ (2026-11-01, unverified since).** Metafiles that carry EMF+ records are played through `emf/plus.rs` when every drawing
record is supported (solid brushes/pens, paths, rectangles, ellipses, polygons, transforms, clips, bitmaps). Gradient/hatch
brushes, dashed or custom-cap pens, strings and curves fall back to the GDI records. Visio's linear gradient angle is
clockwise on screen (0 = left to right, 90 = top to bottom).

## 3. Editing and saving

The engine writes each edit into the opened package and saves that package. Since the previous backlog these exist (**Code**):
deleting, adding, renaming, duplicating and reordering pages; drawing order; replacing a shape with another master; comments;
layers; guides; ungrouping imported groups.

| Gap | Status | Notes |
| --- | --- | --- |
| In some imported drawings a glued connector end does not follow when its target is moved | Measured | Seen in all 4 samples tried (in three of them no tested end followed), with the same result on copies saved by Visio; a connector drawn in LayoutCraft followed |
| A rotated connector cannot be edited | Code | `groupedit.rs` refuses it |
| Ungrouping is refused for a group that is, or sits inside, an instance of a master | Code | |
| Replacing a shape is refused for connectors and members of groups | Code | `replace.rs` |
| Point editing is refused for shapes with several geometry sections and for some row types | Code | `groupedit/vertices.rs` |
| Some copy, turn and group operations are refused for connectors in turned or flipped groups | Code | |
| Adding or removing lanes of an imported cross-functional chart | Unverified | |
| `docProps/thumbnail` is not regenerated | Code | Nothing writes it when an opened drawing is edited |
| Page titles in `docProps/app.xml` are not updated when pages change | Code | No code writes `TitlesOfParts` |
| No schema or Visio validation of saved files | Code | See section 7 |

## 4. Building a package from the model

`write_fresh_document` builds a package from the model alone. The engine does not use it to save an opened drawing (**Code**);
it serves the built-in stencil and tests. Its limitations as listed before are all **unverified**: masters are dropped; one
fill and line per shape; rich text, macros, custom properties, comments, native data graphics, layers, backgrounds and page
setup are not carried over; connectors keep only their current route; the theme is generic; page IDs are renumbered.

## 5. Desktop app and AI assistant

| Gap | Status | Notes |
| --- | --- | --- |
| Not every command of the browser ribbon exists in the desktop app | Unverified | Changing quickly; see [43](43-ui-parity.md). The list in [42](42-javascript-removal.md) is a snapshot |
| No automatic layout in the engine, app, shell or MCP tools | Code | |
| No autosave or recovery of unsaved work | Code | |
| The model's own JSON file cannot be opened or saved from the hosts | Unverified | [42](42-javascript-removal.md) |
| No dedicated command for containers in the desktop app | Unverified | [48](48-features.md) |
| Connector routing: no obstacle avoidance; walk preference and some route settings are not fully used | Unverified | |

## 6. Browser/platform gaps

| Gap | Status | Notes |
| --- | --- | --- |
| The "Compare with file" picker offers `.vsd` but none of the other old formats | Code | The open and stencil pickers offer them all |
| Opened stencils are remembered in IndexedDB, per browser and origin | Code | Cleared with site data |
| No service worker or offline install | Code | |
| Supported browsers beyond Chrome and Edge | Unverified | |
| ZIP64 and encrypted packages | Unverified | The reader limits entries to 256 MiB each (`MAX_ENTRY_BYTES`) |

## 7. Verification and engineering gaps

| Gap | Status | Notes |
| --- | --- | --- |
| No Visio round trip in CI | Code | CI runs the tests on three systems and the public conformance fixtures |
| No XSD validation against [MS-VSDX] | Code | |
| Tests that need the private corpus do not run without it | Code | `cfg(private_fixtures)`, [38](38-private-fixtures.md) |
| `dxf_oracle` and `webpage_oracle` fail on one local sample drawing | Measured | On a maintainer machine with the private files, 2026-11-26 |
| No test for very large documents or memory use | Unverified | |
| No accessibility review of the canvas and ribbon | Unverified | |
| Windows builds are not code-signed | Code | [36](36-packaging.md), [50](50-code-signing-policy.md) |

## Next steps

Suggested order, from the items above:

1. Make imported glued connectors follow moved shapes (section 3).
2. Repeat the Visio acceptance run on files written by the Rust writer (section 1) and record it.
3. Check the **unverified** rows, and remove or confirm them.
4. Decide on a version 5 reader ([47](47-legacy-formats.md)).

Out of scope unless explicitly approved: a full Visio formula or automation model, 3-D rendering beyond the current bevel
approximation, and running OLE, ink or VBA.

## Risk notes for a new developer

Carried over; the reasons still apply, the details are **unverified** for the Rust writer.

- **Do not write `<Connects>` without the formulas.** A connector with glue rows but plain endpoint values opened as a loose
  line in Visio. A whole-shape end needs `_WALKGLUE(...)` on both X and Y and the matching `_XFTRIGGER` trigger cell.
- **Do not apply unit attributes twice.** Cached `V` values are internal units.
- **Do not write `LineColor="none"` or `Color="none"`.** Use a valid colour with `LinePattern` 0.
- **Preserve package entries you do not edit.** Regenerating document, theme or master parts loses fidelity.
- **A LibreOffice success is not a Visio success**, and neither is a successful re-import.
