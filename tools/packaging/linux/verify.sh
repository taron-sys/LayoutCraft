#!/usr/bin/env bash
# Checks of the Linux packages made by package.sh (docs/36-packaging.md), run by the release workflow after packaging.
#   tools/packaging/linux/verify.sh x86_64|aarch64
# Every architecture: the packages are readable and have the files and dependencies they should. On x86_64 (the runner's own architecture) the .deb is
# also installed and the installed MCP server is started, and the AppImage is unpacked and its MCP server started. The aarch64 packages cannot be run here.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

arch="${1:?usage: verify.sh x86_64|aarch64}"
version="$(lc_version)"
base="$LC_DIST/LayoutCraft-$version-linux-$arch"
for ext in tar.gz deb rpm AppImage; do [ -s "$base.$ext" ] || lc_die "$base.$ext is missing"; done

want_files='usr/bin/layoutcraft-app usr/bin/layoutcraft-mcp usr/share/applications/layoutcraft.desktop usr/share/mime/packages/layoutcraft.xml usr/share/icons/hicolor/256x256/apps/layoutcraft.png usr/share/doc/layoutcraft/THIRD-PARTY-NOTICES.txt usr/share/doc/layoutcraft/LICENSE-MIT usr/share/doc/layoutcraft/LICENSE-APACHE usr/share/doc/layoutcraft/PROVENANCE.md'

lc_log ".deb contents"
deb_list="$(dpkg-deb --contents "$base.deb" | awk '{ print $NF }')"
for f in $want_files; do printf '%s\n' "$deb_list" | grep -qx "\./$f" || lc_die ".deb has no $f"; done
dpkg-deb --field "$base.deb" Package Version Architecture Depends

lc_log ".rpm contents"
rpm_list="$(rpm -qpl "$base.rpm")"
for f in $want_files; do printf '%s\n' "$rpm_list" | grep -qx "/$f" || lc_die ".rpm has no /$f"; done
rpm -qpi "$base.rpm" | grep -E '^(Name|Version|Architecture|License)'
rpm -qp --requires "$base.rpm"

lc_log ".tar.gz contents"
# list first, then search: `tar | grep -q` fails under pipefail when grep exits early and tar gets SIGPIPE
listing=$(tar -tzf "$base.tar.gz")
grep -q '/layoutcraft-mcp$' <<<"$listing" || lc_die "the tarball has no layoutcraft-mcp"

lc_log ".AppImage header"
file "$base.AppImage"

if [ "$arch" = "$(uname -m)" ]; then
  lc_log "install the .deb"
  sudo apt-get install -y --no-install-recommends "$base.deb"
  "$LC_ROOT/tools/packaging/smoke-mcp.sh" /usr/bin/layoutcraft-mcp
  # the shared libraries the program links must all be there; what it loads later (window system, OpenGL) is not a link-time dependency
  if ldd /usr/bin/layoutcraft-app | grep 'not found'; then lc_die "layoutcraft-app has unresolved shared libraries"; fi
  desktop-file-validate /usr/share/applications/layoutcraft.desktop

  lc_log "unpack the AppImage"
  scratch="$(mktemp -d)"
  (cd "$scratch" && "$base.AppImage" --appimage-extract > /dev/null)
  "$LC_ROOT/tools/packaging/smoke-mcp.sh" "$scratch/squashfs-root/usr/bin/layoutcraft-mcp"
  [ -L "$scratch/squashfs-root/AppRun" ] || lc_die "the AppImage has no AppRun"
fi
lc_log "packages of $arch are fine"
