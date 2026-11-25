#!/usr/bin/env bash
# Checks of the disk image made by make-dmg.sh (docs/36-packaging.md), run by the release workflow.
#   tools/packaging/macos/verify.sh
# Mounts the image, checks the bundle (Info.plist, universal programs, signature) and starts the MCP server from inside it.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

version="$(lc_version)"
dmg="$LC_DIST/LayoutCraft-$version-macos-universal.dmg"
[ -s "$dmg" ] || lc_die "$dmg is missing"
[ -s "$LC_DIST/LayoutCraft-$version-macos-universal.tar.gz" ] || lc_die "the tarball is missing"

mount="$(mktemp -d)"
trap 'hdiutil detach "$mount" -quiet -force > /dev/null 2>&1 || true' EXIT
hdiutil verify "$dmg" > /dev/null
hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mount" > /dev/null
app="$mount/LayoutCraft.app"
plutil -lint "$app/Contents/Info.plist"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")" = layoutcraft-app ] || lc_die "CFBundleExecutable is not layoutcraft-app"
for exe in layoutcraft-app layoutcraft-mcp; do
  archs="$(lipo -archs "$app/Contents/MacOS/$exe")"
  case "$archs" in *arm64*x86_64* | *x86_64*arm64*) ;; *) lc_die "$exe is not universal ($archs)" ;; esac
done
[ -s "$app/Contents/Resources/AppIcon.icns" ] || lc_die "the bundle has no icon"
for f in THIRD-PARTY-NOTICES.txt LICENSE-MIT LICENSE-APACHE PROVENANCE.md; do [ -s "$mount/$f" ] || lc_die "the disk image has no $f"; done
codesign --verify --strict --verbose=2 "$app"
# the runner is an Apple-silicon Mac: this starts the arm64 half
"$LC_ROOT/tools/packaging/smoke-mcp.sh" "$app/Contents/MacOS/layoutcraft-mcp"
lc_log "disk image is fine"
