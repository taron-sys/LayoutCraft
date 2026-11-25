# Document schema and golden fixtures

> Historical note: the JavaScript editor described here (the `js/` folder) was removed; see [42](42-javascript-removal.md). The last commit that still has it is `c9aa852`. The Rust crates carry this work now.

LayoutCraft's portable interchange is JSON. It is used by **Export JSON / Import
JSON**, browser autosave and the golden fixtures for the planned native port.
The current schema is **version 1**.

Machine-readable starting points:

- [`schema/layoutcraft-document-v1.schema.json`](../schema/layoutcraft-document-v1.schema.json)
- [`schema/golden-manifest-v1.schema.json`](../schema/golden-manifest-v1.schema.json)
- [`schema/visual-manifest-v1.schema.json`](../schema/visual-manifest-v1.schema.json)
- [`schema/fixture-provenance-v1.schema.json`](../schema/fixture-provenance-v1.schema.json)
- [`test/golden/README.md`](../test/golden/README.md)

These schemas describe the top-level contract and the fixture manifest. Shape
and connector payloads are deliberately permissive (`additionalProperties:
true`), because the editable model is still allowed to grow; the authoritative
field descriptions are below.

## Versioning policy

`js/schema.js` owns `SCHEMA_VERSION` and `normalizeDocument()`.

- Every exported/autosaved document carries `schemaVersion`.
- A document without `schemaVersion` is treated as version 0 and migrated.
- A document with a **newer** `schemaVersion` than the running app is rejected
  with an explanatory error; the open document is left untouched.
- Bump `SCHEMA_VERSION` when a persisted field changes meaning or an older
  reader cannot safely understand the document. Add a migration keyed by the
  old version. Do not reuse a version number.
- Migrations are applied in order, then `schemaVersion` is set to the current
  value. Page arrays are filled and `pageIndex` is clamped as part of
  normalisation.
- The separate `version: 1` field is the legacy document-format marker and is
  retained for compatibility; `schemaVersion` is the migration key.

Version history:

| Version | Change |
|---|---|
| 0 | Pre-versioning JSON, no `schemaVersion` field. |
| 1 | First versioned schema: Shape Data, rich text, pictures/media, groups/containers, callouts/headings/bars/icons and colour rules. Migration 0→1 only adds the marker. |

## Top-level document

```json
{
  "name": "Diagram",
  "version": 1,
  "schemaVersion": 1,
  "pageIndex": 0,
  "pages": [ ... ],
  "theme": { ... },
  "media": { "m1": { ... } }
}
```

- `name`: display name; file import overwrites it with the source basename.
- `version`: legacy format marker.
- `schemaVersion`: current document schema version.
- `pageIndex`: active page, clamped to the page list.
- `pages`: at least one page.
- `theme`: resolved theme/variation state (`ensureTheme` fills defaults on
  boot); optional in hand-written documents.
- `media`: picture records keyed by `mN`; optional when there are no pictures.
- `visioKey` and the non-enumerable package cache are session/persistence
  details, not part of the portable model.

## Page

```json
{
  "id": "p...",
  "name": "Page-1",
  "shapes": [],
  "connectors": [],
  "groups": [],
  "containers": []
}
```

`id` is runtime-only. Imported pages also carry `visio` metadata (`part`,
`pageId`, `pageH`, `ids`); it is package provenance, not part of the golden
model. `groups` and `containers` hold imported group/container records;
membership IDs are Visio IDs and are stable for a given source.

## Shape

Common fields:

```text
id, type, x, y, w, h, rot, flipX, flipY, label, group, z,
fill, fillOpacity, stroke, strokeWidth, strokeStyle,
fontSize, fontFamily, textColor, bold, italic, underline, align, vAlign,
data?, hyperlinks?, textFit?, rich?, textBox?, textBoxRel?,
parts?, image?, callout?, attachedTo?, dataColor?,
containerInfo?, containers?, listOf?, visio?
```

- `id` is runtime-only; `visio.id`/`visio.orig*` are import provenance.
  `visio.orig` snapshots are used by the patch writer and are not part of the
  canonical golden model.
- `type`: catalogue key (`rect`, `ellipse`, `custom`, `dataCallout`,
  `dataBar`, `dataIcon`, `image`, ...).
- `parts`: imported/custom sub-paths with their own fill/stroke; the writer
  flattens them when saving.
- `data`: Shape Data rows (`name`, `label`, `value`, `prompt`, `format`,
  `type`, `invisible`, ...). String values use `U='STR'` when written to VSDX.
- `image`: `mediaId`, normalised crop (`offsetX/offsetY/scaleX/scaleY`),
  brightness/contrast/gamma/transparency, `name`, `ext`, `unsupported`.
- `callout`: attached data graphic `{ kind: 'text'|'heading'|'bar'|'icon',
  field, label, position, gap, autoSize, min, max, showValue, iconSet,
  targetVisioId }`. `attachedTo` points at another shape on the same page.
