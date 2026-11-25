#!/usr/bin/env bash
# The browser build as a .zip of static files (docs/36-packaging.md): the web shell (web/, files tracked by git) and web/layoutcraft.wasm built
# from the public sources. Writes $LC_DIST/LayoutCraft-<version>-web.zip.
#   tools/packaging/web/package.sh              (LC_SKIP_WASM_BUILD=1 reuses an existing web/layoutcraft.wasm)
# Needs the Rust target wasm32-unknown-unknown.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

version="$(lc_version)"
base="LayoutCraft-$version-web"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "${LC_SKIP_WASM_BUILD:-}" != 1 ]; then
  # "public": exactly what the public repository builds (no private modules). CARGO_TOOLCHAIN empty: the toolchain of the runner, not the
  # maintainers' pinned one. The script also remaps the build paths out of the file.
  (cd "$LC_ROOT" && CARGO_TOOLCHAIN='' CARGO_BUILD_FLAGS='--locked' sh tools/build-web-wasm.sh public)
fi
wasm="$LC_ROOT/web/layoutcraft.wasm"
[ -s "$wasm" ] || lc_die "$wasm is missing or empty"
[ "$(head -c 4 "$wasm" | od -An -c | tr -d ' ')" = '\0asm' ] || lc_die "$wasm is not a WebAssembly module"
# the module loads through the shell's own loader (the same call the page makes); a build that does not instantiate is not released
if command -v node > /dev/null 2>&1; then
  (cd "$LC_ROOT" && node --input-type=module -e "
    import { loadEngine } from './web/engine.js'
    import { readFileSync } from 'node:fs'
    const engine = await loadEngine(readFileSync('web/layoutcraft.wasm'))
    if (typeof engine.import !== 'function') throw new Error('the engine has no import()')")
else
  lc_log "node is not installed: the wasm module was not loaded as a check"
fi

dir="$work/$base"
mkdir -p "$dir"
# the shell: only what git tracks in web/ (no local leftovers), then the wasm next to it
git -C "$LC_ROOT" ls-files -z web | while IFS= read -r -d '' file; do
  rel="${file#web/}"
  mkdir -p "$dir/$(dirname "$rel")"
  cp "$LC_ROOT/$file" "$dir/$rel"
done
cp "$wasm" "$dir/layoutcraft.wasm"
[ -f "$dir/shell.html" ] || lc_die "web/shell.html is not tracked by git"
cp "$dir/shell.html" "$dir/index.html" # so that a static host serves the app at the folder's address
lc_stage_docs "$dir"
cat > "$dir/README-web.txt" << EOF
LayoutCraft $version (browser build)

Serve this folder with any static web server and open it in a current browser (index.html and shell.html are the same page).
The page loads layoutcraft.wasm, which browsers only do over http(s): opening the file from the disk (file://) does not work.
  for example:  python3 -m http.server 8080   then open http://localhost:8080/

The fonts in fonts/ are under the SIL Open Font License 1.1 (the licence texts are in that folder). THIRD-PARTY-NOTICES.txt lists the Rust
libraries inside layoutcraft.wasm with their licences. PROVENANCE.md states what the project's provenance rules allow and what this package
does not contain. LICENSE-MIT and LICENSE-APACHE: LayoutCraft is available under either licence.
EOF
lc_archive zip "$LC_DIST/$base.zip" "$dir"
ls -l "$LC_DIST/$base.zip"
