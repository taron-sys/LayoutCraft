# 0003 — v1 desktop UI toolkit

- **Status:** Accepted for the v1 port
- **Date:** 2026-10-28
- **Scope:** Linux-first native editor
- **Related:** [v1 scope and provenance](14-v1-scope-provenance.md), [Handover brief](12-handover-rust.md)

## Decision

Use **`eframe`/`egui` 0.37** for the v1 desktop shell, panels, dialogs, property
inspectors and interaction overlays.

Use **`resvg` 0.49 + its `tiny-skia` raster backend** for a single shared
page-render path: a scene-to-SVG renderer feeds the interactive canvas texture,
SVG export, PNG/page-PNG export and visual-golden comparison. `cosmic-text` +
`fontdb` are the preferred text-measurement/shaping stack; the exact text
integration is validated in the renderer spike.

Version numbers are the versions current on 2026-10-28; pin them in
`Cargo.lock` and re-audit licences when upgrading. `eframe`/`egui` are
MIT OR Apache-2.0, `resvg` is MIT OR Apache-2.0, `tiny-skia` is BSD-3-Clause,
and `cosmic-text`/`fontdb` are MIT/Apache-compatible.

## Why egui

- Immediate-mode custom painting is a good fit for a diagram editor: selection,
  resize handles, glue hints, marquee, leader lines and connector previews are
  overlays over one scene texture.
- Native windowing/GPU backends, no webview or Linux WebKitGTK dependency.
- Permissive licence and a large, active ecosystem; supports desktop and WASM.
- `egui` handles the editor chrome (menus, inspector, palette, dialogs) without
  imposing a retained state machine on the document model.

## Why resvg for the canvas/export path

The JS implementation already renders a scene to SVG and validates page PNGs.
Keeping **one SVG scene renderer** in Rust:

- keeps interactive display, SVG export, PNG export and visual goldens on the
  same layout/style/text path;
- avoids maintaining an egui-painter renderer and a separate export renderer
  that drift;
- allows deterministic test rendering with a bundled OFL font set and
  `resvg`/`tiny-skia`;
- matches the existing visual-golden contract.

The egui canvas displays a `resvg`-rasterized page texture; selection handles,
carets and snapping feedback are egui overlays. Text editing uses an egui
editor surface with the same measured text runs; no proprietary fonts are
bundled.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Tauri / webview + existing JS UI** | Reuses the JS frontend but is not a native v1; Linux requires WebKitGTK and the renderer/UI contract remains browser-shaped. |
| **Slint** | Desktop licensing is GPLv3 or commercial; the project wants a permissive dependency set. |
| **Iced** | Viable retained-mode framework, but custom canvas interaction and immediate overlays are more work. Keep as a fallback if egui accessibility/IME proves insufficient. |
| **GTK4 / gtk-rs** | Native but a large C/GSettings stack; not the desired Rust-first UI model. |
| **winit + tiny-skia hand-rolled UI** | Maximum control but reimplements every widget, text editor and accessibility path; too much v1 cost. |

## Consequences

- The interactive canvas is a rendered texture plus egui overlays. Hit-testing
  stays in the scene model, not in the renderer.
- Accessibility, IME and advanced text editing need explicit testing; egui is
  weaker than a retained native toolkit here. This is accepted for v1 but is a
  tracked risk.
- A bundled open-licensed font is required for deterministic visual goldens;
  system-font mode can be offered separately with fallback metrics.
- `eframe`/`resvg` add a substantial dependency tree; the dependency inventory
  and licence audit must include them and their transitive crates.
- The Windows validation host currently lacks an MSVC linker; egui/winit/wgpu
  builds should be attempted with the GNU toolchain or on Linux before
  committing the app skeleton.

## Implementation order

1. `layoutcraft-model` and `layoutcraft-vsdx` (headless, manifest-driven tests) —
   done.
2. Shared scene model and scene-to-SVG renderer; verify with golden page PNGs
   via `resvg` — done (37/37 visual-golden pages pass).
3. `layoutcraft-app` shell: document lifecycle, page tabs, palette, inspector —
   **viewer + shape editing slice started**: open/reload, page navigation,
   fit/actual size, pan/zoom, hit-testing, inspector, move/resize, undo/redo and
   patch-based Save copy.
4. Canvas overlays for connector editing, shape creation/palette and grouped
   transforms.
5. Text editor/IME, accessibility and deterministic font layout.
6. Add `layoutcraft-app` to the licence inventory and CI matrix.

The headless renderer now reproduces the golden page geometry and colours
within the documented visual tolerance, so the shell work has started. It uses
the same scene → SVG → `resvg` path as export and visual goldens, which keeps
the canvas from drifting away from the tested renderer.

## Current shell slice

Run from `rust/`:

```powershell
cargo +stable-x86_64-pc-windows-gnu run -p layoutcraft-app -- "..\examples\MS Example.vsdx"
```

The window opens a VSDX/VSTX from the command line, toolbar path or drag-and-
drop; `Load`/`Reload` reread the source, `◀`/`▶` change pages, `Fit` and `100%`
reset the view, drag pans and the wheel zooms around the pointer. Clicking a
shape highlights its box and fills the inspector with its Visio ID, bounds,
part count and text.

Dragging a shape moves it; dragging a corner handle resizes it. The app patches
the shape's `PinX`/`PinY`/`Width`/`Height`/`LocPin*` (and the text box on resize)
into an in-memory package, re-imports it so parts/text stay current, and keeps
an undo/redo stack of package bytes (`Ctrl+Z`, `Ctrl+Shift+Z`/`Ctrl+Y`).
`Save copy` writes `<name>-edited.<ext>` next to the source. Reload or opening a
different file with unsaved changes asks before discarding them. Grouped
shapes are selectable but read-only; resize is disabled for rotated shapes.
Connector editing and palette/stencil insertion are next; simple rectangle
insertion and text editing are in.

The editing slice uncovered and fixed a patch-writer bug: `<Shapes>` was
counted as a nested `<Shape>` open, so shapes containing child shapes (and
`OriginalID` collisions) could not be patched or removed. The writer now matches
attribute boundaries and skips `<Shapes>` when computing the shape range.

## Reconfirmed 2026-11-19
The user chose again: the desktop build is this native egui application, no webview and no Tauri. Since this decision the web shell grew into the full editor (about 8,000 lines of JS, 1,164 UI checks); the native app is still the viewer and editing slice described above. How the two share one engine (`layoutcraft-engine`, extracted from the wasm crate), which JS features are ported to Rust and in which order, the MCP server in the native app, builds and milestones: **docs/34-desktop-build.md**.
