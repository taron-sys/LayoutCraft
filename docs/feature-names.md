# Feature names: naming checks and decisions

Date of the checks: 2026-11-25.

This is a naming policy record, not legal advice or trademark clearance. It records what was looked up, what was found, and
which labels LayoutCraft should show. "Nothing found" never means "free" (`naming.md` rule 4). Where a check did not settle a
name, the decision is "do not ship the name" (rule 5).

Policy: the shared Craft `naming.md`, rules 1 to 6. Command ids, preference keys,
stored values, code and comments keep their current names (rule 6); only user-visible labels change, the old label stays as a
hidden search alias, and the new label gets translated.

## 1. What was checked and how

| Check (rule 4) | What was done | Result |
|---|---|---|
| Owner's trademark list | Fetched the Microsoft list ("Microsoft Trademarks", October 2026, linked from https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks): https://cdn-dynmedia-1.microsoft.com/is/content/microsoftcorp/microsoft/mscle/documents/presentations/Trademarks_List_FY27Q2.pdf . Converted to text and searched every name below. | Lists `Visio` and `Visio Designs`. None of the feature names below is on it (`SmartScreen` and `Tell Me Why` are unrelated entries). The list says it is non-exhaustive and that absence is no waiver. |
| ™ or ® in Microsoft documentation | Web searches for Microsoft Learn and support pages on ShapeSheet, AutoConnect, Quick Shapes, Data graphics, Backstage, Quick Access Toolbar. | Microsoft's pages write "Microsoft® Visio®" and use the feature names as product feature names (AutoConnect, Quick Shapes, ShapeSheet, Data graphics, Backstage). Our tools return page text without reliable ™/® glyphs per feature name, so a per-name ™/® check is **not established**; treat as open. |
| USPTO | https://tmsearch.uspto.gov/ driven in a browser pane (wordmark search, 2026-11-25). Positive control: `Visio` returned 114 results including `VISIO`, live and registered to Microsoft Corporation (serial 74275413, class 9; serial 74640800, class 41). | Run for the names marked "USPTO: run" below. The search is word-based and fuzzy; only the first results page (up to 50 marks) per query was read, `&` was dropped from queries, and only marks containing the full phrase (ignoring spaces and punctuation) were counted. It is not an exhaustive clearance search. |
| EUIPO eSearch | Tried https://euipo.europa.eu/eSearch/ . | **Not reachable by our tools** (connection reset). Not run. |
| TMview | Tried https://www.tmdn.org/tmview/ . | **Not reachable by our tools** (page returned empty to a fetch). Not run. |
| WIPO Global Brand Database | Tried https://branddb.wipo.int/ . | **Not reachable by our tools**: the site puts an ALTCHA verification step in front of search. We do not bypass bot checks. Not run. |
| Three unrelated products using the name descriptively | Web searches. Collected only where listed in section 3. | Incomplete. Where fewer than three unrelated products are listed, the name is not called common. |

Because EUIPO, TMview and WIPO were not run for any name, **no decision below is final**. Every "replace" proposal and every
"keep" is subject to the missing register searches and a Craft owner's review.

## 2. Findings that matter most

1. **ShapeSheet** was a registered US software mark of Microsoft: `SHAPESHEET`, serial 74686981, registration 2082080 (class 9,
   "feature of computer software that facilitates the editing of drawings and graphics ..."), first use 1992, owner Microsoft
   Corporation. Status 2026: dead, cancelled 2020-04-24 because no Section 8 declaration was filed. A cancelled registration is
   not a release of any other rights, and Microsoft still uses the word as its feature name, so it is the strongest signal in this
   record. LayoutCraft shows "Shape Sheet pane" and a "Shape Sheet" heading today.
2. **Visio** is on Microsoft's list and live-registered (class 9 and 41). It may appear only in factual compatibility statements
   ("opens .vsdx files"), never in a feature label (naming.md rule 3).
3. **AutoConnect, Quick Shapes, Data graphics, Backstage, ShapeSheet** are used as Microsoft's own feature names in Microsoft
   support and Learn pages (section 3). Third parties also hold live US marks on `AUTOCONNECT` (other fields) and `BACKSTAGE`
   (including software), so the words are crowded; that does not make Microsoft's feature name ours to use.

## 3. Per-name tables

Each table: date 2026-11-25 for every row. "USPTO: run" means a wordmark search as described in section 1; "EUIPO / TMview /
WIPO: not reachable by our tools" applies to every name.

