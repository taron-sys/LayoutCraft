# LayoutCraft

A diagram editor written in Rust that opens and saves Microsoft Visio `.vsdx` files. It ships as a native desktop app
(egui), a stdio [MCP](https://modelcontextprotocol.io) server so an AI assistant can create and edit drawings, and a
browser build.

> Microsoft and Visio are trademarks of Microsoft Corporation. LayoutCraft is not affiliated with or endorsed by Microsoft.
> "Visio" and ".vsdx" are used only to say which file format LayoutCraft is compatible with.

## Screenshots
Linux Version ( Build 0.1.0-rc.2)
<img width="2353" height="1629" alt="image" src="https://github.com/user-attachments/assets/76de3f97-f425-4452-a96b-aa1c822fec30" />

## Status

Early software (version 0.1.0 in every crate), developed in the open.

- The desktop app and the MCP server share one Rust editing engine (`layoutcraft-engine`); the browser build runs the
  same engine compiled to WebAssembly.
- The desktop app does not yet have every command of the browser shell. Commands it lacks appear greyed out in its
  ribbon with the hover text "not in the native app yet" (see [What it does](#what-it-does)).
- Fidelity with files made by Microsoft Visio is incomplete. The backlog is in
  [`docs/07-known-gaps.md`](docs/07-known-gaps.md); some docs under `docs/` predate the move to Rust and still describe
  the old JavaScript editor, so check the code when in doubt.

## Install

Downloads are published on the
[GitHub Releases page](https://github.com/taron-sys/LayoutCraft/releases). Planned files per release:

| Platform | Files |
| --- | --- |
| Windows x64 and ARM64 | `.zip` (portable), `.msi` |
| macOS (universal) | `.dmg`, `.tar.gz` |
| Linux x64 and ARM64 | `.tar.gz`, `.deb`, `.rpm`, AppImage |
| Browser | `.zip` (static files; see [Browser build](#browser-build)) |

The release pipeline is new. If the page lists no release yet, [build from source](#building-from-source).

The builds are not code-signed, so your system will warn you:

- **Windows:** SmartScreen shows "Windows protected your PC". Choose **More info**, then **Run anyway**.
- **macOS:** Control-click (right-click) the app and choose **Open**, then confirm. Or remove the quarantine flag:
  `xattr -d com.apple.quarantine /path/to/LayoutCraft.app` (use the path of the file you downloaded or installed).
- **Linux:** make an AppImage executable (`chmod +x`) before running it. `.deb` and `.rpm` install with your package manager.

## What it does

**Files**

- Open: `.vsdx`, `.vstx`, `.vsdm`, `.vstm`, draw.io (`.drawio`), Excalidraw (`.excalidraw`). Stencils (`.vssx`) open in
  the Shapes pane.
- The older `.vsd` / `.vss` / `.vst` (Visio 2000-2010) and `.vdx` / `.vsx` / `.vtx` files are converted to a new drawing when
  opened (desktop app and MCP server); the app says what could not be carried over ([docs/47](docs/47-legacy-formats.md)).
- Save: `.vsdx`. The writer patches the original package, so parts it does not edit are kept as they were.
- Export from the desktop app (File > Export): SVG, PDF (all pages), DXF, draw.io (all pages), Excalidraw, and a single
  HTML web page with a tab per page. The browser shell also exports PNG (and SVG/PNG per page in a ZIP) and can copy the
  selection as an image.

**Editing (desktop app)**

- Draw and move shapes, resize, rotate, flip, group/ungroup, change z-order, align and distribute, duplicate,
  cut/copy/paste, undo/redo, snapping to grid, shapes and guides, pan and zoom.
- Connectors that stay glued to shapes, a connector tool, and text editing on shapes.
- Pages: add, duplicate, rename, delete, reorder; page setup; borders and titles; layers; document properties.
- A Shapes pane with a built-in "Basic shapes" stencil plus stencils you open.
- Check Diagram (finds loose connectors, overlaps and similar problems), Compare File, Reports (inventory of shapes and
  Shape Data), and comments.
- Visio containers and swimlanes are imported and rendered; the desktop ribbon has no dedicated container command.
- Keyboard shortcuts are listed under Help > Keyboard shortcuts.

**Browser shell only (not yet in the desktop app):** the theme gallery, PNG export, copy as image, and the other ribbon
commands that are greyed out natively. The MCP server can list and apply themes.

**AI assistant:** the MCP server (below) exposes drawing tools to any MCP client. The desktop app's
Help > AI assistant (MCP) window shows the client configuration.

## Building from source

You need a current stable Rust toolchain (<https://rustup.rs>). The workspace is in [`rust/`](rust/).

```bash
cd rust
cargo build --release -p layoutcraft-app -p layoutcraft-mcp
cargo run --release -p layoutcraft-app                  # start the editor
cargo run --release -p layoutcraft-app -- drawing.vsdx  # or open a file
```

Binaries land in `rust/target/release/` (`layoutcraft-app`, `layoutcraft-mcp`). Set `LAYOUTCRAFT_PUBLIC_BUILD=1` to
build exactly what the public repository contains (see [Testing](#testing)).

**Linux** needs these system packages (the list CI uses on Ubuntu):

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev \
  libxi-dev libgl1-mesa-dev libegl1-mesa-dev libgtk-3-dev
```

**Windows:** if the checkout path is very long, the linker can fail on the default `target` directory. Point Cargo at a
short one, for example `set CARGO_TARGET_DIR=C:\lct` (cmd) or `$env:CARGO_TARGET_DIR = "C:\lct"` (PowerShell).

## Browser build

The browser build is the Rust engine compiled to WebAssembly behind a small page in [`web/`](web/). The shell in `web/`
is still JavaScript today; it is to be replaced by the egui app compiled to WebAssembly.

```bash
rustup target add wasm32-unknown-unknown
tools/build-web-wasm.sh public     # writes web/layoutcraft.wasm
cd rust && cargo xtask serve       # http://127.0.0.1:8137/web/shell.html
```

`tools/build-web-wasm.sh` defaults to a Windows GNU toolchain (`cargo +stable-x86_64-pc-windows-gnu`). Elsewhere run it
as `CARGO_TOOLCHAIN= tools/build-web-wasm.sh public` to use your default toolchain. The `public` argument leaves out
private modules; use it for anything you publish.

## MCP server

`layoutcraft-mcp` speaks JSON-RPC 2.0 over stdio (one message per line): no network port, no shell. It is started by
your MCP client, not by the app. Files are read and written only below the folder given with `--root` (default: the
current directory); paths outside it are refused. Drawings are held in memory until `save`.

```bash
rust/target/release/layoutcraft-mcp --root /path/to/my/drawings
```

Client configuration (Claude Desktop `claude_desktop_config.json`, Claude Code `.mcp.json`):

```json
{ "mcpServers": { "layoutcraft": {
    "command": "/path/to/layoutcraft-mcp",
    "args": ["--root", "/path/to/my/drawings"] } } }
```

An assistant connected this way can change files below the root, so choose that folder with care. Tools include
creating, opening, saving and exporting drawings, drawing and styling shapes, connectors, pages, layers, themes,
stencils, Check Diagram, find, compare and inventory; units are inches. Tool list and details:
[`docs/32-mcp.md`](docs/32-mcp.md) and [`docs/35-mcp-native.md`](docs/35-mcp-native.md).

## Project layout

| Path | What it is |
| --- | --- |
| `rust/layoutcraft-model` | Typed, portable document model (schema v1) |
| `rust/layoutcraft-vsdx` | VSDX package reader, importer and writers (patching and fresh) |
| `rust/layoutcraft-render` | Scene, SVG and PNG renderer (`resvg`) |
| `rust/layoutcraft-engine` | The editing engine shared by all hosts: undo/redo, clipboard, stencils, draw.io / Excalidraw import, PDF / DXF / SVG / HTML export, Check Diagram, Compare |
| `rust/layoutcraft-app` | Desktop app (`eframe` / egui) |
| `rust/layoutcraft-mcp` | MCP server (`layoutcraft-mcp`) |
| `rust/layoutcraft-wasm` | WebAssembly build of the engine for the browser |
| `rust/layoutcraft-conformance` | Golden-fixture checker ([`rust/README.md`](rust/README.md)) |
| `rust/layoutcraft-fixtures` | Generates the self-authored public fixtures in [`fixtures/public/`](fixtures/public/README.md) |
| `web/` | Browser shell (JavaScript, to be replaced) |
| `tools/` | Build, packaging and provenance scripts |
| `docs/` | Design notes and handover docs ([index](docs/README.md)) |

## Testing

```bash
cd rust
cargo test --workspace
cargo run -p layoutcraft-conformance -- --root .. --manifest ../fixtures/public/manifest.json
```

CI sets `LAYOUTCRAFT_PUBLIC_BUILD=1` so tests run against only what the public repository contains; set it locally to
reproduce that. Some extra tests use a private corpus of Visio-made files that is not in this repository
([`docs/38-private-fixtures.md`](docs/38-private-fixtures.md)); they are skipped when it is absent. The public fixtures
are generated by this project ([`docs/39-fixture-replacement.md`](docs/39-fixture-replacement.md)).

Other developer commands, run from `rust/`:

```bash
cargo xtask provenance       # provenance checks
cargo xtask notices --check  # verify THIRD-PARTY-NOTICES.txt is up to date
```

## Contributing

Read [`AGENTS.md`](AGENTS.md) first; it applies to people and AI agents alike. See also
[`CONTRIBUTING.md`](CONTRIBUTING.md) and [`PROVENANCE.md`](PROVENANCE.md). The key rules: do not submit code, assets or
text copied from Microsoft Visio or other products, do not read GPL/LGPL/AGPL source to learn how something is done, and
only add test files you made yourself. `PROVENANCE.md` states what the project does and does not claim about its sources.
Report security problems privately, as described in [`SECURITY.md`](SECURITY.md).

## Licence

MIT OR Apache-2.0, at your option: [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE) (see also
[`LICENSE`](LICENSE)). Third-party Rust crates and their licences are listed in
[`THIRD-PARTY-NOTICES.txt`](THIRD-PARTY-NOTICES.txt). The browser shell bundles open-licensed fonts (Liberation,
Carlito, Caladea) with their licence files in `web/fonts/`.
