# 32. MCP server (Model Context Protocol)

`mcp/server.mjs` lets an AI assistant (Claude Code, Claude Desktop, any MCP client) create and edit LayoutCraft drawings. It drives the same engine as the web shell
(`web/engine.js` on `web/layoutcraft.wasm`), so every format the shell handles works here too. No dependencies, no network, no shell: stdio only, one JSON-RPC 2.0 message per line
(protocol revisions 2025-06-18, 2025-03-26, 2024-11-05; spec: modelcontextprotocol.io).

## Start

```
node mcp/server.mjs --root <folder>
```

Files are read and written only below `<folder>` (default: the current directory); a path outside it is refused. The drawing lives in memory until `save`.
Node 20 or newer. The wasm must be built (`visio-test-harness/build-web-wasm.sh`, or `... public` for the public build; both work).

Client configuration (Claude Code `.mcp.json`, Claude Desktop `claude_desktop_config.json`):

```json
{ "mcpServers": { "layoutcraft": { "command": "node", "args": ["C:/path/to/LayoutCraft/mcp/server.mjs", "--root", "C:/path/to/my/drawings"] } } }
```

## Units

Inches. The origin is the lower left corner of the page and y grows upward (the tools convert from the engine's pixels). Page indexes start at 0.

## Tools

| Tool | What it does |
|---|---|
| `new_drawing` | empty drawing, `width`, `height` (inches), `metric`, `name` |
| `open` / `save` | `.vsdx`, `.drawio`, `.excalidraw` (open) and `.vsdx` (save), below the root |
| `export` | `svg`, `dxf` (one page), `drawio` (all pages), `excalidraw` (one page); with `path` it writes the file, without it returns the text |
| `list_pages`, `list_shapes` | pages with sizes and shape counts; shapes with id, type, x, y, w, h, rotation, text, fill, line |
| `draw_shape` | `kind` rectangle / rounded / ellipse / diamond / triangle / textbox / line, corners `x0 y0 x1 y1`, optional `text` and `style` |
| `add_connector` | glue a connector between `from` and `to` (`id`, `side` 0 whole shape / 1 top / 2 right / 3 bottom / 4 left) |
| `set_text`, `style_shapes`, `delete_shapes` | text, style (fill, line, weight, pattern, fontPt, fontFamily, textColor, bold, italic, underline, fillTrans, lineTrans, shadow, beginArrow, endArrow), delete |
| `move_shapes`, `rotate_shapes`, `flip_shapes` | move by (dx, dy) inches, rotate by degrees counter-clockwise, mirror |
| `align_shapes`, `distribute_shapes` | left / center / right / top / middle / bottom to the first shape; even spacing of three or more |
| `group_shapes`, `ungroup_shape`, `reorder_shapes` | group (returns the group id), ungroup, front / back / forward / backward |
| `list_connectors` | the connectors of a page and what they are glued to |
| `set_shape_name` | name a shape (Shape.Name); unique on the page (`.<shape id>` is added to a taken name); empty = automatic name; list_shapes shows `name` |
| `set_alt_text` | alt text of a shape (Shape > Alt Text); an empty text removes it; list_shapes shows `altText` |
| `set_hyperlinks`, `set_shape_data` | replace a shape's links (address, subAddress, description); write a Shape Data row |
| `add_page`, `rename_page`, `duplicate_page`, `delete_page`, `set_page_size` | pages (the last page stays) |
| `add_layer`, `list_layers`, `set_layer`, `assign_layer`, `delete_layer` | layers by name: rename, show / hide, print, lock, colour; put shapes on a layer or take them off |
| `check_diagram` | problems on a page: loose connector ends, duplicate connectors, shapes nothing connects to, overlaps, shapes outside the page, and with `altText: true` shapes with neither text nor alt text; changes nothing ; `allPages: true` checks every page (each problem then has a page index); `csv: path` also writes the list as a CSV file (Page, Problem, Shape, Message, Fix) |
| `inventory` | the Report of the application: a row per shape (page, shape id, text, one column per Shape Data name; `columns` picks and orders them, `allPages`, optional `csv` file) |
| `find_shapes` | find shapes by a part of the text, alt text, name or Shape Data value (`query`), or by a Shape Data row (`dataName`, optional exact `dataValue`); one page or `allPages`; the rows of `list_shapes` plus the page index |
| `add_border`, `remove_border` | Design > Borders & Titles: a border (Frame, Double frame, Frame with title block, Title bar) as a background page beneath a page, and off again. The model shows the background as part of the page, so `list_shapes` then lists the border shapes too |
| `get_page` | one page in one answer: index, name, size, shape and connector counts, layers, the assigned background page |
| `compare_files` | compare two drawing files below the root (.vsdx, .drawio, .excalidraw) without touching the open drawing: added / removed / changed, optional CSV file; ids of .drawio / .excalidraw files shift when shapes are added or removed |
| `insert_picture` | a PNG / JPEG / GIF / BMP file below the root as a picture shape in the box (x0, y0)-(x1, y1) inches |
| `open_stencil`, `drop_master` | open a stencil (a .vssx / .vsdx below the root, or the built-in Basic shapes without a path) and drop one of its masters with its centre at (x, y) inches |
| `shape_operation` | union, combine, fragment, intersect, subtract, join, trim (the first shape is the one the others work on) |
| `list_themes`, `apply_theme` | the gallery (whatever the build has) and applying a theme to the drawing or one page |
| `undo`, `redo` | one change set |

Resources: `layoutcraft://page/<n>/svg` (the page picture) and `layoutcraft://model` (the drawing model as JSON).
Errors (bad input, a path outside the root, an unknown shape) come back as a tool result with `isError: true`, never as a crash.

## Test

`node visio-test-harness/test-mcp.mjs` (also step 5 of the gate) starts the server as a child process and talks to it over stdio (84 checks: handshake, tool list, a drawing with a connector, styles, the exports, a
save / open round trip for .vsdx and .drawio, themes, pages, undo / redo, resources, every refusal).

## Not yet

Connector routing options, PNG / PDF export (they need a canvas) (they need a browser canvas), a long-lived multi-document mode.

## Connecting a client from the application, and desktop builds

* **Help > AI assistant (MCP)** (step 358) opens a dialog with the configuration for a client: type the path of `mcp/server.mjs` and the drawings folder, copy the JSON block (Claude Desktop: Settings > Developer > Edit Config, then restart it) or the `claude mcp add` command (Claude Code). The entries are remembered in the browser. The server itself is started by the client, never by the page.
* The desktop builds (Windows, macOS, Linux) are the **native egui application** (docs/17, decision of 2026-11-19; nothing of the MCP part is built yet). It gets its own AI assistant window with the same configuration text. First the server runs as a sidecar program next to the application, later the application itself is the server (`layoutcraft --mcp --root <folder>`); `test-mcp.mjs` is the parity test. The plan, the shared engine, the build matrix and the milestones: **docs/34-desktop-build.md**.
