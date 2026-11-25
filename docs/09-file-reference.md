# File reference

This file used to describe every function of the JavaScript editor. That editor was removed ([42](42-javascript-removal.md); the last commit that still has it
is `c9aa852`, where the old version of this file and the code it describes can be read). What follows is the map of the repository as it is now.

## Repository root

| Path | What it is |
| --- | --- |
| `rust/` | The Cargo workspace: everything that runs (see the crates below) |
| `web/` | The browser shell: `shell.html`, `shell.js` and the modules next to it, over `layoutcraft.wasm` (built by `tools/build-web-wasm.sh`, not committed); `web/fonts` and `web/assets` |
| `tools/` | `build-web-wasm.sh` (the wasm build), `dump-ribbon.mjs` (browser shell only, see 42), `package-portable.ps1` and `installer/` (packaging, [36](36-packaging.md)) |
| `schema/` | JSON schemas of the document model, fixtures, goldens and manifests |
| `fixtures/public/` | The self-authored, redistributable fixture set ([39](39-fixture-replacement.md)) |
| `docs/` | Numbered notes; documentation only, not loaded by any program |
| `AGENTS.md`, `CONTRIBUTING.md`, `PROVENANCE.md`, `SECURITY.md` | The rules and statements for contributors, agents and users |
| `THIRD-PARTY-NOTICES.txt` | The crates of the two programs and their licence texts, written by `cargo xtask notices` |
| `examples/`, `test/`, `private/` | Not in this repository: the private corpus ([38](38-private-fixtures.md)) |

## Crates (`rust/`)

| Crate | Responsibility |
| --- | --- |
| `layoutcraft-model` | The document model (JSON, schema version pinned by the golden models) |
| `layoutcraft-vsdx` | `.vsdx` / `.vssx` packages: reading (`lib.rs`, `import.rs`, `shapes.rs`, `geometry.rs`, `styles.rs`, `masterbody.rs`, `media.rs`, `emf.rs`), the patch writer (`writer.rs`), the fresh writer (`fresh.rs`), and the edits that work on a package (`pageops.rs`, `groupedit.rs`, `stencil.rs`, `themes.rs`, `styleedit.rs`, comments, hyperlinks, guides and others, one file each) |
| `layoutcraft-render` | Scene, SVG and PNG rendering (`scene.rs`, `svg.rs`, `png.rs`, `text.rs`, `fonts.rs`) |
| `layoutcraft-engine` | The session: the open drawing, edits, undo and redo, stencils, and the exporters and importers (draw.io, Excalidraw, DXF, PDF, web page), Check Diagram, Compare, Reports, Borders; the one API both hosts call |
| `layoutcraft-wasm` | The WebAssembly binding of the engine for the browser shell |
| `layoutcraft-app` | The native egui desktop app (`main.rs`, `ribbon.rs`, `mcpinfo.rs`, `help.rs`) |
| `layoutcraft-mcp` | The MCP server `layoutcraft-mcp --root <folder>` ([32](32-mcp.md), [35](35-mcp-native.md)) |
| `layoutcraft-conformance` | Compares the core against recorded Visio results |
| `layoutcraft-fixtures` | Generator of the public fixtures |
| `xtask` | The repository tools, run as `cargo xtask <command>`: `provenance`, `notices [--check]`, `serve [port] [--host <address>]` |

## Where the modules of the JavaScript editor went

| Removed module (`js/`) | Now |
| --- | --- |
| `visio.js` (import) | `layoutcraft-vsdx` (`import.rs`, `geometry.rs`, `styles.rs`, `masterbody.rs`, `media.rs`, ...) |
| `vsdx-writer.js`, `vsdx-source.js` (patch, fresh package, ZIP, package cache) | `layoutcraft-vsdx` (`writer.rs`, `fresh.rs`); the engine keeps the open package |
| `model.js`, `schema.js` | `layoutcraft-model`, with the engine for editing and undo |
| `render.js`, `shapes.js`, `svg-path.js` | `layoutcraft-render` and `layoutcraft-vsdx` (`draw.rs`, `geometry.rs`) |
| `router.js` | `layoutcraft-vsdx` (`router.rs`, connection points in `connpoints.rs`) |
| `layout.js` | the engine |
| `interact.js`, `ui.js`, `main.js`, `file.js`, `util.js`, `index.html`, `css/app.css` | `layoutcraft-app` (egui) and the browser shell in `web/` |
| `mcp/server.mjs` | `layoutcraft-mcp` |
| `server.mjs`, `package.json` (`npm start`) | `cargo xtask serve` |

## Quick file-to-task map

| Task | Start here |
| --- | --- |
| Import appearance | `rust/layoutcraft-vsdx/src/import.rs`, `rust/layoutcraft-render/src/scene.rs` |
| Connector routing | `rust/layoutcraft-vsdx/src/router.rs` |
| Save patch / fresh | `rust/layoutcraft-vsdx/src/writer.rs`, `rust/layoutcraft-vsdx/src/fresh.rs` |
| Editing, undo, export | `rust/layoutcraft-engine/src/lib.rs` |
| Desktop UI | `rust/layoutcraft-app/src/main.rs`, `rust/layoutcraft-app/src/ribbon.rs` |
| Browser shell | `web/shell.html`, `web/shell.js` |
| MCP tools | `rust/layoutcraft-mcp/src/lib.rs`, `rust/layoutcraft-mcp/tests/oracle/tools-list.json` |
| Provenance wording check | `rust/xtask/src/provenance/rules.rs` |
| Windows startup | [02](02-windows-setup.md) |
| Visio validation | [06](06-testing.md) |
| Remaining work | [07](07-known-gaps.md) |
