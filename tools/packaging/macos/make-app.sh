#!/usr/bin/env bash
# Assembles LayoutCraft.app from the universal programs built by build.sh (docs/36-packaging.md). The bundle is NOT signed yet: run sign.sh next.
#   tools/packaging/macos/make-app.sh            writes $LC_APP_DIR/LayoutCraft.app (default $LC_DIST/macos-app)
# Contents/MacOS has layoutcraft-app (the program the bundle starts) and layoutcraft-mcp (the MCP server of the AI assistant feature, next to it as the
# app expects), Contents/Resources the icon (AppIcon.icns, made here from rust/resources/layoutcraft-256.png with sips and iconutil) and the notices.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

version="$(lc_version)"
bundle_version="${version%%-*}" # CFBundleVersion is numeric: 0.1.0-rc1 becomes 0.1.0
bin="${LC_BIN_DIR:-${CARGO_TARGET_DIR:-$LC_ROOT/rust/target}/universal-apple-darwin/release}"
app_dir="${LC_APP_DIR:-$LC_DIST/macos-app}"
app="$app_dir/LayoutCraft.app"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/layoutcraft-app" "$bin/layoutcraft-mcp" "$app/Contents/MacOS/"
chmod 755 "$app/Contents/MacOS/"*
sed "s/@VERSION@/$bundle_version/g" "$LC_ROOT/tools/packaging/macos/Info.plist.in" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" > /dev/null
lc_stage_docs "$app/Contents/Resources"

# AppIcon.icns: the sizes up to 256 px from the committed 256 px PNG (no larger source exists; macOS scales it up where it needs more)
src="$LC_ROOT/rust/resources/layoutcraft-256.png"
iconset="$work/AppIcon.iconset"
mkdir -p "$iconset"
for entry in "16 icon_16x16" "32 icon_16x16@2x" "32 icon_32x32" "64 icon_32x32@2x" "128 icon_128x128" "256 icon_128x128@2x" "256 icon_256x256"; do
  size="${entry%% *}"
  name="${entry#* }"
  sips -z "$size" "$size" "$src" --out "$iconset/$name.png" > /dev/null
done
iconutil --convert icns --output "$app/Contents/Resources/AppIcon.icns" "$iconset"

lc_log "assembled $app"
echo "$app"
