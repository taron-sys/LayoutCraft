#!/usr/bin/env bash
# Linux packages of the built programs (docs/36-packaging.md): .tar.gz, .deb, .rpm and .AppImage.
#   tools/packaging/linux/package.sh x86_64|aarch64 [tar] [deb] [rpm] [appimage]     (default: all four)
# Run build.sh first. Writes $LC_DIST/LayoutCraft-<version>-linux-<arch>.{tar.gz,deb,rpm,AppImage}.
# All four are made from ONE staged file tree (the layout under /usr), so they cannot drift apart. The .deb needs dpkg-deb, the .rpm rpmbuild
# (apt: rpm) and the AppImage a download of appimagetool (version and SHA-256 pinned in versions.env). Nothing here needs the target architecture
# to be the machine's: the packages only contain files, nothing is run.
# LC_BIN_DIR overrides the folder with the built programs (for trying the packaging with other files).
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

arch="${1:?usage: package.sh x86_64|aarch64 [tar] [deb] [rpm] [appimage]}"
shift
formats=("$@")
[ "${#formats[@]}" -gt 0 ] || formats=(tar deb rpm appimage)
case "$arch" in
  x86_64) triple=x86_64-unknown-linux-gnu; deb_arch=amd64 ;;
  aarch64) triple=aarch64-unknown-linux-gnu; deb_arch=arm64 ;;
  *) lc_die "unknown architecture \"$arch\" (x86_64 or aarch64)" ;;
esac

here="$LC_ROOT/tools/packaging/linux"
version="$(lc_version)"
pkgver="$(lc_pkg_version "$version")"
bin="${LC_BIN_DIR:-${CARGO_TARGET_DIR:-$LC_ROOT/rust/target}/$triple/release}"
base="LayoutCraft-$version-linux-$arch"
summary="Diagram editor that opens .vsdx files"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$LC_DIST"

