# 44 - Document model: an in-memory document as the source of truth

Status: design proposal, 2026-11-25. No code changes accompany this note. Written against `main` at `c9aa852`.
Authority: AGENTS.md rules 5, 8, 9, 14, 22, 24, 25 shape this design (one core with two hosts, wasm-safe code, evidence for format
behaviour, no timing assertions, goldens never regenerated to pass). Nothing here makes a claim about legal status or clearance; format
behaviour that new code relies on must be recorded as evidence in the PR that adds it (AGENTS 14).

## 0. Summary

Today the document of an open drawing is a zipped `.vsdx` byte string. Every edit rewrites XML text, re-zips, re-inflates and re-imports
(section 1). The proposal replaces that with a typed, in-memory **Doc** that owns the ShapeSheet cells (formula, cached value, unit), the
shape tree, masters, styles, themes, connections and media, plus every part it does not understand as opaque bytes. Edits are typed
**commands** that run in a **transaction**; a transaction records a **journal** of keyed copy-on-write replacements, which is undo, redo,
the render invalidation set, the save diff and the recalculation input at the same time. Save generates the package by **splicing** onto the
opened package (the existing patch writer, driven by a diff), so untouched parts stay byte-identical, and a clean document saves as the
original bytes.

| Question | Decision |
|---|---|
| Source of truth | `Doc` in a new crate `layoutcraft-doc`; the zip bytes are only the *base* (what was opened) and an *output* (what save makes) |
| Cells | `Cell { formula, value (cached), unit, flags }` in generic sections/rows; nothing hard-coded to a subset, so unknown cells survive |
| Unknown XML | unknown parts are opaque lazy blobs; unknown attributes and elements inside a modelled part are kept as positioned raw text; clean shapes are copied from their source byte span |
| Undo | keyed copy-on-write journal (section 3.3): memory = old versions of the shapes an edit touched, not the file |
| Gestures | one open transaction, before-images kept from the first touch, absolute (not incremental) updates; commit or cancel |
| Recalc | `Doc` owns the dependency graph of `layoutcraft-formula`; recalculation runs inside the transaction and its result is journaled, so undo restores cached values without recalculating |
| Render | the model crate (`layoutcraft-model`) stays as the flattened **view** (golden JSON, scene input), rebuilt per dirty node from a `ChangeSet` |
| Save | diff of `Doc` against the opened state -> existing patch writer; clean doc = source bytes verbatim; fresh save later from a `Doc` serializer |
| Hosts | `Engine` keeps every current method and op code; the 49 MCP tools and the wasm `vl_*` exports are untouched; internals stop leaking |
| Migration | 17 steps (PRs 0-14, some split; section 6); PR 2 alone removes the undo-memory problem, PR 7 the per-edit cost |

Measured today (section 1.4, a generated 3,000-shape drawing, release build): one move = **1.5 s**, **0.95 GB allocated**, 100 undo steps
= **10.7 MB**; with 20 MB of pictures: **1.6 s**, **1.1 GB** per edit and **2.0 GB** of undo stack after 100 edits. Targets are in 7.

## 1. The system today

### 1.1 What the document is

`Engine` (`rust/layoutcraft-engine/src/lib.rs:32-54`) holds five representations of one drawing:

| Field | Role |
|---|---|
| `bytes: Vec<u8>` | the zipped package: **the document**; `export()` returns it (`lib.rs:121`) |
| `package: Option<Package>` | the same bytes inflated into `BTreeMap<String, Vec<u8>>` plus an `Arc<Vec<u8>>` copy of the zip as `source` (`vsdx/src/lib.rs:349-354`) |
| `document: Option<Document>` | the *flattened* render model (`layoutcraft-model`), derived by import |
| `import_state` | caches that make the next import incremental (`vsdx/src/import.rs:29`) |
| `history: Vec<Vec<u8>>`, `redo: Vec<(u64, Vec<u8>)>` | undo and redo: **a full copy of the zip per step** (`lib.rs:40-43`) |

The model is not a candidate for the source of truth. It is a one-way projection: pixels at 96 dpi with y down, colours already resolved
to CSS strings, master bodies flattened into SVG path strings (`ShapePart.d`), formulas gone (`layoutcraft-model/src/lib.rs:279-427`,
`467-504`). Several fields are `skip_serializing` render-only caches (e.g. `layer_color`, `paragraph_spacing`, `front_parts`), so the JSON is
not even lossless for the renderer. Edits cannot be expressed on it and turned back into cells.

The structured thing that *does* exist is transient: `ShapeNode` / `ResolvedShape` / `MasterShape` with `Cell { value, formula, unit }`,
`Section`, `SectionRow` (`vsdx/src/shapes.rs:30-55`, `56-75`, `92-109`, `128-168`). `ResolvedShape` already carries inherited master cells,
own sections vs master sections, master children and `master_top_user` (`shapes.rs:150-166`). It is rebuilt from XML text on every edit
(`GroupEditor::open`, `groupedit.rs:797`) and thrown away. The proposal promotes this shape of data to the long-lived store.

### 1.2 What one edit does (`Engine::edit`, `lib.rs:291-532`, a move is op 2)

1. `state.open_editor(...)` -> `GroupEditor` parses the page part with roxmltree into a `ResolvedShape` tree (`lib.rs:293-298`).
2. The op builds `ShapeCellPatch`es (`lib.rs:496`, `vsdx/src/writer.rs:11-51`).
3. `patch_page` splices the page XML *as text* and writes a new zip (`writer.rs:545-634`, `write_package_edit` `782-812`); untouched parts are
   `raw_copy_file`d from the source zip (`writer.rs:796`), so they are **not recompressed** (the premise "recompress 20 MB per nudge" is already
   fixed for parts that did not change; the page part is deflated again).
4. `Package::from_bytes(&bytes)` copies the zip into an `Arc` and inflates every entry (`lib.rs:507`, `vsdx/src/lib.rs:371-408`).
5. `moved.connects(&part)` parses the page again (`lib.rs:509`); `reroute_connectors` opens a second `GroupEditor` (a third parse) and scans
   `connects.iter().find(..)` per connector end, i.e. O(connectors x connects) (`groupedit.rs:1185-1215`); if anything moved, patch + zip +
   `from_bytes` all run again (`lib.rs:520-527`).
6. `adopt_package` -> `import_document_with` parses the page a fourth time, resolves master bodies, flattens, builds the model
   (`import.rs:118-329`). Only pages whose parts changed are redone (`page_unchanged`), and top-level nodes are reused only when the page
   has at most `MAX_CACHED_NODES = 400` of them (`import.rs:40`, `241`); reuse compares whole `ResolvedShape`s deeply (`import.rs:245`) and
   clones the cached `model::Shape`s (`import.rs:246`).
7. `self.history.push(previous)` keeps the whole previous zip (`lib.rs:529-530`).

Smaller costs on the same path: `preview_move` (called per mouse move during a drag, `wasm vl_preview_move` `lib.rs:281`) hashes **all
document bytes** on every call (`engine lib.rs:538`, `fingerprint` `57-63`); `redo`, `redo_depth` and `can_redo` hash them too
(`lib.rs:173,189,1258`); `chain()` finds a shape by walking the whole tree (`groupedit.rs:1315`); `find_shape_range` is a text search over
the whole page for every patched shape (`writer.rs:1132`), so a multi-select edit is O(selected x page size).

### 1.3 How hosts use it

* **App** (`layoutcraft-app/src/main.rs`, contains NUL bytes, read with `grep -a`): owns `engine: Engine` (`:920`); reads the public fields
  `engine.package` (`:1242, 2277, 2344, 2458`) and `engine.document` (11 sites); **bypasses the engine's edit API** in `commit_rect`, where it
  builds a `ShapeCellPatch`, calls `patch_page_cells` itself and `engine.commit(bytes)` (`:2344-2355`); keeps `baseline: Option<Document>`, a
  full clone of the model taken at open (`:1198`), and decides "modified" with `has_changes()`, an O(shapes^2) field-by-field compare after every
  edit (`:2396`). It calls `engine.edit` with op codes at ~20 sites, `undo/redo/undo_depth/collapse_history`-style methods, `export()` for save
  and `json()` for reports.
