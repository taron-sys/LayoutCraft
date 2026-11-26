# 48. Features and file formats

What LayoutCraft does today. For what is missing or inexact, see [Known gaps](07-known-gaps.md).

## Status

- Early software, released as pre-releases and developed in the open.
- The desktop app and the MCP server share one Rust editing engine (`layoutcraft-engine`); the browser build runs the same
  engine compiled to WebAssembly.
- Fidelity with files made by Microsoft Visio is incomplete. Some notes in this folder predate the move to Rust and still
  describe the old JavaScript editor, so check the code when in doubt.

## Files

- Open: `.vsdx`, `.vstx`, `.vsdm`, `.vstm`, draw.io (`.drawio`), Excalidraw (`.excalidraw`). Stencils (`.vssx`) open in
  the Shapes pane.
- The older `.vsd` / `.vss` / `.vst` (Visio 2000-2010) and `.vdx` / `.vsx` / `.vtx` files are converted to a new drawing when
  opened (desktop app and MCP server); the app says what could not be carried over ([47](47-legacy-formats.md)).
- Save: `.vsdx`. The writer patches the original package, so parts it does not edit are kept as they were.
- Export from the desktop app (File > Export): SVG, PDF (all pages), DXF, draw.io (all pages), Excalidraw, and a single
  HTML web page with a tab per page. The browser shell also exports PNG (and SVG/PNG per page in a ZIP) and can copy the
  selection as an image.

## Editing (desktop app)

- Draw and move shapes, resize, rotate, flip, group/ungroup, change z-order, align and distribute, duplicate,
  cut/copy/paste, undo/redo, snapping to grid, shapes and guides, pan and zoom.
- Connectors that stay glued to shapes, and text editing on shapes. Draw them with the connector tool (Ctrl+3) between
  shapes or the empty page, or from the arrows shown around the shape under the pointer (View > Add and Connect Arrows):
  drag an arrow to connect, click it to place a connected shape. Arrowheads are under Home > Shape Styles > Line ends;
  Reverse Ends is in Home > Arrange.
- Pages: add, duplicate, rename, delete, reorder; page setup; borders and titles; layers; document properties.
- A Shapes pane with a built-in "Basic shapes" stencil plus stencils you open.
- Check Diagram (finds loose connectors, overlaps and similar problems), Compare File, Reports (inventory of shapes and
  Shape Data), and comments.
- Visio containers and swimlanes are imported and rendered; the desktop ribbon has no dedicated container command.
- View > Palette: the window is dark, light or follows the operating system (dark when the system does not say). The drawing
  keeps its own colours; those are Design > Themes and Variants.
- Keyboard shortcuts are listed under Help > Keyboard shortcuts.

## Browser version

A few commands are not in the desktop app yet. They appear greyed out in its ribbon with the hover text
"not in the native app yet". PNG export and copy as image are in the browser shell. The browser shell does not offer the older
`.vsd` / `.vdx` formats in its file pickers yet.

## AI assistant

The [MCP server](32-mcp.md) exposes drawing tools to any MCP client. The desktop app's
Help > AI assistant (MCP) window shows the client configuration.
