# 34. Desktop builds (Windows, macOS, Linux): the native app

**Decision 2026-11-19 (the user):** the desktop build is the **native egui application** of docs/17-ui-toolkit.md (`rust/layoutcraft-app`). No webview, no Tauri. (An earlier draft of this page recommended Tauri; it contradicted docs/17 and was withdrawn the same day.) The web application stays as the browser product and as the reference for the features the native app has to reach; it is not packaged for the desktop.

The release chain (signing, checksums, drafts) and the legal questions are covered in private release notes and are not answered here.

## 1. Where the native app stands

`rust/layoutcraft-app` (about 1,600 lines, eframe / egui 0.37 + the scene-to-SVG renderer of `layoutcraft-render` through resvg): open a .vsdx / .vstx (command line, path field, drag and drop), reload, page navigation, fit / 100 %, pan and zoom, hit-testing, an inspector, move and resize of shapes, undo and redo, "Save copy", simple rectangle insertion and text editing. It edits by patching the package (the same approach as the web editor). Baseline build on this host: see section 7. docs/17 lists the remaining order of work (connector editing, palette, grouped transforms, text editor / IME, accessibility).

What the web application has that the native one does not (measured 2026-11-19): about 8,000 lines of JS around the same Rust core: the ribbon with 9 tabs and the File backstage, 16 dialogs in the page plus the help dialogs made by script, the importers and exporters for draw.io and Excalidraw, PDF / SVG / DXF / web page / PNG export, Check Diagram, Compare Documents, the inventory report, borders, data graphics UI, comments, themes, stencils pane, and the MCP dialog; 1,164 UI checks, 738 engine checks, 107 MCP checks cover it.

## 2. Architecture: one engine, two hosts

The editing engine already is Rust, but it sits in the **wasm crate**: `layoutcraft-wasm` holds one `State` (the package bytes, the undo and redo history, the clipboard, the opened stencils) behind about 31 `vl_*` C-ABI functions and a page-operation dispatcher (`vl_page_op`, op numbers 1..40). The logic those functions call is in `layoutcraft-vsdx` (pageops, groupedit, draw, alttext, shapename, ...).

Step N1 (first code step): extract **`layoutcraft-engine`**, a normal Rust library with a safe API (`Engine::import / new / export / draw / page_op / undo / redo / ...`), by moving `State` and the bodies of the `vl_*` functions out of the wasm crate. Then:

* `layoutcraft-wasm` becomes a thin wrapper: a thread-local `Engine` and the C-ABI functions that call it (the web application keeps working unchanged; the JS `engine.js` does not change);
* `layoutcraft-app` uses the same `Engine` directly instead of its own patching code, so a fix or a feature (alt text, shape names, borders, ...) is written once;
* **gate for N1:** nothing may change for the web side: the wasm spike (11/11, it compares every golden model), the engine suite (738), the conformance runner (390 + 188 visual), the Visio whole run and the MCP test must give the same numbers as before.

The model JSON (`model()`) and the wasm exports must not change (the spike compares all golden models).

## 3. Porting the JS-only features

Pure functions of the drawing model are ported into the engine crate (or a `layoutcraft-tools` crate beside it) **with the JS as the oracle**: the same input gives the same output, checked by a test that runs both (Node calls the wasm build, Rust calls the library), until the JS side is retired. In this order (small, testable, needed by the MCP server too):

1. Check Diagram rules, Compare (`compareModels`), the inventory report (`inventoryData` / CSV), borders (`BORDER_STYLES`, `borderShapes`), `mcpConfig`;
2. importers and exporters: draw.io (importer, exporter, 37 shape outlines, edges), Excalidraw, DXF, PDF / SVG text export helpers (`svgpdf`, `ttf`, `pdffonts`), web page export, ZIP;
3. the UI itself: ribbon (tabs and groups as data, the same names and the same icon files `web/assets/icons/*.svg`, rendered by egui as masks), dialogs, quick access toolbar, tooltips with the same delays, the comments pane, shape data dialog, stencils pane, page setup; keyboard shortcuts as one table shared with the Help dialog.

Every ported feature keeps its test: the UI checks of the web shell are the specification for the egui screens (they cannot run against egui, so each dialog gets a Rust test of its state and a short manual check list).

## 4. The MCP server in the native app

The server is `layoutcraft-mcp` (`rust/layoutcraft-mcp`, [35](35-mcp-native.md), [32](32-mcp.md)): a native program on `layoutcraft-engine` with 49 tools, shipped next to the application (`layoutcraft-app` and `layoutcraft-mcp` are two executables in one folder). An earlier plan was a Node sidecar, and later the application itself as the server (`layoutcraft --mcp --root <folder>`); neither was built. The Node server `mcp/server.mjs` that served as the reference was removed together with the JavaScript editor ([42](42-javascript-removal.md)).