# ---- the staged tree: what an installed package looks like ----
root="$work/root"
doc="$root/usr/share/doc/layoutcraft"
install -Dm755 "$bin/layoutcraft-app" "$root/usr/bin/layoutcraft-app"
install -Dm755 "$bin/layoutcraft-mcp" "$root/usr/bin/layoutcraft-mcp"
install -Dm644 "$here/layoutcraft.desktop" "$root/usr/share/applications/layoutcraft.desktop"
install -Dm644 "$here/layoutcraft-mime.xml" "$root/usr/share/mime/packages/layoutcraft.xml"
install -Dm644 "$LC_ROOT/rust/resources/layoutcraft-256.png" "$root/usr/share/icons/hicolor/256x256/apps/layoutcraft.png"
install -Dm644 "$LC_ROOT/rust/resources/layoutcraft.svg" "$root/usr/share/icons/hicolor/scalable/apps/layoutcraft.svg"
mkdir -p "$doc"
lc_stage_docs "$doc"
chmod 644 "$doc"/*
if command -v desktop-file-validate > /dev/null 2>&1; then
  desktop-file-validate "$root/usr/share/applications/layoutcraft.desktop"
else
  lc_log "desktop-file-validate is not installed: the .desktop file was not validated"
fi

make_tar() {
  local dir="$work/tar/$base"
  mkdir -p "$dir/share"
  cp "$root/usr/bin/layoutcraft-app" "$root/usr/bin/layoutcraft-mcp" "$dir/"
  cp "$doc"/* "$dir/"
  cp -r "$root/usr/share/applications" "$root/usr/share/icons" "$root/usr/share/mime" "$dir/share/"
  cat > "$dir/README-linux.txt" << EOF
LayoutCraft $version (Linux $arch)

Start:  ./layoutcraft-app  (or: ./layoutcraft-app drawing.vsdx)
The app needs a desktop session (Wayland or X11) with OpenGL, and libxkbcommon; every desktop distribution has them.
Built on Ubuntu 22.04: it needs glibc 2.35 or newer (Debian 12, Ubuntu 22.04, Fedora 36, RHEL 9 and later).

AI assistants (MCP): layoutcraft-mcp is the server an AI client (for example Claude) starts itself; it needs no node and opens no network port.
Open Help > AI assistant in the app for the configuration (keep layoutcraft-app and layoutcraft-mcp in the same folder).

share/ has the desktop entry, icons and MIME types of the drawing files for the menu and the file manager. To install them for your user:
  cp share/applications/layoutcraft.desktop ~/.local/share/applications/   (and edit Exec= to the full path of layoutcraft-app)
  cp -r share/icons/hicolor ~/.local/share/icons/ ; cp share/mime/packages/layoutcraft.xml ~/.local/share/mime/packages/
  update-mime-database ~/.local/share/mime
The .deb, .rpm and .AppImage packages do all of this themselves.

THIRD-PARTY-NOTICES.txt lists the libraries inside the programs with their licences.
PROVENANCE.md states what the project's provenance rules allow and what this package does not contain.
LICENSE-MIT and LICENSE-APACHE: LayoutCraft is available under either licence.
EOF
  lc_archive targz "$LC_DIST/$base.tar.gz" "$dir"
}

make_deb() {
  command -v dpkg-deb > /dev/null || lc_die "dpkg-deb is missing"
  local pkg="$work/deb" size
  cp -a "$root" "$pkg"
  # Debian wants a copyright file in the documentation folder; the licence texts are next to it
  cat > "$pkg/usr/share/doc/layoutcraft/copyright" << EOF
LayoutCraft is available under either of the licences MIT or Apache-2.0: see LICENSE-MIT and LICENSE-APACHE in this folder.
The libraries linked into the programs have their own licences, listed in THIRD-PARTY-NOTICES.txt.
EOF
  chmod 644 "$pkg/usr/share/doc/layoutcraft/copyright"
  size="$(du -sk "$pkg/usr" | cut -f1)"
  mkdir -p "$pkg/DEBIAN"
  # The programs load libxkbcommon and OpenGL / EGL when they start, the window-system libraries according to the session, so they are not in
  # the shared-library list of the binaries and the dependencies are written out. Built on Ubuntu 22.04: glibc 2.35.
  cat > "$pkg/DEBIAN/control" << EOF
Package: layoutcraft
Version: $pkgver
Architecture: $deb_arch
Maintainer: LayoutCraft contributors <noreply@github.com>
Installed-Size: $size
Depends: libc6 (>= 2.35), libgcc-s1, libxkbcommon0, libegl1 | libgl1
Recommends: libwayland-client0, libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0, libfontconfig1, xdg-desktop-portal
Section: graphics
Priority: optional
Homepage: https://github.com/taron-sys/LayoutCraft
Description: $summary
 LayoutCraft is a diagram editor that opens and saves .vsdx files.
 This package has the desktop app (layoutcraft-app) and the MCP server
 (layoutcraft-mcp) that an AI assistant can start to read and edit drawings.
EOF
  # reproducible: the files carry the commit time
  find "$pkg" -exec touch -h -d "@$(lc_epoch)" {} +
  dpkg-deb --root-owner-group -Zxz --build "$pkg" "$LC_DIST/$base.deb" > /dev/null
}

make_rpm() {
  command -v rpmbuild > /dev/null || lc_die "rpmbuild is missing (apt-get install rpm)"
  mkdir -p "$work/rpm"
  rpmbuild -bb "$here/layoutcraft.spec" --target "$arch-linux" \
    --define "_topdir $work/rpm" \
    --define "_binary_payload w9.gzdio" \
    --define "lc_version $pkgver" \
    --define "lc_root $root" \
    --define "lc_summary $summary" > "$work/rpmbuild.log" 2>&1 || { cat "$work/rpmbuild.log" >&2; lc_die "rpmbuild failed"; }
  cp "$work/rpm/RPMS/$arch/layoutcraft-$pkgver-1.$arch.rpm" "$LC_DIST/$base.rpm"
}

make_appimage() {
  local tool="$work/appimagetool" runtime="$work/runtime-$arch" appdir="$work/AppDir" sha_var
  local url="https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-x86_64.AppImage"
  # the tool is an x86_64 program for both architectures: it only packs files, and the runtime for the target architecture is given explicitly
  [ "$(uname -m)" = x86_64 ] || lc_die "the AppImage is built on an x86_64 machine"
  lc_download "$url" "$APPIMAGETOOL_SHA256_X86_64" "$tool"
  chmod +x "$tool"
  sha_var="APPIMAGE_RUNTIME_SHA256_${arch^^}"
  lc_download "https://github.com/AppImage/type2-runtime/releases/download/$APPIMAGE_RUNTIME_VERSION/runtime-$arch" "${!sha_var}" "$runtime"
  cp -a "$root" "$appdir"
  ln -s usr/bin/layoutcraft-app "$appdir/AppRun"
  cp "$here/layoutcraft.desktop" "$appdir/layoutcraft.desktop"
  cp "$LC_ROOT/rust/resources/layoutcraft-256.png" "$appdir/layoutcraft.png"
  cp "$LC_ROOT/rust/resources/layoutcraft-256.png" "$appdir/.DirIcon"
  # APPIMAGE_EXTRACT_AND_RUN: the tool is itself an AppImage and the runners have no FUSE
  ARCH="$arch" VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream --runtime-file "$runtime" "$appdir" "$LC_DIST/$base.AppImage" > "$work/appimagetool.log" 2>&1 \
    || { cat "$work/appimagetool.log" >&2; lc_die "appimagetool failed"; }
}

for format in "${formats[@]}"; do
  lc_log "$format ($arch)"
  case "$format" in
    tar) make_tar ;;
    deb) make_deb ;;
    rpm) make_rpm ;;
    appimage) make_appimage ;;
    *) lc_die "unknown format \"$format\"" ;;
  esac
done
ls -l "$LC_DIST"
