#!/usr/bin/env bash
# Collects the release files of all build jobs into one folder, checks that exactly the expected set is there and writes SHA256SUMS.txt over it.
#   tools/packaging/assemble.sh <folder>
# The folder has the files of every job (the workflow downloads the job artifacts into it). The list of expected files is the release contract:
# a job that silently produced nothing fails the release here instead of publishing a release with a hole in it.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

dir="${1:?usage: assemble.sh <folder>}"
version="$(lc_version)"
p="LayoutCraft-$version"
expected=(
  "$p-windows-x64-portable.zip" "$p-windows-x64.msi"
  "$p-windows-arm64-portable.zip" "$p-windows-arm64.msi"
  "$p-macos-universal.dmg" "$p-macos-universal.tar.gz"
  "$p-linux-x86_64.tar.gz" "$p-linux-x86_64.deb" "$p-linux-x86_64.rpm" "$p-linux-x86_64.AppImage"
  "$p-linux-aarch64.tar.gz" "$p-linux-aarch64.deb" "$p-linux-aarch64.rpm" "$p-linux-aarch64.AppImage"
  "$p-web.zip"
)

cd "$dir"
rm -f SHA256SUMS.txt
failed=0
for name in "${expected[@]}"; do
  if [ ! -s "$name" ]; then
    echo "error: missing or empty release file: $name" >&2
    failed=1
  fi
done
for found in *; do
  # shellcheck disable=SC2076 # a plain string comparison, not a pattern
  if [[ ! " ${expected[*]} " =~ " $found " ]]; then
    echo "error: unexpected file in the release folder: $found" >&2
    failed=1
  fi
done
[ "$failed" = 0 ] || lc_die "the release files are not the expected set (see above)"

# every archive carries the notices and licences (the installers and images are made from the same files)
for name in "${expected[@]}"; do
  case "$name" in
    *.zip) listing="$(python3 -m zipfile -l "$name")" ;;
    *.tar.gz) listing="$(tar -tzf "$name")" ;;
    *.deb) listing="$(dpkg-deb --contents "$name")" ;;
    *) continue ;;
  esac
  for doc in THIRD-PARTY-NOTICES.txt LICENSE-MIT LICENSE-APACHE PROVENANCE.md; do
    printf '%s\n' "$listing" | grep -q "$doc" || lc_die "$name does not contain $doc"
  done
done

sha256sum -- "${expected[@]}" > SHA256SUMS.txt
sha256sum --check --strict SHA256SUMS.txt
lc_log "${#expected[@]} release files and SHA256SUMS.txt in $dir"