* **wasm** (`layoutcraft-wasm/src/lib.rs`): `State { engine, out }` with `Deref` to `Engine` (`:12-30`), 57 `extern "C" fn vl_*` (the brief says
  88; I count 57 in that file and 57 distinct `vl_` names used by `web/`; the compatibility rule below is the same either way). `vl_edit`
  (`:440`) forwards the op code; `vl_export` (`:898`) copies `export()` to the output buffer.
* **MCP** (`layoutcraft-mcp/src/lib.rs`): 49 tools (`tests/oracle/tools-list.json`, checked in `tools_list_is_the_one_of_the_node_server`, `:755`)
  implemented with `engine.edit(p, <op>, ...)`, `page_op`, `group_op`, `draw`, `add_connector`, `undo`/`redo`, `export`.
* **Everything else that reads the drawing** (check, compare, inventory, drawio / excalidraw / dxf / pdf / webpage export, `lib.rs:1264-1290`)
  consumes `engine.json()` or `engine.document()`.

### 1.4 Measured cost of one small edit

Method: a throw-away example (not committed) built with `cargo build --release --offline -p layoutcraft-engine`, run on Windows 11, one machine,
one run, not CPU-pinned. It counts allocation with a wrapping `#[global_allocator]` (cumulative bytes requested per edit = "allocated"), times
`Engine::edit(page, 2, id, 0.1, ...)` 100 times (alternating +/-0.1 in) on a shape that has a glued connector, then reads `engine.history`.
Phase times are one call of the same steps `Engine::edit` performs, made with the public vsdx API. "Undo stack" is the sum of
`history[i].len()`. The generator (`write_fresh_document`: N rectangles, N/2 glued elbow connectors, optional random-pixel 1500x1500 PNGs
inserted with `Engine::insert_picture`) was kept outside the repository (scratch files are not committed, AGENTS 28); PR 0 proposes checking in a cleaned-up version.

Caveat that matters: this worktree has no private corpus (`test/`, `examples/` are absent), and the **largest public fixture is 9.6 KB**
(`fixtures/public/sources/public-pages.vsdx`). Generated drawings are the stand-in; a maintainer should take the same numbers on the
private 20-35 MB drawings with the PR 0 harness before the targets in section 7 are frozen.

| Scenario | zip | page XML | edit median / p95 | allocated per edit | undo stack after 100 edits | one undo |
|---|---|---|---|---|---|---|
| public fixture `public-pages` (1 shape) | 9.6 KB | < 5 KB | 2.6 ms / 3.9 ms | 3.0 MB | 0.92 MB | 1.1 ms |
| generated 300 shapes + 150 connectors | 17 KB | 0.50 MB | 108 ms / 124 ms | 97 MB | 1.67 MB | 48 ms |
| generated 3,000 shapes + 1,500 connectors | 0.11 MB | 5.0 MB | **1,479 ms** / 1,561 ms (max 5,542, first edit) | **954 MB** | 10.7 MB | 551 ms |
| same + 3 pictures (20.5 MB zip) | 20.5 MB | 5.0 MB | **1,579 ms** / 1,674 ms | **1,097 MB** | **2,048 MB** (peak heap 2.8 GB) | 569 ms |
| 300 shapes + 150 connectors + 3 pictures (20.4 MB zip, small page) | 20.4 MB | 0.50 MB | 163 ms / 177 ms | 240 MB | **2,039 MB** (peak heap 2.3 GB) | 81 ms (mean of 10) |

Phases of one move, ms (editor open, `move_member`, `patch_page` incl. zip, `Package::from_bytes`, reroute incl. connects parse + second editor,
incremental import, history clone):

| Scenario | editor | move | patch+zip | from_bytes | reroute | import | history clone |
|---|---|---|---|---|---|---|---|
| 300 shapes (no media) | 14.0 | 0.0 | 1.1 | 0.7 | 31.3 | 45.8 | 0.0 |
| 3,000 shapes | 196.6 | 0.1 | 11.0 | 6.2 | 635.4 | 554.7 | 0.1 |
| 3,000 shapes + 20 MB pictures | 205.1 | 0.1 | 16.9 | 19.4 | 668.4 | 599.2 | 4.6 |
| 300 shapes + 20 MB pictures | 14.8 | 0.0 | 7.3 | 14.6 | 34.7 | 48.5 | 8.1 |

Other numbers taken: scene + SVG of the 3,000-shape page 68 ms (1.6 MB SVG; 262 ms / 28.8 MB with the pictures embedded); `open` of the
3,000-shape drawing 567 ms; `fingerprint` of 20.4 MB = 4.4 ms, which is what `preview_move` pays on every call (a later call measured 5.6 ms
including that; the first, which builds the editor, 25.5 ms).

What the numbers say:

1. **Undo memory is O(file x steps)**: 100 steps of the 20 MB file hold 2.0 GB (exactly 100 x the zip), the peak heap 2.3-2.8 GB. This is
   the problem that is independent of what the user does and it is fixable cheaply (PR 2).
2. **Edit latency is O(page XML), not O(file)**: the 300-shape page costs 108 ms without and 163 ms with 20 MB of pictures (the
   media adds ~55 ms of whole-package copies: `bytes.clone()` for history, the new zip, `to_vec` into `source`, inflating 20.9 MB), but 3,000 shapes
   cost 1.5 s with a 0.1 MB zip. The page part is parsed with roxmltree at least five times per move (editor, `connects`, reroute editor,
   import `page_shapes`, import `connects`), and reroute plus import are ~80 % of the time.
3. Past 400 top-level nodes the node cache is switched off (`import.rs:241`) and the whole page is flattened again per edit.
4. The `allocated` column is ~190x the page XML for the 3,000-shape page: it is mostly parse trees and resolved-shape clones, not the file.
5. A fixed ~3 MB per edit even for a 9.6 KB file (compressor state and parse buffers are the likely sources; I did not profile it).

## 2. Goals and constraints

Goals: edit latency independent of file size and (for a local edit) of page size; undo memory proportional to what changed; a formula engine
that can own recalculation; untouched bytes of the opened file preserved; the same host API.

Constraints taken from the repository: engine, vsdx, render and model compile to wasm and must not use `Instant` or timing code (AGENTS 9),
so time-based gesture coalescing is the host's job; dependencies must be in `rust/Cargo.lock` (AGENTS "built offline"); tests assert relations
not timings (AGENTS 22); goldens are not regenerated to pass (AGENTS 25); Visio-made drawings stay private (AGENTS 17), so every guard below
has a public-fixture form that runs in CI and an optional private-corpus form behind `cfg(private_fixtures)`.

Non-goals: a new renderer (the `perf/direct-raster` branch is separate), a ShapeSheet UI, multi-user editing, a Visio-complete formula
language (that is `layoutcraft-formula`).

Dependencies available for the new crate (checked in `rust/Cargo.lock`): `slab`, `slotmap`, `smallvec`, `indexmap`, `rustc-hash`, `hashbrown`,
`fixedbitset`, `arrayvec`, `petgraph`, `foldhash`. **Not present**: `im`, `rpds`, `imbl` (no persistent collections); `Arc` and `Arc::make_mut`
from std are enough for the design.

## 3. Design

### 3.1 Crates

```
layoutcraft-formula   (parallel branch: units, references, dependency graph, incremental recalc)
layoutcraft-model     (kept: flattened view schema v1; goldens)
layoutcraft-doc       (NEW: Doc, parts, Shape/Cell, Command, Transaction, History, Recalc glue, Indexes, ChangeSet)  -> formula
layoutcraft-vsdx      (+ loader Package->Doc, emitter Doc->Package, view flattening; legacy patch writer stays)   -> doc, model
layoutcraft-render    (unchanged: model -> scene -> svg/png)
layoutcraft-engine    (Engine facade: op-code adapters, clipboard, stencils, exports)                          -> all
```

