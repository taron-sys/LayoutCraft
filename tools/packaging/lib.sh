#!/usr/bin/env bash
# Shared helpers of the packaging scripts (docs/36-packaging.md). Source it, do not run it.
# shellcheck shell=bash
set -euo pipefail

LC_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LC_DIST="${LC_DIST:-$LC_ROOT/dist}"
export LC_ROOT LC_DIST

# versions.env: KEY=VALUE lines. Values already in the environment win (the workflow exports them).
while IFS='=' read -r key value; do
  case "$key" in '' | '#'*) continue ;; esac
  if [ -z "${!key:-}" ]; then export "$key=$value"; fi
done < "$LC_ROOT/tools/packaging/versions.env"

lc_log() { printf '==> %s\n' "$*" >&2; }
lc_die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# lc_crate_version <Cargo.toml>: the version of the [package] table.
lc_crate_version() {
  awk '/^\[package\]/ { in_pkg = 1; next } /^\[/ { in_pkg = 0 } in_pkg && /^version[[:space:]]*=/ { gsub(/^[^"]*"|".*$/, ""); print; exit }' "$1"
}

# lc_version: the release version; both programs must carry the same one.
lc_version() {
  local app mcp
  app="$(lc_crate_version "$LC_ROOT/rust/layoutcraft-app/Cargo.toml")"
  mcp="$(lc_crate_version "$LC_ROOT/rust/layoutcraft-mcp/Cargo.toml")"
  [ -n "$app" ] || lc_die "no version in rust/layoutcraft-app/Cargo.toml"
  [ "$app" = "$mcp" ] || lc_die "layoutcraft-app ($app) and layoutcraft-mcp ($mcp) have different versions"
  printf '%s' "$app"
}

# lc_pkg_version <version>: the spelling of a pre-release in .deb and .rpm versions (0.1.0-rc1 -> 0.1.0~rc1, so it sorts before 0.1.0).
lc_pkg_version() { printf '%s' "${1//-/\~}"; }

# lc_epoch: timestamp for reproducible archives (the commit time; SOURCE_DATE_EPOCH wins).
lc_epoch() {
  if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then printf '%s' "$SOURCE_DATE_EPOCH"; else git -C "$LC_ROOT" log -1 --format=%ct 2> /dev/null || date +%s; fi
}

# lc_build_env: the environment of every release build.
#  - LAYOUTCRAFT_PUBLIC_BUILD=1: no private modules, exactly what the public repository builds.
#  - strip=symbols for the release profile (smaller files, no symbol names). It is set here and not in rust/Cargo.toml, so developers' release builds keep theirs.
#  - --remap-path-prefix: the build machine's home and checkout paths do not end up in the binaries (rustc embeds source paths of dependencies).
#    CARGO_ENCODED_RUSTFLAGS (separator 0x1f) so a path with spaces survives. It replaces RUSTFLAGS; nothing else sets rustflags here.
#  - RUST_FONTCONFIG_DLOPEN: fontconfig is loaded at run time (the fontique feature already asks for that), so no libfontconfig-dev is needed to build, also when cross-compiling.
lc_build_env() {
  export LAYOUTCRAFT_PUBLIC_BUILD=1
  export CARGO_PROFILE_RELEASE_STRIP=symbols
  export RUST_FONTCONFIG_DLOPEN=1
  local us=$'\x1f' cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=${cargo_home}=/cargo${us}--remap-path-prefix=${LC_ROOT}=/layoutcraft"
}

# lc_stage_docs <dir>: the files every package carries.
lc_stage_docs() {
  local f
  for f in THIRD-PARTY-NOTICES.txt LICENSE-MIT LICENSE-APACHE PROVENANCE.md; do
    [ -f "$LC_ROOT/$f" ] || lc_die "$f is missing from the repository root"
    cp "$LC_ROOT/$f" "$1/"
  done
}

# lc_archive zip|targz <out> <dir>: deterministic archive of <dir> (entries below its own name), see archive.py.
lc_archive() { python3 "$LC_ROOT/tools/packaging/archive.py" "$1" "$2" "$3" --epoch "$(lc_epoch)"; }

# lc_download <url> <sha256> <out>: download and refuse anything that does not match the pinned hash.
lc_download() {
  local url="$1" sha="$2" out="$3" got
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 --output "$out" "$url"
  if command -v sha256sum > /dev/null 2>&1; then got="$(sha256sum "$out" | cut -d' ' -f1)"; else got="$(shasum -a 256 "$out" | cut -d' ' -f1)"; fi
  if [ "$got" != "$sha" ]; then rm -f "$out"; lc_die "sha256 of $url is $got, expected $sha"; fi
}
