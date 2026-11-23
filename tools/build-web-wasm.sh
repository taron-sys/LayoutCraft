#!/bin/sh
# Build the Rust core for the browser and copy it next to the shell page (web/layoutcraft.wasm).
# Default: the full build (with the private theme module when private/themes-extra exists).
# `build-web-wasm.sh public`: exactly what the public repository builds (the private module is left out); use it for anything that gets published.
cd "$(dirname "$0")/../rust" || exit 1
if [ "$1" = "public" ]; then export LAYOUTCRAFT_PUBLIC_BUILD=1; echo "PUBLIC build (no private modules)"; fi
# the maintainers' Windows machine pins a toolchain (cargo +stable-x86_64-pc-windows-gnu); elsewhere set CARGO_TOOLCHAIN= to use the default
TC="${CARGO_TOOLCHAIN-+stable-x86_64-pc-windows-gnu}"
# Keep the maintainer's build paths (home folder, user name) out of the wasm: rustc embeds source paths of dependencies in panic and debug strings.
# CARGO_ENCODED_RUSTFLAGS (separator 0x1f) so a path with spaces survives; cygpath gives the Windows spelling rustc sees under Git Bash.
winpath() { cygpath -w "$1" 2>/dev/null || printf '%s' "$1"; }
CARGO_DIR="$(winpath "${CARGO_HOME:-$HOME/.cargo}")"
REPO_DIR="$(winpath "$(cd .. && pwd)")"
US="$(printf '')"
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=${CARGO_DIR}=/cargo${US}--remap-path-prefix=${REPO_DIR}=/layoutcraft"
cargo $TC build -p layoutcraft-wasm --release --target wasm32-unknown-unknown || exit 1
cp target/wasm32-unknown-unknown/release/layoutcraft_wasm.wasm ../web/layoutcraft.wasm && echo "web/layoutcraft.wasm updated"
