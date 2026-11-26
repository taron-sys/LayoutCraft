#!/usr/bin/env bash
# Checks that the desktop entry and the MIME file belong together (docs/36-packaging.md): installed into a scratch folder, every file
# type LayoutCraft opens as a drawing is recognised as a type the desktop entry names. Needs update-mime-database and gio.
#   tools/packaging/linux/check-mime.sh
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
install -Dm644 "$here/layoutcraft-mime.xml" "$scratch/share/mime/packages/layoutcraft.xml"
XDG_DATA_HOME="$scratch/share" update-mime-database "$scratch/share/mime"
desktop-file-validate "$here/layoutcraft.desktop"
accepted=";$(sed -n 's/^MimeType=//p' "$here/layoutcraft.desktop")"
mkdir "$scratch/files"
failed=0
for extension in vsdx vsdm vstx vstm vsd vst vss vdx vtx vsx; do
  # one meaningless byte, so that the type comes from the name
  printf 'x' > "$scratch/files/drawing.$extension"
  type="$(XDG_DATA_HOME="$scratch/share" gio info -a standard::content-type "$scratch/files/drawing.$extension" | sed -n 's/.*standard::content-type: //p')"
  if [[ "$accepted" == *";$type;"* ]]; then
    echo "ok   .$extension  $type"
  else
    echo "FAIL .$extension  $type is not in the desktop entry"
    failed=1
  fi
done
exit "$failed"
