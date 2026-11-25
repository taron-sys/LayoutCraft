#!/usr/bin/env bash
# Code signing on macOS (docs/36-packaging.md, "Adding signing").
#   tools/packaging/macos/sign.sh app <LayoutCraft.app>     the programs inside, then the bundle
#   tools/packaging/macos/sign.sh file <file.dmg>           a disk image (only with a Developer ID, otherwise nothing to do)
# WITHOUT the secrets below the bundle gets an ad-hoc signature ("-"): it is not a trusted identity (Gatekeeper still asks the user), but Apple-silicon
# Macs refuse to run code without any signature, and lipo removes the one the linker wrote.
# WITH them it is signed with the Developer ID Application certificate, hardened runtime and a secure timestamp (what notarisation requires).
# Secrets (given to this step only, never to the build): APPLE_DEVELOPER_ID_CERT_BASE64 (the .p12 certificate with its private key, base64) and
# APPLE_CERT_PASSWORD. Not exercised yet: no Apple Developer ID exists for the project.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

mode="${1:?usage: sign.sh app <LayoutCraft.app> | file <file>}"
target="${2:?usage: sign.sh app <LayoutCraft.app> | file <file>}"

identity="-"
keychain_args=()
if [ -n "${APPLE_DEVELOPER_ID_CERT_BASE64:-}" ]; then
  [ -n "${APPLE_CERT_PASSWORD:-}" ] || lc_die "APPLE_DEVELOPER_ID_CERT_BASE64 is set but APPLE_CERT_PASSWORD is not"
  tmp="$(mktemp -d)"
  keychain="$tmp/lc-signing.keychain-db"
  keychain_password="$(uuidgen)"
  trap 'security delete-keychain "$keychain" > /dev/null 2>&1 || true; rm -rf "$tmp"' EXIT
  printf '%s' "$APPLE_DEVELOPER_ID_CERT_BASE64" | base64 --decode > "$tmp/cert.p12"
  security create-keychain -p "$keychain_password" "$keychain"
  security set-keychain-settings -lut 3600 "$keychain"
  security unlock-keychain -p "$keychain_password" "$keychain"
  security import "$tmp/cert.p12" -k "$keychain" -P "$APPLE_CERT_PASSWORD" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" > /dev/null
  identity="$(security find-identity -v -p codesigning "$keychain" | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' | head -n 1)"
  [ -n "$identity" ] || lc_die "the certificate is not a Developer ID Application certificate"
  keychain_args=(--keychain "$keychain" --options runtime --timestamp)
  lc_log "signing with \"$identity\""
else
  lc_log "no APPLE_DEVELOPER_ID_CERT_BASE64 secret: ad-hoc signature only"
fi

# ${array[@]+...}: an empty array is an unset variable for the bash 3.2 of macOS under set -u
sign() { codesign --force --sign "$identity" ${keychain_args[@]+"${keychain_args[@]}"} "$@"; }

case "$mode" in
  app)
    for exe in layoutcraft-mcp layoutcraft-app; do sign "$target/Contents/MacOS/$exe"; done
    sign "$target"
    codesign --verify --strict --verbose=2 "$target"
    ;;
  file)
    if [ "$identity" = "-" ]; then lc_log "nothing to sign: $target"; else sign "$target"; fi
    ;;
  *) lc_die "unknown mode \"$mode\"" ;;
esac