The native app's **AI assistant (MCP)** window shows the configuration (the same text as the web dialog: JSON block for Claude Desktop and other clients, the `claude mcp add` command for Claude Code) with the path of the bundled `layoutcraft-mcp` filled in, and Copy buttons.

Safety and consent: stdio only, no open port, never started by the application itself (the MCP client starts it), the drawings folder has no default (a placeholder only), and the window says that a connected assistant can change files below that folder. A button that writes the entry into a client's configuration file needs a design with an explicit confirmation of the file and the change; the first version offers Copy only.

## 5. Builds and release

* **Toolchain:** this Windows host has no MSVC linker; the app builds with `cargo +stable-x86_64-pc-windows-gnu` (docs/17 asks for exactly this check before committing the skeleton). CI builds with the platform toolchains.
* **Targets** (see the release notes): Windows x64 and arm64 (MSI + portable zip), macOS universal .dmg (aarch64 + x86_64), Linux x86_64 and aarch64 (AppImage, deb, rpm, Flatpak). No WebKitGTK or WebView2 dependency; Linux needs the usual windowing libraries (X11 / Wayland, GL / Vulkan through wgpu or glow, whichever eframe's backend is chosen).
* **Packaging without Tauri:** candidates are `cargo-packager`, `cargo-dist`, WiX (MSI), `create-dmg` / `hdiutil`, `linuxdeploy` (AppImage), `cargo-deb`, `cargo-generate-rpm`, a Flatpak manifest. Pick one chain per platform when N6 starts; they are all build tools, not dependencies of the application.
* **Signing:** as in the release notes (Developer ID + notarisation on macOS, signtool with a timestamp on Windows); the MCP program is signed and notarised like the application. SBOM (cargo cyclonedx), `cargo-deny`, `SHA256SUMS.txt`, draft releases, nightly dry run.
* **Public build only:** the desktop artefacts are built with `LAYOUTCRAFT_PUBLIC_BUILD=1` (only the project's own themes; any additional private theme module stays out of every public artefact).

## 6. Risks (from docs/17, still true)

* Accessibility and IME in egui are weaker than in a retained native toolkit or a browser; they need explicit tests from N3 on. If they prove insufficient, Iced is the named fallback (docs/17).
* The size of the port (section 3): the UI is the larger part. The engine and the pure functions are the smaller, well-testable part and should come first, because they also serve the MCP server.
* Fonts: deterministic text layout needs the bundled open font set (docs/17); the web application uses the browser's fonts.

## 7. Milestones

| Step | What | Done when |
|---|---|---|
| N0 | the existing app builds and starts with the GNU toolchain; this page and docs/17 agree | see the baseline result below |
| N1 | `layoutcraft-engine` extracted; wasm crate a thin wrapper; the app uses the engine for open / save / undo | the web numbers are unchanged (section 2) |
| N2 | the native editing slice reaches the engine's operations: draw tools, connectors, text, style, pages, layers, undo | a Rust test per operation; a manual check list |
| N3 | UI chrome: ribbon as data, dialogs, shortcuts, tooltips, QAT | the checks of section 3 step 3 |
| N4 | ports of Check Diagram / Compare / inventory / borders, then importers and exporters | each with the JS as the oracle |
| N5 | MCP: window with the configuration, and the separate `layoutcraft-mcp` program (`layoutcraft --mcp` was not built) | `test-mcp.mjs` against the binary |
| N6 | packaging, signing, CI matrix, draft release | a signed draft release on each platform |

Baseline result (N0, 2026-11-19, Windows 11 host): `cargo +stable-x86_64-pc-windows-gnu build -p layoutcraft-app --release` in `rust/` finished in 2 min 31 s (release profile, the dependencies were not cached) and gives `rust/target/release/layoutcraft-app.exe`, 43.6 MB. Started with `examples/MS Example.vsdx` the window opens (1296 x 899), the process answers, uses about 85 MB, and the screenshot shows the drawing (Block Diagram: boxes with 3-D bevel, connectors with arrowheads and dashes, the light-blue group) with the toolbar (Open, path, Load, Reload, page arrows, Fit, 100 %, Undo, Redo, Save copy, Save as, + Rectangle), the inspector (document: pages, media, shapes and connectors counts; selection) and a status bar. **N0 is done.** Not checked yet: Linux and macOS builds (they need the CI matrix, N6), behaviour of the editing functions beyond the existing app tests, and the licence inventory entry for `layoutcraft-app` (docs/17 step 6).

Progress N1 (2026-11-19): step 360 created the crate `layoutcraft-engine` with open / new / export / json / svg / undo / redo / history; the wasm crate delegates to it (the unmoved `vl_*` functions reach the engine fields through `Deref`). Remaining for N1: the edit, page-op, group, clipboard, connector and stencil functions, then the app switches to `Engine`.

Progress N1 (2026-11-20, checkpoint after step 365): the engine crate now holds open / new / export / json / svg / undo / redo, the draw tools, edit (ops 1-18), preview_move, page_op (1-40), group_op, clipboard, connectors, stencils and masters (9 tests). Gate unchanged (conformance 390 + 188, spike 11/11, hostile 25/25, MCP 107/107, UI 1165, Visio 831). Remaining for N1: the read-only info functions (N1.2f), then N1.3, the app switching to `Engine`.

Progress N1 (step 366): N1.2 is complete: every `vl_*` function of the wasm crate is a thin wrapper over `layoutcraft-engine` (10 engine tests). Remaining for N1: N1.3, the native app switches to `Engine`.

Progress N1 (step 367): the native app holds an `Engine` for open / undo / redo / export / commit; its own bytes, document and undo stacks are gone. Next (N1.3b): the edit paths use `Engine::edit` / `draw` instead of building patches in the app.

## 8. Manual check list for the native app

Start `layoutcraft-app.exe "examples/MS Example.vsdx"`. Automated: an engine test per operation (`cargo test -p layoutcraft-engine`); a mouse-driven smoke test is possible from PowerShell (SetCursorPos + mouse_event on the window, then a screenshot of the window rect; the click position is the screenshot position + 8 px).

* **Draw tools (step 369):** pick Ellipse (or any tool), drag on empty canvas: an outline follows the drag; on release the shape appears selected with handles, the inspector shows its box, the status says "Drew shape", Undo is enabled, the tool is back on Select. Undo removes it in one step. A very small drag draws nothing. Line: the shape runs from the press to the release point.
* **Connector tool (step 370):** pick Connector, press on one shape, drag to another shape and release: a connector appears between them (Connectors count +1 in the inspector), it is selected, Undo removes it in one step. Release on empty canvas: nothing is added and the status says so. Moving either shape afterwards moves the connector end with it.
* **Text and format (step 371):** select a shape, "Edit text": type several lines, toggle B / I / U or change the size, Apply: the shape shows the text with the format in one undo step; all connectors keep their routes; Undo restores text and format together.

Progress (checkpoint after step 371, 2026-11-20): N1 is done; N2.1 draw tools, N2.2 connector tool and N2.3 text + format are in the native app. Gate unchanged (conformance 390 + 188, spike 11/11, hostile 25/25, MCP 107/107, UI 1165, Visio 831). Remaining N2: style (N2.4), pages and layers (N2.5), clipboard / group / order / align / distribute / connector selection / resize via engine (N2.6).
* **Style (step 372):** select a shape: the inspector shows Style (Fill, Line, weight, Pattern). Change a value, "Apply style" becomes active; click it: the shape changes (a dashed pattern shows on its outline), Undo restores it in one step, connectors do not move.
* **Pages (step 373):** "+ Page" adds a page after the current one and shows it; "Duplicate" copies the current page (same shapes and connectors); "Rename" opens a window with the name; "<" / ">" move the page; "Delete page" removes it (not the last one); Undo reverses each in one step; the "page n / m" label and Pages count follow.
* **Layers (step 374):** the inspector lists the page's layers with V / P / L toggles. Untick V on a layer that has shapes: they disappear; tick it: they return. Select a shape and tick "Sel" on a layer: the shape is on it (untick takes it off). Type a name and "+ Layer": a new row. "x" deletes the layer, the shapes stay. Undo reverses each in one step.
* **Copy / paste / duplicate (step 375):** select a shape; "Copy" (or Ctrl+C), "Paste" (Ctrl+V): a copy appears a quarter inch right and down, selected; every further Paste goes another step; "Dup shape" or Ctrl+D copies and pastes in one go; Undo removes one paste per step; the original and all connectors stay as they were.
* **Multi-selection and arrange (step 376):** click a shape, shift-click two more: the inspector says "Arrange (3 selected)" and the extra shapes have an orange outline. Group / Ungroup, Front / Back / Fwd / Bwd, Align L C R T M B (to the first), Spread H / V (3 or more), Rot 90 and Flip H / V act on all selected shapes in one undo step; glued connectors follow. Limits: the outline of a rotated shape is the unrotated frame; dragging moves only the primary shape.
* **Connectors (step 377):** click on a connector's line: it turns yellow and the inspector offers Delete; Delete removes only that connector (Connectors count -1), Undo brings it back. Known: dragging a shape does not move other shapes; resizing keeps the app's own text scaling.

Progress (checkpoint after step 377, 2026-11-20): N2 (native editing slice) is done: draw tools, connector tool, text + format, style, pages, layers, clipboard, multi-selection with arrange, connector selection and delete; 25 engine tests; real mouse-driven tests for each tool (section 8). Gate unchanged (conformance 390 + 188, spike 11/11, hostile 25/25, MCP 107/107, UI 1165, Visio 831). Open engine improvements: reroute only the connectors glued to moved shapes; resize op with text scaling. Next: N3 UI chrome.
* **Ribbon (step 378):** the window starts with a tab strip File, Home, Insert, Draw, Design, Data, Process, Review, View, Help (the web's order) and the groups of the chosen tab. Home > Tools: click an icon (rectangle ... line), drag on the page: the shape is drawn. Commands the native app lacks are greyed; hover shows "(not in the native app yet)". Data comes from `web/ribbon.js` through `node visio-test-harness/dump-ribbon.mjs` (run it again after changing the web ribbon; `--check` verifies).
* **Home ribbon (step 379):** select a shape: Font > B / I / U toggle bold, italic, underline (the button shows the state), A- / A+ change the size by a point; Shape Styles > Fill / Line open a swatch menu; Clipboard > Cut removes the shape and Paste brings it back; Arrange > Rotate Left / Right turn the shape(s) a quarter. Each is one undo step; connectors stay as they are. With several shapes selected (shift-click) the format applies to all.
* **File, quick access, zoom (step 380):** the quick access row has Save, Undo, Redo (greyed when there is nothing to undo). File > New: a blank drawing (asks first when there are unsaved changes); File > Info: the document properties; View > Zoom In / Zoom Out / Fit / 100 %: the page zooms (status bar shows the percentage).
* **Window layout (step 381):** quick access row, ribbon tabs, the page area, the inspector on the right and a status bar with "<" / "page n / m" / ">" (page navigation), the message, "modified" and the zoom. There is no other toolbar. The window must be tall enough for the status bar (a window taller than the screen hides it behind the taskbar). File tab > Open path: type a path, Load; Reload; Save copy; Save as.
* **Help (step 382):** Help tab: Keyboard shortcuts opens the table (Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z, Ctrl+X / C / V, Ctrl+D, Delete / Backspace, Ctrl+E, Ctrl+S and the mouse rows); About shows the product, the version and the privacy line; AI assistant (MCP) shows the configuration for Claude Desktop (JSON) and the Claude Code command for the server file and drawings folder typed in the window, with Copy buttons. Check Ctrl+X on a selected shape: it disappears and Ctrl+V brings it back.
* **Page setup and comments (step 383):** Design > Page Setup: change the width / height, Apply: the page changes size (Undo restores it); Review > New Comment / Show Comments: the window lists the comments; select a shape, type a text, "Add comment": it appears with "Me", the time and the shape id. Insert > Text Box: pick the tool, drag on the page.

Progress (checkpoint after step 383, 2026-11-20): N3 (UI chrome) is done as far as the native app has features: the ribbon is generated from the web's `ribbon.js` data, the old toolbar is gone, Help / File / View / Design / Review commands are live. Gate unchanged (conformance 390 + 188, spike 11/11, hostile 25/25, MCP 107/107, UI 1165, Visio 831). Next: N4, the ports of the JS-only features with the JS as the oracle (Check Diagram first). The Visio whole run may stall: the harness retries a step once.
* **Check Diagram (step 384):** open `examples/AWS - Git to S3 Webhooks.vsdx`, Process > Verify: the window lists "not connected: shape 57 ..."; click it: the shape is selected; "Check again" / the alt-text option re-run; "Save list..." writes the CSV. Delete a connector with a loose end through its "Delete connector" button (one undo step).

Porting method (step 384): for each ported function a Node script (`visio-test-harness/dump-<name>-oracle.mjs`) stores what the WEB code returns on a set of real drawings under `rust/layoutcraft-engine/tests/oracle/`; a Rust test compares the port with it exactly; `--check` runs in `gate.ps1`. Done: Check Diagram (145 drawings), Compare (330 pairs), Inventory (145 drawings, 3,553 rows), Borders (48 designs, 12 applied). N4.1-N4.4 done; DXF export (640 pages) and web page export (145 files), Excalidraw export/import, draw.io export/import done. Next: Compare, inventory, borders, then importers / exporters.
* **Compare Documents (step 385):** Review > Compare (icon): choose a second .vsdx; the window lists added / removed / changed items; click one to go to it; "Save list..." writes the CSV. Compare a drawing with itself: "no differences found."
* **Inventory report (step 386):** Review > Reports: the table of shapes (Shape ID, Text, Shape Data columns; "All pages" adds Page); tick columns; "Save CSV..." writes all rows.
* **Borders & Titles (step 387):** Design > Borders & Titles (any of the four controls): choose a style and a title, Apply (shapes appear on a background page beneath the page; one undo step), Remove takes it off. Undo removes the border.
* **Export (step 388):** File > Export: choose DXF or SVG, "Export...", name the file; DXF opens in a CAD program / Visio (inches, layers = Visio layers). SendKeys note for tests: `~` in a typed path means Enter; use the long path form.
* **Web page export (step 389):** File > Export > Web page: one HTML file with a tab per page; links in shapes become real links. Open the file in a browser to check the tabs.
* **Checkpoint s389 (2026-11-20):** N4.1-N4.6 verified: gate PASS (7 oracles), UI 1165/0, public build clean (80 test result lines, public wasm 705/705), Visio 831/0. Next: N4.7 Excalidraw.
* **Excalidraw export (step 390):** File > Export > Excalidraw writes the current page as a .excalidraw scene (open it at excalidraw.com to check). Note: native and wasm engines can differ in the last bit of computed numbers; oracles on engine output round to 10 significant digits.
* **Excalidraw import (step 391):** File > Open: choose a .excalidraw file: it opens as a new drawing (Save as keeps it). Test files: test/excalidraw/*.excalidraw.
* **draw.io export (step 392):** File > Export > draw.io writes every page as an uncompressed .drawio (open it at app.diagrams.net). Cargo tests now build with opt-level 2 (profile.test).
* **draw.io import (step 393):** File > Open: choose a .drawio file (plain or compressed): it opens as a new drawing. Test files: test/drawio/*.drawio.
* **PDF export (steps 394-395):** File > Export > PDF: every page; pages the converter can draw are vector PDF pages (selectable text in the 14 standard fonts), others are pictures. No embedded fonts (no font files in the native app). N4 (ports with the JS as oracle) is complete: Check Diagram, Compare, Inventory, Borders, DXF, web page, Excalidraw, draw.io, PDF; 13 oracles in gate.ps1.
* **Checkpoint s395 (2026-11-20):** N4.1-N4.9 verified: gate PASS (13 oracles), UI 1165/0, public build clean (86 test result lines, public wasm 705/705), Visio 831/0. exe dropped in dist\ after each checkpoint (user request). Next: Shapes pane (stencils), then N5 MCP, N6 packaging.
* **Shapes pane (step 396):** View > Shapes: the pane lists the built-in Basic shapes; "Open stencil..." takes a .vssx (e.g. examples/Dell-Networking/*.vssx); click a master, then click on the page to place it (undo works, Esc cancels).


### Section 8 addendum (step 397): ribbon click positions changed
Tab strip is unchanged (y=68; File 34, Home 85, Insert 143, Draw 199, Design 258, Data 316, Process 376, Review 444, View 502, Help 554). The collapse chevron is at (1268,66) in a 1296 px window. Panel buttons are now in two rows (y about 91 and 115 at 1296 px, icon-only) in column-major order, so old single-row x positions no longer apply: take a fresh screenshot of the tab before clicking (View > Shapes is the highlighted "Shapes" button in the Show group, about 390,91).

* **Checkpoint s401 (2026-11-20):** N5 done: native MCP server (rust/layoutcraft-mcp, 49 tools, test-mcp.mjs 107/107 against the exe and node), Help > AI assistant uses the bundled exe, ribbon parity (2-row groups, compact mode, collapse), Shapes pane. Gate PASS (14 oracles + mcp-native), UI 1164/0, public build clean (89 result lines, wasm 705/705), Visio 831/0. exes dropped in dist\. Next: N6 packaging.

* **N6 done (2026-11-20, steps 402-406):** third-party notices, portable zip, icon/version info/manifest (windres), WiX installer definition (not built), CI/release workflow drafts (never run). Only Windows verified; macOS/Linux need portable file dialogs. Decisions open for the user: name/register check, signing certificate / Apple notarisation. Provenance items are tracked in [PROVENANCE.md](../PROVENANCE.md), [docs/14](14-v1-scope-provenance.md) and [docs/39](39-fixture-replacement.md). See docs/36-packaging.md.
