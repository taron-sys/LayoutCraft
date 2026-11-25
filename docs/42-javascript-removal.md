# 42 - Removal of the JavaScript editor

- **Status:** done (change note)
- **Date:** 2026-11-25
- **Last commit with the removed code:** `c9aa852` (`git show c9aa852:js/visio.js`, `git ls-tree -r c9aa852 -- js mcp css`)

## What was removed

| Removed | Replaced by |
| --- | --- |
| `js/` (16 modules: importer, writer, model, renderer, router, auto-layout, interaction, UI, file helpers), `index.html`, `css/app.css` | the Rust core (`layoutcraft-vsdx`, `layoutcraft-model`, `layoutcraft-render`, `layoutcraft-engine`), the native app (`layoutcraft-app`) and the browser shell (`web/`); the file-by-file map is [09](09-file-reference.md) |
| `mcp/server.mjs` (the Node MCP server) | `layoutcraft-mcp` (`rust/layoutcraft-mcp`), the same 49 tools, [32](32-mcp.md), [35](35-mcp-native.md) |
| `server.mjs`, `package.json` (`npm start`) | `cargo xtask serve` |
| `tools/check-provenance.mjs` | `cargo xtask provenance` |
| `tools/gen-third-party.mjs` | `cargo xtask notices [--check]` |

That is about 16,000 lines of JavaScript, HTML and CSS (the editor and the two Node servers), and the two Node build scripts. Nothing else in this repository imports them: a search of the Rust crates, `web/`, `tools/` and CI finds no use of the editor modules.

## Why

The editor was frozen (`AGENTS.md`, rule 10): new import and export behaviour went into the Rust crates, and the Rust core had taken over reading, patching and writing `.vsdx`, the renderer, the exporters and the MCP server
([18](18-rust-core.md) to [21](21-fresh-writer.md), [34](34-desktop-build.md), [35](35-mcp-native.md)). What was left was a second implementation that had to be kept running (Node, npm), patched for every
review and checked for provenance, and that the maintainers did not extend. The Node MCP server was kept as a reference for the Rust one; the Rust server has passed the parity run (107 of 107 checks, [35](35-mcp-native.md)) and owns its tool list
(`rust/layoutcraft-mcp/tests/oracle/tools-list.json`) now.

## What changed around it

- **Help > AI assistant (MCP)** in the desktop app and in the browser shell shows the configuration for the `layoutcraft-mcp` program (`command` = its path, `args` = `--root <folder>`). It no longer asks for a Node script.
  The browser shell ignores a remembered path that ends in `.js` or `.mjs` (it was the old server).
- `cargo xtask <command>` runs from `rust/` (alias in `rust/.cargo/config.toml`). The commands use the standard library, `serde_json` and `sha2`, which the workspace already had, so a machine that builds offline needs nothing new.
- CI runs `cargo xtask provenance` and `cargo xtask notices --check` instead of the two Node scripts and no longer installs Node.
- `AGENTS.md` rules 10 and 11, `CONTRIBUTING.md`, `SECURITY.md` and the docs that named the removed files were updated; docs 04, 05, 12 and 13 describe the old editor and got a note at the top instead.

### The tools

`cargo xtask provenance`: the same checks as the Node script, rule by rule (the phrases it rejects are in `rust/xtask/src/provenance/rules.rs`, each with its regular expression in a comment and its
allow exception and edge cases in the unit tests). It searches the files that git lists (`git ls-files -co --exclude-standard`; without git it walks the tree), skips binary and large files, and checks that no private
path or third-party drawing is tracked and that the provenance files exist. Output lines and the exit code are unchanged. Two differences: the sources of the tool itself are on the skip list (they contain the phrases as test
cases), and a file that git lists but that is gone from the working tree is skipped, where the Node script stopped with a stack trace.

`cargo xtask notices [--check]`: the same crate closure, licence classification and licence-text search as the Node script, and the same two output files. `LC_TARGET` and `CARGO_TOOLCHAIN` mean what they meant (an empty
`CARGO_TOOLCHAIN` passes no `+toolchain` argument, an unset one means `+stable-x86_64-pc-windows-gnu`). Differences: the `generated` field of `docs/36-licences.json` says `by cargo xtask notices`; an unknown argument
is an error instead of being ignored; and the licence files of a crate are read in file-name (byte) order. The Node script used the order in which the operating system lists a folder, which is case-insensitive on Windows and
byte order on Linux. That is why "Third-party notices are current" failed in CI on Linux for a file generated on Windows: the font licence texts of `epaint_default_fonts` came in another order. The committed
`THIRD-PARTY-NOTICES.txt` was regenerated with the byte order, which every platform now gives.

