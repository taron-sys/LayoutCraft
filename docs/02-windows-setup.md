# Running on Windows

The JavaScript editor that this page used to start (`npm start`, `server.mjs`) was removed ([42](42-javascript-removal.md)). What runs now is the Rust core: the native
app, and the browser shell over the core compiled to WebAssembly.

## Prerequisites

- Windows 10 or 11
- Rust, the current stable toolchain, through `rustup`. The maintainers' machine has no MSVC linker and builds with `cargo +stable-x86_64-pc-windows-gnu`
  (the scripts that name a toolchain default to it: set `CARGO_TOOLCHAIN=` to the empty string to use your default toolchain instead). The MSVC toolchain works too;
  the MSVC linker fails on very long paths, so keep the repository at a short path or point `CARGO_TARGET_DIR` at a short one.
- Git for Windows (it brings the `sh` that runs `tools/build-web-wasm.sh`)
- For the browser shell: the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`) and a current Chrome or Edge
- Microsoft Visio 2021/365 for the compatibility checks
- No Node.js and no `npm install`

## Get the files

Clone or copy the whole `LayoutCraft` folder to a local path, for example `C:\src\LayoutCraft`. Avoid a network share or a path with unusual characters.
The `examples/` folder (the private test corpus, see [38](38-private-fixtures.md)) is filled in separately; the redistributable set is `fixtures/public/`.

## The desktop app

```powershell
cd C:\src\LayoutCraft\rust
cargo run --release -p layoutcraft-app
```

The release build of both programs is `cargo build --release -p layoutcraft-app -p layoutcraft-mcp`; they are `rust\target\release\layoutcraft-app.exe` and `layoutcraft-mcp.exe`
([36](36-packaging.md) packages them). Use `CARGO_TARGET_DIR=<a short folder>` if the linker complains about the path length.

## The browser shell

The shell is `web/shell.html` with the modules next to it. It needs `web/layoutcraft.wasm`, which is not committed:

```sh
cd /c/src/LayoutCraft
CARGO_TOOLCHAIN= sh tools/build-web-wasm.sh public    # the build that may be published; leave CARGO_TOOLCHAIN= out on the maintainers' machine
cd rust
cargo xtask serve
```

Open <http://127.0.0.1:8137/web/shell.html> in Chrome or Edge. `cargo xtask serve [port] [--host <address>]` takes a port (or the `PORT` variable); if 8137 is busy it says so and
`cargo xtask serve 8138` picks another. A server is required because browsers do not run ES modules or fetch the wasm from `file://` pages.

### What the server does

`cargo xtask serve` replaces the Node script `server.mjs`. It serves the repository root, so `/web/shell.html` is the page; it maps a few extensions to a type (`.html`, `.js`, `.mjs`, `.css`, `.json`,
`.svg`, `.png`, `.ico`, `.wasm` as `application/wasm`; anything else `application/octet-stream`), sends `Cache-Control: no-cache`, answers a folder without a trailing slash with a redirect and a folder
with its `index.html`, and answers 404 for what is not there. It is made for development, not for the network:

- it listens on 127.0.0.1 only, unless you pass `--host` (`--host 0.0.0.0` shares the files of the repository with everyone who can reach the machine; it says so when it starts);
- it answers GET and HEAD only (405 otherwise) and, while bound to a loopback address, only requests whose `Host` is an IP address or `localhost` (a web page on another site cannot read it by pointing a name at your machine);
- it refuses (403) a path with `..`, a backslash, a drive letter or stream name (`C:`, `file:stream`), a Windows device name (`NUL`, `COM1`, ...), a hidden file or folder (`.git`, `.claude`), and a
  link that leads out of the repository; a bad percent escape or a NUL is a 400;
- one thread per connection, at most 64 at a time, a time limit for the request head and for the response.

## Source control

Do not commit generated `.vsdx` outputs or browser test folders; `.gitignore` already lists `web/layoutcraft.wasm`, the private folders and the build output.

## First-run smoke test

1. Start the app (or the browser shell) and open a `.vsdx` with the Open command; `fixtures/public/sources/` has self-authored ones.
2. Check that the page renders: shapes, connectors and labels.
3. Save a copy (`Ctrl+S` in the app, which writes a copy; the browser shell downloads the file).
4. Open the saved file in Microsoft Visio and check that it opens without a repair dialog.

## Working with Visio on Windows

- Browser downloads go to the normal Downloads folder. Move them out before comparing or opening several versions with the same name.
- Office may show a Protected View or "Enable Editing" bar. That is normal for a downloaded file; enable editing before checking whether Visio performs a repair.
- Watch for these Visio behaviours and record them:
  - "Visio found an error... repair" / "repaired" dialogs
  - page tabs missing or duplicated
  - connectors detached from shapes
  - text shifted, clipped or missing
  - shapes visibly different from the LayoutCraft canvas
  - Save/Save As in Visio succeeding, then re-open in LayoutCraft
- Do not overwrite the original source `.vsdx` while testing; compare against saved copies.

## File types on Windows

- `.vsdx`, `.vstx`, `.vssx` are ZIP/OPC packages. You can rename a copy to `.zip` and use Explorer or `Expand-Archive` to inspect the XML.
- The older `.vsd` and `.vdx` formats are converted to `.vsdx` when opened in the desktop app or the MCP server ([47](47-legacy-formats.md)); the browser shell's file pickers do not offer them yet.

## Browser choice

Chrome and Edge are the primary targets for the browser shell. The wasm core does the file work, so the shell needs no more of the browser than the page itself.

## Troubleshooting

| Symptom | Likely cause | Action |
| --- | --- | --- |
| Blank page / module error | Opened `web/shell.html` via `file://` | Use `cargo xtask serve` and `http://127.0.0.1:8137/web/shell.html` |
| 404 for `/web/layoutcraft.wasm` | The wasm is not built | `sh tools/build-web-wasm.sh public` |
| `cargo xtask serve` says the port is in use | Another program has 8137 | `cargo xtask serve 8138` |
| 403 for a page you know exists | A hidden name (`.x`), or a `Host` that is not local | Use `127.0.0.1` or `localhost` in the address |
| The linker fails on a long path | MSVC linker limit | Short repository path or a short `CARGO_TARGET_DIR` |
| Import fails with "Not a ZIP archive" | Not a Visio drawing | See [47](47-legacy-formats.md) for the formats that are read |
| Save opens in Visio with a repair dialog | Visio compatibility gap | Record the repair message and the exact test file ([07](07-known-gaps.md)) |

## Security and licensing notes

- The programs are local and make no network calls (the dev server listens on loopback only).
- No Microsoft binaries or Microsoft stencil files are part of this repository.
- The generated VSDX files are ordinary documents. Test them with your own Visio licensing before distributing them.