`layoutcraft-doc` has no zip and no XML dependency (it knows part *names* and bytes), so it is unit-testable and wasm-safe. The existing
vsdx crate keeps the format knowledge (parsing, XML text patching) and gains `load` and `emit` modules.

### 3.2 Data model

Sketch (names are proposals; fields are the contract that matters):

```rust
pub struct Doc {
    parts:   PartStore,                         // every package part, see "Parts"
    pages:   Vec<PageSlot>,                     // order = pages.xml order
    masters: BTreeMap<MasterId, Arc<Master>>,
    styles:  BTreeMap<StyleId, Arc<StyleSheet>>,// StyleSheets of document.xml
    themes:  Vec<Arc<ThemePart>>,
    docsheet: Arc<Sheet>,                       // DocumentSheet cells
    media:   BTreeMap<MediaId, Arc<MediaBlob>>,
    idx:     Indexes,                           // derived, section "Lookup structures"
    recalc:  Recalc,                            // owns the formula graph, section 3.4
    base:    Arc<Base>,                         // the opened state (source bytes + Arc handles) for the save diff
    history: History,                           // section 3.3
    epoch:   u64,                               // +1 per committed transaction (replaces fingerprint())
}
pub struct Page {                               // behind Arc, copied on first write
    id: PageId, name: Arc<str>, part: PartName, sheet: Arc<Sheet>,           // PageSheet cells (size, scale, layers, theme index...)
    roots: Arc<[ShapeId]>,                      // top-level shapes in z order
    shapes: FxHashMap<ShapeId, Arc<Shape>>,
    connects: Arc<[Connect]>,                   // the <Connect> rows
    back_page: Option<PageId>, background: bool, guides: Arc<[Guide]>, comments: ...
}
pub struct Shape {                              // ~ShapeNode + inheritance links, immutable once in an Arc
    id: ShapeId, parent: Option<ShapeId>, children: Arc<[ShapeId]>, kind: ShapeKind,   // Type= Shape|Group|Guide|Foreign
    master: Option<MasterRef>,                  // <Shape Master= MasterShape=>
    styles: StyleRefs,                          // FillStyle / LineStyle / TextStyle ids
    cells: CellMap,                             // direct cells, sorted small vector keyed by CellName
    sections: Vec<Arc<Section>>,                // Geometry n, Character, Paragraph, Property, User, Prop, Connection, Control, Action, Layer, ...
    text: Option<Arc<TextBlock>>,               // tokens as in shapes.rs:14: Text / cp / pp / tp / fld
    foreign: Option<Arc<Foreign>>, names: Names, extra: Extra, src: Option<SrcSpan>,
}
pub struct Cell { formula: Option<FormulaId>, value: CellValue, unit: Option<Unit>, flags: CellFlags }
```

**Cells.** `value` is the cached value in file-internal units (inches, radians, seconds: the importer already reads `V`, not the display unit
`U`, `docs/03-architecture.md` "Coordinate systems"); `unit` keeps `U` (a closed enum plus `Other(Arc<str>)` so `ED/EH/EM` durations
survive, see `DataEdit` `writer.rs:55-70`); `formula` is an index into a document-wide interner (hundreds of thousands of `Width*0.5` strings
become one); `flags` hold `inherits_formula` (`F='Inh'`), `stale` (cached value could not be recomputed), `error`. A cell is ~32 bytes; a
shape's own cells are a sorted small vector, not a `BTreeMap<String, _>` as in `ShapeNode`. Section/row/cell are generic: `Section { name,
index, cells, rows: Vec<Row{ix, row_type, name, cells, deleted}> }` exactly as `shapes.rs:30-47`, so a cell that no code knows about is still
a cell. Typed accessors (`pin_x`, `width`, `angle`, `geometry()`, `data_rows()`) are a thin layer on top.

**Masters and inheritance.** `Master { id, name, part, page_ratio, top, shapes: tree of Shape (master-scoped ids) }` is shared by `Arc`; an
instance holds only its own cells plus `MasterRef { master, shape }`. Effective cell lookup is a chain, resolved on demand:
`own cell -> master shape cell -> style chain (the cell group's FillStyle/LineStyle/TextStyle sheet, then its basedOn parents) -> default`
(`ResolvedShape` computes the same merge eagerly today, `shapes.rs:247`). Instances do **not** copy master cells, so 10,000 instances of one
master cost their own cells only. The master's *nested body shapes* are virtual: they are not stored on the page (Visio does not store them
either, `ResolvedShape.master_children`, `shapes.rs:157`); the view derives them per instance (section 3.5), which is what `NodeCache`
(`import.rs:54`) does today.