`cargo xtask serve [port] [--host <address>]`: a static file server for the repository root (default 127.0.0.1:8137, so `/web/shell.html` works), with the content types of the old server and `Cache-Control: no-cache`. It
differs from `server.mjs` where that was weak: it listens on loopback only unless `--host` is given (the Node server listened on all interfaces); it serves GET and HEAD only; it refuses `..`, absolute and drive-letter
paths, backslashes, NUL, stream and device names, hidden files such as `.git`, and links that lead out of the repository; a bad percent escape is a 400 (the Node server answered 500); and it bounds threads and time. Details:
[02](02-windows-setup.md).

### How the port was checked

Before the Node scripts were deleted, old and new ran side by side. For `provenance` on copies of the working tree (git index only, nothing committed): the real tree, a tree with one violation of each rule and its allow
exception, a tree with 240,000 generated lines built from fragments of every rule and near misses (control, no-break and byte-order-mark whitespace, astral characters, word boundaries), a tree with tracked private paths
and a tracked Visio file, a tree with a missing required file and a missing icon provenance line, and the same without git (the directory walk). The outputs, including the exit code, were identical. For `notices` the generated
files were compared byte for byte with `cmp` (against the Node script reading folders in byte order, as it does on Linux). The unit tests of the crate cover every rule with its allow exception and boundary cases.

## What the Rust side does not have yet

Found by searching the Rust crates and the browser shell for what the editor's commands and files did. These were in the JavaScript editor only:

- **Re-layout** (layered auto-layout of a page or the selection, top-down or left-to-right, groups as units). There is no auto-layout in the engine, the app, the shell or the MCP tools.
- **Autosave** of the open drawing in the browser's local storage and its restore at the next start.
- **The editor's own JSON file** (Export JSON and Import JSON of `*.layoutcraft.json`, [13](13-document-schema.md)). The Rust model reads and writes the JSON (`Document::from_json`, the MCP resource `layoutcraft://model`),
  but neither the app nor the shell opens or saves such a file.

The list comes from the commands of the editor's command switch and its file helpers; the inspector and the canvas were not compared screen by screen. Features that other documents list as gaps of the Rust side
([07](07-known-gaps.md), [34](34-desktop-build.md)) are not repeated here.

## The JavaScript that remains

- `web/*.js` and `web/shell.html`: the browser shell and the browser-side modules next to it (ribbon, dialogs, the draw.io, Excalidraw, DXF and PDF helpers that the engine also has in Rust).
- `tools/dump-ribbon.mjs`: dumps the shell's ribbon into `rust/layoutcraft-app/src/ribbon-web.json`, the data the native ribbon is built from (needs Node, only when the ribbon of the shell changes). `tools/build-web-wasm.sh` builds the wasm the shell loads.

Both belong to the browser shell, which is replaced in a later change: it, `tools/dump-ribbon.mjs` and the wasm binding go when the egui app runs in the browser. Until then the shell stays a thin layer over the wasm build (`AGENTS.md`, rule 10).

### Ribbon commands of the browser shell that the desktop app does not have

The native ribbon is built from the shell's ribbon (`ribbon-web.json`) and maps a command to an action in `native()` in `rust/layoutcraft-app/src/ribbon.rs`. These 61 of the 106 commands of the shell have no
action there today (command id, by tab and group):

- **File**: Export (`copyImgBtn`)
- **Home**: Clipboard (`painter`); Font (`p:fontFamily`, `p:fontSize`, `p:bold`, `p:strike`); Paragraph (`p:alignL`, `p:indLeft`, `p:lineSp`); Tools (`drawTools`, `connPointTool`, `editPointsBtn`, `fitCurveBtn`); Shape Styles (`p:fillColor`, `p:lineColor`, `p:shadow`); Arrange (`arrange`, `sameW`, `sameH`, `sameWH`, `reverseEnds`, `lockShape`, `opUnion`, `opCombine`, `opFragment`, `opIntersect`, `opSubtract`, `opJoin`, `opTrim`); Editing (`stylesBtn`, `changeShapeBtn`, `findBtn`, `layersBtn`)
- **Insert**: Pages (`pageTools`); Illustrations (`insertPicture`, `pictureFile`); Links (`insertHyperlink`)
- **Design**: Page Setup (`designOrientation`, `designSize`); Themes (`themeGallery`); Variants (`variantGallery`); Backgrounds (`paperColor`)
- **Data**: Show/Hide (`dataBtn`)
- **Review**: Compare (`compareFile`)
- **View**: Views (`fullScreen`, `print`, `fixedPage`); Show (`showRulers`, `showGuides`, `showBreaks`, `paneSizePos`, `paneSheet`, `panePanZoom`); Zoom (`zl`, `zoomWidth`, `wheelZoom`); Visual Aids (`autoConnectToggle`, `refit`); Palette (`paletteSel`, `tipDelay`); Window (`newWindow`)
