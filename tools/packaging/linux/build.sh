#!/usr/bin/env bash
# Release build of the two Linux programs for one architecture (docs/36-packaging.md).
#   tools/packaging/linux/build.sh x86_64|aarch64
# Both are built on an x86_64 Ubuntu runner: aarch64 is cross-compiled with gcc-aarch64-linux-gnu (apt: gcc-aarch64-linux-gnu, libc6-dev-arm64-cross) and the
# Rust target aarch64-unknown-linux-gnu. GitHub's free arm64 runners exist only for public repositories, and nothing in the build needs a library of the
# target system (the window system, OpenGL, xkbcommon and fontconfig are loaded at run time), so cross-compiling is the more robust way.
# The result is in <target dir>/<triple>/release; the glibc the programs need is checked against GLIBC_MAX (the oldest runner's glibc).
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

arch="${1:?usage: build.sh x86_64|aarch64}"
case "$arch" in
  x86_64) triple=x86_64-unknown-linux-gnu; machine='Advanced Micro Devices X86-64' ;;
  aarch64) triple=aarch64-unknown-linux-gnu; machine='AArch64' ;;
  *) lc_die "unknown architecture \"$arch\" (x86_64 or aarch64)" ;;
esac
GLIBC_MAX="${GLIBC_MAX:-2.35}" # Ubuntu 22.04, the oldest runner image we build on; Debian 12, Fedora 36 and RHEL 9 are at or above it

lc_build_env
if [ "$arch" = aarch64 ] && [ "$(uname -m)" != aarch64 ]; then
  command -v aarch64-linux-gnu-gcc > /dev/null || lc_die "aarch64-linux-gnu-gcc is missing (apt-get install gcc-aarch64-linux-gnu libc6-dev-arm64-cross)"
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
  export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
fi

lc_log "cargo build --release --locked --target $triple"
(cd "$LC_ROOT/rust" && cargo build --release --locked --target "$triple" -p layoutcraft-app -p layoutcraft-mcp)

bin="${CARGO_TARGET_DIR:-$LC_ROOT/rust/target}/$triple/release"
for exe in layoutcraft-app layoutcraft-mcp; do
  [ -x "$bin/$exe" ] || lc_die "$bin/$exe was not built"
  # the right architecture (a cross-compile that silently used the host compiler would fail here)
  readelf -h "$bin/$exe" | grep -q "Machine:.*$machine" || lc_die "$exe is not a $arch binary"
  # no symbol newer than GLIBC_MAX (the programs must start on the oldest system we claim to support)
  newest="$(readelf --dyn-syms --wide "$bin/$exe" | grep -o 'GLIBC_[0-9][0-9.]*' | sort -uV | tail -n 1 | sed 's/^GLIBC_//')"
  if [ -n "$newest" ] && [ "$(printf '%s\n%s\n' "$GLIBC_MAX" "$newest" | sort -V | tail -n 1)" != "$GLIBC_MAX" ]; then
    lc_die "$exe needs glibc $newest, more than the $GLIBC_MAX we support"
  fi
  lc_log "$exe: $arch, needs glibc ${newest:-(no versioned symbols)} (limit $GLIBC_MAX)"
done