Inherited cells are **copy-on-write**. When recalculation changes the cached value of a cell that the instance inherits (a resize changes
`Width*0.5` of an instance of a master), the instance gets an own cell `{formula: inherited text, value: new}`. This is exactly what the
writer does by hand with `RowOverride` ("Visio does not recalculate the master's formulas for an instance when it loads a file: a resized
instance must carry the recalculated cells itself", `writer.rs:73-80`, `refresh_formula_values` `2032`) and what the save diff then emits.

**Styles and themes.** `StyleSheet { id, name, fill/line/text parent ids, cells, sections }` is `styles.rs:22-31` made mutable; the
document default style ids stay on `Doc`. A theme is `Arc<ThemePart>` (colour scheme, variants, `fmtScheme` styles, `styles.rs:423-`) plus
its original bytes in the part store (theme edits `pageops.rs:975-1177` replace whole theme parts today). Shapes whose formulas contain
`THEMEVAL` are flagged at load (`uses_theme`) so a theme change dirties them without scanning every cell.

**Connections and glue.** Two things are stored in the file and both are modelled: the page's `<Connect FromSheet FromCell ToSheet ToCell>` rows
(`Connect`, `vsdx/src/lib.rs:272`) as `Page.connects`, and the connector's own end cells (`BeginX/BeginY/EndX/EndY` and their glue formulas
such as `PAR(PNT(Sheet.5!Connections.X1,...))`) as ordinary cells. The glue formulas are what the formula engine recalculates; the **route**
(the connector's Geometry) is not a formula in practice, it is produced by Visio's router, which `router.rs` / `reroute_connectors`
imitate. So rerouting is a **rule that runs after recalculation** on exactly the connectors whose end cells changed (section 3.4), writing
cached values (today's `cached_cells`, `writer.rs:34`).

**Containers and lists, layers, groups.** Containers/lists are the `Relationships` cell graph; the doc keeps the cells and a derived
`container_members` index (today `page.containers`, `model Page.containers`). Layers are the PageSheet `Layer` section plus each shape's
`LayerMember` cell (`pageops.rs:467-620`); layer visibility/colour are *derived* for the view (`apply_layer_visibility`, `apply_layer_colors`,
`import.rs`), not stored. Groups are just `kind = Group` shapes with children; child cells are in the group's local frame, so moving a group
changes **one** shape's cells (its pin) and zero descendants - the view recomputes world coordinates.

**Media.** `MediaBlob { part, content_type, sha256, bytes: LazyBytes }` behind `Arc`: a picture used on five pages is one blob. Page
relationships (`rId -> media part`) are kept in the page's rels part; ForeignData references them. The renderer-ready form
(`media_bytes`: BMP->PNG, EMF/WMF->SVG, `vsdx/src/media.rs:113`) is cached in the view layer keyed by `sha256`.

**IDs.** `PageId` = Visio `Page ID`, `ShapeId` = Visio shape ID (page-scoped `u32`), `MasterId`, `StyleId`, `MediaId` (the importer's `mN`).
Public handles are `(PageId, ShapeId)`. Host strings (`"12"`, newline-separated lists) are parsed once at the `Engine` boundary. New ids
come from the same rule as `next_shape_id` including the sub-shape gap for group masters (`writer.rs:1893-1925`); this is pinned by a
differential test (section 6, PR 7). Page *indices* are positions in `Doc.pages`; ops that today take `page: usize` keep doing so at the
boundary.

**Lookup structures (derived, never journaled).** `Indexes` is rebuilt from nothing at load and maintained by the single choke point
`Doc::replace_slot` (every journal apply/revert goes through it): `glue_in/out: ShapeId -> [connector ids]` (from `Connect` rows and end cells),
`parent chain` (via `Shape.parent`, replacing `chain()`'s O(N) walk), `names` (unique shape names per page, `shapename.rs`), `container_members`,
`layer_members`, `style_users`, `master_users`, `shape_cells -> graph keys`. Invariant tested by property test: indexes after any command
sequence equal indexes rebuilt from scratch.

**Parts and lossless preservation.**

* `PartStore: BTreeMap<PartName, Part>` with `Part::Source { entry, len, crc, inflated: OnceLock<Arc<[u8]>> }` (not inflated until read),
  `Part::Bytes(Arc<[u8]>)` (changed or generated). Today every entry is inflated at open and the zip is also kept (`Package::from_zip_bytes`,
  `vsdx/src/lib.rs:377-408`): a 12 MB EMF is held twice for no reason.
* **Opaque parts** (docProps, custom XML, `vbaProject.bin`, thumbnails, windows.xml, EMF/PNG media, anything unknown): never parsed;
  on save `raw_copy_file` of the source entry, which keeps even the compressed bytes. Content types and relationships are handled textually,
  as the writer does today.
* **Modelled XML parts** (pages, masters, document.xml, themes): parsed into the model *and* the source text is retained with per-node spans:
  `SrcSpan { part, own: Range<u32>, tail: Range<u32> }` per shape (`own` = from `<Shape` to before `<Shapes`, `tail` = after `</Shapes>`).
  Anything the parser does not model is kept as `Extra { attrs: Vec<(name, value)>, elements: Vec<(sibling index, raw text)> }` so a
  re-serialized shape puts it back in place (unknown shape attributes, unknown child elements such as `Data1`, `RefBy`, vendor extensions,
  cell attributes like `E`).
* **Clean shapes are never serialized.** The emitter copies their source span (section 3.6). `Extra` only matters when a shape that came from the
  file is changed structurally.

### 3.3 Edits and undo

**Commands.** A command is a typed value; the engine's op codes are an adapter onto them (table below). A command runs against a
`Transaction`, which is the only way to mutate a `Doc`:

```rust
pub enum Command { Move{ids, delta, grow_frames}, Resize{..}, Rotate{..}, Flip{..}, Align{..}, Distribute{..}, Delete{ids},
                   SetText{..}, Format{ids, style}, FormatTextRange{..}, SetDataRow{..}, RemoveDataRow{..}, SetLayerMembership{..},
                   AddShape{..}, Paste{..}, Group{..}, Ungroup{..}, Reorder{..}, AddConnector{..}, MoveConnectorEnd{..}, ...,
                   SetCells{target, cells},                    // the typed form of what app commit_rect builds by hand
                   Legacy(LegacyOp) }                          // bridge, section 6 PR 7
impl Doc { fn apply(&mut self, cmd: Command) -> Result<Outcome, EditError>;       // begin, run, recalc, rules, commit | rollback }
```

`Transaction` provides `update_shape(page, id, |shape| ..)` (clone-on-write of that one shape, before-image recorded on first touch),
`insert_shape`, `remove_shape`, `set_roots`, `set_page_sheet`, `replace_style`, `replace_part`, ... Every method boils down to
`Doc::replace_slot(slot, new)`, which swaps one `Arc`, updates `Indexes`, tells `Recalc` (`on_replace(old, new)`) and appends
`Change { slot, old, new }` to the journal.

**Journal = undo = redo = diff.** `Change` is `{ slot: Slot, old: Option<Payload>, new: Option<Payload> }` where `Slot` is `Shape(page,id) |
PageRoots(page) | PageSheet(page) | PageMeta(page) | Connects(page) | Master(id) | Style(id) | Theme(ix) | DocSheet | Media(id) | PageList
| Part(name)` and `Payload` an enum of the `Arc`s. Undo applies the changes in reverse, swapping `old` back; redo applies forward. There is no
per-command inverse to write and no recalculation on undo (the cached values are in the restored shapes; the graph is repaired by
`on_replace`, which only touches cells whose formula `FormulaId` differs). A failed command or a failed recalculation reverts its own journal:
the document is exactly as before, as `Engine::edit` guarantees today by erroring before it commits.

| Option | Memory per step | Undo cost | Risk / complexity | Verdict |
|---|---|---|---|---|
| A. zip snapshots (today) | the whole file | re-inflate + re-import (48-569 ms measured) | none, and wrong at scale | replace |
| B. part-level snapshots (`Arc` parts, changed parts only) | changed page XML (0.5-5 MB) | re-parse the page | low; no model needed | **interim step, PR 2** |
| C. per-command inverse operations | O(change), smallest | O(change) | every command needs a hand-written, tested inverse, and recalc effects need a journal anyway; an inverse bug corrupts silently | rejected |
| D. whole-`Doc` persistent snapshots (chunked persistent vector + persistent map) | O(change x log N) + N/64 pointers per step | O(1) swap | needs a hand-rolled persistent collection (no `im`/`rpds` in the lock); id->shape lookup is awkward | rejected as primary |
| E. keyed copy-on-write journal (chosen) | old versions of touched shapes only: ~1-4 KB each | O(touched) | one primitive (swap an `Arc` in a slot), inverse is trivially correct; shape heap must stay shallow-cloneable | **chosen** |

Shape cloning must be shallow for E to be cheap: sections are `Arc<Section>`, text and foreign data are `Arc`s, children is `Arc<[ShapeId]>`; a
`PinX` change clones the shape header and its small cell vector and shares the 200-row Geometry section. `Page.roots` is one `Arc<[ShapeId]>`
replaced as a unit when shapes are added, deleted or reordered at the top level (12 KB for 3,000 shapes; chunk it if a 100k-shape page makes
that visible, which is a local change).

**Bounds.** `History { undo: VecDeque<Step>, redo: Vec<Step>, bytes: usize }`, `Step { label, coalesce_key, changes: Vec<Change>, approx_bytes }`.
Defaults: at most 1,000 steps and 64 MiB of `approx_bytes` (sum of the old payloads' heap estimates), oldest evicted first; both configurable
by the host. A select-all-and-move of 3,000 shapes is one step of ~9 MB, so the byte cap, not the step cap, is what protects memory. Eviction
never affects save: the save diff uses `base`, not the history. `redo` is cleared by any new commit; the `fingerprint` staleness check
(`lib.rs:41-43`, `173-181`) disappears because there is no other way to change the document.

**Gestures and coalescing.**

* `begin_gesture(kind) -> Gesture`: opens a transaction that stays open. `gesture.update(params)` is **absolute**: it restores the first-touch
  before-images of the touched shapes and applies `delta_total` again (no drift, no accumulation); recalculation and the reroute rule run per
  update on the dependents only. `end(commit)` pushes one `Step` or restores everything. The 60 fps budget is a target for the dependents-only
  path (section 7); the current `preview_move` (editor cached by fingerprint, `lib.rs:537-550`) becomes a thin wrapper over a throw-away gesture
  so the wasm export keeps working.
* Because the engine must not read the clock (AGENTS 9), "type for a second = one undo step" stays a host policy: the host passes
  `coalesce_key: Option<u64>` and the engine merges a new step into the previous one when keys are equal and no other step intervened.
  `collapse_history(depth)` (`lib.rs:202`, today "truncate the stack so the last k steps become the first") becomes `merge_since(depth)`:
  merge steps keeping, per slot, the *oldest* `old` and the *newest* `new`. Same observable behaviour, same signature.
* Undo of a coalesced step restores the first before-image of each slot, which is the invariant tested below.

**How the current op codes map.** All current codes are kept in the adapter (`Engine::edit` `lib.rs:291`, `page_op` `557`, `group_op` `785`); the
typed API is additive.

| Current | Command | Notes |
|---|---|---|
| `edit` 2 / 5 | `Move { ids, delta, grow_frames }` | `flag` = grow ancestor frames |
| `edit` 1 / 3 | `ResizeGroup`, `ResizeMember` | master instances carry recalculated cells (copy-on-write, 3.2) |
| `edit` 4 / 6 | `Delete { ids }` | glue rows removed, connector ends keep position (`detach_patches`, `groupedit.rs:1864`) |
| `edit` 7 / 11 / 12 | `RotateTo`, `Rotate`, `Flip` | |
| `edit` 14 / 15 | `Align`, `Distribute` | |
| `edit` 8 / 13 | `SetText`, `FormatTextRange` | text tokens and Character/Paragraph rows are model data |
| `edit` 10 | `Format { ids, StyleEdit }` | JSON parsed at the boundary (`lib.rs:333-395`); font names register in the face list |
| `edit` 16 / 17 / 18 | `SetLayerMembership`, `SetDataRow`, `RemoveDataRow` | |
| `draw`, `draw_curve/arc/polyline`, `insert_picture` | `AddShape { geometry }`, `AddPicture` | shape builders move from XML strings (`draw.rs`, `writer.rs:865`) to `Shape` constructors |
| `copy` / `paste` | `Paste` | clipboard becomes `Vec<Arc<Shape>>` subtree snapshots + the masters they need, not XML strings (`lib.rs:44-50`) |
| `add_connector`, `move_connector_end` | `AddConnector`, `MoveConnectorEnd` | glue rows + end cells + reroute rule |
| `group_op` 0-5 | `Group`, `Ungroup`, `Reorder` | |
| `group_op` 6-12 | `ShapeOperation` | `groupedit/shapeops.rs` geometry becomes `Shape` output |
| `page_op` 1-5 | `AddPage`, `DeletePage`, `RenamePage`, `MovePage`, `DuplicatePage` | `DuplicatePage` shares `Arc` shapes |
| `page_op` 6, 15, 23 | `SetPageSize`, `SetDrawingScale`, `SetPrintSetup` | PageSheet cells |
| `page_op` 7-10 | `AddLayer`, `SetLayer`, `DeleteLayer`, `SetLayerColor` | |
| `page_op` 11 | `SetDocProps` | docProps part (opaque-edited) |
| `page_op` 12-14 | `SetBackPage`, `MakeBackground`, `MakeForeground` | |
| `page_op` 16-18 | `AddGuide`, `MoveGuide`, `DeleteGuide` | |
| `page_op` 19, 39, 40 | `SetHyperlinks`, `SetAltText`, `SetShapeName` | |
| `page_op` 20-22 | `AddComment`, `SetCommentText`, `DeleteComment` | comments part |
| `page_op` 24-26 | `Add/Move/DeleteConnectionPoint` | later points renumber, glue follows |
| `page_op` 27-29, 34, 35 | `MoveVertex`, `InsertVertex`, `DeleteVertex`, `Bulge`, `FitCurve` | then reroute rule |
| `page_op` 30-33 | `ApplyTheme`, `SetThemeVariant` (document / page) | theme parts |
| `page_op` 36-38 | `ApplyStyle`, `SetStyle`, `DeleteStyle` | `styleedit.rs` |
| `stencil_drop`, `add_master`, `replace_shape` | `AddMasterInstance`, `ReplaceShape` | masters imported from a stencil `Package` (stencils stay byte packages, read-only) |
| `undo`, `redo`, `redo_depth`, `undo_depth`, `can_redo`, `collapse_history` | `Doc::undo/redo`, `History::{depth, merge_since}` | |

### 3.4 Recalculation

**Ownership.** `Doc.recalc: Recalc` wraps `layoutcraft_formula`'s graph and is the only writer of computed cached values. The formula crate is
being written on another branch (the branch `feat/formula-engine` currently equals `main`, so there is no API to read); this section therefore
states what `Doc` needs from it as a narrow contract, to be reconciled in review:

1. Generic over an opaque `CellKey: Copy + Eq + Hash + Ord`. Doc supplies `CellKey { owner: Owner, path: CellPath }` (16 bytes: scope
   page/master/style/doc + shape id + interned section/row/cell), so the graph needs no knowledge of Visio structure.
2. A `Resolver` trait implemented by `Doc` that turns a reference (`Sheet.5!PinX`, `Geometry1.X2`, `Prop.Cost`, `User.Row_1`, `ThePage!...`,
   `Controls.Row_1.X`) written in a given cell's scope into `CellKey`s, and answers unresolved ones as `#REF!`.
3. `Value { Num(f64, Unit), Str, Bool, Err }` with unit conversion on literals (`1 cm`), matching `Cell.value/unit`.
4. `set_formula(key, text)`, `clear(key)`, `mark_dirty(key)`, `recalc(&mut dyn CellStore) -> Vec<(CellKey, Value)>` returning changed cells in
   dependency order; cycles produce an error value for the cycle members and do not loop; functions flagged volatile are recomputed only when a
   dependency changed.
5. Callbacks for things only the host knows: `text_extent(shape, ...)` for `TEXTWIDTH/TEXTHEIGHT` (font metrics live in `layoutcraft-render`,
   so the formula crate must not depend on it), `theme_val(...)`.
6. A "verify" mode: evaluate every formula and report cells whose result differs from the cached `V` (used by the oracle run that compares
   against Visio, `vsdx/src/formula.rs:1-5`).

**What gets a graph node.** Only cells with their own formula, plus inherited formulas *lazily*: a master's formulas are compiled once into an
`Arc<MasterPlan>` (topological order of the formulas that depend on `Width/Height/Angle/flip` and on `User.*` of the top shape); an instance
is re-evaluated through the plan when one of those cells changes, and results are written copy-on-write (3.2). This avoids 10,000 x 40
graph nodes for 10,000 instances and replaces `masterbody.rs`'s ad-hoc evaluation (`masterbody.rs:1149,1563,1582,1663,1878`) and the
import-time evaluator call sites (`import.rs:1374,1388,4187,4219,4264,4303`; `formula.rs` itself has 580 lines of numeric-only evaluator).

**When it runs.**

* **Open: not at all.** The file's cached values are trusted; this keeps load fast and keeps the bytes that Visio wrote. A cell without `V` is
  computed on first read.
* **Edit:** command sets input cells -> `recalc` on the dirty set -> results written through the transaction (so they are in the journal) ->
  **rules** run (connector reroute for connectors whose end cells changed; container growth; text-fit) and may dirty more cells -> repeat
  until quiet (bounded at 4 rounds; more is an error and rolls back). Cost is proportional to the transitive dependents of the edited cells.
  A per-transaction evaluation budget (default 100,000 cells) rolls the transaction back instead of hanging.
* **Undo/redo:** no recalculation.
* **Unsupported function:** the cell keeps its cached value, gets `stale`, and the host can see it (AGENTS 7: do not present partial behaviour
  as success). A cached value is never replaced by an error unless an input actually changed.

**Replaces today's special cases:** `refresh_formula_values` (text-level `Width*k` fix-up, `writer.rs:2032`), the reroute-everything pass
(`reroute_connectors`, `groupedit.rs:1185`, O(connectors x connects) = 635 ms of the 3,000-shape move) which becomes "reroute the dependents
of changed glue cells", and import-time evaluation.

### 3.5 View, cached values to the renderer, invalidation

`layoutcraft-model::Document` stays the schema-v1 *view*: golden models, `engine.json()` consumers, the scene input and the drawio / DXF / PDF
exporters keep working unchanged. What changes is who builds it and how often.

* `ViewCache` per page: `Vec<ViewNode { shape: ShapeId, inputs: InputKey, z_start, z_end, out: Arc<[model::Shape]> }>` where `InputKey` is
  `(Arc ptr of Shape, Arc ptr of Master, style-chain epoch, theme ptr, page height, page theme/variation)`. A node is reused when its key is
  pointer-equal; this is `NodeCache` (`import.rs:54-66`) with pointer equality instead of deep `ResolvedShape ==` and **without the 400-node limit**
  (`import.rs:40`) because a node is two pointers and a slice. The flatten functions (`flatten_node`, `import.rs:952`) are moved, not rewritten:
  PR 6 gives them a `Doc`-backed `ResolvedShape` adapter first.
* The renderer only ever reads cached values (the view reads `Cell.value`; it never evaluates a formula), so "cached values flow to the
  renderer" means: transaction commits -> `ChangeSet` -> view nodes whose `InputKey` changed are rebuilt -> `model::Page` is patched in place
  (same count: overwrite; count changed: splice) -> scene rebuild for the page (68 ms measured at 3,000 shapes: acceptable for now, section 3.5
  last bullet).
* **Invalidation granularity.** Each commit returns

  ```rust
  pub struct ChangeSet { epoch: u64, pages: BTreeMap<PageId, PageChange>, doc: DocChange /* bitflags: styles, themes, masters, pages, layers, media */ }
  pub struct PageChange { shapes: Vec<ShapeId> /* changed, incl. recalculated */, added, removed, order_changed: bool, sheet_changed: bool,
                          dirty_rect: Rect /* union of old and new bbox, inches */ }
  ```

  Rules: a shape cell edit dirties that shape; recalculated dependents (glued connectors, group children formulas) are in the journal so
  they are dirty too; descendants of a moved group are dirty *in the view* (world transform) but not in the doc; a style edit dirties
  `style_users` (index), a theme edit the `uses_theme` shapes, a master edit `master_users`; a page-size change dirties every node (the view's
  y-flip uses the page height, which is why `page_height` is in the key, `import.rs:230`); reorder sets `order_changed` and only the moved range.
* The host asks `engine.changes_since(epoch)`; the app can repaint `dirty_rect` instead of the whole page (it rasterises the whole page to a
  texture today, `build_page` `main.rs:1240`); that is a host change after the engine API lands (PR 13), and the `perf/direct-raster` branch
  consumes the same `ChangeSet`.
* Scene-level incrementality (per-shape `SceneItem` identity) is left to the renderer work; the engine exposes stable `ShapeId`s and the
  dirty set so that work does not need to change the doc.

### 3.6 Save

`Doc::save() -> SaveResult { bytes: Arc<[u8]>, kind: Verbatim | Patched | Fresh }`, cached by `epoch` (`export()` is called by the app per save
and by MCP/wasm on demand; it stops being a free borrow of an always-current `Vec`).

1. **Clean:** `epoch == base epoch`, or the journal's net diff against `base` is empty (edit then undo): return the opened bytes verbatim. This
   is stronger than today, where a no-op patch recreates the central directory (archive bytes are not compared, `docs/13` "untouched.entries").
2. **Patched (opened packages):** per part:
   * part not touched -> `raw_copy_file` from the source zip (existing code, `writer.rs:782-812`); compressed bytes identical.
   * touched page/master/document part -> **emit by walking the new shape tree**; for each node choose the finest correct granule:
     *Verbatim* (shape is `ptr_eq` to its base `Arc`, or structurally equal: copy `own` and `tail` spans), *Patched* (diff -> `ShapeCellPatch`es applied
     to the shape's own region with the existing `patch_shape_xml`, `writer.rs:1757`: cell attribute splice, then row replace, then section replace,
     then own-region replace), *Fresh* (new shape: `serialize_shape`, which replaces `new_shape_xml`). The page wrapper, `<Connects>` and the
     children lists are emitted around them. A move therefore changes one `V` attribute and nothing else.
   * `[Content_Types].xml`, `*.rels`, `pages.xml`, `masters.xml`: textual patches as today (adding a picture adds a rel and a content-type default).
   * The shape diff base is `Doc.base`: it holds the pages exactly as opened (their `Arc<Page>`). The first write to a page copies its
     shape map once (O(N) pointers, ~0.2 ms for 3,000 shapes) and the base keeps the original; unchanged shapes stay `ptr_eq` to the base.
     Undo restores the old `Arc`, so "edited then undone" is `ptr_eq` and costs nothing to skip.
3. **Fresh (new, drawio/excalidraw-imported or page-deleting rebuild):** today `write_fresh_document(&model::Document)` flattens masters into inline
   geometry (`docs/21`, `fresh.rs:11`). Stage 1 keeps that, fed by the view. Stage 2 (PR 14) adds `serialize_doc(&Doc)` that writes every part from
   the model (masters and styles kept, `Extra` re-inserted), which is the same `serialize_shape` used for new shapes in patched mode.
4. Determinism: BTreeMap order for parts, tree order for shapes, `entry_options` pins zip host/compression (`writer.rs:814-821`); saving
   twice gives identical bytes. No `HashMap` iteration may influence output.

**Guarantees and the tests that pin each** (public-fixture form runs in CI; the private corpus form is `cfg(private_fixtures)`):

| Guarantee | Test |
|---|---|
| G1 a clean document saves as the opened bytes | new: for each of the 7 public sources and the generated large drawing, `open -> export == source` (byte equality); with edits then full undo, same |
| G2 untouched parts keep their compressed bytes | existing manifest `untouched.entries` hashes (`fixtures/public/manifest.json`, `layoutcraft-fixtures/src/lib.rs:98`, checked by `public_fixtures.rs`), plus new: after one edit every entry except the edited part is byte-equal as a raw zip slice |
| G3 inside a touched part, shapes the edit did not touch are byte-identical | new: split the page text at `<Shape` boundaries before/after, assert every region of an untouched shape is equal; a move changes one attribute (assert the diff is one `V=` token) |
| G4 save then reimport equals the live view | extend `tests/incremental_import.rs` (private, today it compares the engine's model with a fresh import of `export()` after every edit) with a public version over the generated drawing and the 7 fixtures, run per command in the differential suite |
| G5 parse then serialize is identity per shape | new: for every shape of every fixture, `canonical(serialize_shape(parse(x))) == canonical(x)` (canonical = attribute order and quote style normalised); this guards `Extra` |
| G6 undo is exact | property test: random command sequences over fixtures; undo everything -> `Doc == initial` (structural) and `save == source`; redo everything -> equals the final state; `Indexes == rebuild` at every step |
| G7 the new op equals the old op | differential: for each ported op and fixture, `legacy(op)` page bytes == `new(op)` page bytes (PR 7+); where behaviour is intentionally different the PR says so (AGENTS 5) |
| G8 save is deterministic | save twice, compare |
| G9 goldens unchanged | `layoutcraft-conformance --manifest ../fixtures/public/manifest.json`; `golden_models.rs`, `golden_import.rs`, `golden_packages.rs` (private) - never regenerated (AGENTS 25) |

### 3.7 Host API and the JSON model

`Engine` keeps its public methods and `Result<_, String>` convention (`lib.rs:1-4`). Changes:

* **Seal the fields.** `package`, `document`, `bytes`, `history`, `redo`, `import_state`, `preview`, `clipboard*` become private; hosts get
  `document()` (the view, `&Document`, always current because commits update it incrementally), `has_document()` (replaces `export().is_some()`,
  `main.rs:2820`) and `is_modified()` (epoch != saved epoch; replaces the app's baseline clone and `has_changes()` O(n^2), `main.rs:1198, 2396`).
  The app's `commit_rect` (`main.rs:2344`) moves to `Engine::set_cells`/`Command::SetCells`. wasm's `Deref<Target=Engine>` keeps compiling
  because it only calls methods.
* **`export(&mut self) -> Result<Arc<[u8]>, String>`** (was `Option<&[u8]>`): three callers copy the bytes anyway (`main.rs:2432, 2543`, MCP
  `lib.rs:233`, wasm `vl_export`).
* **Additive typed API:** `apply(Command)`, `begin_gesture/end_gesture`, `changes_since(epoch)`, `undo/redo` unchanged, `merge_since`
  alongside `collapse_history`.
* **wasm:** every one of the 57 `vl_*` exports keeps its name and signature; the web shell is untouched (AGENTS 10). `vl_preview_move`
  and `vl_collapse_history` are adapters. Guard: the wasm spike (it compares every golden model) and `web/engine.js` smoke.
* **MCP:** the 49 tools call the same `edit/page_op/group_op/draw/...` adapters. Guard: `tools_list_is_the_one_of_the_node_server` and the
  `tests/oracle` scripts (Node server kept as oracle, `AGENTS.md` layout section), which compare output shapes.
* **JSON model:** kept, as a view/export. `engine.json()` and `document()` are produced from the `Doc` by the view layer; consumers (`check`,
  `compare`, `inventory`, drawio/excalidraw/dxf/pdf exports) are unchanged. `model::Document` memory is held once (view) next to the `Doc`;
  building the view lazily per page (with `OnceLock`) is an option if measured memory demands it. Schema version stays 1; `SCHEMA_VERSION`
  (`layoutcraft-model/src/lib.rs:13`) changes only if a persisted field changes meaning. `Doc` itself is not a persisted JSON format.
* **Stencils** (`stencils: Vec<Option<Package>>`) stay read-only byte packages with their thumbnails; dropping a master imports its parts
  into the `Doc` (`stencil::import_master`) as a `replace_part` + `masters` change.

## 4. Why this is the right cut

* The three expensive things today (whole-file undo, re-parse of the page per edit, O(N^2) glue scans) are removed by three independent
  mechanisms: part sharing (PR 2), the long-lived `Doc` with indexes (PRs 4-8), and dependents-only recalculation (PR 12).
* The formula engine gets a real home: cells have identity, a resolver, units and a place to write results; today formulas are strings that a
  580-line numeric evaluator reads at import time.
* The save path reuses what is proven against Visio (the patch writer and its tests) instead of replacing it: the diff produces the same
  `ShapeCellPatch` values the ops build by hand today, and the differential tests prove it per op.

## 5. Risks

| Risk | Mitigation |
|---|---|
| Two writers (XML-string ops and `Doc` ops) disagree during the migration | one writer at a time per op family; `Legacy` bridge materialises a package from the `Doc` through the *same* save, so both paths see the same state; differential tests G7 |
| Lossless parse/serialize is incomplete (unknown attribute, element order) | clean shapes are copied from source and never serialized; `Extra` only on structurally changed shapes; G5 over every fixture; fallback granule is a bigger splice, never a rebuild from the flattened model |
| Memory per shape in `Doc` + view doubles RAM | budget: <= 2 KB per typical shape in `Doc`, measured in PR 4 on the generated drawing and the private corpus before PR 6; per-page lazy view as the fallback |
| The formula crate's API differs from the contract in 3.4 | `Recalc` is a trait object behind `Doc`; PR 7 lands with a null engine plus the legacy rules, PR 12 swaps it; nothing else depends on the formula crate's types |
| Formula results differ from Visio's cached values | open does not recalculate; a cell is recalculated only when an input changed; verify mode (3.4 item 6) reports mismatches over the corpus before recalc is enabled by default; Visio COM probes on a maintainer machine (AGENTS 26) |
| Behaviour regressions in Visio acceptance (repair dialogs) | the patch granule matches today's (cell splice), so output XML equals today's for ported ops (G7); the maintainer's Visio open run (`docs/06-testing.md`) is part of the merge gate for PRs 7, 9-11, 14 |
| wasm size / panics | no new heavy dependencies (`smallvec`, `rustc-hash` at most); no `Instant`; `Arc` only |
| Hash-order nondeterminism leaks into output | `FxHashMap` for lookup only; all emitters iterate ordered structures (G8) |
| Huge drawings with sparse ids | `ShapeId` map is a hash map, not a dense vector; ids are only compared |
| Scope: ~12k lines of XML-string patch code to port (`writer.rs` 3,499; `groupedit.rs` 3,464 + `groupedit/` 2,595; `pageops.rs` 1,241; `styleedit.rs` 578; `stencil.rs` 554; `draw.rs` 419; others) | the `Legacy` bridge makes each family independent; PR 2 and PR 7 already pay for most of the user-visible win, so the long tail is not on the critical path |

## 6. Migration plan

Each PR ships on its own: the app, MCP server and web shell behave as before unless the PR says otherwise. Moves are separated from behaviour
changes (AGENTS 5). "Guard" lists what must be green; private-corpus tests are the maintainer's gate.

| # | PR | Kind | Content | Guard | Shippable because |
|---|---|---|---|---|---|
| 0 | Cost harness | tooling | checked-in generator of large drawings (`layoutcraft-fixtures`, bin) and an example that prints per-edit allocation, phase times, undo stack bytes and one-edit latency; counting allocator in the example only (no timing code in wasm crates, AGENTS 9); CI runs it as a smoke test without timing asserts; allocation *ratios* (bytes per edit / page XML) may be asserted, not milliseconds (AGENTS 22) | harness runs on public fixtures | no product code touched |
| 1 | Seal `Engine` | move | fields private; accessors `document()`, `has_document()`; app `commit_rect` through an engine method that builds the same `ShapeCellPatch`; no behaviour change | all engine tests (`tests/engine.rs`, private), app tests, wasm spike, MCP oracle | pure move; bytes output identical |
| 1b | `is_modified()` | behaviour (small) | epoch-free flag set by commit/undo/redo, cleared by save; app drops `baseline` clone and `has_changes()` | app test; manual dirty-flag check | removes an O(n^2) per-edit loop |
| 2 | Shared parts, revision counter | behaviour, perf | `Package` holds `Arc` parts, lazily inflated, `from_bytes` shares the source `Arc` (no `to_vec`); `Package::from_bytes_sharing(bytes, &prev)` reads the central directory and reuses `prev`'s part when CRC and size match, inflating only changed parts; `history`/`redo` store part-level deltas instead of zips; `fingerprint` -> revision counter (kills the 20 MB hash per `preview_move`) | engine tests, `incremental_import.rs`, manifest untouched hashes (G2), new test: history bytes <= k x changed parts after N edits | undo memory O(changed page XML) instead of O(file): 2.0 GB -> ~0.05 GB on the 300-shape + 20 MB case (estimate); no API change |
| 3 | `layoutcraft-doc` skeleton | new code | `Doc`, `Shape`, `Cell`, `Section`, ids, `PartStore`, `Transaction`, `History`, `Recalc` trait with a null engine; unit tests (journal swap, merge, bounds, indexes == rebuild) on hand-built docs; not wired | crate tests; `cargo test --workspace` | unused code, reviewable in isolation |
| 4 | Loader in shadow mode | behaviour (none visible) | `vsdx::load(Package) -> Doc` (cells, sections, masters, styles, themes, connects, media, `Extra`, `SrcSpan`); the engine builds a `Doc` at open and, in tests/debug builds, asserts `Doc -> ResolvedShape` equals `package.resolve_page_shapes` for every page | new equality test on 7 fixtures + generated drawing; private corpus equality | memory/open cost only; measure bytes per shape and open time here (<= 20 % open regression, <= 2 KB/shape) |
| 5 | Emitter and verbatim save | behaviour (save only) | `emit` with `Verbatim` granule only + clean-doc shortcut; `export()` returns source bytes for an unedited document | G1, G2, G5, G8 | an unedited document now saves byte-identical (new guarantee); edited documents still go through the old path |
| 6a | Move flatten | move | `import.rs` flatten / master-body functions take a `ResolvedShape` supplier trait | all golden tests unchanged; file diff is moves only | pure move |
| 6b | View from `Doc` | behaviour (none visible) | `Engine.document()` built from `Doc` (per-page `ViewCache`, pointer keys, no 400-node limit); reads of the 20+ `*_json` info functions (`lib.rs:1074-1250`) come from `Doc` | goldens (public + private), `incremental_import.rs`, conformance runner, check/compare/inventory oracles | model JSON identical; first PR where `Doc` serves reads |
| 7 | Commands, journal, gestures, transform ops | behaviour | `apply(Command)`; ports `Move/Resize/Rotate/Flip/Align/Distribute/SetCells/Delete` (edit 1-7, 11, 12, 14, 15, 4, 6) with the connector reroute as a rule; `History` on the journal replaces byte history; `merge_since`; `Legacy` bridge for every other op; app drag uses a gesture | G3, G6, G7 (differential vs legacy for each op on fixtures), engine tests, MCP oracle, wasm spike, Visio open run by a maintainer | the user-visible win: move on 3,000 shapes 1.5 s -> target <= 15 ms; undo memory O(touched) |
| 8 | Indexes and dependents-only reroute | behaviour (perf) | glue/parent/name indexes live; reroute only connectors whose glue dependencies changed; delete `chain()` walk and the per-end `find` | `reroute.rs`, `route_sweep.rs` (public), G7 | same output, O(1) lookups |
| 9 | Port content ops | behaviour | text (8, 13), format (10), data rows (17, 18), layers (16, page_op 7-10), hyperlinks/alt text/name (19, 39, 40), styles (36-38) | G7 per op, `rich_text`, `style`, `hyperlinks`, `shape_name`, `alt_text` tests | each op family independently switches from bridge to native |
| 10 | Port structure ops (2-3 PRs) | behaviour | add/draw tools, pictures, paste/copy, group/ungroup/reorder, connectors, stencil drop/replace, shape operations | G7, `group_edit.rs` (141 tests), `shape_ops.rs`, `insert_picture.rs`, public fixtures | as above |
| 11 | Port page/doc ops | behaviour | pages, size/scale/print, background pages, guides, comments, docprops, connection points, vertices, themes | G7, `page_order.rs`, `print_setup.rs`, `drawing_scale.rs`, `comments.rs`, `docprops.rs` | as above |
| 12 | Formula engine | behaviour | (a) `Recalc` backed by `layoutcraft-formula`, verify mode over corpus, report only; (b) incremental recalc on edit with master plans and copy-on-write instance cells, replacing `refresh_formula_values`; (c) retire `vsdx/src/formula.rs` and import-time evaluation | verify-mode mismatch report reviewed; G7 where outputs must stay equal; Visio COM probe by a maintainer for any deliberate change | needs only PR 7; can run in parallel with 9-11 once the crate lands |
| 13 | Change sets to hosts | behaviour | `changes_since`; app repaints the dirty rect; incremental scene hand-off; `perf/direct-raster` consumes it | app manual check, conformance visual run | host change after API exists |
| 14 | Serializer, remove the byte path | cleanup | `serialize_doc` for fresh save; delete `Engine.bytes`, the `Legacy` bridge and `Package` from the engine; fresh packages keep masters instead of flattening | fresh tests (`golden_packages.rs`, fixtures), re-import equality, Visio open run | final state; all byte ops gone |

Order constraints: 0 and 1 first; 2 independent of 3-8; 3 -> 4 -> 5 -> 6 -> 7 -> 8; 9-11 after 7 in any order; 12 after 7; 13 after 7; 14 last.

## 7. How success is measured

Same harness, same scenarios as section 1.4 (PR 0). The numbers below are **targets proposed by this note, not measurements**; PR 7 must meet
the first block and PR 2 the second before they merge.

| Metric | Today (measured) | Target |
|---|---|---|
| Move one shape, 300 shapes + 150 connectors | 108 ms median | <= 5 ms |
| Move one shape, 3,000 shapes + 1,500 connectors | 1,479 ms median (max 5.5 s) | <= 15 ms median, <= 30 ms p95 |
| Same with 20 MB of pictures | 1,579 ms | same as above (independent of file size) |
| Allocation per edit, 3,000-shape page | 954 MB | <= 1 MB |
| Gesture update (drag frame), 3,000 shapes, shape with 5 connectors | 5.6 ms (`preview_move`, hash only) + full edit on release | <= 4 ms per frame; one 15 ms commit |
| Undo or redo, 3,000 shapes | 551 ms | <= 2 ms |
| Undo stack after 100 moves, 20 MB file | 2,048 MB | <= 2 MB |
| Undo stack after 100 moves, 3,000 shapes | 10.7 MB | <= 1 MB |
| Peak heap after 100 edits, 20 MB file | 2.3-2.8 GB | within 1.3 x the heap right after open |
| Save after one move, 20 MB file | n/a (bytes always current, but each edit paid) | <= 150 ms; clean save = 0 ms (source bytes) |
| Open, 3,000 shapes | 567 ms | <= 1.2 x today |
| Doc heap per typical shape | n/a | <= 2 KB |
| Untouched bytes | zip entries equal (manifest hashes) | G1-G3 hold |

Reporting: the PR description lists the harness output before and after (AGENTS 27 asks to say what ran and what did not); private-corpus
numbers on 20-35 MB drawings are taken by a maintainer.

## 8. Open questions

1. The exact `layoutcraft-formula` API (3.4): this note guesses the narrow contract; reconcile when the branch has a public surface.
2. `model::Page.shapes` is `Vec<Shape>` (owned). In-place patching of the view is O(count) when counts change; switching to `Vec<Arc<Shape>>`
   is cheap for readers (`Deref`) but touches every constructor and needs serde's `rc` feature (a feature of a crate already in the lock). Decide
   in PR 6b with a measurement.
3. Whether `Part::Source` should keep compressed bytes of *changed-then-undone* parts to avoid re-deflating on export; irrelevant until export
   is measured in PR 5.
4. `Engine.export()` signature change (3.7) touches three hosts; if that is too disruptive, keep `export(&self) -> Option<&[u8]>` backed by a
   cached buffer refreshed by `save()` and add `save()` as the new explicit call.
5. 88 vs 57 wasm exports: I could only find 57 (3.7 note); if another build adds more, the rule (name and signature unchanged) is the same.
6. Visio's behaviour for any cell class the formula engine will now write (which inherited cells Visio recalculates on load): to be settled
   with COM probes on a maintainer machine, recorded in `docs/` (AGENTS 14, 26), not by guessing.

## Appendix A: measurement program

Two scratch examples (`measure_edit` and `measure_edit2`, not committed): wrapping `GlobalAlloc` counting `TOTAL`/`LIVE`/`PEAK`;
`generated(shapes, pictures)` builds a `layoutcraft_model::Document` of N rectangles (`x = 20 + 120 c`, `y = 20 + 70 r`, 60 columns) and N/2 elbow
connectors, `write_fresh_document`, opens it in `Engine`, then `insert_picture` of 1500x1500 random-pixel PNGs (about 6.8 MB each, stored, not
recompressed); `run()` performs `Engine::open`, 100 x `Engine::edit(0, 2, id, +/-0.1, ...)`, one pass of the phases using
`ImportState::open_editor`, `GroupEditor::move_member`, `patch_page`, `Package::from_bytes`, `reroute_connectors`, `import_document_with`,
`engine.svg`, then `engine.undo()` to the bottom of the stack (10 for the media scenario). Raw output: the table in section 1.4.

## Appendix B: code references used

`engine/src/lib.rs`: 32-54 (Engine), 57 (fingerprint), 71-118 (open/adopt), 161-208 (undo/redo/collapse), 234 (commit), 291-532 (edit),
507-530 (reparse/reroute/history), 537-550 (preview_move), 557-780 (page_op), 785-830 (group_op), 836-896 (clipboard/connectors),
949-1066 (stencils/masters), 1074-1250 (info JSON).
`vsdx/src/lib.rs`: 16-18 (limits), 272 (Connect), 281 (PageRef), 349-408 (Package), 496 (pages).
`vsdx/src/import.rs`: 29-66 (ImportState/NodeCache), 118-329 (import_document_with), 952 (flatten_node).
`vsdx/src/shapes.rs`: 30-168 (Cell/Section/ShapeNode/MasterShape/ResolvedShape), 188-250 (catalog load/resolve), 333 (page_shapes).
`vsdx/src/writer.rs`: 11-80 (patches), 545-634 (patch_page), 782-821 (write_package_edit/entry_options), 1047-1162 (patch_shape_in_text,
find_shape_range), 1757 (patch_shape_xml), 1893-1925 (ids), 2032 (refresh_formula_values).
`vsdx/src/groupedit.rs`: 765 (GroupEditor), 1185 (reroute_connectors), 1315 (chain), 1864 (detach_patches).
`model/src/lib.rs`: 60-116 (Document/Page), 279-427 (Shape), 681 (Connector). `render/src/scene.rs`: 343 (scene_from_document_with_options).
`wasm/src/lib.rs`: 12-30, 440, 898. `mcp/src/lib.rs`: 8, 755. `app/src/main.rs`: 920, 1198, 1242, 2344-2355, 2396.
