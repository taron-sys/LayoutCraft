# Running on Windows

## Prerequisites

- Windows 10 or 11
- Node.js 18 or newer (Node 20/22 recommended)
- Current Google Chrome or Microsoft Edge
- Microsoft Visio 2021/365 for the compatibility checks
- Optional: Python 3.10+ if you prefer `python -m http.server` over Node
- Optional: Git for Windows (recommended before further development)

No npm dependencies are installed by this project. `npm install` is not needed.

## Get the files

Copy the whole `LayoutCraft` folder to a local path, for example:

```text
C:\src\LayoutCraft
```

Keep `examples/` with the app; they are the manual test corpus. Keep `docs/`
with the project for handover continuity.

Avoid running the app from a network share or from a path containing
unusual characters; the static server and browser file dialogs are simpler
with a normal local path.

If the folder arrived as a ZIP, extract it first, then optionally unblock the
files:

```powershell
Get-ChildItem -Recurse C:\src\LayoutCraft | Unblock-File
```

## Start the server

### Node

```powershell
cd C:\src\LayoutCraft
npm start
```

Expected output:

```text
LayoutCraft running at http://localhost:8137
```

Open <http://localhost:8137/> in Chrome or Edge.

If port 8137 is busy:

```powershell
npm start -- 8138
# or
node server.mjs 8138
```

### Python alternative

```powershell
cd C:\src\LayoutCraft
python -m http.server 8137
```

Then open <http://localhost:8137/>.

### Why a server is required

`index.html` loads ES modules from `js/`. Browsers do not allow module scripts
from `file://` pages. Use the local server.

## The static server on Windows

`server.mjs` was changed during the handover pass to use:

- `fileURLToPath(import.meta.url)` instead of `new URL(...).pathname` — the
  latter produces `/C:/...` on Windows.
- normalized relative paths with the leading URL slash stripped before
  resolving against the project root.

It serves the repository root, maps unknown extensions to
`application/octet-stream`, sends `Cache-Control: no-cache`, and returns 404
for missing files. The server was tested on Linux after the change but not on
Windows itself. If startup fails on Windows, run `node server.mjs` from the
project folder and report the exact error; there is no build step that could
hide it.

## Source control

The handover copy was not a Git repository. Initialise it manually before
making changes, for example:

```powershell
cd C:\src\LayoutCraft
git init
git add .
# create the first commit manually
```

Do not commit generated `.vsdx` outputs or browser test folders; add them to
`.gitignore` as needed.

## First-run smoke test

1. Open <http://localhost:8137/>. A sample network diagram should appear.
2. Open `examples/MS Example.vsdx` with the folder button or `Ctrl+O`.
3. Check the page renders: title, 3-D blocks, arrows and labels.
4. Press `Ctrl+Shift+S` or click the VSDX toolbar button.
5. A file named `MS Example-edited.vsdx` should download.
6. Open that downloaded file in Microsoft Visio.

The first Visio test is the highest-value action in this handover.

## Where browser data lives

- **Autosave:** localStorage, key `layoutcraft.document.v1`.
- **Original VSDX package cache:** IndexedDB, database `layoutcraft-vsdx`,
  object store `packages`.
- **Undo history:** in memory only; it is lost on reload.
- **Open document:** if a source package is cached, reloading restores the
  package from IndexedDB. If not, the document can still be saved as a fresh
  VSDX from the model.

To start completely clean, use DevTools → Application → Storage → Clear site
data, or run in a private window. Clearing site data removes the cached original
package, so VSDX save will fall back to a fresh rebuild.

## Working with Visio on Windows

- Save downloads go to the browser's normal Downloads folder. Move them out
  before comparing or opening several versions with the same name.
- Office may show a Protected View or "Enable Editing" bar. That is normal for
  a downloaded file; enable editing before checking whether Visio performs a
  repair.
- Watch for these Visio behaviours and record them:
  - "Visio found an error... repair" / "repaired" dialogs
  - page tabs missing or duplicated
  - connectors detached from shapes
  - text shifted, clipped or missing
  - shapes visibly different from the LayoutCraft canvas
  - Save/Save As in Visio succeeding, then re-import into LayoutCraft
- Do not overwrite the original source `.vsdx` while testing. Keep the original
  in `examples/` and compare against downloaded copies.

## File types on Windows

- `.vsdx`, `.vstx`, `.vsdm`, `.vstm` are ZIP/OPC packages. You can rename a copy
  to `.zip` and use Explorer or `Expand-Archive` to inspect the XML.
- `.vdx` is a single XML file.
- `.vsd` is not supported by LayoutCraft.

## Browser choice

Chrome and Edge are the primary targets. Both provide the required
`DecompressionStream`, `CompressionStream`, DOMParser and IndexedDB APIs.

Firefox may import if its current version supports the required streams, but
the writer can fall back to stored ZIP entries only if `CompressionStream` is
missing. The importer still needs `DecompressionStream` for normal deflated
VSDX entries.

## Optional automated testing

There is no test runner committed. The Linux development used Playwright with a
local Chromium, driving the UI by:

- selecting an example with `#fileInput`
- interacting with `window.layoutcraft`
- reading the document model
- downloading the VSDX and re-importing it
- opening the result with LibreOffice

If you add Playwright on Windows, keep it as a dev-only dependency and do not
make the app runtime depend on it. The repository currently has no
`node_modules` folder.

## Troubleshooting

| Symptom | Likely cause | Action |
| --- | --- | --- |
| Blank page / module error | Opened `index.html` via `file://` | Use `npm start` and `http://localhost:8137/` |
| `DecompressionStream` error | Unsupported browser | Use current Chrome/Edge |
| Import fails with "Not a ZIP archive" | File is `.vsd` or not a VSDX | `.vsd` is unsupported |
| "Could not open file: ..." alert | Malformed/unsupported package | Check the exact message; test the original in Visio |
| Save says "No original package..." | No source package and no rebuild happened | Current code rebuilds; this message belongs to older Phase A builds |
| Save downloads but Visio repairs it | Visio compatibility gap | Record the repair message and the exact test file |
| Save works but reload lost package | Site data/IndexedDB cleared | Reopen the original or save a fresh VSDX |
| Port 8137 in use | Another dev server | Use `npm start -- 8138` |

## Security and licensing notes

- The app is local and makes no network calls.
- No Microsoft binaries or Microsoft stencil files are part of this repository.
- The generated VSDX files are ordinary documents. Test them with your own
  Visio licensing before distributing them.
