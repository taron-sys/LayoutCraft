#!/usr/bin/env bash
# Disk image and tarball from the (signed) LayoutCraft.app (docs/36-packaging.md).
#   tools/packaging/macos/make-dmg.sh [<LayoutCraft.app>]
# Writes $LC_DIST/LayoutCraft-<version>-macos-universal.dmg (the app, a link to /Applications, the notices and licences) and
# $LC_DIST/LayoutCraft-<version>-macos-universal.tar.gz (the two programs taken from the app, so they carry its signature, plus the notices).
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

version="$(lc_version)"
app="${1:-${LC_APP_DIR:-$LC_DIST/macos-app}/LayoutCraft.app}"
[ -d "$app" ] || lc_die "$app does not exist (run make-app.sh first)"
base="LayoutCraft-$version-macos-universal"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$LC_DIST"

readme="$work/README-macos.txt"
cat > "$readme" << EOF
LayoutCraft $version (macOS, Apple silicon and Intel)

Drag LayoutCraft.app to the Applications folder. Needs macOS 11 or newer.

This build is not notarised by Apple (the project has no Apple Developer ID yet), so macOS asks before the first start:
- macOS 15 and newer: try to open the app once, then System Settings > Privacy & Security > "Open Anyway".
- older macOS: Control-click the app, Open.
- or remove the download mark in Terminal:  xattr -dr com.apple.quarantine /Applications/LayoutCraft.app

AI assistants (MCP): LayoutCraft.app/Contents/MacOS/layoutcraft-mcp is the server an AI client (for example Claude) starts itself; it needs no
node and opens no network port. Help > AI assistant in the app shows the configuration with the right path. The tar.gz has both programs
without the app bundle.

THIRD-PARTY-NOTICES.txt lists the libraries inside the programs with their licences. PROVENANCE.md states what the project's provenance rules
allow and what this package does not contain. LICENSE-MIT and LICENSE-APACHE: LayoutCraft is available under either licence.
EOF

# ---- disk image ----
dmg_root="$work/dmg"
mkdir -p "$dmg_root"
cp -R "$app" "$dmg_root/LayoutCraft.app"
ln -s /Applications "$dmg_root/Applications"
lc_stage_docs "$dmg_root"
cp "$readme" "$dmg_root/"
rm -f "$LC_DIST/$base.dmg"
for attempt in 1 2 3; do
  # hdiutil sometimes fails with "Resource busy" on the runners; a second try works
  if hdiutil create -volname LayoutCraft -srcfolder "$dmg_root" -fs HFS+ -format UDZO -ov "$LC_DIST/$base.dmg"; then break; fi
  [ "$attempt" -lt 3 ] || lc_die "hdiutil create failed three times"
  sleep 5
done

# ---- tarball of the raw programs ----
tar_root="$work/tar/$base"
mkdir -p "$tar_root"
cp "$app/Contents/MacOS/layoutcraft-app" "$app/Contents/MacOS/layoutcraft-mcp" "$tar_root/"
lc_stage_docs "$tar_root"
cp "$readme" "$tar_root/"
lc_archive targz "$LC_DIST/$base.tar.gz" "$tar_root"
ls -l "$LC_DIST/$base.dmg" "$LC_DIST/$base.tar.gz"
