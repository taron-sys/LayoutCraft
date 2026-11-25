#!/usr/bin/env bash
# Universal (arm64 + x86_64) release build of the two macOS programs (docs/36-packaging.md).
#   tools/packaging/macos/build.sh
# Both architectures are built on one Apple-silicon runner (Apple's clang targets either) and joined with lipo. The result is in
# <target dir>/universal-apple-darwin/release. Needs the Rust targets aarch64-apple-darwin and x86_64-apple-darwin.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

# The oldest macOS the programs are built to run on (the deployment target is what the linker writes into the binaries).
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
lc_build_env

targets=(aarch64-apple-darwin x86_64-apple-darwin)
target_dir="${CARGO_TARGET_DIR:-$LC_ROOT/rust/target}"
out="$target_dir/universal-apple-darwin/release"
for target in "${targets[@]}"; do
  lc_log "cargo build --release --locked --target $target"
  (cd "$LC_ROOT/rust" && cargo build --release --locked --target "$target" -p layoutcraft-app -p layoutcraft-mcp)
done

mkdir -p "$out"
for exe in layoutcraft-app layoutcraft-mcp; do
  lipo -create -output "$out/$exe" "$target_dir/${targets[0]}/release/$exe" "$target_dir/${targets[1]}/release/$exe"
  archs="$(lipo -archs "$out/$exe")"
  case "$archs" in
    *arm64*x86_64* | *x86_64*arm64*) lc_log "$exe: $archs" ;;
    *) lc_die "$exe has the architectures \"$archs\", expected arm64 and x86_64" ;;
  esac
done
echo "$out"