- `dataColor`: target fill rule `{ field, min, max, low, mid, high, baseFill }`.
- `group`, `containers`, `listOf`, `containerInfo`: group/container membership
  and authored frame metadata; IDs are stable Visio IDs where imported.
- `rich`: paragraphs/runs, with field runs using `Prop.*` formulas and cached
  values.

## Connector

```text
id, from, to, route, waypoints, label, group,
stroke, strokeWidth, strokeStyle, arrowStart, arrowEnd,
routeStyle?, conFixedCode?, lineRouteExt?, walkPreference?, plowCode?, z, visio?
```

`from`/`to` are `{ shapeId, side, point, local }`; `shapeId` is a runtime id and
is normalised to a stable key in the golden model. `visio` carries import
provenance and the original connector/geometry state.

## Media

```json
{
  "id": "m1",
  "src": "data:image/png;base64,...",
  "type": "image/png",
  "ext": "png",
  "name": "image1.png",
  "width": 400,
  "height": 300,
  "adjust": true,
  "unsupported": false
}
```

The canonical golden model removes `src` and records `bytes` plus `sha256` of
the decoded bytes. This keeps fixtures small while still detecting a changed
picture.

## Canonical golden model

`visio-test-harness/golden-lib.mjs` produces the portable fixture model:

- object keys sorted recursively; array order preserved;
- random model ids removed and all references normalised to
  `shape:visio:<id>` / `connector:visio:<id>` (or stable local indices);
- `visio.orig*` snapshots, package caches and `visioKey` omitted; `visioId`
  retained;
- data URLs replaced by byte length and SHA-256;
- top-level `schemaVersion`, `version`, `pageIndex`, `pages`, `theme`, `media`
  retained.

The manifest stores a SHA-256 of the canonical compact JSON and a pretty-printed
copy under `test/golden/models/`.

## Golden manifest

`test/golden/manifest.json`:

- `manifestVersion: 1`: the manifest format.
- `documentSchemaVersion`: the document schema the fixtures were captured with.
- `fixtures[]`: source path, byte/hash, per-fixture document schema version,
  counts, model path/hash (`modelSha256` canonical and `modelFileSha256` file
  bytes) and the untouched-patch ZIP entry hashes.
- `untouched.entries`: SHA-256 per ZIP entry. This is the strongest check for
  the "untouched patch saves reproduce every entry" invariant; archive-level
  bytes are not compared.

Regenerate and verify with the Windows harness (it needs the app server on
port 8137 and Brave):

```powershell
cd ..\visio-test-harness
node make-golden-fixtures.mjs
node verify-golden-fixtures.mjs
```

Generation is deterministic for a fixed source set: model ids are normalised,
no timestamps are written, and manifest entries are sorted. A schema or model
change that alters a canonical hash must intentionally regenerate the fixtures
and records the change in the manifest.

On 2026-10-29 the canonical models were refreshed from the Rust importer after
the compound-master-body, stencil-detail, theme-variation and connector-arrow
fixes (`generatedBy` records the Rust baseline). The visual PNGs are still the
external Visio oracle. The JavaScript model generator is now behind and must be
synchronised before using both model producers as a cross-check.

## Public self-authored fixture set

`fixtures/public/` is the redistributable golden set: seven packages written by
LayoutCraft's own engine, their canonical models, `manifest.json` (same shape as
above, `kind: "self-authored"`) and a `provenance.json` that marks every entry
`self-authored`/`cleared`. It contains no Visio-authored drawing, stencil artwork
or Visio-rendered image; see [Public fixture replacement](39-fixture-replacement.md).
Regenerate it with `cargo run -p layoutcraft-fixtures --bin make-public-fixtures`
and check it with `layoutcraft-conformance --manifest ../fixtures/public/manifest.json`.
The private `test/golden` set described above stays an optional validation oracle
and is not distributed.

## Visual goldens

`test/golden/visual-manifest.json` optionally pins every page of those 20
sources as a Visio `Page.Export` PNG at 96 dpi. Each entry records the fixture,
page name, dimensions, bytes and SHA-256; the manifest records the renderer,
font/locale environment and the comparison thresholds. The PNGs live under
`test/golden/visuals/` and are validation-only; see the licence note in
[`test/golden/visuals/README.md`](../test/golden/visuals/README.md).

Comparison is a smoke check, not pixel parity: images are letterboxed to
640×480 on white and evaluated by pixel-delta, mismatched-pixel ratio and mean
error. The current JS renderer does not pass every page because page
backgrounds and theme fidelity are known gaps; the references target the future
Rust renderer.

Fixture provenance is separate from hashes: see
 and
`test/golden/provenance.json`.

The browser-free Rust checker is
[`rust/layoutcraft-conformance`](../rust/README.md); usage and check counts are in
[Native conformance runner](15-conformance-runner.md).