### 3.1 ShapeSheet / Shape Sheet

| Item | Record |
|---|---|
| Where LayoutCraft shows it | `web/shell.html` ("Shape Sheet pane", tooltip "Show the ShapeSheet ..."), `web/shell.js` (heading `Shape Sheet - Shape ...`, "Select one shape to see its ShapeSheet.") |
| Microsoft list | Not on the list. |
| Microsoft docs | Feature name in Learn: https://learn.microsoft.com/office/client-developer/visio/open-the-shapesheet-for-a-shape-page-or-document , https://learn.microsoft.com/en-us/archive/blogs/wmorein/introduction-to-the-shapesheet |
| USPTO: run | `ShapeSheet` and `Shape Sheet`: one mark, `SHAPESHEET`, Microsoft Corporation, 74686981 / 2082080, class 9, DEAD cancelled 2020-04-24. https://tmsearch.uspto.gov/search/search-results/74686981 |
| Common use by three unrelated products | None found; only Visio and tools that read Visio files use the word. |
| Decision | **Replace.** Proposed `Shape Formulas`. USPTO run on the replacement: no mark containing it in the first page of results. EUIPO / TMview / WIPO not run. Needs a Craft owner to confirm. **Urgent: this label ships today.** |

### 3.2 AutoConnect

| Item | Record |
|---|---|
| Where | `web/shell.html` (checkbox "AutoConnect", tooltip), `web/shell.js` (status messages "AutoConnect: ..."), `web/ribbon.js` (command `autoConnectToggle` keeps its id). |
| Microsoft list | Not on the list. |
| Microsoft docs | Feature name: https://support.microsoft.com/en-us/visio/add-connectors-between-visio-shapes , https://techcommunity.microsoft.com/blog/microsoft_365blog/autoconnect-in-visio-2010/237417 |
| USPTO: run | `AutoConnect` / `Auto Connect`: no Microsoft mark in the first page. Live marks by others: `AUTOCONNECT` (Frost Brown Todd LLP, class 41; SpringBig, class 42; Carl Zeiss Meditec AG, class 9 data-management software), `AUTO CONNECT` (Electrans Technologies, class 12), others in finance and insurance. |
| Common use by three unrelated products | Not collected. |
| Decision | **Replace** ("Auto..." coined name, Microsoft's feature name, and a crowded word including class 9 software). Proposed `Add and Connect Arrows` (USPTO run on "Connect Arrows": none). Owner to confirm; the label is long, a shorter own name may be preferred. |

### 3.3 Quick Shapes

| Item | Record |
|---|---|
| Where | Not in LayoutCraft's UI today (planned in docs/43). |
| Microsoft docs | Feature name: https://support.microsoft.com/en-gb/office/find-most-used-shapes-easily-with-quick-shapes-dddc397d-23bd-47f0-a81a-b6fb1a4df6cb |
| USPTO: run | No mark containing `Quick Shapes` in the first page. Near matches by others (`QUIKSHAPES`, class 19 drywall; `SHAPES` in unrelated classes). |
| Decision | **Do not use.** Proposed `Frequent Shapes` (USPTO run: none). Owner to confirm. |

### 3.4 Dynamic Grid

| Item | Record |
|---|---|
| Where | Not in LayoutCraft's UI today. |
| USPTO: run | No mark containing `Dynamic Grid` in the first page (`DYNAGRID`, class 16, is a different word). |
| Common use | "dynamic guides" is used by Apple Motion (https://help.apple.com/motion/mac/5.2/en.lproj/motn173b4203.html); "alignment guides" by draw.io (https://www.drawio.com/docs/manual/editor/alignment-tools/), Easel (https://easel.com/features/snap-alignment), Canvas X Draw (https://help.canvasgfx.com/en/canvas-x-draw/user_guide/Content/c_Documents/Using_alignment_guides.htm). Three unrelated products use "alignment guides" for their own feature. |
| Decision | **Do not use "Dynamic Grid".** Use `Live Alignment Guides`; the descriptive core "alignment guides" is common (three products above). USPTO run on `Live Alignment Guides`: none. |

### 3.5 Data Graphics

| Item | Record |
|---|---|
| Where | Not in LayoutCraft's UI today. |
| Microsoft docs | Feature name: https://support.microsoft.com/en-us/visio/data-graphics-an-overview |
| USPTO: run | Only dead marks containing it (`TOBIN DATA GRAPHICS`, `MOUNTAIN DATA GRAPHICS`, `DATA GRAPHICS.COM`); no Microsoft mark found. |
| Common use | Not collected. |
| Decision | **Do not use** (unclear, rule 5). Proposed `Data Visuals` (USPTO run: none). |

### 3.6 SmartShapes

| Item | Record |
|---|---|
| Where | Not in the UI. docs/43 notes it was not found in a cited page. |
| USPTO: run | `SMARTSHAPES`, live registered, Brakebush Brothers Inc, class 29 (formed chicken); unrelated field. No Microsoft mark in the first page. |
| Decision | **Do not use.** "Smart..." coined family; unverified. If a name is ever needed, `Shape Behaviours` (not checked). |

### 3.7 Shape Data

| Item | Record |
|---|---|
| Where | `web/shell.html` ("Shape Data columns"), `web/shell.js` (section heading, error messages "Select one shape: its Shape Data rows become the set"), `rust/layoutcraft-app/src/main.rs` (report dialog text). |
| Microsoft docs | Feature name in Visio docs (https://learn.microsoft.com/en-us/office/client-developer/visio/shape-data-section). |
| USPTO: run | No mark containing `Shape Data` in the first page. |
| Common use | Not collected (three unrelated products not found). |
| Decision | **Replace** (unclear, rule 5). Proposed `Shape Fields` (USPTO run: none). Names of data rows that come from the user's document stay as they are (rule 3). |

### 3.8 Format Painter

| Item | Record |
|---|---|
| Where | `rust/layoutcraft-app/src/ribbon.rs` `label()` (`painter` -> "Format Painter"), `web/shell.html` (tooltip and aria-label "Format painter"), `web/shell.js` comments. |
| Microsoft list | Not on the list. |
| USPTO: run | No mark containing `Format Painter` in the first page. |
| Common use | Zoho Writer and Zoho Sheet (https://help.zoho.com/portal/en/kb/writer/editing-formatting/working-with-text/articles/how-to-quickly-copy-formatting-in-writer-document), WPS Office (https://www.wps.com/academy/how-to-use-the-format-painter-quick-tutorials-1861540/). Two unrelated products found, not three. |
| Decision | **Replace** with `Copy Format` (USPTO run: none). The name is widely used, so an owner may reasonably decide to keep it once a third product and the register searches are in; until then the shipped label is the own name. |

### 3.9 Quick Access Toolbar

| Item | Record |
|---|---|
| Where | `web/ribbon.js` (aria-label "Quick access toolbar"), `web/qat.js` (menu items "Add to / Remove from / Reset Quick Access Toolbar"). |
| Microsoft docs | Feature name: https://support.microsoft.com/office/customize-the-quick-access-toolbar-43fff1c9-ebc4-4963-bdbd-c2b6b0739e52 |
| USPTO: run | No mark containing it in the first page. |
| Common use | WPS Office copies the name (https://www.wps.com/academy/how-to-customize-the-quick-access-toolbar-quick-tutorials-1862134/). LibreOffice does not use it (https://books.libreoffice.org/en/GS70/GS7014-CustomizingLO.html). Not common. |
| Decision | **Replace** with `Favourite Commands Bar` (USPTO run: none). The shell already uses "Favourites" for shapes. Owner may prefer a shorter name, e.g. "Pinned Commands". |

### 3.10 Backstage

| Item | Record |
|---|---|
| Where | Only code, ids and comments (`window.layoutcraftBackstage`, `#backstage`, `web/backstage.js`). No visible label found; the File tab is labelled "File". |
| Microsoft docs | Feature name: https://support.microsoft.com/office/start-backstage-with-the-file-tab-04610088-406c-43d0-98a0-c1999ab4ef53 |
| USPTO: run | Microsoft Corporation `BACKSTAGE`, dead cancelled, class 9 ("a business pr..." software). Many other `BACKSTAGE` marks, live ones include Wella International Operations (class 9 computer hardware and software) and Hit'n'Mix Limited (class 9, pending). |
| Decision | **Do not show the word to users.** Ids and comments stay (rule 2, 6). If a label is needed: `File page` (not checked). |

### 3.11 Key Tips, Tell me, Mini toolbar, Task panes, Snap & Glue, Re-Layout Page, Auto Align & Space, Visual Aids

| Name | USPTO: run | Where in LayoutCraft | Decision |
|---|---|---|---|
| Key Tips | `KEYTIPS` dead, SteelSeries, class 9. Microsoft list: none. | Not named in the UI (the tab badges carry no feature name). | Do not name it Key Tips. If needed: `Key hints` (USPTO run: none). |
| Tell me | No Microsoft mark found; the Microsoft list has only `Tell Me Why` (a game). | Not shipped. | Use `Command Search` (USPTO run: none) when built. |
| Mini toolbar | none | Not shipped. | Use `Floating Toolbar` (USPTO run: none). |
| Task panes | none | `web/shell.html` tooltips "(View > Task panes)". | Replace with `Side panels` (not checked). Evidence for common use not collected. |
| Snap & Glue | none | Not shipped. | `Snap and Attach` (USPTO run: none). |
| Re-Layout Page | none | Not shipped. | `Rearrange Page` (USPTO run: none). |
| Auto Align & Space | Live `AUTO-ALIGN` marks by others, incl. Sound Radix Ltd (class 9 software), John M. Fallon (class 10), others. | Not shipped. | `Auto...` coined name with a live class 9 mark nearby: do not use. `Tidy Alignment and Spacing` (not checked on USPTO). |
| Visual Aids | `CREATE VISUAL AIDS` live, Create Visual Aids Ltd (class 16 teaching materials); no other. | Ribbon group name (`ribbon-web.json`, `web/ribbon.js`). | Unclear (three-product evidence not collected): ship `Guides and Aids` (not checked on USPTO) after an owner confirms. |

### 3.12 Check Diagram

| Item | Record |
|---|---|
| Where | Window title and button in `rust/layoutcraft-app/src/main.rs`, `web/shell.html` (dialog, button), `ribbon.rs` `label()`, group "Verify" in `ribbon-web.json`. |
| USPTO: run | No mark containing `Check Diagram` or `Check Drawing`. Not on Microsoft's list. |
| Common use | Not collected. |
| Decision | **Replace** with `Check Drawing`: it is Visio's wording as far as known (docs/43 S4), and the check did not settle it. Command id `checkDiagram` stays. |

### 3.13 Pan & Zoom, Size & Position, Format Shape, Define Styles

| Name | USPTO: run | Decision |
|---|---|---|
| Pan & Zoom (pane in `web/shell.js`, tooltip in `shell.html`) | none (queried as `Pan Zoom`) | Replace with `Page Overview`, which describes what the thumbnail pane does (USPTO run: none). |
| Size & Position (pane) | none | Keep; ordinary words. Write `Size and Position` for consistency. |
| Format Shape (planned) | none | Ordinary words; ship `Shape Format panel` as planned or keep. Low risk. |
| Define Styles (dialog, `shell.html`) | none | Keep; describes the action in ordinary words. |

### 3.14 Edit Points, Change Shape, Connection Point, Fit Curve

| Name | USPTO: run | Decision |
|---|---|---|
| Edit Points | none | Keep; ordinary words. |
| Change Shape | `CHANGE SHAPE` dead, individual owner, none live | Keep; ordinary words. |
| Connection Point | `CONNECTION POINT` live: FNF Intellectual Property Holdings (class 42), Harland Clarke (class 35); `CONNECTIONPOINT` live, ConnectionPoint Systems (class 35). Other fields. | Keep as an ordinary technical term; draw.io documents snapping to connection points (https://www.drawio.com/docs/manual/editor/alignment-tools/). The docs/43 proposals (Replace Shape, Glue Point, Edit Vertices) are optional and were not checked. |
| Fit Curve | dead marks only (`FITCURVE` Kokuyo; `FIT CURVE LIFE`) | Keep. |

### 3.15 Other labels seen in the UI

Checked by reading only (ordinary descriptive words, no distinctive name found): Layers, Find, Group, Ungroup, Align, Distribute,
Duplicate, Styles, Themes, Variants, Show/Hide, Palette, Illustrations, Borders & Titles, Backgrounds, Inventory report, Compare
Documents, Page Setup, Document properties, Comments, Keyboard shortcuts, Export, Text Box, Fit to Window. Tab names File, Home,
Insert, Draw, Design, Data, Process, Review, View, Help are ordinary words; the layout follows Visio's (allowed), the names are
not a feature mark. "Process" as a tab name is kept; the "Flow" idea in docs/43 is optional.
"AI assistant (MCP)": MCP names a protocol we support (rule 3).
Stencil / master: `.vssx` stencil is a file-type word and stays in file-type contexts (rule 3). Where "stencil" or "master" names
our own feature in a label (Shapes panel tooltips, "Shape source" lists), prefer `shape library` and `library shape`; evidence for
the plain words was not collected, so this is **unclear: do not ship the Visio-specific wording until a Craft owner decides**.
Visio: remains only in factual compatibility text ("opens Visio files"); review text such as "as in Visio" in tooltips.

## 4. Summary: current label -> label to ship

All decisions are provisional until the EUIPO, TMview and WIPO searches are run and a Craft owner has looked at the "replace"
and "unclear" rows.

| Current label (where) | Label to ship | Status |
|---|---|---|
| Shape Sheet pane / ShapeSheet (shell.html, shell.js) | Shape Formulas | replace, **urgent** |
| AutoConnect (shell.html, shell.js) | Add and Connect Arrows (owner may shorten) | replace |
| Shape Data (shell.html, shell.js, main.rs) | Shape Fields (user-document row names unchanged) | replace |
| Format Painter / Format painter (ribbon.rs, shell.html) | Copy Format | replace (interim) |
| Check Diagram (main.rs window title, ribbon.rs, shell.html) | Check Drawing | replace |
| Quick Access Toolbar (ribbon.js, qat.js) | Favourite Commands Bar | replace |
| Task panes (shell.html tooltips) | Side panels | replace |
| Pan & Zoom (shell.js, shell.html) | Page Overview | replace |
| Visual Aids (group, ribbon-web.json) | Guides and Aids | unclear, owner decision |
| Stencil / Master where used as our feature label | shape library / library shape | unclear, owner decision |
| Backstage (ids and comments only) | no visible label; File | no change to ids |
| Size & Position | Size and Position | keep |
| Define Styles, Edit Points, Change Shape, Connection Point, Fit Curve | same | keep (ordinary words) |
| Layers, Find, Group, Align, Distribute, Themes, Variants, Process, Verify, Reports and the rest of 3.15 | same | keep |
| Not in UI yet: Quick Shapes, Dynamic Grid, Data Graphics, SmartShapes, Key Tips, Tell me, Mini toolbar, Snap & Glue, Re-Layout Page, Auto Align & Space | Frequent Shapes, Live Alignment Guides, Data Visuals, (none), Key hints, Command Search, Floating Toolbar, Snap and Attach, Rearrange Page, Tidy Alignment and Spacing | do not use the Microsoft name |

## 5. Searches not run

- EUIPO eSearch: connection reset; not run for any name.
- TMview: empty page to our fetch; not run for any name.
- WIPO Global Brand Database: bot-verification (ALTCHA) in front of the search; not run, not bypassed.
- ™/® check per feature name in Microsoft documentation: not established (see section 1).
- Three-product common-use evidence: collected only for alignment guides (3), Format Painter (2), Quick Access Toolbar (1 copy);
  not collected for the other names.
- USPTO searches read only the first results page of a fuzzy word search; marks outside it and non-text (design) marks were not
  reviewed. Replacement names marked "not checked" were not searched at all.

## 6. Follow-up

Record the same table in the pull request that renames a label (rule 4). Do the renames as one labels-only change: keep command
ids, keep the old label as a hidden search alias, translate the new label, and update `docs/43-ui-parity.md` section 6 statuses
once the owner agrees.

## 7. Status of the renames (2026-11-25)

The eight "replace" rows of section 4 are applied to the text people see, as one labels-only change. The decisions stay provisional
(section 5: EUIPO, TMview and WIPO were not searched); a Craft owner still has to review the "unclear" rows.

| Old label | New label | Where changed |
|---|---|---|
| Shape Sheet / ShapeSheet | Shape Formulas | `web/shell.html` (pane checkbox, tooltip, heading), `web/shell.js` (heading, empty message) |
| AutoConnect | Add and Connect Arrows | `web/shell.html` (checkbox, tooltip), `web/shell.js` (status messages, arrow tooltips), `web/engine.js` (error text) |
| Shape Data / Shape data | Shape Fields / Shape fields | `web/shell.html` (button, dialog, report note), `web/shell.js` (error messages), `web/contextmenu.js` |
| Format Painter / Format painter | Copy Format / Copy format | `rust/layoutcraft-app/src/ribbon.rs`, `web/shell.html` (tooltip, aria-label) |
| Check Diagram | Check Drawing | `rust/layoutcraft-app/src/ribbon.rs`, `rust/layoutcraft-app/src/main.rs` (window title), `web/shell.html` (button, dialog) |
| Quick Access Toolbar | Favourite Commands Bar | `web/qat.js` (menu items, aria-label), `web/ribbon.js` (aria-label) |
| Task panes | Side panels | `web/shell.html` (tooltips) |
| Pan & Zoom | Page Overview | `web/shell.html` (pane checkbox, tooltip, heading) |

Not changed, on purpose:

- Command ids, element ids, function names, storage keys and comments keep the old words (rule 6 of the naming policy); the engine's
  internal error texts ("shape data: ...") and the stored Visio names are untouched.
- The Check Drawing result line "(Shape data > Alt text)" in `web/checkdiagram.js` and `layoutcraft-engine/src/check.rs`: the private
  oracle `tests/oracle/check-oracle.json` records that text. A maintainer has to change both places and regenerate the oracle with
  `visio-test-harness/dump-check-oracle.mjs` in one step.
- MCP tool descriptions (`rust/layoutcraft-mcp/tests/oracle/tools-list.json` is a recorded list) still use the old words.
- No hidden search alias for the old labels: LayoutCraft has no command search yet. Add the aliases when it has one.
- No translation of the new labels: the shell has no translation files.
- "Visual Aids" and "stencil / master" (owner decision) and the "keep" rows.

## 8. Review by the naming owner: what has to be decided

Sections 3 to 7 are provisional. This list is what a Craft owner has to settle before the names count as final. It adds no new
finding; everything in it comes from sections 1 to 7. Nothing below is legal advice.

### 8.1 Decisions on the labels that ship now

For each row: **approve**, **shorten or change** (write the new label), or **go back to the old label**. The old label can only come
back when the missing searches (8.2) and the three-product evidence (8.3) are in.

| # | Label now shipped | Record's own doubt | Decision | Initials / date |
|---|---|---|---|---|
| 1 | Shape Formulas (was Shape Sheet) | none beyond the missing register searches | | |
| 2 | Add and Connect Arrows (was AutoConnect) | long; "owner may shorten"; `Connect Arrows` had no USPTO hit | | |
| 3 | Shape Fields (was Shape Data) | replaced as "unclear" (rule 5), not because of a found mark | | |
| 4 | Copy Format (was Format Painter) | marked **interim**: two unrelated products use Format Painter, not three | | |
| 5 | Check Drawing (was Check Diagram) | the record's reason is that it is "Visio's wording as far as known" (docs/43 S4) and says the check "did not settle it". Confirm the reason or pick a different word | | |
| 6 | Favourite Commands Bar (was Quick Access Toolbar) | owner "may prefer a shorter name, e.g. Pinned Commands" | | |
| 7 | Side panels (was Task panes) | USPTO not checked; common-use evidence not collected | | |
| 8 | Page Overview (was Pan & Zoom) | none beyond the missing register searches | | |

### 8.2 Searches nobody has run

EUIPO eSearch, TMview and the WIPO Global Brand Database were not reachable by our tools (section 5), and we do not bypass the bot
check. A person with a browser has to run them. Search each string below as a word mark, with the owner **Microsoft Corporation** in
mind, and note the classes found (software is class 9, services class 42; section 3 lists the USPTO results already read).

- The eight new labels: `Shape Formulas`, `Add and Connect Arrows` (also `Connect Arrows`), `Shape Fields`, `Copy Format`, `Check Drawing`,
  `Favourite Commands Bar` (also `Pinned Commands`), `Side panels`, `Page Overview`.
- The old names, to see who else holds them: `ShapeSheet`, `AutoConnect`, `Format Painter`, `Quick Access Toolbar`.
- Names for features not built yet (section 4, last row), so the choice is made before they ship: `Frequent Shapes`,
  `Live Alignment Guides`, `Data Visuals`, `Key hints`, `Command Search`, `Floating Toolbar`, `Snap and Attach`, `Rearrange Page`,
  `Tidy Alignment and Spacing`, `Guides and Aids`.

### 8.3 Evidence still missing

- A third unrelated product that calls the feature by the same words: Format Painter has two, Quick Access Toolbar one copy, the rest none
  (section 5).
- A ™ or ® check per feature name in Microsoft's own documentation (section 1: not established).
- The USPTO reading covered only the first results page of a fuzzy search, and no design marks. Replacement names marked "not checked"
  in section 3 were not searched at all.

### 8.4 Open rows from section 4

- **Visual Aids** (ribbon group) to `Guides and Aids`, and **stencil / master** where used as our own feature label to
  `shape library` / `library shape`: both are "unclear, owner decision".
- The check message "(Shape data > Alt text)" and the MCP tool descriptions still use the old words (section 7): decide whether they
  follow the new label, then have a maintainer change them together with the recorded oracle files.
